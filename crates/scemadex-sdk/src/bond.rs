use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::{Result, ScemaDexError};
use crate::policy::{Conviction, Solution};
use crate::primitives::Usdc;
use crate::promise::{Outcome, Promise};
use crate::route::Fill;

/// A performance bond escrowed by the policy against its own promise.
///
/// This is the defining **Conviction Routing** primitive (D): if the realized
/// [`Fill`] meets the guarantee, the agent reclaims the bond and collects the
/// inference fee; otherwise the bond settles to the caller as compensation. It
/// makes a *paid black-box inference* trustworthy — the agent has slashable skin
/// in the game.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Bond {
    pub intent_digest: String,
    /// Escrowed amount (conviction-weighted), in micro-USDC over x402.
    pub amount: Usdc,
    /// Minimum acceptable output (base units) the agent guarantees.
    pub min_out_raw: u64,
    /// Latest unix second by which the fill must land (`0` = unbounded in scaffold).
    pub deadline_unix: u64,
}

impl Bond {
    /// The guarantee this bond encodes, as a domain-agnostic [`Promise`]. A swap
    /// bond promises a minimum output; this is the bridge that lets the swap path
    /// settle through the same [`Outcome`] seam as any other domain.
    pub fn promise(&self) -> Promise {
        Promise::MinOutput(self.min_out_raw)
    }
}

/// The result of settling a bond against a realized fill.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BondOutcome {
    /// Promise met — bond returns to the agent.
    Honored,
    /// Promise missed — bond is slashed to the caller.
    Slashed,
}

/// Escrows and settles bonds over the x402 rails.
///
/// The default crate ships [`NoBondEngine`]; the `scematica` feature backs this
/// with the real `scematica-protocol` facilitator so bonds settle on-chain.
#[async_trait]
pub trait BondEngine: Send + Sync {
    /// Size and escrow a bond for a solution (conviction-weighted).
    async fn escrow(&self, solution: &Solution) -> Result<Bond>;
    /// Settle a bond against the realized fill.
    async fn settle(&self, bond: &Bond, fill: &Fill) -> Result<BondOutcome>;
}

/// A reference engine that escrows a zero bond and settles purely on whether the
/// fill met the guaranteed minimum output. Useful for tests and for callers who
/// want the routing/intent surface without live x402 settlement.
pub struct NoBondEngine;

#[async_trait]
impl BondEngine for NoBondEngine {
    async fn escrow(&self, solution: &Solution) -> Result<Bond> {
        Ok(Bond {
            intent_digest: solution.intent_digest.clone(),
            amount: Usdc(0),
            min_out_raw: solution.route.expected_out.raw,
            deadline_unix: 0,
        })
    }

    async fn settle(&self, bond: &Bond, fill: &Fill) -> Result<BondOutcome> {
        // The swap fill judges the bond's promise — the generic seam, specialized.
        Ok(fill.evaluate(&bond.promise()))
    }
}

/// Configuration for [`EscrowBondEngine`].
#[derive(Clone, Debug)]
pub struct BondConfig {
    /// Bond escrowed at full conviction (`1.0`); scaled linearly by conviction.
    pub max_bond: Usdc,
    /// Inference fee charged at full conviction; scaled by conviction.
    pub max_fee: Usdc,
    /// Haircut applied to a solution's expected output to set the guaranteed
    /// minimum, in basis points. e.g. `100` guarantees 99% of expected output.
    pub guarantee_haircut_bps: u32,
}

impl Default for BondConfig {
    fn default() -> Self {
        Self {
            max_bond: Usdc::from_usdc(5.0),
            max_fee: Usdc::from_usdc(0.05),
            guarantee_haircut_bps: 100,
        }
    }
}

/// Running tally of bond outcomes — the raw material the reputation oracle sells.
/// Honored/slashed history is far harder to fake than self-reported scores.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct BondLedger {
    pub honored: u32,
    pub slashed: u32,
}

impl BondLedger {
    pub fn total(&self) -> u32 {
        self.honored + self.slashed
    }

    /// Fraction of bonds honored, in `[0, 1]`. Defaults to `1.0` with no history.
    pub fn honor_rate(&self) -> f64 {
        if self.total() == 0 {
            1.0
        } else {
            self.honored as f64 / self.total() as f64
        }
    }
}

/// A conviction-weighted, in-process bond engine.
///
/// Sizes each bond and fee by the policy's [`Conviction`], guarantees a haircut
/// off the expected output, escrows the bond internally, and records
/// honored/slashed outcomes into a [`BondLedger`]. This is the real Conviction
/// Routing settlement logic minus the on-chain x402 transfer — the `scematica`
/// feature's x402 engine adds the actual USDC movement on top of this state
/// machine.
pub struct EscrowBondEngine {
    config: BondConfig,
    escrowed: Mutex<HashMap<String, Bond>>,
    ledger: Mutex<BondLedger>,
}

impl EscrowBondEngine {
    pub fn new(config: BondConfig) -> Self {
        Self {
            config,
            escrowed: Mutex::new(HashMap::new()),
            ledger: Mutex::new(BondLedger::default()),
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(BondConfig::default())
    }

    /// The fee a buyer pays to receive this solution's inference
    /// (conviction-weighted). Primitive A's price tag.
    pub fn quote_fee(&self, conviction: Conviction) -> Usdc {
        Usdc((self.config.max_fee.0 as f64 * conviction.0.clamp(0.0, 1.0)) as u64)
    }

    /// Snapshot of the honored/slashed ledger.
    pub fn ledger(&self) -> BondLedger {
        self.ledger.lock().map(|l| *l).unwrap_or_default()
    }

    /// Number of bonds currently escrowed (awaiting settlement).
    pub fn open_bonds(&self) -> usize {
        self.escrowed.lock().map(|e| e.len()).unwrap_or(0)
    }
}

#[async_trait]
impl BondEngine for EscrowBondEngine {
    async fn escrow(&self, solution: &Solution) -> Result<Bond> {
        let conv = solution.conviction.0.clamp(0.0, 1.0);
        let amount = Usdc((self.config.max_bond.0 as f64 * conv) as u64);
        let expected = solution.route.expected_out.raw;
        let keep_bps = 10_000u64.saturating_sub(self.config.guarantee_haircut_bps as u64);
        let min_out_raw = expected.saturating_mul(keep_bps) / 10_000;
        let bond = Bond {
            intent_digest: solution.intent_digest.clone(),
            amount,
            min_out_raw,
            deadline_unix: 0,
        };
        self.escrowed
            .lock()
            .map_err(|_| ScemaDexError::Bond("escrow lock poisoned".into()))?
            .insert(bond.intent_digest.clone(), bond.clone());
        Ok(bond)
    }

    async fn settle(&self, bond: &Bond, fill: &Fill) -> Result<BondOutcome> {
        self.escrowed
            .lock()
            .map_err(|_| ScemaDexError::Bond("escrow lock poisoned".into()))?
            .remove(&bond.intent_digest);
        let outcome = fill.evaluate(&bond.promise());
        let mut ledger = self
            .ledger
            .lock()
            .map_err(|_| ScemaDexError::Bond("ledger lock poisoned".into()))?;
        match outcome {
            BondOutcome::Honored => ledger.honored += 1,
            BondOutcome::Slashed => ledger.slashed += 1,
        }
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::Conviction;
    use crate::primitives::Amount;
    use crate::route::{Route, RouteLeg, Venue};

    fn solution(expected_raw: u64, conviction: f64) -> Solution {
        let leg = RouteLeg {
            venue: Venue::Jupiter,
            input_mint: crate::primitives::Address::new(
                "So11111111111111111111111111111111111111112",
            )
            .unwrap(),
            output_mint: crate::primitives::Address::new(
                "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
            )
            .unwrap(),
            split_bps: 10_000,
            expected_out: Amount::new(expected_raw, 6),
        };
        Solution {
            intent_digest: "deadbeef".into(),
            route: Route {
                legs: vec![leg],
                expected_out: Amount::new(expected_raw, 6),
            },
            conviction: Conviction::clamped(conviction),
            rationale: "test".into(),
        }
    }

    #[tokio::test]
    async fn bond_scales_with_conviction() {
        let engine = EscrowBondEngine::with_defaults();
        let low = engine.escrow(&solution(1_000_000, 0.2)).await.unwrap();
        let high = engine.escrow(&solution(1_000_000, 0.9)).await.unwrap();
        assert!(high.amount.0 > low.amount.0);
        assert_eq!(engine.open_bonds(), 1); // same digest overwrites
    }

    #[tokio::test]
    async fn honored_when_fill_meets_guarantee() {
        let engine = EscrowBondEngine::with_defaults();
        let sol = solution(1_000_000, 0.8);
        let bond = engine.escrow(&sol).await.unwrap();
        // 100 bps haircut → guarantee 990_000; a 1_000_000 fill honors.
        let fill = Fill {
            amount_out: Amount::new(1_000_000, 6),
            executed_unix: 0,
        };
        assert_eq!(
            engine.settle(&bond, &fill).await.unwrap(),
            BondOutcome::Honored
        );
        assert_eq!(engine.ledger().honored, 1);
        assert_eq!(engine.open_bonds(), 0);
    }

    #[tokio::test]
    async fn slashed_when_fill_misses_guarantee() {
        let engine = EscrowBondEngine::with_defaults();
        let sol = solution(1_000_000, 0.8);
        let bond = engine.escrow(&sol).await.unwrap();
        let fill = Fill {
            amount_out: Amount::new(500_000, 6),
            executed_unix: 0,
        };
        assert_eq!(
            engine.settle(&bond, &fill).await.unwrap(),
            BondOutcome::Slashed
        );
        assert_eq!(engine.ledger().slashed, 1);
        assert_eq!(engine.ledger().honor_rate(), 0.0);
    }
}
