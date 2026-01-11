use parking_lot::RwLock;
use remote_prover::{network_name, prover_routes, CurrentNetwork, ProverConfig};
use snarkvm::synthesizer::Process;
use std::sync::Arc;
use tracing::info;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .init();

    info!("Aleo Remote Prover starting...");
    info!("Network: {}", network_name());
    info!("HOME: {}", std::env::var("HOME").unwrap_or_else(|_| "~/.aleo (default)".to_string()));

    let config = Arc::new(ProverConfig::from_env());
    let listen_addr = config.listen_addr();

    info!("Listen address: {}", listen_addr);

    let process = Process::<CurrentNetwork>::load().expect("Failed to initialize snarkvm process");
    let process = Arc::new(RwLock::new(process));

    let prove_route = prover_routes(process, config);

    info!("Remote Prover ready on http://{}", listen_addr);
    warp::serve(prove_route).run(listen_addr).await;
}
