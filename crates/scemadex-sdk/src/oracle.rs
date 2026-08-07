use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::primitives::Address;

/// Deployer / token reputation distilled from historical rug/success outcomes —
/// and, under Conviction Routing, from on-chain **bond settlement history**
/// (honored vs. slashed), which is far harder to fake than self-reported scores.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Reputation {
    /// `0.0` (rug-prone) … `1.0` (trusted).
    pub score: f64,
    pub samples: u32,
}

/// A predictive 0–100 quality score for a pool.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PoolScore {
    pub score: f64,
}

/// A directional suggestion with the agent's conviction and reasoning.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Advice {
    pub action: String,
    pub conviction: f64,
    pub reason: String,
}

/// The read side of the marketplace (Primitive C): signals other agents pay for
/// per-query via x402. The `scematica` feature serves these from the bot's
/// reputation ledger, pool scorer, and Deep Q* advice.
#[async_trait]
pub trait SignalSource: Send + Sync {
    async fn reputation(&self, deployer: &Address) -> Result<Reputation>;
    async fn pool_score(&self, pool: &Address) -> Result<PoolScore>;
    async fn advice(&self, token: &Address) -> Result<Advice>;
}
