use reqwest::Client;
use std::{env, net::SocketAddr};

/// API base URL for the Aleo explorer (without version/network suffix).
pub const API_BASE_URL: &str = "https://api.explorer.provable.com";

/// Returns the versioned API endpoint for the current network.
/// Uses compile-time feature flags to determine the network.
pub fn network_api_base() -> &'static str {
    #[cfg(feature = "testnet")]
    {
        "https://api.explorer.provable.com/v2/testnet"
    }
    #[cfg(feature = "mainnet")]
    {
        "https://api.explorer.provable.com/v2/mainnet"
    }
}

/// Returns the network name as a string.
pub fn network_name() -> &'static str {
    #[cfg(feature = "testnet")]
    {
        "testnet"
    }
    #[cfg(feature = "mainnet")]
    {
        "mainnet"
    }
}

/// Returns the transaction broadcast endpoint for the current network.
pub fn broadcast_endpoint() -> String {
    format!("{}/transaction/broadcast", network_api_base())
}

#[derive(Clone)]
pub struct ProverConfig {
    listen_addr: SocketAddr,
    max_concurrent_proofs: usize,
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
                    "Invalid PROVER_LISTEN_ADDR '{}', using default {}",
                    addr, config.listen_addr
                ),
            }
        }

        if let Ok(limit) = env::var("MAX_CONCURRENT_PROOFS") {
            match limit.parse::<usize>() {
                Ok(value) if value > 0 => config.max_concurrent_proofs = value,
                _ => eprintln!(
                    "Invalid MAX_CONCURRENT_PROOFS '{}', using default {}",
                    limit, config.max_concurrent_proofs
                ),
            }
        }

        if let Ok(flag) = env::var("ENFORCE_PROGRAM_EDITIONS") {
            match parse_bool(&flag) {
                Some(value) => config.enforce_program_editions = value,
                None => eprintln!(
                    "Invalid ENFORCE_PROGRAM_EDITIONS '{}', using default {}",
                    flag, config.enforce_program_editions
                ),
            }
        }

        if let Ok(endpoint) = env::var("REST_ENDPOINT_OVERRIDE") {
            let trimmed = endpoint.trim();
            if !trimmed.is_empty() {
                config.rest_endpoint_override = Some(trimmed.to_string());
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

    pub fn http_client(&self) -> &Client {
        &self.http_client
    }

    pub fn enforce_program_editions(&self) -> bool {
        self.enforce_program_editions
    }

    /// Returns the REST API endpoint for fetching programs.
    /// Uses override if set, otherwise uses the default API base.
    pub fn rest_endpoint(&self) -> String {
        self.rest_endpoint_override
            .clone()
            .unwrap_or_else(|| API_BASE_URL.to_string())
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
