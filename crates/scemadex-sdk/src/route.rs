use serde::{Deserialize, Serialize};

use crate::primitives::{Address, Amount};

/// A liquidity venue the router can route through.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Venue {
    Raydium,
    Orca,
    Meteora,
    Jupiter,
    Custom,
}

/// One hop of a route through a single venue.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RouteLeg {
    pub venue: Venue,
    pub input_mint: Address,
    pub output_mint: Address,
    /// Fraction of the parent order routed through this leg, in basis points
    /// (`0..=10_000`).
    pub split_bps: u32,
    pub expected_out: Amount,
}

/// A complete route: one or more legs whose splits sum to `10_000` bps.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Route {
    pub legs: Vec<RouteLeg>,
    pub expected_out: Amount,
}

impl Route {
    /// True when the leg splits sum to a full order (`10_000` bps).
    pub fn splits_valid(&self) -> bool {
        !self.legs.is_empty()
            && self.legs.iter().map(|l| l.split_bps).sum::<u32>() == 10_000
    }
}

/// The realized outcome of executing a route — feeds bond settlement and the RL
/// reward (realized fill vs. oracle mid, minus any slashed bond).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Fill {
    pub amount_out: Amount,
    pub executed_unix: u64,
}
