use parking_lot::RwLock;
use remote_prover::{network_name, prover_routes, CurrentNetwork, ProverConfig};
use snarkvm::synthesizer::Process;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    eprintln!("Aleo Remote Prover starting...");
    eprintln!("  Network: {}", network_name());
    eprintln!("  ALEO_HOME: {}", std::env::var("ALEO_HOME").unwrap_or_else(|_| "~/.aleo (default)".to_string()));

    let config = Arc::new(ProverConfig::from_env());
    let listen_addr = config.listen_addr();

    eprintln!("  Listen address: {}", listen_addr);
    eprintln!("  Max concurrent proofs: {}", config.max_concurrent_proofs());
    eprintln!("  Enforce program editions: {}", config.enforce_program_editions());

    eprintln!("Loading snarkvm process (this may take a while on first run)...");
    let process = Process::<CurrentNetwork>::load().expect("Failed to initialize snarkvm process");
    let process = Arc::new(RwLock::new(process));
    eprintln!("snarkvm process loaded successfully");

    let prove_route = prover_routes(process, config);

    eprintln!("Remote Prover ready on http://{}", listen_addr);
    warp::serve(prove_route).run(listen_addr).await;
}
