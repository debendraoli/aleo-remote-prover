#[cfg(all(feature = "testnet", feature = "mainnet"))]
compile_error!("Cannot enable both testnet and mainnet features simultaneously");

#[cfg(not(any(feature = "testnet", feature = "mainnet")))]
compile_error!("Must enable either testnet or mainnet feature");

#[cfg(feature = "testnet")]
pub type CurrentNetwork = snarkvm::prelude::TestnetV0;

#[cfg(feature = "mainnet")]
pub type CurrentNetwork = snarkvm::prelude::MainnetV0;

pub type CurrentAleo = snarkvm::circuit::AleoV0;

pub mod config;
pub mod model;

mod programs;
mod proving;
mod server;

pub use config::{Network, ProverConfig};
pub use model::{AuthorizationPayload, ProveRequest};
pub use server::prover_routes;
