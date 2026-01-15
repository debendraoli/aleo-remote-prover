use crate::{
    config::ProverConfig,
    model::ProveRequest,
    programs::ensure_programs_available,
    proving::prove_transaction,
    CurrentNetwork, NETWORK,
};
use parking_lot::RwLock;
use snarkvm::{prelude::Authorization, synthesizer::Process};
use std::{str::FromStr, sync::Arc};
use tracing::{debug, error, info, warn};
use warp::{http::StatusCode, Filter};

#[derive(Clone)]
struct ProverState {
    process: Arc<RwLock<Process<CurrentNetwork>>>,
    config: Arc<ProverConfig>,
}

pub fn prover_routes(
    process: Arc<RwLock<Process<CurrentNetwork>>>,
    config: Arc<ProverConfig>,
) -> impl Filter<Extract = (impl warp::Reply,), Error = warp::Rejection> + Clone {
    let state = ProverState { process, config };

    let prove_route = warp::path("prove")
        .and(warp::post())
        .and(warp::body::json())
        .and(with_state(state.clone()))
        .and_then(handle_prove);

    let health_route = warp::path::end().and(warp::get()).map(|| {
        json_reply(
            StatusCode::OK,
            serde_json::json!({
                "status": "ok",
            }),
        )
    });

    health_route.or(prove_route)
}

fn with_state(
    state: ProverState,
) -> impl Filter<Extract = (ProverState,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || state.clone())
}

async fn handle_prove(
    req: ProveRequest,
    state: ProverState,
) -> Result<impl warp::Reply, warp::Rejection> {
    info!(
        "Received proving request. Broadcast requested: {:?}",
        req.broadcast.unwrap_or(true)
    );

    let authorization = match parse_authorization("authorization", &req.authorization) {
        Ok(auth) => auth,
        Err(err) => {
            warn!("Invalid authorization payload: {}", err);
            return Ok(bad_request(err));
        }
    };
    debug!("Authorization payload parsed successfully.");

    let fee_authorization = match req.fee_authorization.as_ref() {
        Some(payload) => match parse_authorization("fee_authorization", payload) {
            Ok(auth) => Some(auth),
            Err(err) => {
                warn!("Invalid fee authorization payload: {}", err);
                return Ok(bad_request(err));
            }
        },
        None => None,
    };
    if fee_authorization.is_some() {
        debug!("Fee authorization payload parsed successfully.");
    }

    let client = state.config.http_client();
    let api_base = ProverConfig::network_api_base();

    debug!("Ensuring programs are available locally...");
    if let Err(err) =
        ensure_programs_available(&state.process, client, &api_base, &authorization).await
    {
        error!("Failed to ensure programs available: {}", err);
        return Ok(error_reply(err));
    }

    if let Some(fee_auth) = &fee_authorization {
        if let Err(err) =
            ensure_programs_available(&state.process, client, &api_base, fee_auth).await
        {
            error!("Failed to ensure fee programs available: {}", err);
            return Ok(error_reply(err));
        }
    }

    info!("Starting proof generation...");

    let process_for_exec = state.process.clone();
    let endpoint = ProverConfig::api_base_url();
    let fee_authorization_for_exec = fee_authorization.clone();

    let proving_join = tokio::task::spawn_blocking(move || {
        prove_transaction(
            process_for_exec,
            authorization,
            fee_authorization_for_exec,
            endpoint,
        )
    })
    .await;

    let artifacts = match proving_join {
        Ok(Ok(artifacts)) => {
            info!(
                "Proof generation successful. Execution ID: {}",
                artifacts.execution_id
            );
            artifacts
        }
        Ok(Err(err)) => {
            error!("Proof generation failed w/ logic error: {}", err);
            return Ok(error_reply(err));
        }
        Err(join_error) => {
            error!("Worker panicked while proving: {}", join_error);
            return Ok(error_reply(format!("Worker panicked while proving: {join_error}")));
        }
    };

    let transaction_id = artifacts.transaction.id().to_string();
    info!("Transaction ID: {}", transaction_id);

    let transaction_type = if artifacts.transaction.is_deploy() {
        "deploy"
    } else if artifacts.transaction.is_fee() {
        "fee"
    } else {
        "execute"
    };

    let transaction_string = match serde_json::to_string(&artifacts.transaction) {
        Ok(value) => value,
        Err(err) => {
            error!("Failed to serialize transaction: {}", err);
            return Ok(error_reply(format!("Failed to serialize transaction: {err}")));
        }
    };

    let transaction_value: serde_json::Value = match serde_json::from_str(&transaction_string) {
        Ok(value) => value,
        Err(err) => {
            error!("Failed to parse transaction JSON: {}", err);
            return Ok(error_reply(format!("Failed to parse transaction JSON: {err}")));
        }
    };
    let transaction_preview = truncate_for_log(&transaction_string, 256);

    let mut response_json = serde_json::json!({
        "status": "success",
        "network": NETWORK,
        "transaction_id": transaction_id,
        "transaction_type": transaction_type,
        "execution_id": artifacts.execution_id,
        "transaction": transaction_value.clone(),
        "transaction_payload": transaction_string,
        "summary": artifacts.summary,
    });

    if let Some(fee_info) = artifacts.fee_info {
        if let Some(object) = response_json.as_object_mut() {
            object.insert(
                "fee".to_string(),
                serde_json::to_value(fee_info).unwrap_or(serde_json::Value::Null),
            );
        }
    }

    let broadcast_requested = req.broadcast.unwrap_or(true);
    if broadcast_requested {
        let endpoint = ProverConfig::broadcast_endpoint();
        let client = state.config.http_client();
        info!("Broadcasting transaction {} to {}", transaction_id, endpoint);

        let broadcast_meta = match client.post(&endpoint).json(&transaction_value).send().await {
            Ok(resp) => {
                let status = resp.status();
                let body = match resp.text().await {
                    Ok(text) => truncate_for_log(&text, 256),
                    Err(err) => {
                        error!("Error reading broadcast response body: {}", err);
                        format!("<error reading body: {err}>")
                    }
                };

                if status.is_success() {
                    info!(": Status {}", status);
                } else {
                    warn!("Broadcast returned error status: {}. Body: {}", status, body);
                }

                serde_json::json!({
                    "requested": true,
                    "endpoint": endpoint,
                    "status": status.as_u16(),
                    "success": status.is_success(),
                    "response": body,
                    "payload_preview": transaction_preview,
                })
            }
            Err(err) => {
                error!("Broadcast request failed: {}", err);
                serde_json::json!({
                    "requested": true,
                    "endpoint": endpoint,
                    "success": false,
                    "error": err.to_string(),
                    "payload_preview": transaction_preview,
                })
            }
        };

        if let Some(object) = response_json.as_object_mut() {
            object.insert("broadcast".to_string(), broadcast_meta);
        }
    } else if let Some(object) = response_json.as_object_mut() {
        object.insert(
            "broadcast".to_string(),
            serde_json::json!({
                "requested": false,
            }),
        );
        info!("Broadcast skipped (not requested).");
    }

    Ok(json_reply(StatusCode::OK, response_json))
}

fn parse_authorization(
    label: &str,
    payload: &serde_json::Value,
) -> Result<Authorization<CurrentNetwork>, String> {
    let json = serde_json::to_string(payload)
        .map_err(|err| format!("Invalid {label} payload: {err}"))?;
    Authorization::<CurrentNetwork>::from_str(&json)
        .map_err(|err| format!("Error parsing {label}: {err}"))
}

fn json_reply(
    status: StatusCode,
    body: serde_json::Value,
) -> warp::reply::WithStatus<warp::reply::Json> {
    warp::reply::with_status(warp::reply::json(&body), status)
}

fn error_reply(message: impl Into<String>) -> warp::reply::WithStatus<warp::reply::Json> {
    json_reply(
        StatusCode::INTERNAL_SERVER_ERROR,
        serde_json::json!({ "status": "error", "message": message.into() }),
    )
}

fn bad_request(message: impl Into<String>) -> warp::reply::WithStatus<warp::reply::Json> {
    json_reply(
        StatusCode::BAD_REQUEST,
        serde_json::json!({ "status": "error", "message": message.into() }),
    )
}

fn truncate_for_log(input: &str, max_len: usize) -> String {
    if input.chars().count() <= max_len {
        return input.to_owned();
    }

    let mut truncated: String = input.chars().take(max_len).collect();
    truncated.push('…');
    truncated
}
