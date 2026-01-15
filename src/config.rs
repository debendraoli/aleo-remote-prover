use crate::NETWORK;
use reqwest::Client;
use std::{env, net::SocketAddr};

pub const API_BASE_URL: &str = "https://api.explorer.provable.com";

#[derive(Clone)]
pub struct ProverConfig {
    listen_addr: SocketAddr,
    http_client: Client,
}

impl Default for ProverConfig {
    fn default() -> Self {
        Self {
            listen_addr: SocketAddr::from(([0, 0, 0, 0], 3030)),
            http_client: Client::new(),
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

        config
    }

    pub fn listen_addr(&self) -> SocketAddr {
        self.listen_addr
    }

    pub fn http_client(&self) -> &Client {
        &self.http_client
    }

    pub fn api_base_url() -> String {
        API_BASE_URL.to_string()
    }

    pub fn network_api_base() -> String {
        format!("{}/v2/{}", Self::api_base_url(), NETWORK)
    }

    pub fn broadcast_endpoint() -> String {
        format!(
            "{}/transaction/broadcast?check_transaction=true",
            Self::network_api_base()
        )
    }
}
