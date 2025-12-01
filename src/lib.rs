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
