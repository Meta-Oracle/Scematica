//! **The Counter-Market** — adversarial conviction staking (Primitive E).
//!
//! Conviction Routing (Primitive D) is unilateral: an agent self-assesses its
//! confidence, escrows a bond, and the caller waits for settlement. The
//! counter-market adds the missing half: **any third agent can stake against an
//! open bond** before it settles.
//!
//! - The bond **honors** → every challenger forfeits its stake to the inferring
//!   agent, which earned a premium for being doubted and right.
//! - The bond **slashes** → each challenger reclaims its stake plus a pro-rata
//!   share of the slashed bond.
//!
//! Three things fall out that exist nowhere else:
//!
//! 1. **Conviction becomes market-discovered.** The policy's self-reported
//!    [`Conviction`] meets a real price: the [`CounterMarket::market_conviction`]
//!    implied by challenger stake. Their difference — the
//!    [`CounterMarket::doubt_spread`] — is a brand-new signal: how much more (or
//!    less) the mesh believes in this inference than the agent itself does.
//! 2. **The mesh becomes self-policing.** Hunting overconfident peers is a
//!    profitable activity, so auditing is an economy, not a governance process.
//! 3. **Per-decision prediction markets.** Prediction markets price *events*;
//!    the counter-market prices *individual machine inferences*.
//!
//! The book is engine-agnostic: list any [`Bond`] (from any
//! [`crate::bond::BondEngine`]) and feed the eventual [`BondOutcome`] into
//! [`CounterMarket::settle`]. Run `cargo run -p scemadex-sdk --example
//! counter_market` for the full loop.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::bond::{Bond, BondOutcome};
use crate::error::{Result, ScemaDexError};
use crate::policy::Conviction;
use crate::primitives::Usdc;

/// A stake posted *against* an open bond: the challenger is betting the
/// inference will be slashed.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Challenge {
    pub challenger_id: String,
    pub stake: Usdc,
    /// Unix second the challenge was opened (`0` = unset in the scaffold).
    pub opened_unix: u64,
}

/// The resolved economics of one bond's challenges.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChallengeSettlement {
    pub intent_digest: String,
    pub outcome: BondOutcome,
    /// `(challenger_id, payout)` — non-empty only when the bond slashed: each
    /// challenger receives its stake back plus a stake-weighted share of the
    /// slashed bond.
    pub challenger_payouts: Vec<(String, Usdc)>,
    /// Forfeited challenger stakes paid to the inferring agent — non-zero only
    /// when the bond honored. The premium for being doubted and right.
    pub agent_premium: Usdc,
}

/// Aggregate counter-market history — the adversarial complement to
/// [`crate::bond::BondLedger`].
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct CounterStats {
    /// Challenged bonds settled.
    pub settled: u32,
    /// Settlements where the challengers were right (bond slashed).
    pub challenger_wins: u32,
    /// Settlements where the agent was right (bond honored).
    pub agent_wins: u32,
    /// Cumulative premiums paid to agents from forfeited stakes (micro-USDC).
    pub agent_premiums: u64,
    /// Cumulative slashed collateral paid out to challengers (micro-USDC).
    pub challenger_winnings: u64,
}

struct OpenChallenge {
    bond: Bond,
    self_conviction: Conviction,
    challenges: Vec<Challenge>,
}

/// An in-process challenge book over open bonds.
///
/// Lifecycle: [`list`](Self::list) a freshly escrowed bond →
/// [`challenge`](Self::challenge) it any number of times →
/// [`settle`](Self::settle) with the bond's realized [`BondOutcome`]. Between
/// listing and settlement, [`market_conviction`](Self::market_conviction) and
/// [`doubt_spread`](Self::doubt_spread) expose the live market view — feed the
/// spread into your policy's state vector and the agent learns from how much
/// the market disbelieves it.
#[derive(Default)]
pub struct CounterMarket {
    book: Mutex<HashMap<String, OpenChallenge>>,
    stats: Mutex<CounterStats>,
}

impl CounterMarket {
    pub fn new() -> Self {
        Self::default()
    }

    /// List an open bond as challengeable, alongside the policy's self-assessed
    /// conviction it will later be compared against.
    pub fn list(&self, bond: &Bond, self_conviction: Conviction) -> Result<()> {
        self.book
            .lock()
            .map_err(|_| ScemaDexError::Mesh("counter book lock poisoned".into()))?
            .insert(
                bond.intent_digest.clone(),
                OpenChallenge {
                    bond: bond.clone(),
                    self_conviction,
                    challenges: Vec::new(),
                },
            );
        Ok(())
    }

    /// Stake against a listed bond. Fails if the bond isn't listed or the stake
    /// is zero — doubt must cost something to be a signal.
    pub fn challenge(
        &self,
        intent_digest: &str,
        challenger_id: impl Into<String>,
        stake: Usdc,
    ) -> Result<Challenge> {
        if stake.0 == 0 {
            return Err(ScemaDexError::Mesh("challenge stake must be non-zero".into()));
        }
        let mut book = self
            .book
            .lock()
            .map_err(|_| ScemaDexError::Mesh("counter book lock poisoned".into()))?;
        let entry = book.get_mut(intent_digest).ok_or_else(|| {
            ScemaDexError::Mesh(format!("no listed bond for intent {intent_digest}"))
        })?;
        let challenge = Challenge {
            challenger_id: challenger_id.into(),
            stake,
            opened_unix: 0,
        };
        entry.challenges.push(challenge.clone());
        Ok(challenge)
    }

    /// Total challenger stake currently posted against a listed bond.
    pub fn total_stake(&self, intent_digest: &str) -> Usdc {
        self.book
            .lock()
            .ok()
            .and_then(|b| {
                b.get(intent_digest)
                    .map(|e| Usdc(e.challenges.iter().map(|c| c.stake.0).sum()))
            })
            .unwrap_or(Usdc(0))
    }

    /// The conviction *implied by the market*: the agent's collateral weighed
    /// against everyone betting it will fail, in `[0, 1]`.
    ///
    /// `bond / (bond + total_challenger_stake)` — an unchallenged bond implies
    /// `1.0` (nobody paid to doubt it); mounting stake drives it toward `0`.
    pub fn market_conviction(&self, intent_digest: &str) -> Option<f64> {
        let book = self.book.lock().ok()?;
        let entry = book.get(intent_digest)?;
        let bond = entry.bond.amount.0 as f64;
        let stake: u64 = entry.challenges.iter().map(|c| c.stake.0).sum();
        if bond == 0.0 && stake == 0 {
            return Some(1.0);
        }
        Some(bond / (bond + stake as f64))
    }

    /// `self_conviction − market_conviction`, in `[-1, 1]`.
    ///
    /// Positive → the market doubts the agent more than it doubts itself
    /// (an overconfidence signal); negative → the market is *more* confident
    /// than the agent. This is the feature to append to a policy's state vector.
    pub fn doubt_spread(&self, intent_digest: &str) -> Option<f64> {
        let market = self.market_conviction(intent_digest)?;
        let own = self
            .book
            .lock()
            .ok()?
            .get(intent_digest)?
            .self_conviction
            .0;
        Some(own - market)
    }

    /// Settle every challenge on a bond against its realized outcome and close
    /// the book entry. Total value is conserved: on a slash, stakes plus the
    /// full bond flow to challengers; on an honor, stakes flow to the agent.
    pub fn settle(&self, intent_digest: &str, outcome: BondOutcome) -> Result<ChallengeSettlement> {
        let entry = self
            .book
            .lock()
            .map_err(|_| ScemaDexError::Mesh("counter book lock poisoned".into()))?
            .remove(intent_digest)
            .ok_or_else(|| {
                ScemaDexError::Mesh(format!("no listed bond for intent {intent_digest}"))
            })?;

        let total_stake: u64 = entry.challenges.iter().map(|c| c.stake.0).sum();
        let mut settlement = ChallengeSettlement {
            intent_digest: intent_digest.to_string(),
            outcome,
            challenger_payouts: Vec::new(),
            agent_premium: Usdc(0),
        };

        match outcome {
            BondOutcome::Honored => {
                settlement.agent_premium = Usdc(total_stake);
            }
            BondOutcome::Slashed => {
                let bond = entry.bond.amount.0;
                let n = entry.challenges.len() as u64;
                let mut distributed = 0u64;
                for (i, c) in entry.challenges.iter().enumerate() {
                    // Stake-weighted share of the slashed bond; the last
                    // challenger absorbs integer-division dust so the full bond
                    // is always paid out.
                    let share = if i as u64 + 1 == n {
                        bond - distributed
                    } else {
                        bond.saturating_mul(c.stake.0)
                            .checked_div(total_stake)
                            .unwrap_or(0)
                    };
                    distributed += share;
                    settlement
                        .challenger_payouts
                        .push((c.challenger_id.clone(), Usdc(c.stake.0 + share)));
                }
            }
        }

        if !entry.challenges.is_empty() {
            let mut stats = self
                .stats
                .lock()
                .map_err(|_| ScemaDexError::Mesh("counter stats lock poisoned".into()))?;
            stats.settled += 1;
            match outcome {
                BondOutcome::Honored => {
                    stats.agent_wins += 1;
                    stats.agent_premiums += settlement.agent_premium.0;
                }
                BondOutcome::Slashed => {
                    stats.challenger_wins += 1;
                    stats.challenger_winnings += settlement
                        .challenger_payouts
                        .iter()
                        .map(|(_, p)| p.0)
                        .sum::<u64>();
                }
            }
        }

        Ok(settlement)
    }

    /// Snapshot of the aggregate counter-market history.
    pub fn stats(&self) -> CounterStats {
        self.stats.lock().map(|s| *s).unwrap_or_default()
    }

    /// Number of bonds currently listed (open to challenge).
    pub fn open_listings(&self) -> usize {
        self.book.lock().map(|b| b.len()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bond(digest: &str, amount: u64) -> Bond {
        Bond {
            intent_digest: digest.into(),
            amount: Usdc(amount),
            min_out_raw: 1_000_000,
            deadline_unix: 0,
        }
    }

    #[test]
    fn challenge_requires_listed_bond() {
        let market = CounterMarket::new();
        assert!(market.challenge("missing", "doubter", Usdc(100)).is_err());
        market.list(&bond("d1", 1_000), Conviction::clamped(0.8)).unwrap();
        assert!(market.challenge("d1", "doubter", Usdc(0)).is_err());
        assert!(market.challenge("d1", "doubter", Usdc(100)).is_ok());
    }

    #[test]
    fn unchallenged_bond_implies_full_market_conviction() {
        let market = CounterMarket::new();
        market.list(&bond("d1", 1_000), Conviction::clamped(0.6)).unwrap();
        assert_eq!(market.market_conviction("d1"), Some(1.0));
        // Agent doubts itself more than the (silent) market does.
        assert!(market.doubt_spread("d1").unwrap() < 0.0);
    }

    #[test]
    fn stake_drives_market_conviction_down() {
        let market = CounterMarket::new();
        market.list(&bond("d1", 1_000), Conviction::clamped(0.9)).unwrap();
        market.challenge("d1", "a", Usdc(1_000)).unwrap();
        // Equal stake vs bond → market conviction 0.5.
        assert_eq!(market.market_conviction("d1"), Some(0.5));
        // Self 0.9 vs market 0.5 → overconfidence spread +0.4.
        assert!((market.doubt_spread("d1").unwrap() - 0.4).abs() < 1e-9);
        market.challenge("d1", "b", Usdc(3_000)).unwrap();
        assert_eq!(market.market_conviction("d1"), Some(0.2));
    }

    #[test]
    fn honored_pays_agent_the_forfeited_stakes() {
        let market = CounterMarket::new();
        market.list(&bond("d1", 1_000), Conviction::clamped(0.9)).unwrap();
        market.challenge("d1", "a", Usdc(300)).unwrap();
        market.challenge("d1", "b", Usdc(700)).unwrap();
        let s = market.settle("d1", BondOutcome::Honored).unwrap();
        assert_eq!(s.agent_premium, Usdc(1_000));
        assert!(s.challenger_payouts.is_empty());
        assert_eq!(market.stats().agent_wins, 1);
        assert_eq!(market.open_listings(), 0);
    }

    #[test]
    fn slashed_pays_challengers_stake_plus_prorata_bond() {
        let market = CounterMarket::new();
        market.list(&bond("d1", 1_000), Conviction::clamped(0.9)).unwrap();
        market.challenge("d1", "a", Usdc(250)).unwrap();
        market.challenge("d1", "b", Usdc(750)).unwrap();
        let s = market.settle("d1", BondOutcome::Slashed).unwrap();
        assert_eq!(s.agent_premium, Usdc(0));
        // a: 250 stake + 250 bond share; b: 750 stake + 750 bond share.
        assert_eq!(s.challenger_payouts[0], ("a".into(), Usdc(500)));
        assert_eq!(s.challenger_payouts[1], ("b".into(), Usdc(1_500)));
        // Conservation: stakes (1_000) + bond (1_000) fully distributed.
        let total: u64 = s.challenger_payouts.iter().map(|(_, p)| p.0).sum();
        assert_eq!(total, 2_000);
        assert_eq!(market.stats().challenger_wins, 1);
    }

    #[test]
    fn dust_from_integer_division_goes_to_last_challenger() {
        let market = CounterMarket::new();
        market.list(&bond("d1", 1_000), Conviction::clamped(0.5)).unwrap();
        market.challenge("d1", "a", Usdc(1)).unwrap();
        market.challenge("d1", "b", Usdc(1)).unwrap();
        market.challenge("d1", "c", Usdc(1)).unwrap();
        let s = market.settle("d1", BondOutcome::Slashed).unwrap();
        let total: u64 = s.challenger_payouts.iter().map(|(_, p)| p.0).sum();
        // 3 stakes + the full 1_000 bond, no lamport lost to rounding.
        assert_eq!(total, 1_003);
    }

    #[test]
    fn settling_unchallenged_bond_records_no_stats() {
        let market = CounterMarket::new();
        market.list(&bond("d1", 1_000), Conviction::clamped(0.5)).unwrap();
        let s = market.settle("d1", BondOutcome::Honored).unwrap();
        assert_eq!(s.agent_premium, Usdc(0));
        assert_eq!(market.stats().settled, 0);
    }
}
