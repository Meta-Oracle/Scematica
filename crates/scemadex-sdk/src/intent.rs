use serde::{Deserialize, Serialize};

use crate::primitives::{Address, Amount};

/// What the caller is optimizing for. The RL policy weights its route search by
/// this objective — and, crucially, can optimize *timing* and *footprint*, which
/// static pathfinders cannot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Objective {
    /// Best effective price / minimal slippage.
    Price,
    /// Fastest confirmation; price is secondary.
    Speed,
    /// Minimal MEV footprint — split and time the order to resist sandwiching.
    Stealth,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    Buy,
    Sell,
}

/// Hard limits the solver must respect; a solution that violates any of these is
/// rejected before bonding.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Constraints {
    pub max_slippage_bps: u32,
    /// Deadline in unix seconds; `0` means "no explicit deadline" in the scaffold.
    pub deadline_unix: u64,
    /// Optional cap on how many legs/splits the route may use.
    pub max_legs: Option<u8>,
}

/// A declarative request: *what* the caller wants, not *how* to route it. The
/// [`crate::policy::RoutePolicy`] turns this into a [`crate::policy::Solution`].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Intent {
    pub input_mint: Address,
    pub output_mint: Address,
    pub amount_in: Amount,
    pub side: Side,
    pub objective: Objective,
    pub constraints: Constraints,
}

impl Intent {
    /// Stable content digest used as a key for metering, caching, and bonds.
    ///
    /// Uses an FNV-1a hash over the canonical JSON encoding to avoid pulling a
    /// crypto-hash dependency into the lean core. Good enough as a scaffold key;
    /// the `scematica` feature can swap in a stronger hash where it matters.
    pub fn digest(&self) -> String {
        let s = serde_json::to_string(self).unwrap_or_default();
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in s.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        format!("{h:016x}")
    }
}
