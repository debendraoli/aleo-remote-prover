use crate::{
    config::ProverConfig,
    model::{AuthorizationPayload, ProveRequest},
    programs::ensure_programs_available,
    proving::prove_transaction,
    CurrentNetwork,
};
use parking_lot::RwLock;
use snarkvm::{prelude::Authorization, synthesizer::Process};
use std::{str::FromStr, sync::Arc};
use tokio::sync::Semaphore;
use warp::{http::StatusCode, Filter};

#[derive(Clone)]
struct ProverState {
    process: Arc<RwLock<Process<CurrentNetwork>>>,
    config: Arc<ProverConfig>,
    limiter: Arc<Semaphore>,
}

pub fn prover_routes(
    process: Arc<RwLock<Process<CurrentNetwork>>>,
    config: Arc<ProverConfig>,
) -> impl Filter<Extract = (impl warp::Reply,), Error = warp::Rejection> + Clone {
    let limiter = Arc::new(Semaphore::new(config.max_concurrent_proofs().max(1)));
    let state = ProverState {
        process,
        config,
        limiter,
    };

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
    println!("Received proving request...");

    let authorization = match parse_authorization_payload("authorization", &req.authorization) {
        Ok(auth) => auth,
        Err(err) => return Ok(bad_request(err)),
    };

    let fee_authorization = match req.fee_authorization.as_ref() {
        Some(payload) => match parse_authorization_payload("fee_authorization", payload) {
            Ok(auth) => Some(auth),
            Err(err) => return Ok(bad_request(err)),
        },
        None => None,
    };

    let client = state.config.http_client();
    let api_base = state.config.network().base_url();

    if let Err(err) =
        ensure_programs_available(&state.process, client, api_base, &authorization).await
    {
        return Ok(error_reply(err));
    }

    if let Some(fee_auth) = &fee_authorization {
        if let Err(err) =
            ensure_programs_available(&state.process, client, api_base, fee_auth).await
        {
            return Ok(error_reply(err));
        }
    }

    let permit = state
        .limiter
        .clone()
        .acquire_owned()
        .await
        .expect("Semaphore closed");

    let process_for_exec = state.process.clone();
    let rest_endpoint = state.config.rest_endpoint_for(effective_network);
    let fee_authorization_for_exec = fee_authorization.clone();
    let enforce_program_editions = state.config.enforce_program_editions();

    let proving_join = tokio::task::spawn_blocking(move || {
        prove_transaction(
            process_for_exec,
            authorization,
            fee_authorization_for_exec,
            rest_endpoint,
            enforce_program_editions,
        )
    })
    .await;

    drop(permit);

    let artifacts = match proving_join {
        Ok(Ok(artifacts)) => artifacts,
        Ok(Err(err)) => return Ok(error_reply(err)),
        Err(join_error) => {
            return Ok(error_reply(format!("Worker panicked while proving: {join_error}")));
        }
    };

    let transaction_id = format!("{:?}", artifacts.transaction.id());
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
            return Ok(error_reply(format!("Failed to serialize transaction: {err}")));
        }
    };

    let transaction_value: serde_json::Value = match serde_json::from_str(&transaction_string) {
        Ok(value) => value,
        Err(err) => {
            return Ok(error_reply(format!("Failed to parse transaction JSON: {err}")));
        }
    };
    let transaction_preview = truncate_for_log(&transaction_string, 256);

    let mut response_json = serde_json::json!({
        "status": "success",
        "network": effective_network,
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
        let endpoint = effective_network.broadcast_endpoint();
        let client = state.config.http_client();
        let payload_string = response_json
            .get("transaction_payload")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .to_string();
        let payload = serde_json::json!({
            "transaction": payload_string,
        });

        let broadcast_meta = match client.post(&endpoint).json(&payload).send().await {
            Ok(resp) => {
                let status = resp.status();
                let body = match resp.text().await {
                    Ok(text) => truncate_for_log(&text, 256),
                    Err(err) => format!("<error reading body: {err}>"),
                };

                serde_json::json!({
                    "requested": true,
                    "endpoint": endpoint,
                    "status": status.as_u16(),
                    "success": status.is_success(),
                    "response": body,
                    "payload_preview": transaction_preview,
                })
            }
            Err(err) => serde_json::json!({
                "requested": true,
                "endpoint": endpoint,
                "success": false,
                "error": err.to_string(),
                "payload_preview": transaction_preview,
            }),
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
    }

    Ok(json_reply(StatusCode::OK, response_json))
}

fn parse_authorization_payload(
    label: &str,
    payload: &AuthorizationPayload,
) -> Result<Authorization<CurrentNetwork>, String> {
    let json = payload
        .to_compact_string()
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
