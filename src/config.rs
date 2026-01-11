use reqwest::Client;
use std::{env, net::SocketAddr};

pub const API_BASE_URL: &str = "https://api.explorer.provable.com";

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

pub fn broadcast_endpoint() -> String {
    format!("{}/transaction/broadcast", network_api_base())
}

#[derive(Clone)]
pub struct ProverConfig {
    listen_addr: SocketAddr,
    http_client: Client,
    endpoint: String,
}

impl Default for ProverConfig {
    fn default() -> Self {
        Self {
            listen_addr: SocketAddr::from(([0, 0, 0, 0], 3030)),
            http_client: Client::new(),
            endpoint: API_BASE_URL.to_string(),
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

        if let Ok(endpoint) = env::var("ENDPOINT") {
            let trimmed = endpoint.trim();
            if !trimmed.is_empty() {
                config.endpoint = trimmed.to_string();
            }
        }

        config
    }

    pub fn listen_addr(&self) -> SocketAddr {
        self.listen_addr
    }

    pub fn http_client(&self) -> &Client {
        &self.http_client
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub fn with_endpoint<S: Into<String>>(mut self, endpoint: S) -> Self {
        self.endpoint = endpoint.into();
        self
    }
}
