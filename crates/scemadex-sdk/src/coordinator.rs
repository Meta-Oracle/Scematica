//! **Dispute coordination** — wiring the adversarial primitives into finality.
//!
//! [`crate::settlement::SettlementMachine`] is an outcome-agnostic spine; it knows
//! *when* a bond finalizes but nothing about the markets that resolve a dispute.
//! [`crate::counter::CounterMarket`] prices doubt but, on its own, only bets on an
//! outcome someone else already decided. This module fuses them so a challenge
//! becomes **load-bearing**: staking against an open bond moves it into
//! `Disputed`, and the adjudicated truth *flips* finality — challengers are no
//! longer spectators, they are the fraud-proof mechanism.
//!
//! It also closes the loop to the **Scar Market** (Primitive F): a bond that
//! finalizes `Slashed` *by an upheld challenge* is the cleanest possible trigger
//! for [`crate::scar::certify_scar`] — the loss is adversarially proven, not
//! self-reported. [`DisputeCoordinator::mint_scar`] enforces exactly that.
//!
//! ## The slash-slice reconciliation
//!
//! A slash is split four ways by [`crate::settlement::SlashRouting`]
//! (caller / challengers / insurance / lineage). The counter-market must arbitrate
//! **only the challenger slice**, or challengers would be paid the whole bond
//! twice over. So the coordinator lists a *synthetic* bond in the counter-market
//! whose amount equals the challenger reward pool; per-challenger payouts then sum
//! to exactly that slice, while the caller/insurance/lineage slices come from the
//! machine's [`SlashDistribution`]. The scar, by contrast, is certified against
//! the *full* original collateral — that is the real pain the agent took.
//!
//! ## Typical flow
//!
//! ```text
//! open(bond, self_conviction)               // Escrowed + listed as challengeable
//!   → provision(digest, outcome_from_fill)   // Provisional, window opens
//!   → challenge(digest, "skeptic", stake)    // → Disputed
//!   → resolve(digest, adjudicated_truth)     // Finalized; counter-market pays out
//!   → mint_scar(digest, …)                   // only if the slash was adversarial
//! ```
//! Unchallenged bonds finalize via [`finalize_unchallenged`](DisputeCoordinator::finalize_unchallenged)
//! or in bulk via [`sweep`](DisputeCoordinator::sweep).

use crate::bond::{Bond, BondOutcome};
use crate::counter::{Challenge, ChallengeSettlement, CounterMarket};
use crate::error::{Result, ScemaDexError};
use crate::policy::Conviction;
use crate::primitives::Usdc;
use crate::scar::{certify_scar, ScarRecord};
use crate::settlement::{
    BondState, Clock, FinalizeReason, SettlementConfig, SettlementMachine, SlashDistribution,
    SystemClock,
};

/// The combined result of finalizing one bond across the settlement machine and
/// the counter-market.
#[derive(Clone, Debug)]
pub struct SettlementReport {
    pub intent_digest: String,
    pub outcome: BondOutcome,
    pub reason: FinalizeReason,
    /// The four-way split of the slashed collateral — `Some` only on a slash.
    pub slash: Option<SlashDistribution>,
    /// The counter-market payout for this bond — `Some` when it was listed (always,
    /// in the coordinator), carrying per-challenger payouts or the agent premium.
    pub challenge: Option<ChallengeSettlement>,
}

impl SettlementReport {
    /// True when the bond slashed *because a challenge was upheld* — the only case
    /// that yields a scar-eligible, adversarially-proven loss.
    pub fn is_adversarial_slash(&self) -> bool {
        self.outcome == BondOutcome::Slashed && self.reason == FinalizeReason::ChallengeUpheld
    }
}

/// Composes a [`SettlementMachine`] with a [`CounterMarket`] so challenges resolve
/// finality and adversarial slashes feed the Scar Market.
///
/// Intended for optimistic configs (a non-zero dispute window). An atomic config
/// (`dispute_window_secs == 0`) still works — `provision` finalizes immediately and
/// `finalize_unchallenged` settles the counter-market and reports — but there is no
/// window in which to challenge.
pub struct DisputeCoordinator<C: Clock = SystemClock> {
    machine: SettlementMachine<C>,
    counter: CounterMarket,
}

impl DisputeCoordinator<SystemClock> {
    /// A coordinator driven by the wall clock.
    pub fn system(config: SettlementConfig) -> Self {
        Self::new(config, SystemClock)
    }
}

impl<C: Clock> DisputeCoordinator<C> {
    pub fn new(config: SettlementConfig, clock: C) -> Self {
        Self {
            machine: SettlementMachine::new(config, clock),
            counter: CounterMarket::new(),
        }
    }

    /// The underlying settlement machine (state queries, slash distributions).
    pub fn machine(&self) -> &SettlementMachine<C> {
        &self.machine
    }

    /// The underlying counter-market (doubt spread, market conviction, stats).
    pub fn counter(&self) -> &CounterMarket {
        &self.counter
    }

    /// Track a freshly escrowed bond and list it as challengeable, tagged with the
    /// policy's self-assessed conviction (the baseline the market prices against).
    pub fn open(&self, bond: &Bond, self_conviction: Conviction) -> Result<()> {
        self.machine.open(bond)?;
        // List a synthetic bond carrying only the challenger reward pool so the
        // counter-market never pays out more than the routed slice.
        let challenger_pool = self
            .machine
            .config()
            .slash_routing
            .distribute(bond.amount)
            .challengers;
        let listed = Bond {
            amount: challenger_pool,
            ..bond.clone()
        };
        self.counter.list(&listed, self_conviction)
    }

    /// Record a fill's provisional outcome and open the dispute window.
    pub fn provision(&self, digest: &str, provisional: BondOutcome) -> Result<BondState> {
        self.machine.provision(digest, provisional)
    }

    /// Stake against a bond. The first challenge moves the bond into `Disputed`
    /// (subject to the machine's window/outcome checks); later challengers pile
    /// into the open book. Returns the recorded [`Challenge`].
    pub fn challenge(
        &self,
        digest: &str,
        challenger_id: impl Into<String>,
        stake: Usdc,
    ) -> Result<Challenge> {
        let id = challenger_id.into();
        match self.machine.state(digest) {
            Some(BondState::Provisional { .. }) => {
                // Enforces the dispute window and rejects challenging a slash.
                self.machine.file_challenge(digest, id.clone())?;
            }
            Some(BondState::Disputed { .. }) => { /* already disputed — pile on */ }
            other => {
                return Err(ScemaDexError::Bond(format!(
                    "bond {digest} is not open to challenge (state {other:?})"
                )))
            }
        }
        self.counter.challenge(digest, id, stake)
    }

    /// Resolve an open dispute with the adjudicated truth (from a re-check, oracle,
    /// or proof). `adjudicated == Slashed` upholds the challenge and flips the bond
    /// to `Slashed`; `Honored` rejects it and the provisional honor stands. Settles
    /// the counter-market and returns the combined report.
    pub fn resolve(&self, digest: &str, adjudicated: BondOutcome) -> Result<SettlementReport> {
        let challenger_won = adjudicated == BondOutcome::Slashed;
        let outcome = self.machine.resolve_dispute(digest, challenger_won)?;
        self.report(digest, outcome)
    }

    /// Finalize a bond that reached maturity without a live dispute (window elapsed,
    /// or an already-atomic-finalized bond), settle its counter-market listing, and
    /// report. Errors if the bond is currently `Disputed` — call [`resolve`](Self::resolve).
    pub fn finalize_unchallenged(&self, digest: &str) -> Result<SettlementReport> {
        let outcome = self.machine.finalize(digest)?;
        self.report(digest, outcome)
    }

    /// Finalize a `Provisional` (or `Disputed`) bond deterministically from an
    /// authoritative external signal — a ground-truth oracle read, or a verified
    /// zkML / re-execution proof (Primitive I). Collapses the dispute window with
    /// reason [`FinalizeReason::ProofVerified`], settles the counter-market against
    /// `outcome`, and reports.
    ///
    /// This is the seam a **forecast** resolves through (ground truth is the oracle)
    /// and the seam a **succinct proof** resolves through (the proof is the oracle) —
    /// either can flip a provisional honor to `Slashed` without waiting out the
    /// window, which is exactly how a real zk backend shrinks `W` toward zero.
    pub fn resolve_via_oracle(&self, digest: &str, outcome: BondOutcome) -> Result<SettlementReport> {
        let outcome = self.machine.resolve_with_proof(digest, outcome)?;
        self.report(digest, outcome)
    }

    /// Finalize every matured, undisputed bond and report each. Disputed bonds are
    /// left for [`resolve`](Self::resolve).
    pub fn sweep(&self) -> Result<Vec<SettlementReport>> {
        let matured = self.machine.sweep()?;
        matured
            .into_iter()
            .map(|(digest, outcome)| self.report(&digest, outcome))
            .collect()
    }

    /// Mint a Scar-Market record from a bond that finalized `Slashed` here. Refuses
    /// any non-slashed bond on top of [`certify_scar`]'s own checks, so a scar can
    /// only ever encode a real, machine-proven loss. The scar carries the *full*
    /// original collateral, not the challenger slice.
    pub fn mint_scar(
        &self,
        digest: &str,
        peer_id: impl Into<String>,
        transitions: u32,
        payload: Vec<u8>,
        price: Usdc,
    ) -> Result<ScarRecord> {
        match self.machine.state(digest) {
            Some(BondState::Finalized {
                outcome: BondOutcome::Slashed,
                ..
            }) => {}
            other => {
                return Err(ScemaDexError::Bond(format!(
                    "scar requires a finalized-slashed bond; {digest} is {other:?}"
                )))
            }
        }
        let bond = self
            .machine
            .bond(digest)
            .ok_or_else(|| ScemaDexError::Bond(format!("no bond tracked for {digest}")))?;
        certify_scar(&bond, BondOutcome::Slashed, peer_id, transitions, payload, price)
    }

    /// Build the combined report for a just-finalized bond.
    fn report(&self, digest: &str, outcome: BondOutcome) -> Result<SettlementReport> {
        // Settle (and remove) the counter-market listing against the final outcome.
        let challenge = self.counter.settle(digest, outcome).ok();
        let slash = if outcome == BondOutcome::Slashed {
            self.machine.slash_distribution(digest)
        } else {
            None
        };
        let reason = self
            .machine
            .state(digest)
            .and_then(|s| match s {
                BondState::Finalized { reason, .. } => Some(reason),
                _ => None,
            })
            .ok_or_else(|| ScemaDexError::Bond(format!("bond {digest} is not finalized")))?;
        Ok(SettlementReport {
            intent_digest: digest.to_string(),
            outcome,
            reason,
            slash,
            challenge,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settlement::{ManualClock, SlashRouting};

    fn bond(digest: &str, amount: u64) -> Bond {
        Bond {
            intent_digest: digest.into(),
            amount: Usdc(amount),
            min_out_raw: 1_000_000,
            deadline_unix: 0,
        }
    }

    /// A config with a live window and a 50/50 caller/challenger slash split, so a
    /// correct challenge is +EV.
    fn dueling_config() -> SettlementConfig {
        SettlementConfig::optimistic(60).with_slash_routing(SlashRouting {
            to_caller_bps: 5_000,
            to_challengers_bps: 5_000,
            to_insurance_bps: 0,
            to_lineage_bps: 0,
        })
    }

    #[test]
    fn unchallenged_bond_honors_after_window() {
        let coord = DisputeCoordinator::new(dueling_config(), ManualClock::new(1_000));
        coord.open(&bond("d", 1_000), Conviction::clamped(0.8)).unwrap();
        coord.provision("d", BondOutcome::Honored).unwrap();
        assert!(matches!(
            coord.machine().state("d"),
            Some(BondState::Provisional { .. })
        ));
        // No challenge; sweep once the window has elapsed.
        advance(&coord, 61);
        let reports = coord.sweep().unwrap();
        assert_eq!(reports.len(), 1);
        let r = &reports[0];
        assert_eq!(r.outcome, BondOutcome::Honored);
        assert_eq!(r.reason, FinalizeReason::WindowElapsed);
        assert!(r.slash.is_none());
        assert!(!r.is_adversarial_slash());
    }

    #[test]
    fn upheld_challenge_slashes_and_pays_challenger_only_the_slice() {
        let coord = DisputeCoordinator::new(dueling_config(), ManualClock::new(1_000));
        coord.open(&bond("d", 1_000), Conviction::clamped(0.9)).unwrap();
        coord.provision("d", BondOutcome::Honored).unwrap();
        coord.challenge("d", "skeptic", Usdc(200)).unwrap();
        // Market now doubts more than the agent's self-conviction.
        assert!(coord.counter().doubt_spread("d").unwrap() > 0.0);

        let r = coord.resolve("d", BondOutcome::Slashed).unwrap();
        assert_eq!(r.outcome, BondOutcome::Slashed);
        assert_eq!(r.reason, FinalizeReason::ChallengeUpheld);
        assert!(r.is_adversarial_slash());

        // Machine split: 500 to caller, 500 to challengers.
        let slash = r.slash.unwrap();
        assert_eq!(slash.caller, Usdc(500));
        assert_eq!(slash.challengers, Usdc(500));

        // Counter payout: the one challenger gets stake (200) + the full 500 slice.
        let cs = r.challenge.unwrap();
        assert_eq!(cs.challenger_payouts, vec![("skeptic".to_string(), Usdc(700))]);
        // Bond-share paid to challengers == the machine's challenger slice.
        let share: u64 = cs
            .challenger_payouts
            .iter()
            .map(|(_, p)| p.0)
            .sum::<u64>()
            - 200;
        assert_eq!(share, slash.challengers.0);
    }

    #[test]
    fn rejected_challenge_keeps_honor_and_pays_agent_premium() {
        let coord = DisputeCoordinator::new(dueling_config(), ManualClock::new(1_000));
        coord.open(&bond("d", 1_000), Conviction::clamped(0.9)).unwrap();
        coord.provision("d", BondOutcome::Honored).unwrap();
        coord.challenge("d", "a", Usdc(150)).unwrap();
        coord.challenge("d", "b", Usdc(350)).unwrap();
        let r = coord.resolve("d", BondOutcome::Honored).unwrap();
        assert_eq!(r.outcome, BondOutcome::Honored);
        assert_eq!(r.reason, FinalizeReason::ChallengeRejected);
        assert!(r.slash.is_none());
        // Both challengers forfeit their stakes to the agent.
        assert_eq!(r.challenge.unwrap().agent_premium, Usdc(500));
    }

    #[test]
    fn scar_mints_only_from_an_adversarial_slash() {
        let coord = DisputeCoordinator::new(dueling_config(), ManualClock::new(1_000));

        // Honored bond → no scar.
        coord.open(&bond("h", 1_000), Conviction::clamped(0.8)).unwrap();
        coord.provision("h", BondOutcome::Honored).unwrap();
        advance(&coord, 61);
        coord.sweep().unwrap();
        assert!(coord.mint_scar("h", "peer", 5, vec![], Usdc(100)).is_err());

        // Slashed-by-challenge bond → scar carries the FULL 1_000 collateral.
        coord.open(&bond("s", 1_000), Conviction::clamped(0.9)).unwrap();
        coord.provision("s", BondOutcome::Honored).unwrap();
        coord.challenge("s", "skeptic", Usdc(200)).unwrap();
        let r = coord.resolve("s", BondOutcome::Slashed).unwrap();
        assert!(r.is_adversarial_slash());
        let scar = coord.mint_scar("s", "peer", 12, vec![1, 2, 3], Usdc(250)).unwrap();
        assert_eq!(scar.slashed_collateral, Usdc(1_000));
        assert_eq!(scar.transitions, 12);
    }

    #[test]
    fn cannot_challenge_outside_the_window() {
        let coord = DisputeCoordinator::new(dueling_config(), ManualClock::new(1_000));
        coord.open(&bond("d", 1_000), Conviction::clamped(0.9)).unwrap();
        coord.provision("d", BondOutcome::Honored).unwrap();
        advance(&coord, 61);
        assert!(coord.challenge("d", "late", Usdc(100)).is_err());
    }

    // Advance the ManualClock inside the coordinator's machine.
    fn advance(coord: &DisputeCoordinator<ManualClock>, secs: u64) {
        coord.machine().clock().advance(secs);
    }
}
