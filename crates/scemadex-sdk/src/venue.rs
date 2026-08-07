use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::route::{Fill, Route};

/// Opaque, venue-specific instruction bytes the caller submits on-chain.
///
/// The SDK core does not depend on `solana-sdk`; under the `scematica` feature a
/// real executor returns `solana_sdk::instruction::Instruction`s built by the
/// `scematica-executor` crate.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SwapInstructions {
    pub serialized: Vec<u8>,
    pub describe: String,
}

/// Builds (and optionally submits) executable swaps for a solved route.
#[async_trait]
pub trait VenueExecutor: Send + Sync {
    /// Build submittable instructions for the route without sending them.
    async fn build(&self, route: &Route) -> Result<SwapInstructions>;
    /// Submit and await a realized fill. Reference impls may simulate.
    async fn execute(&self, route: &Route) -> Result<Fill>;
}

/// A simulating executor: it "fills" at the route's expected output and stamps a
/// zero timestamp. Lets the intent → bond → settle loop run end-to-end offline.
pub struct SimVenueExecutor;

#[async_trait]
impl VenueExecutor for SimVenueExecutor {
    async fn build(&self, route: &Route) -> Result<SwapInstructions> {
        Ok(SwapInstructions {
            serialized: Vec::new(),
            describe: format!("sim: {} leg(s)", route.legs.len()),
        })
    }

    async fn execute(&self, route: &Route) -> Result<Fill> {
        Ok(Fill {
            amount_out: route.expected_out,
            executed_unix: 0,
        })
    }
}
