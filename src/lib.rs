#[cfg(all(feature = "testnet", feature = "mainnet"))]
compile_error!("Cannot enable both testnet and mainnet features simultaneously");

#[cfg(not(any(feature = "testnet", feature = "mainnet")))]
compile_error!("Must enable either testnet or mainnet feature");

#[cfg(feature = "testnet")]
pub type CurrentNetwork = snarkvm::prelude::TestnetV0;
#[cfg(feature = "testnet")]
pub type CurrentAleo = snarkvm::circuit::AleoTestnetV0;
#[cfg(feature = "testnet")]
pub const NETWORK: &str = "testnet";

#[cfg(feature = "mainnet")]
pub type CurrentNetwork = snarkvm::prelude::MainnetV0;
#[cfg(feature = "mainnet")]
pub type CurrentAleo = snarkvm::circuit::AleoV0;
#[cfg(feature = "mainnet")]
pub const NETWORK: &str = "mainnet";

pub mod config;
pub mod model;

mod programs;
mod proving;
mod server;

pub use config::{ProverConfig, API_BASE_URL};
pub use model::ProveRequest;
pub use server::prover_routes;
