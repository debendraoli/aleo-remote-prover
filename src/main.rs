use parking_lot::RwLock;
use remote_prover::{prover_routes, CurrentNetwork, ProverConfig};
use snarkvm::synthesizer::Process;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let config = Arc::new(ProverConfig::from_env());
    let listen_addr = config.listen_addr();

    let process = Process::<CurrentNetwork>::load().expect("failed to initialize snarkvm process");
    let process = Arc::new(RwLock::new(process));

    let prove_route = prover_routes(process, config);

    println!("🚀 Remote Prover running on {listen_addr}...");
    warp::serve(prove_route).run(listen_addr).await;
}
