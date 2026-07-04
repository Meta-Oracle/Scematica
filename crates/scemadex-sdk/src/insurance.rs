//! **Bond insurance / reinsurance** — the hedge side of Conviction Routing (Primitive J).
//!
//! The Counter-Market (E) is the *speculation* side of a bond: stake that it will
//! fail. Insurance is the missing *hedge* side: an underwriter accepts a **premium**
//! up front and, if the bond slashes, pays the insured agent **coverage** from a
//! pooled balance. Three things fall out:
//!
//! 1. **Capital efficiency for agents.** Instead of over-collateralizing, an agent
//!    posts its bond and buys coverage — turning a lumpy tail loss into a small,
//!    predictable premium.
//! 2. **A yield product for passive USDC.** Underwriting honest agents earns
//!    premiums; the pool is a market-making book on *machine trustworthiness*.
//! 3. **A sink for the slash.** [`crate::settlement::SlashRouting::to_insurance_bps`]
//!    routes a slice of every slash into the pool, so the system partially
//!    reinsures itself — the losses recapitalize the fund that covers them.
//!
//! Premiums are priced off the **reputation oracle** (Primitive C): a high honor-rate
//! agent with a long track record pays far less than an unproven one. New agents
//! (few [`Reputation::samples`]) earn little of the trust discount, so the pool
//! isn't gamed by fresh identities.
//!
//! The pool is engine-agnostic: feed it the [`crate::bond::BondOutcome`] a bond
//! finalized with (from any settlement path) and, on a slash, the insurance slice
//! from the [`crate::settlement::SlashDistribution`]. See
//! `cargo run -p scemadex-sdk --example insurance`.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::bond::BondOutcome;
use crate::error::{Result, ScemaDexError};
use crate::oracle::Reputation;
use crate::primitives::Usdc;

/// How premiums are priced from coverage and reputation.
#[derive(Clone, Copy, Debug)]
pub struct PremiumConfig {
    /// Premium as basis points of coverage at *maximum* risk (an untrusted agent).
    pub base_bps: u32,
    /// Floor multiplier a fully-trusted agent pays, in bps of `base_bps`
    /// (e.g. `2500` → a trusted agent pays 25% of the base rate).
    pub min_factor_bps: u32,
    /// Samples at which an agent has earned half of the trust discount. Guards the
    /// pool against fresh-identity gaming (few samples → little discount).
    pub confidence_k: u32,
}

impl Default for PremiumConfig {
    fn default() -> Self {
        Self {
            base_bps: 500,       // 5% of coverage at max risk
            min_factor_bps: 2_500, // trusted agents pay 25% of that → 1.25%
            confidence_k: 10,
        }
    }
}

impl PremiumConfig {
    /// Premium for `coverage` given the insured's `reputation`. Monotonically
    /// decreasing in both reputation score and sample count.
    pub fn premium(&self, coverage: Usdc, reputation: Reputation) -> Usdc {
        let score = reputation.score.clamp(0.0, 1.0);
        let samples = reputation.samples as f64;
        let confidence = samples / (samples + self.confidence_k as f64); // 0..1
        let earned_trust = score * confidence; // trust the agent has actually earned
        let min_factor = self.min_factor_bps as f64 / 10_000.0;
        let factor = min_factor + (1.0 - min_factor) * (1.0 - earned_trust); // [min_factor, 1]
        let premium = coverage.0 as f64 * (self.base_bps as f64 / 10_000.0) * factor;
        Usdc(premium as u64)
    }
}

/// A quoted premium for a requested coverage — what an agent inspects before binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InsuranceTerms {
    pub coverage: Usdc,
    pub premium: Usdc,
}

/// A bound insurance policy against a specific bond.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Policy {
    pub intent_digest: String,
    pub insured_id: String,
    pub coverage: Usdc,
    pub premium: Usdc,
}

/// The result of settling a policy at bond finalization.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Payout {
    pub intent_digest: String,
    pub insured_id: String,
    pub outcome: BondOutcome,
    /// Coverage paid to the insured on a slash (capped at pool capital); `0` on honor.
    pub paid: Usdc,
    /// Premium the pool keeps as income — always the policy premium.
    pub premium: Usdc,
    /// True when the pool was undercapitalized and paid less than full coverage.
    pub shortfall: bool,
}

struct PoolState {
    capital: u64,
    policies: HashMap<String, Policy>,
}

/// An in-process reinsurance pool: prices premiums, binds policies, banks premiums
/// and slash slices, and pays coverage on slashes.
///
/// Solvency is enforced at bind time — a policy is only written if the pool can
/// back its full coverage — so a single policy always pays in full. Under a burst
/// of correlated slashes the pool can still run short; [`Payout::shortfall`] flags
/// a capped payout rather than failing.
pub struct InsurancePool {
    config: PremiumConfig,
    state: Mutex<PoolState>,
}

impl InsurancePool {
    /// An empty pool with the given pricing.
    pub fn new(config: PremiumConfig) -> Self {
        Self::with_capital(config, Usdc(0))
    }

    /// A pool seeded with underwriting capital (LP deposits).
    pub fn with_capital(config: PremiumConfig, seed: Usdc) -> Self {
        Self {
            config,
            state: Mutex::new(PoolState {
                capital: seed.0,
                policies: HashMap::new(),
            }),
        }
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, PoolState>> {
        self.state
            .lock()
            .map_err(|_| ScemaDexError::Bond("insurance pool lock poisoned".into()))
    }

    /// Current pool capital.
    pub fn capital(&self) -> Usdc {
        self.lock().map(|s| Usdc(s.capital)).unwrap_or(Usdc(0))
    }

    /// Policies currently in force.
    pub fn active_policies(&self) -> usize {
        self.lock().map(|s| s.policies.len()).unwrap_or(0)
    }

    /// Quote coverage for an agent without binding.
    pub fn quote(&self, coverage: Usdc, reputation: Reputation) -> InsuranceTerms {
        InsuranceTerms {
            coverage,
            premium: self.config.premium(coverage, reputation),
        }
    }

    /// Add capital to the pool — an LP top-up, or the insurance slice of a slash.
    /// Returns the new capital.
    pub fn deposit(&self, amount: Usdc) -> Result<Usdc> {
        let mut st = self.lock()?;
        st.capital = st.capital.saturating_add(amount.0);
        Ok(Usdc(st.capital))
    }

    /// Bind a policy: quote the premium, bank it, and record the coverage. Fails if
    /// the pool (after the premium) cannot back the full coverage.
    pub fn bind(
        &self,
        intent_digest: impl Into<String>,
        insured_id: impl Into<String>,
        coverage: Usdc,
        reputation: Reputation,
    ) -> Result<Policy> {
        let premium = self.config.premium(coverage, reputation);
        let mut st = self.lock()?;
        if st.capital.saturating_add(premium.0) < coverage.0 {
            return Err(ScemaDexError::Bond(
                "insurance pool undercapitalized to bind this coverage".into(),
            ));
        }
        st.capital = st.capital.saturating_add(premium.0);
        let policy = Policy {
            intent_digest: intent_digest.into(),
            insured_id: insured_id.into(),
            coverage,
            premium,
        };
        st.policies.insert(policy.intent_digest.clone(), policy.clone());
        Ok(policy)
    }

    /// Settle a policy against the bond's finalized outcome. On honor the pool keeps
    /// the premium and pays nothing; on a slash it pays coverage (capped at capital).
    /// Returns `None` if no policy was bound for `digest`.
    pub fn settle(&self, intent_digest: &str, outcome: BondOutcome) -> Result<Option<Payout>> {
        let mut st = self.lock()?;
        let policy = match st.policies.remove(intent_digest) {
            Some(p) => p,
            None => return Ok(None),
        };
        let (paid, shortfall) = match outcome {
            BondOutcome::Honored => (0u64, false),
            BondOutcome::Slashed => {
                let pay = policy.coverage.0.min(st.capital);
                st.capital -= pay;
                (pay, pay < policy.coverage.0)
            }
        };
        Ok(Some(Payout {
            intent_digest: policy.intent_digest,
            insured_id: policy.insured_id,
            outcome,
            paid: Usdc(paid),
            premium: policy.premium,
            shortfall,
        }))
    }

    /// Convenience for the settlement path: bank the slash's insurance slice (if
    /// any) into the pool, then settle this bond's policy. `insurance_slice` is the
    /// `insurance` field of the [`crate::settlement::SlashDistribution`]; pass
    /// `Usdc(0)` on an honor.
    pub fn on_settlement(
        &self,
        intent_digest: &str,
        outcome: BondOutcome,
        insurance_slice: Usdc,
    ) -> Result<Option<Payout>> {
        if outcome == BondOutcome::Slashed && insurance_slice.0 > 0 {
            self.deposit(insurance_slice)?;
        }
        self.settle(intent_digest, outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rep(score: f64, samples: u32) -> Reputation {
        Reputation { score, samples }
    }

    #[test]
    fn premium_falls_with_reputation_and_track_record() {
        let cfg = PremiumConfig::default();
        let cov = Usdc(1_000_000);
        let unproven = cfg.premium(cov, rep(0.0, 0));
        let trusted = cfg.premium(cov, rep(1.0, 1_000));
        let trusted_but_new = cfg.premium(cov, rep(1.0, 1));
        assert!(trusted < unproven, "a trusted, proven agent pays less");
        assert!(
            trusted_but_new > trusted,
            "few samples earn little of the discount"
        );
        // At max risk the premium is the full base rate (5% of coverage).
        assert_eq!(unproven, Usdc(50_000));
    }

    #[test]
    fn bind_requires_the_pool_to_back_the_coverage() {
        let pool = InsurancePool::new(PremiumConfig::default());
        // Empty pool can't back 1 USDC of coverage (premium alone won't cover it).
        assert!(pool.bind("d", "agent", Usdc(1_000_000), rep(0.5, 50)).is_err());

        let pool = InsurancePool::with_capital(PremiumConfig::default(), Usdc(2_000_000));
        let policy = pool.bind("d", "agent", Usdc(1_000_000), rep(0.5, 50)).unwrap();
        // Premium was banked into capital.
        assert_eq!(pool.capital(), Usdc(2_000_000 + policy.premium.0));
        assert_eq!(pool.active_policies(), 1);
    }

    #[test]
    fn honored_bond_keeps_the_premium_pays_nothing() {
        let pool = InsurancePool::with_capital(PremiumConfig::default(), Usdc(2_000_000));
        let policy = pool.bind("d", "agent", Usdc(1_000_000), rep(0.8, 100)).unwrap();
        let before = pool.capital();
        let payout = pool.settle("d", BondOutcome::Honored).unwrap().unwrap();
        assert_eq!(payout.paid, Usdc(0));
        assert_eq!(payout.premium, policy.premium);
        assert_eq!(pool.capital(), before, "premium retained, nothing paid");
        assert_eq!(pool.active_policies(), 0);
    }

    #[test]
    fn slashed_bond_pays_coverage_from_the_pool() {
        let pool = InsurancePool::with_capital(PremiumConfig::default(), Usdc(2_000_000));
        pool.bind("d", "agent", Usdc(1_000_000), rep(0.8, 100)).unwrap();
        let before = pool.capital();
        let payout = pool.settle("d", BondOutcome::Slashed).unwrap().unwrap();
        assert_eq!(payout.paid, Usdc(1_000_000));
        assert!(!payout.shortfall);
        assert_eq!(pool.capital(), Usdc(before.0 - 1_000_000));
    }

    #[test]
    fn correlated_slashes_can_cap_a_payout() {
        // Seed exactly one coverage unit; both binds pass solvency, but paying the
        // first claim leaves the second short.
        let cov = Usdc(1_000_000);
        let pool = InsurancePool::with_capital(PremiumConfig::default(), cov);
        pool.bind("a", "agent", cov, rep(0.5, 50)).unwrap();
        pool.bind("b", "agent", cov, rep(0.5, 50)).unwrap();
        let first = pool.settle("a", BondOutcome::Slashed).unwrap().unwrap();
        assert_eq!(first.paid, cov);
        assert!(!first.shortfall);
        let second = pool.settle("b", BondOutcome::Slashed).unwrap().unwrap();
        assert!(second.paid < cov, "only the remaining capital is paid");
        assert!(second.shortfall);
    }

    #[test]
    fn on_settlement_banks_the_slash_slice_before_paying() {
        let pool = InsurancePool::with_capital(PremiumConfig::default(), Usdc(1_000_000));
        pool.bind("d", "agent", Usdc(1_000_000), rep(0.9, 200)).unwrap();
        let cap_after_bind = pool.capital();
        // A slash routes 300k of insurance slice into the pool, then pays coverage.
        let payout = pool
            .on_settlement("d", BondOutcome::Slashed, Usdc(300_000))
            .unwrap()
            .unwrap();
        assert_eq!(payout.paid, Usdc(1_000_000));
        assert_eq!(pool.capital(), Usdc(cap_after_bind.0 + 300_000 - 1_000_000));
    }

    #[test]
    fn settle_without_a_policy_is_none() {
        let pool = InsurancePool::new(PremiumConfig::default());
        assert!(pool.settle("missing", BondOutcome::Slashed).unwrap().is_none());
    }
}
