//! **The forecast venue** — proof that the rail is not a swap widget.
//!
//! Conviction Routing was born settling swaps: an agent bonded a *minimum output*
//! and the fill judged it. [`crate::promise`] split that into a domain-agnostic
//! [`Promise`] + [`Outcome`] seam, and [`crate::coordinator::DisputeCoordinator`]
//! settles any [`BondOutcome`] regardless of where it came from. This module cashes
//! that generality in: a **[`ForecastVenue`]** routes *directional forecasts* —
//! "SOL ≥ \$150 at resolution" — through the *identical* settlement machine,
//! counter-market, and scar market that swaps use, with **zero swap-specific code**.
//!
//! It is the [`crate::venue::VenueExecutor`] analogue for a non-swap domain: where a
//! swap executor turns a route into a [`crate::route::Fill`], a forecast venue turns
//! a bonded prediction into a settled [`BondOutcome`] once ground truth arrives.
//!
//! ## Lifecycle
//!
//! ```text
//! submit(promise, conviction, bond)     // Escrowed → optimistically Provisional(Honored)
//!   → [challenge(skeptic, stake)]        // a doubter stakes during the window → Disputed
//!   → resolve(ground_truth)              // Measurement judges the Promise; finalizes
//!        · undisputed  → oracle-finalize (ProofVerified): correct→Honored, wrong→Slashed
//!        · disputed    → ground truth adjudicates the challenge (upheld iff wrong)
//!   → [mint_scar(...)]                   // a wrong forecast is un-fakeable proof of loss
//! ```
//!
//! The agent is **optimistically honored at submit time** — the forecast is presumed
//! correct until either a challenger contests it or ground truth contradicts it. That
//! is the same optimistic posture a swap fill takes; here the "fill" is the claim
//! itself and the adjudicator is reality. Because ground truth resolves the bond
//! deterministically (via [`crate::settlement::SettlementMachine::resolve_with_proof`]),
//! a forecast never needs to *wait out* its window — truth collapses it on arrival.
//!
//! A forecast venue therefore requires an **optimistic** [`SettlementConfig`]
//! (`dispute_window_secs > 0`); an atomic config would finalize the optimistic honor
//! the instant it is provisioned, leaving no window for a challenge or a truth read.
//!
//! See `cargo run -p scemadex-sdk --example forecast_bond`.

use std::collections::HashMap;
use std::sync::Mutex;

use crate::bond::{Bond, BondOutcome};
use crate::coordinator::{DisputeCoordinator, SettlementReport};
use crate::error::{Result, ScemaDexError};
use crate::policy::Conviction;
use crate::primitives::Usdc;
use crate::promise::{Measurement, Outcome, Promise};
use crate::scar::ScarRecord;
use crate::settlement::{BondState, Clock, SettlementConfig, SystemClock};

/// A venue that settles bonded forecasts through the Conviction-Routing stack.
///
/// Generic over a [`Clock`] exactly like the machinery it composes, so forecast
/// lifecycles are deterministic in tests ([`crate::settlement::ManualClock`]) and
/// wall-clock in production ([`SystemClock`]).
pub struct ForecastVenue<C: Clock = SystemClock> {
    coord: DisputeCoordinator<C>,
    /// The promise each open forecast is judged against, keyed by intent digest.
    promises: Mutex<HashMap<String, Promise>>,
}

impl ForecastVenue<SystemClock> {
    /// A forecast venue driven by the wall clock. `config` should be
    /// [`SettlementConfig::optimistic`] so forecasts have a live dispute window.
    pub fn system(config: SettlementConfig) -> Self {
        Self::new(config, SystemClock)
    }
}

impl<C: Clock> ForecastVenue<C> {
    pub fn new(config: SettlementConfig, clock: C) -> Self {
        Self {
            coord: DisputeCoordinator::new(config, clock),
            promises: Mutex::new(HashMap::new()),
        }
    }

    /// The underlying dispute coordinator (counter-market, machine, scar).
    pub fn coordinator(&self) -> &DisputeCoordinator<C> {
        &self.coord
    }

    fn promises(&self) -> Result<std::sync::MutexGuard<'_, HashMap<String, Promise>>> {
        self.promises
            .lock()
            .map_err(|_| ScemaDexError::Bond("forecast promise map poisoned".into()))
    }

    /// Submit a bonded forecast. The bond is escrowed, the `promise` recorded, and
    /// the forecast is **optimistically honored** so its dispute window opens for
    /// doubters — just like a provisionally-honored swap fill. `conviction` is the
    /// agent's self-assessed probability, the baseline the counter-market prices
    /// doubt against.
    pub fn submit(&self, bond: &Bond, promise: Promise, conviction: Conviction) -> Result<()> {
        self.coord.open(bond, conviction)?;
        // Optimistic honor: presumed correct until a challenge or ground truth says
        // otherwise. Opens the window (requires an optimistic config).
        self.coord.provision(&bond.intent_digest, BondOutcome::Honored)?;
        self.promises()?
            .insert(bond.intent_digest.clone(), promise);
        Ok(())
    }

    /// Stake against an open forecast during its window. Moves it to `Disputed` and
    /// lets ground truth adjudicate at [`resolve`](Self::resolve). Returns the
    /// recorded challenge stake pool via the coordinator.
    pub fn challenge(
        &self,
        digest: &str,
        challenger_id: impl Into<String>,
        stake: Usdc,
    ) -> Result<()> {
        self.coord.challenge(digest, challenger_id, stake)?;
        Ok(())
    }

    /// Resolve a forecast against realized ground truth. The [`Measurement`] judges
    /// the recorded [`Promise`] via the generic [`Outcome`] seam, then:
    ///
    /// - a **disputed** forecast has ground truth *adjudicate the challenge* (upheld
    ///   iff the forecast was wrong);
    /// - an **undisputed** forecast is finalized directly from the oracle read,
    ///   collapsing the window.
    ///
    /// Returns the settlement report (outcome, four-way slash split, counter-market
    /// payouts). A correct forecast honors; a wrong one slashes and is
    /// [`mint_scar`](Self::mint_scar)-eligible.
    pub fn resolve(&self, digest: &str, truth: Measurement) -> Result<SettlementReport> {
        let promise = self
            .promises()?
            .get(digest)
            .cloned()
            .ok_or_else(|| ScemaDexError::Bond(format!("no open forecast for {digest}")))?;
        let adjudicated = truth.evaluate(&promise);
        let report = match self.coord.machine().state(digest) {
            Some(BondState::Disputed { .. }) => self.coord.resolve(digest, adjudicated)?,
            Some(BondState::Provisional { .. }) => self.coord.resolve_via_oracle(digest, adjudicated)?,
            other => {
                return Err(ScemaDexError::Bond(format!(
                    "forecast {digest} is not resolvable (state {other:?})"
                )))
            }
        };
        self.promises()?.remove(digest);
        Ok(report)
    }

    /// The current lifecycle state of a tracked forecast.
    pub fn state(&self, digest: &str) -> Option<BondState> {
        self.coord.machine().state(digest)
    }

    /// Mint a Scar-Market record from a forecast that finalized `Slashed`. A wrong,
    /// bonded forecast is the same un-fakeable, adversarially-settled loss a missed
    /// swap fill is — sellable, verified negative training data.
    pub fn mint_scar(
        &self,
        digest: &str,
        peer_id: impl Into<String>,
        transitions: u32,
        payload: Vec<u8>,
        price: Usdc,
    ) -> Result<ScarRecord> {
        self.coord.mint_scar(digest, peer_id, transitions, payload, price)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settlement::{ManualClock, SlashRouting};

    fn forecast_bond(digest: &str, amount: u64) -> Bond {
        Bond {
            intent_digest: digest.into(),
            amount: Usdc(amount),
            min_out_raw: 0, // unused for a forecast — the promise is the guarantee
            deadline_unix: 0,
        }
    }

    fn venue() -> ForecastVenue<ManualClock> {
        // Optimistic window + a caller/challenger split so a correct challenge is +EV.
        let config = SettlementConfig::optimistic(3_600).with_slash_routing(SlashRouting {
            to_caller_bps: 5_000,
            to_challengers_bps: 5_000,
            to_insurance_bps: 0,
            to_lineage_bps: 0,
        });
        ForecastVenue::new(config, ManualClock::new(1_700_000_000))
    }

    fn bullish(strike: f64) -> Promise {
        Promise::Forecast { strike, expect_above: true }
    }

    #[test]
    fn correct_forecast_honors_without_a_challenge() {
        let v = venue();
        v.submit(&forecast_bond("f", 1_000), bullish(150.0), Conviction::clamped(0.8))
            .unwrap();
        assert!(matches!(v.state("f"), Some(BondState::Provisional { .. })));
        // SOL landed at 162 ≥ 150 → the bull was right.
        let r = v.resolve("f", Measurement::new(162.0, 1_700_000_500)).unwrap();
        assert_eq!(r.outcome, BondOutcome::Honored);
        assert!(r.slash.is_none());
        assert!(v.mint_scar("f", "peer", 1, vec![], Usdc(1)).is_err());
    }

    #[test]
    fn wrong_forecast_slashes_and_mints_a_scar() {
        let v = venue();
        v.submit(&forecast_bond("f", 1_000), bullish(200.0), Conviction::clamped(0.6))
            .unwrap();
        // 162 < 200 → the bull was wrong.
        let r = v.resolve("f", Measurement::new(162.0, 1_700_000_500)).unwrap();
        assert_eq!(r.outcome, BondOutcome::Slashed);
        // Ground-truth resolution routes the full four-way slash split.
        let slash = r.slash.expect("a slash distributes collateral");
        assert_eq!(slash.caller.0 + slash.challengers.0, 1_000);
        // The loss is real → a scar carries the full collateral.
        let scar = v.mint_scar("f", "forecaster", 3, vec![1, 2], Usdc(250)).unwrap();
        assert_eq!(scar.slashed_collateral, Usdc(1_000));
    }

    #[test]
    fn ground_truth_vindicates_the_agent_against_a_challenge() {
        let v = venue();
        v.submit(&forecast_bond("f", 1_000), bullish(150.0), Conviction::clamped(0.9))
            .unwrap();
        // A skeptic doubts the bull; the forecast enters Disputed.
        v.challenge("f", "skeptic", Usdc(300)).unwrap();
        assert!(matches!(v.state("f"), Some(BondState::Disputed { .. })));
        // But SOL lands at 162 ≥ 150 → the forecast was right; the challenge fails.
        let r = v.resolve("f", Measurement::new(162.0, 1_700_000_500)).unwrap();
        assert_eq!(r.outcome, BondOutcome::Honored);
        // The skeptic forfeits its stake to the agent as premium.
        assert_eq!(r.challenge.unwrap().agent_premium, Usdc(300));
    }

    #[test]
    fn ground_truth_upholds_a_correct_challenge() {
        let v = venue();
        v.submit(&forecast_bond("f", 1_000), bullish(200.0), Conviction::clamped(0.7))
            .unwrap();
        v.challenge("f", "skeptic", Usdc(300)).unwrap();
        // SOL lands at 162 < 200 → the skeptic was right; the challenge is upheld.
        let r = v.resolve("f", Measurement::new(162.0, 1_700_000_500)).unwrap();
        assert_eq!(r.outcome, BondOutcome::Slashed);
        assert!(r.is_adversarial_slash());
        // Challenger recovers stake (300) + the 500 challenger slice.
        let cs = r.challenge.unwrap();
        assert_eq!(cs.challenger_payouts, vec![("skeptic".to_string(), Usdc(800))]);
    }

    #[test]
    fn resolving_an_unknown_forecast_errors() {
        let v = venue();
        assert!(v.resolve("ghost", Measurement::new(1.0, 0)).is_err());
    }
}
