use reqwest::Client;
use std::{env, net::SocketAddr, str::FromStr};

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

    pub fn rest_base_url(self) -> String {
        // All explorer environments currently share the same REST root. If that changes in the
        // future, adjust this match alongside `base_url`.
        match self {
            Network::Mainnet | Network::Testnet | Network::Canary => {
                "https://api.explorer.provable.com".to_string()
            }
        }
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
    http_client: Client,
    enforce_program_editions: bool,
    rest_endpoint_override: Option<String>,
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
            http_client: Client::new(),
            enforce_program_editions: true,
            rest_endpoint_override: None,
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

        if let Ok(flag) = env::var("ENFORCE_PROGRAM_EDITIONS") {
            match parse_bool(&flag) {
                Some(value) => config.enforce_program_editions = value,
                None => eprintln!(
                    "⚠️  Invalid ENFORCE_PROGRAM_EDITIONS '{flag}', keeping {}",
                    config.enforce_program_editions
                ),
            }
        }

        if let Ok(endpoint) = env::var("REST_ENDPOINT_OVERRIDE") {
            if endpoint.trim().is_empty() {
                eprintln!("⚠️  REST_ENDPOINT_OVERRIDE is empty, ignoring override");
            } else {
                config.rest_endpoint_override = Some(endpoint);
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

    pub fn http_client(&self) -> Client {
        self.http_client.clone()
    }

    pub fn enforce_program_editions(&self) -> bool {
        self.enforce_program_editions
    }

    #[cfg(test)]
    pub fn testing_with_enforced_program_editions(enforce: bool) -> Self {
        Self {
            enforce_program_editions: enforce,
            ..Self::default()
        }
    }

    pub fn with_enforce_program_editions(mut self, enforce: bool) -> Self {
        self.enforce_program_editions = enforce;
        self
    }

    pub fn rest_endpoint_for(&self, network: Network) -> String {
        self.rest_endpoint_override
            .clone()
            .unwrap_or_else(|| network.rest_base_url())
    }

    pub fn with_rest_endpoint_override<S: Into<String>>(mut self, endpoint: S) -> Self {
        self.rest_endpoint_override = Some(endpoint.into());
        self
    }
}

fn parse_bool(input: &str) -> Option<bool> {
    match input.trim().to_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}
