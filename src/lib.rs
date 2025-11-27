use parking_lot::RwLock;
use reqwest::Url;
use snarkvm::prelude::*;
use snarkvm::{circuit, synthesizer::Process};
use std::{
    collections::{HashMap, HashSet},
    env,
    net::SocketAddr,
    str::FromStr,
    sync::Arc,
};
use tokio::sync::Semaphore;
use warp::{http::StatusCode, Filter};

pub type CurrentNetwork = snarkvm::prelude::MainnetV0;
pub type CurrentAleo = circuit::AleoV0;

#[derive(Copy, Clone, Debug, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Network {
    Mainnet,
    Testnet,
    Canary,
}

impl Network {
    pub fn base_url(self) -> &'static str {
        match self {
            Network::Mainnet => "https://api.explorer.provable.com/v2/mainnet",
            Network::Testnet => "https://api.explorer.provable.com/v2/testnet",
            Network::Canary => "https://api.explorer.provable.com/v2/canary",
        }
    }

    pub fn endpoint(self, path: &str) -> String {
        let base = self.base_url().trim_end_matches('/');
        let path = path.trim_start_matches('/');
        format!("{base}/{path}")
    }

    pub fn broadcast_endpoint(self) -> String {
        self.endpoint("transaction/broadcast")
    }
}

impl FromStr for Network {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "mainnet" => Ok(Network::Mainnet),
            "testnet" => Ok(Network::Testnet),
            "canary" => Ok(Network::Canary),
            other => Err(format!("invalid network '{other}'")),
        }
    }
}

#[derive(Clone)]
pub struct ProverConfig {
    listen_addr: SocketAddr,
    max_concurrent_proofs: usize,
    network: Network,
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
            network: Network::Testnet,
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

        if let Ok(network) = env::var("NETWORK") {
            let trimmed = network.trim();
            if trimmed.is_empty() {
                eprintln!("⚠️  NETWORK is empty, keeping {:?}", config.network);
            } else {
                match Network::from_str(trimmed) {
                    Ok(target) => config.network = target,
                    Err(err) => eprintln!(
                        "⚠️  Invalid NETWORK '{network}': {err}. Keeping {:?}",
                        config.network
                    ),
                }
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

    pub fn broadcast_endpoint(&self) -> String {
        self.network.broadcast_endpoint()
    }

    pub fn network(&self) -> Network {
        self.network
    }

    pub fn api_base(&self) -> &'static str {
        self.network.base_url()
    }

    fn http_client(&self) -> reqwest::Client {
        self.http_client.clone()
    }
}

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

#[derive(Clone, serde::Deserialize, serde::Serialize)]
#[serde(untagged)]
pub enum AuthorizationPayload {
    String(String),
    Json(serde_json::Value),
}

impl AuthorizationPayload {
    fn to_compact_string(&self) -> Result<String, serde_json::Error> {
        match self {
            AuthorizationPayload::String(value) => Ok(value.clone()),
            AuthorizationPayload::Json(value) => serde_json::to_string(value),
        }
    }
}

#[derive(Clone, serde::Deserialize, serde::Serialize)]
pub struct ProveRequest {
    pub authorization: AuthorizationPayload,
    #[serde(default)]
    pub broadcast: Option<bool>,
    #[serde(default)]
    pub network: Option<Network>,
}

impl ProveRequest {
    fn authorization_json(&self) -> Result<String, serde_json::Error> {
        self.authorization.to_compact_string()
    }
}

async fn handle_prove(
    req: ProveRequest,
    state: ProverState,
) -> Result<impl warp::Reply, warp::Rejection> {
    println!("Received proving request...");

    let authorization_json = match req.authorization_json() {
        Ok(value) => value,
        Err(err) => {
            return Ok(json_reply(
                StatusCode::BAD_REQUEST,
                serde_json::json!({
                    "status": "error",
                    "message": format!("Invalid authorization payload: {err}"),
                }),
            ));
        }
    };

    let authorization = match Authorization::<CurrentNetwork>::from_str(&authorization_json) {
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

    let client = state.config.http_client();
    let request_network = req.network;
    let effective_network = request_network.unwrap_or_else(|| state.config.network());
    let api_base = effective_network.base_url();
    if let Err(err) =
        ensure_programs_available(&state.process, &client, api_base, &authorization).await
    {
        return Ok(json_reply(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({
                "status": "error",
                "message": err,
            }),
        ));
    }

    let permit = state
        .limiter
        .clone()
        .acquire_owned()
        .await
        .expect("Semaphore closed");

    let process_for_exec = state.process.clone();
    let execution_join = tokio::task::spawn_blocking(move || {
        let mut rng = rand::thread_rng();
        let process_guard = process_for_exec.read();
        process_guard.execute::<CurrentAleo, _>(authorization, &mut rng)
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

            let broadcast_requested = req.broadcast.unwrap_or(true);

            if broadcast_requested {
                let endpoint = request_network
                    .unwrap_or(effective_network)
                    .broadcast_endpoint();
                let client = state.config.http_client();
                let payload = serde_json::json!({
                    "authorization": authorization_json.clone(),
                    "summary": summary.clone(),
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
                        })
                    }
                    Err(err) => serde_json::json!({
                        "requested": true,
                        "endpoint": endpoint,
                        "success": false,
                        "error": err.to_string(),
                    }),
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

async fn ensure_programs_available(
    process: &Arc<RwLock<Process<CurrentNetwork>>>,
    client: &reqwest::Client,
    base_url: &str,
    authorization: &Authorization<CurrentNetwork>,
) -> Result<(), String> {
    let base = Url::parse(base_url)
        .map_err(|err| format!("Invalid program API base '{base_url}': {err}"))?;

    let mut stack: Vec<(ProgramID<CurrentNetwork>, bool)> = authorization
        .to_vec_deque()
        .into_iter()
        .map(|request| (*request.program_id(), false))
        .collect();
    stack.extend(
        authorization
            .transitions()
            .values()
            .map(|transition| (*transition.program_id(), false)),
    );

    let credits_program_id = ProgramID::<CurrentNetwork>::from_str("credits.aleo")
        .map_err(|err| format!("Failed to parse reference program ID: {err}"))?;

    let mut scheduled = HashSet::new();
    let mut pending: HashMap<ProgramID<CurrentNetwork>, Program<CurrentNetwork>> = HashMap::new();

    while let Some((program_id, ready)) = stack.pop() {
        if program_id == credits_program_id {
            continue;
        }

        {
            let guard = process.read();
            if guard.contains_program(&program_id) {
                if ready {
                    pending.remove(&program_id);
                }
                continue;
            }
        }

        if ready {
            if let Some(program) = pending.remove(&program_id) {
                let mut guard = process.write();
                if !guard.contains_program(program.id()) {
                    guard
                        .add_program(&program)
                        .map_err(|err| format!("Failed to add program '{program_id}': {err}"))?;
                }
            }
            continue;
        }

        if !scheduled.insert(program_id) {
            continue;
        }

        let program = fetch_remote_program(client, &base, &program_id).await?;
        let imports: Vec<_> = program.imports().keys().copied().collect();

        pending.insert(program_id, program);
        stack.push((program_id, true));
        for import_id in imports {
            stack.push((import_id, false));
        }
    }

    Ok(())
}

async fn fetch_remote_program(
    client: &reqwest::Client,
    base: &Url,
    program_id: &ProgramID<CurrentNetwork>,
) -> Result<Program<CurrentNetwork>, String> {
    let url = build_program_url(base, program_id, None)?;

    eprintln!(
        "ℹ️  Fetching missing program '{}' from {}",
        program_id,
        url.as_str()
    );

    let response = client
        .get(url.clone())
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|err| format!("Failed to fetch program '{program_id}': {err}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "Program '{program_id}' request failed with status {}",
            response.status()
        ));
    }

    let body = response
        .text()
        .await
        .map_err(|err| format!("Failed to read program '{program_id}': {err}"))?;
    let trimmed = body.trim();
    let source = if trimmed.starts_with('"') {
        serde_json::from_str::<String>(trimmed)
            .map_err(|err| format!("Failed to decode program '{program_id}': {err}"))?
    } else {
        body
    };

    Program::<CurrentNetwork>::from_str(&source)
        .map_err(|err| format!("Failed to parse program '{program_id}': {err}"))
}

fn build_program_url(
    base: &Url,
    program_id: &ProgramID<CurrentNetwork>,
    edition: Option<u16>,
) -> Result<Url, String> {
    let mut url = base.clone();
    {
        let mut segments = url
            .path_segments_mut()
            .map_err(|_| format!("Program API base '{}' must be absolute", base))?;
        segments.pop_if_empty();
        segments.push("program");
        segments.push(&program_id.to_string());
        if let Some(edition) = edition {
            segments.push(&edition.to_string());
        }
    }

    Ok(url)
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
