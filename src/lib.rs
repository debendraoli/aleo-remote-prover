use snarkvm::prelude::*;
use snarkvm::{circuit, synthesizer::Process};
use std::{env, net::SocketAddr, str::FromStr, sync::Arc};
use tokio::sync::Semaphore;
use warp::{http::StatusCode, Filter};

pub type CurrentNetwork = snarkvm::prelude::MainnetV0;
pub type CurrentAleo = circuit::AleoV0;

#[derive(Clone)]
pub struct ProverConfig {
    listen_addr: SocketAddr,
    max_concurrent_proofs: usize,
    broadcast_endpoint: Option<String>,
    http_client: reqwest::Client,
}

impl Default for ProverConfig {
    fn default() -> Self {
        let listen_addr = SocketAddr::from(([0, 0, 0, 0], 3030));
        let max_parallel = std::thread::available_parallelism()
            .map(|nz| nz.get())
            .unwrap_or(1);

        Self {
            listen_addr,
            max_concurrent_proofs: max_parallel,
            broadcast_endpoint: None,
            http_client: reqwest::Client::new(),
        }
    }
}

impl ProverConfig {
    pub fn from_env() -> Self {
        let mut config = Self::default();

        if let Ok(addr) = env::var("PROVER_LISTEN_ADDR") {
            match addr.parse::<SocketAddr>() {
                Ok(parsed) => config.listen_addr = parsed,
                Err(_) => eprintln!(
                    "⚠️  Invalid PROVER_LISTEN_ADDR '{addr}', keeping {}",
                    config.listen_addr
                ),
            }
        }

        if let Ok(limit) = env::var("MAX_CONCURRENT_PROOFS") {
            match limit.parse::<usize>() {
                Ok(value) if value > 0 => config.max_concurrent_proofs = value,
                _ => eprintln!(
                    "⚠️  Invalid MAX_CONCURRENT_PROOFS '{limit}', keeping {}",
                    config.max_concurrent_proofs
                ),
            }
        }

        if let Ok(endpoint) = env::var("BROADCAST_ENDPOINT") {
            let trimmed = endpoint.trim();
            if trimmed.is_empty() {
                config.broadcast_endpoint = None;
            } else {
                config.broadcast_endpoint = Some(trimmed.to_string());
            }
        }

        config
    }

    pub fn listen_addr(&self) -> SocketAddr {
        self.listen_addr
    }

    pub fn max_concurrent_proofs(&self) -> usize {
        self.max_concurrent_proofs
    }

    pub fn broadcast_endpoint(&self) -> Option<&str> {
        self.broadcast_endpoint.as_deref()
    }

    fn http_client(&self) -> reqwest::Client {
        self.http_client.clone()
    }
}

#[derive(Clone)]
struct ProverState {
    process: Arc<Process<CurrentNetwork>>,
    config: Arc<ProverConfig>,
    limiter: Arc<Semaphore>,
}

pub fn prover_routes(
    process: Arc<Process<CurrentNetwork>>,
    config: Arc<ProverConfig>,
) -> impl Filter<Extract = (impl warp::Reply,), Error = warp::Rejection> + Clone {
    let limiter = Arc::new(Semaphore::new(config.max_concurrent_proofs().max(1)));
    let state = ProverState {
        process,
        config,
        limiter,
    };

    warp::post()
        .and(warp::path("prove"))
        .and(warp::body::json())
        .and(with_state(state))
        .and_then(handle_prove)
}

fn with_state(
    state: ProverState,
) -> impl Filter<Extract = (ProverState,), Error = std::convert::Infallible> + Clone {
    warp::any().map(move || state.clone())
}

#[derive(Clone, serde::Deserialize, serde::Serialize)]
pub struct ProveRequest {
    pub authorization: String,
    #[serde(default)]
    pub broadcast: Option<bool>,
    #[serde(default)]
    pub broadcast_endpoint: Option<String>,
}

async fn handle_prove(
    req: ProveRequest,
    state: ProverState,
) -> Result<impl warp::Reply, warp::Rejection> {
    println!("Received proving request...");

    let authorization = match Authorization::<CurrentNetwork>::from_str(&req.authorization) {
        Ok(auth) => auth,
        Err(e) => {
            return Ok(json_reply(
                StatusCode::BAD_REQUEST,
                serde_json::json!({
                    "status": "error",
                    "message": format!("Error parsing authorization: {e}"),
                }),
            ));
        }
    };

    let permit = state
        .limiter
        .clone()
        .acquire_owned()
        .await
        .expect("Semaphore closed");

    let process_for_exec = state.process.clone();
    let execution_join = tokio::task::spawn_blocking(move || {
        let rng = &mut rand::thread_rng();
        process_for_exec.execute::<CurrentAleo, _>(authorization, rng)
    })
    .await;

    let execution_result = match execution_join {
        Ok(result) => result,
        Err(join_error) => {
            drop(permit);
            return Ok(json_reply(
                StatusCode::INTERNAL_SERVER_ERROR,
                serde_json::json!({
                    "status": "error",
                    "message": format!("Worker panicked while proving: {join_error}"),
                }),
            ));
        }
    };

    drop(permit);

    match execution_result {
        Ok((response, trace)) => {
            let summary = serde_json::json!({
                "output_ids": response
                    .output_ids()
                    .iter()
                    .map(|output_id| format!("{output_id:?}"))
                    .collect::<Vec<_>>(),
                "outputs": response
                    .outputs()
                    .iter()
                    .map(|output| format!("{output:?}"))
                    .collect::<Vec<_>>(),
                "transitions": trace.transitions().len(),
                "is_fee": trace.is_fee(),
            });

            let mut response_json = serde_json::json!({
                "status": "success",
                "summary": summary.clone(),
            });

            let default_broadcast = state.config.broadcast_endpoint().is_some();
            let broadcast_requested = req
                .broadcast
                .unwrap_or(default_broadcast || req.broadcast_endpoint.is_some());
            let endpoint_candidate = req
                .broadcast_endpoint
                .clone()
                .or_else(|| state.config.broadcast_endpoint().map(|s| s.to_string()));

            if broadcast_requested {
                let broadcast_meta = if let Some(endpoint) = endpoint_candidate {
                    let client = state.config.http_client();
                    let payload = serde_json::json!({
                        "authorization": req.authorization.clone(),
                        "summary": summary.clone(),
                    });

                    match client.post(&endpoint).json(&payload).send().await {
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
                            })
                        }
                        Err(err) => serde_json::json!({
                            "requested": true,
                            "endpoint": endpoint,
                            "success": false,
                            "error": err.to_string(),
                        }),
                    }
                } else {
                    serde_json::json!({
                        "requested": true,
                        "success": false,
                        "error": "No broadcast endpoint configured",
                    })
                };

                if let Some(object) = response_json.as_object_mut() {
                    object.insert("broadcast".to_string(), broadcast_meta);
                }
            }

            Ok(json_reply(StatusCode::OK, response_json))
        }
        Err(e) => {
            let error_json = serde_json::json!({
                "status": "error",
                "message": e.to_string()
            });
            Ok(json_reply(StatusCode::INTERNAL_SERVER_ERROR, error_json))
        }
    }
}

fn json_reply(
    status: StatusCode,
    body: serde_json::Value,
) -> warp::reply::WithStatus<warp::reply::Json> {
    let reply = warp::reply::json(&body);
    warp::reply::with_status(reply, status)
}

fn truncate_for_log(input: &str, max_len: usize) -> String {
    if input.chars().count() <= max_len {
        return input.to_owned();
    }

    let mut truncated: String = input.chars().take(max_len).collect();
    truncated.push('…');
    truncated
}
