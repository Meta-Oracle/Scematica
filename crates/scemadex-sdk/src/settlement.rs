//! **Settlement v2** — the optimistic conviction-settlement state machine.
//!
//! Conviction Routing (Primitive D) today settles a bond *atomically*: the moment
//! a fill lands, [`crate::bond::EscrowBondEngine::settle`] decides
//! honored-or-slashed and the outcome is final. That is fast, but it has no room
//! for the two things the adversarial primitives need: **time** and
//! **contestability**. A challenger (Primitive E) can only bet on an outcome that
//! was already decided; a zkML proof (Primitive I) has nowhere to land; a slash
//! has no window in which to be disputed before money moves.
//!
//! This module turns that function into a lifecycle:
//!
//! ```text
//!                    ┌──────────── challenge upheld ───────────┐
//!                    │                                         ▼
//! Escrowed ─fill─▶ Provisional ─window elapses, unchallenged─▶ Finalized(Honored)
//!    │               │
//!    │               └─challenge filed─▶ Disputed ─resolve─▶ Finalized(Honored|Slashed)
//!    │
//!    └─deadline passes, no fill─▶ Finalized(Slashed)   // failure to deliver
//! ```
//!
//! A fill provisionally honors immediately (the happy path stays fast), but funds
//! are not considered settled until the bond reaches **Finalized** — either the
//! dispute window elapses unchallenged, or a challenge / proof resolves it.
//!
//! **Backward compatibility.** [`SettlementConfig::atomic`] sets a zero-length
//! window, so [`SettlementMachine::provision`] finalizes in the same call — exactly
//! today's `EscrowBondEngine::settle` behavior. The state machine is a superset,
//! not a replacement.
//!
//! The machine is deliberately outcome-agnostic: [`provision`](SettlementMachine::provision)
//! takes a [`BondOutcome`], not a swap [`crate::route::Fill`]. Deciding *whether the
//! promise held* stays the caller's job (the swap predicate lives in the bond
//! engine), which is what lets a future `Outcome` trait generalize the rail past
//! swaps without touching this file.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::bond::{Bond, BondOutcome};
use crate::error::{Result, ScemaDexError};
use crate::primitives::Usdc;

/// A monotonic-ish source of unix-seconds, injected so the state machine stays
/// deterministic and testable. The lean core ships [`ManualClock`] (advanceable,
/// for tests and simulation) and [`SystemClock`] (wall-clock, for live use).
pub trait Clock: Send + Sync {
    fn unix_now(&self) -> u64;
}

/// A wall-clock [`Clock`] backed by the system time.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn unix_now(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
}

/// A deterministic [`Clock`] whose value only changes when the test advances it.
#[derive(Debug, Default)]
pub struct ManualClock(std::sync::atomic::AtomicU64);

impl ManualClock {
    pub fn new(start_unix: u64) -> Self {
        Self(std::sync::atomic::AtomicU64::new(start_unix))
    }

    /// Jump forward by `secs` and return the new value.
    pub fn advance(&self, secs: u64) -> u64 {
        self.0
            .fetch_add(secs, std::sync::atomic::Ordering::SeqCst)
            .saturating_add(secs)
    }

    /// Set the absolute time.
    pub fn set(&self, unix: u64) {
        self.0.store(unix, std::sync::atomic::Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn unix_now(&self) -> u64 {
        self.0.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// Why a bond reached its terminal outcome — the audit trail behind a settlement.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FinalizeReason {
    /// The dispute window closed with no successful challenge — optimistic honor.
    WindowElapsed,
    /// The bond's deadline passed with no fill — failure to deliver.
    DeadlineMissed,
    /// A challenge (Primitive E) was upheld and flipped the provisional outcome.
    ChallengeUpheld,
    /// A challenge was filed but rejected — the provisional outcome stands.
    ChallengeRejected,
    /// A zkML / re-execution proof (Primitive I) resolved the outcome deterministically.
    ProofVerified,
}

/// Where a bond is in its lifecycle. Only [`BondState::Finalized`] moves money.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BondState {
    /// Collateral locked, awaiting a fill (or the deadline).
    Escrowed,
    /// A fill landed; `outcome` is provisional and the dispute window is open
    /// until `window_closes_unix`.
    Provisional {
        outcome: BondOutcome,
        window_closes_unix: u64,
    },
    /// A challenge was filed during the window; awaiting resolution.
    Disputed {
        provisional: BondOutcome,
        challenger: String,
    },
    /// Terminal. Funds settle here per [`SlashRouting`].
    Finalized {
        outcome: BondOutcome,
        reason: FinalizeReason,
    },
}

impl BondState {
    /// The terminal outcome, if this bond has finalized.
    pub fn final_outcome(&self) -> Option<BondOutcome> {
        match self {
            BondState::Finalized { outcome, .. } => Some(*outcome),
            _ => None,
        }
    }

    pub fn is_final(&self) -> bool {
        matches!(self, BondState::Finalized { .. })
    }
}

/// How slashed collateral is split. This is the economic knob that makes the
/// adversarial primitives load-bearing: challengers (E), the reinsurance pool (J),
/// and the experience lineage (G) are all paid from a slash. The four shares are
/// in basis points and must sum to `10_000`.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct SlashRouting {
    /// Compensation to the wronged caller. Absorbs rounding dust.
    pub to_caller_bps: u32,
    /// Counter-Market winners (Primitive E).
    pub to_challengers_bps: u32,
    /// Reinsurance pool (Primitive J).
    pub to_insurance_bps: u32,
    /// Upstream experience-royalty holders (Primitive G).
    pub to_lineage_bps: u32,
}

impl Default for SlashRouting {
    /// The legacy behavior: a slash compensates the caller in full.
    fn default() -> Self {
        Self {
            to_caller_bps: 10_000,
            to_challengers_bps: 0,
            to_insurance_bps: 0,
            to_lineage_bps: 0,
        }
    }
}

impl SlashRouting {
    /// The four shares summed.
    ///
    /// Widened to `u64`. Summing four `u32`s in a `u32` overflows a little past a quarter
    /// of the space — panicking in debug, wrapping in release — and a wrapped total is
    /// worse than a large one, because `is_valid` can then answer `true` for a routing
    /// that pays out several times the bond.
    fn total_bps(&self) -> u64 {
        self.to_caller_bps as u64
            + self.to_challengers_bps as u64
            + self.to_insurance_bps as u64
            + self.to_lineage_bps as u64
    }

    /// True when the four shares sum to a full `10_000` bps.
    pub fn is_valid(&self) -> bool {
        self.total_bps() == 10_000
    }

    /// Split a slashed bond into its four destinations.
    ///
    /// **Conserves value for every routing, valid or not.** The three named slices are
    /// taken in order against a running remainder and the caller receives what is left,
    /// so the four always sum to exactly the bond.
    ///
    /// Under a valid routing this is the arithmetic it has always been — the three shares
    /// of a routing summing to 10 000 bps cannot exceed the bond, so nothing is ever
    /// capped and the caller's remainder is the rounding dust, as documented. The
    /// difference is only in what an *invalid* routing does. It used to compute
    /// `amount - challengers - insurance - lineage` as a plain subtraction: two 100 %
    /// shares made that underflow, panicking in debug and wrapping to an enormous caller
    /// share in release, off a `SlashRouting` nothing had ever checked — `distribute` did
    /// not call `is_valid`, and `with_slash_routing` did not validate what it stored. The
    /// on-chain twin has always required validity at `escrow`; this is the off-chain half
    /// of that guarantee, made structural rather than checked.
    ///
    /// The share itself is computed in `u128`, matching the program. `saturating_mul` on
    /// `u64` silently pins a large bond's share at `u64::MAX / 10_000`, which is a wrong
    /// payout rather than a refused one.
    pub fn distribute(&self, bond: Usdc) -> SlashDistribution {
        let amount = bond.0;
        let mut left = amount;
        let mut take = |bps: u32| {
            let s = ((amount as u128 * bps as u128) / 10_000) as u64;
            let s = s.min(left);
            left -= s;
            s
        };
        let challengers = take(self.to_challengers_bps);
        let insurance = take(self.to_insurance_bps);
        let lineage = take(self.to_lineage_bps);
        SlashDistribution {
            caller: Usdc(left), // remainder, so dust → caller
            challengers: Usdc(challengers),
            insurance: Usdc(insurance),
            lineage: Usdc(lineage),
        }
    }
}

/// The resolved split of a slashed bond. `caller + challengers + insurance +
/// lineage` always equals the original bond.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlashDistribution {
    pub caller: Usdc,
    pub challengers: Usdc,
    pub insurance: Usdc,
    pub lineage: Usdc,
}

/// Tunables for the settlement lifecycle.
#[derive(Clone, Copy, Debug)]
pub struct SettlementConfig {
    /// Length of the dispute window in seconds. `0` collapses the machine to the
    /// legacy atomic settle (provision finalizes immediately).
    pub dispute_window_secs: u64,
    /// Stake a challenger must post to open a dispute, as bps of the bond. Makes
    /// frivolous challenges costly. (Enforced by the challenge book, not here.)
    pub challenge_bond_bps: u32,
    /// How a slashed bond is distributed.
    pub slash_routing: SlashRouting,
}

impl Default for SettlementConfig {
    fn default() -> Self {
        Self::atomic()
    }
}

impl SettlementConfig {
    /// Zero-window config — reproduces today's atomic `settle`. This is the
    /// backward-compatibility default.
    pub fn atomic() -> Self {
        Self {
            dispute_window_secs: 0,
            challenge_bond_bps: 0,
            slash_routing: SlashRouting::default(),
        }
    }

    /// Optimistic config with a dispute window of `window_secs`.
    pub fn optimistic(window_secs: u64) -> Self {
        Self {
            dispute_window_secs: window_secs,
            challenge_bond_bps: 1_000, // 10% of the bond to challenge
            slash_routing: SlashRouting::default(),
        }
    }

    /// Builder: set how a slashed bond is distributed. A meaningful counter-market
    /// (Primitive E) needs `to_challengers_bps > 0` so doubting is +EV when right.
    ///
    /// Debug-asserts validity. The signature stays infallible because this is published
    /// API and `distribute` is now conserving whatever it is handed, so an invalid
    /// routing can no longer overdraw — it only pays the caller less than the
    /// configuration claims. Use [`SettlementConfig::try_with_slash_routing`] to be told
    /// about that rather than to find out from a ledger.
    pub fn with_slash_routing(mut self, routing: SlashRouting) -> Self {
        debug_assert!(
            routing.is_valid(),
            "slash routing must sum to 10_000 bps, got {}",
            routing.total_bps()
        );
        self.slash_routing = routing;
        self
    }

    /// Builder: set the slash routing, refusing one that does not sum to 10 000 bps.
    ///
    /// The checked counterpart to [`with_slash_routing`](Self::with_slash_routing), and
    /// the off-chain equivalent of the program's `require!(routing.is_valid())`.
    pub fn try_with_slash_routing(mut self, routing: SlashRouting) -> crate::error::Result<Self> {
        if !routing.is_valid() {
            return Err(crate::error::ScemaDexError::ConstraintViolation(format!(
                "slash routing must sum to 10_000 bps, got {}",
                routing.total_bps()
            )));
        }
        self.slash_routing = routing;
        Ok(self)
    }
}

struct Entry {
    bond: Bond,
    state: BondState,
}

/// An in-process settlement state machine over open bonds.
///
/// Generic over a [`Clock`] so time-based transitions (window expiry, deadline
/// misses) are deterministic in tests. Construct with [`SettlementMachine::system`]
/// for live use or [`SettlementMachine::new`] with a [`ManualClock`] for tests and
/// simulation.
///
/// Lifecycle:
/// [`open`](Self::open) → [`provision`](Self::provision) →
/// (optionally [`file_challenge`](Self::file_challenge) →
/// [`resolve_dispute`](Self::resolve_dispute)) → [`finalize`](Self::finalize).
pub struct SettlementMachine<C: Clock = SystemClock> {
    config: SettlementConfig,
    clock: C,
    bonds: Mutex<HashMap<String, Entry>>,
}

impl SettlementMachine<SystemClock> {
    /// A machine driven by the wall clock.
    pub fn system(config: SettlementConfig) -> Self {
        Self::new(config, SystemClock)
    }
}

impl<C: Clock> SettlementMachine<C> {
    pub fn new(config: SettlementConfig, clock: C) -> Self {
        Self {
            config,
            clock,
            bonds: Mutex::new(HashMap::new()),
        }
    }

    pub fn config(&self) -> &SettlementConfig {
        &self.config
    }

    /// The injected clock. Handy for driving time in tests/simulation (e.g.
    /// advancing a [`ManualClock`]) or reading "now".
    pub fn clock(&self) -> &C {
        &self.clock
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, HashMap<String, Entry>>> {
        self.bonds
            .lock()
            .map_err(|_| ScemaDexError::Bond("settlement lock poisoned".into()))
    }

    /// Register a freshly escrowed bond as `Escrowed`.
    pub fn open(&self, bond: &Bond) -> Result<()> {
        self.lock()?.insert(
            bond.intent_digest.clone(),
            Entry {
                bond: bond.clone(),
                state: BondState::Escrowed,
            },
        );
        Ok(())
    }

    /// Record a fill's provisional outcome and open the dispute window.
    ///
    /// With a zero-length window ([`SettlementConfig::atomic`]) the bond finalizes
    /// in this same call with reason [`FinalizeReason::WindowElapsed`] — the legacy
    /// atomic path. Otherwise it enters `Provisional` until the window closes.
    /// Returns the resulting state.
    pub fn provision(&self, digest: &str, provisional: BondOutcome) -> Result<BondState> {
        let now = self.clock.unix_now();
        let mut bonds = self.lock()?;
        let entry = bonds
            .get_mut(digest)
            .ok_or_else(|| ScemaDexError::Bond(format!("no open bond for intent {digest}")))?;
        if !matches!(entry.state, BondState::Escrowed) {
            return Err(ScemaDexError::Bond(format!(
                "bond {digest} cannot be provisioned from {:?}",
                entry.state
            )));
        }
        entry.state = if self.config.dispute_window_secs == 0 {
            BondState::Finalized {
                outcome: provisional,
                reason: FinalizeReason::WindowElapsed,
            }
        } else {
            BondState::Provisional {
                outcome: provisional,
                window_closes_unix: now.saturating_add(self.config.dispute_window_secs),
            }
        };
        Ok(entry.state.clone())
    }

    /// File a challenge against a provisionally-**honored** bond, moving it to
    /// `Disputed`. Fails if the bond isn't provisional, the window has closed, or
    /// the provisional outcome is already `Slashed` (nothing to contest).
    pub fn file_challenge(&self, digest: &str, challenger: impl Into<String>) -> Result<()> {
        let now = self.clock.unix_now();
        let mut bonds = self.lock()?;
        let entry = bonds
            .get_mut(digest)
            .ok_or_else(|| ScemaDexError::Bond(format!("no open bond for intent {digest}")))?;
        match &entry.state {
            BondState::Provisional {
                outcome,
                window_closes_unix,
            } => {
                if now >= *window_closes_unix {
                    return Err(ScemaDexError::Bond(format!(
                        "dispute window for {digest} has closed"
                    )));
                }
                if *outcome == BondOutcome::Slashed {
                    return Err(ScemaDexError::Bond(format!(
                        "bond {digest} is already provisionally slashed; nothing to challenge"
                    )));
                }
                entry.state = BondState::Disputed {
                    provisional: *outcome,
                    challenger: challenger.into(),
                };
                Ok(())
            }
            other => Err(ScemaDexError::Bond(format!(
                "bond {digest} cannot be challenged from {other:?}"
            ))),
        }
    }

    /// Resolve an open dispute. `challenger_won == true` slashes the bond
    /// ([`FinalizeReason::ChallengeUpheld`]); otherwise the provisional honor
    /// stands ([`FinalizeReason::ChallengeRejected`]).
    pub fn resolve_dispute(&self, digest: &str, challenger_won: bool) -> Result<BondOutcome> {
        let mut bonds = self.lock()?;
        let entry = bonds
            .get_mut(digest)
            .ok_or_else(|| ScemaDexError::Bond(format!("no open bond for intent {digest}")))?;
        if !matches!(entry.state, BondState::Disputed { .. }) {
            return Err(ScemaDexError::Bond(format!(
                "bond {digest} is not disputed (state {:?})",
                entry.state
            )));
        }
        let (outcome, reason) = if challenger_won {
            (BondOutcome::Slashed, FinalizeReason::ChallengeUpheld)
        } else {
            (BondOutcome::Honored, FinalizeReason::ChallengeRejected)
        };
        entry.state = BondState::Finalized { outcome, reason };
        Ok(outcome)
    }

    /// Deterministically resolve a bond from a verified proof (Primitive I),
    /// short-circuiting the window from either `Provisional` or `Disputed`.
    pub fn resolve_with_proof(&self, digest: &str, outcome: BondOutcome) -> Result<BondOutcome> {
        let mut bonds = self.lock()?;
        let entry = bonds
            .get_mut(digest)
            .ok_or_else(|| ScemaDexError::Bond(format!("no open bond for intent {digest}")))?;
        match entry.state {
            BondState::Provisional { .. } | BondState::Disputed { .. } => {
                entry.state = BondState::Finalized {
                    outcome,
                    reason: FinalizeReason::ProofVerified,
                };
                Ok(outcome)
            }
            _ => Err(ScemaDexError::Bond(format!(
                "bond {digest} cannot be proof-resolved from {:?}",
                entry.state
            ))),
        }
    }

    /// Advance a single bond's time-based transition and return its outcome once
    /// terminal. Honors a `Provisional` bond whose window has elapsed; slashes an
    /// `Escrowed` bond whose deadline has passed with no fill. Idempotent on an
    /// already-`Finalized` bond. Errors while the window is still open or a dispute
    /// is unresolved.
    pub fn finalize(&self, digest: &str) -> Result<BondOutcome> {
        let now = self.clock.unix_now();
        let mut bonds = self.lock()?;
        let entry = bonds
            .get_mut(digest)
            .ok_or_else(|| ScemaDexError::Bond(format!("no open bond for intent {digest}")))?;
        match &entry.state {
            BondState::Finalized { outcome, .. } => Ok(*outcome),
            BondState::Provisional {
                outcome,
                window_closes_unix,
            } => {
                if now < *window_closes_unix {
                    return Err(ScemaDexError::Bond(format!(
                        "dispute window for {digest} is still open"
                    )));
                }
                let outcome = *outcome;
                entry.state = BondState::Finalized {
                    outcome,
                    reason: FinalizeReason::WindowElapsed,
                };
                Ok(outcome)
            }
            BondState::Escrowed => {
                let deadline = entry.bond.deadline_unix;
                if deadline == 0 || now < deadline {
                    return Err(ScemaDexError::Bond(format!(
                        "bond {digest} has no fill and its deadline has not passed"
                    )));
                }
                entry.state = BondState::Finalized {
                    outcome: BondOutcome::Slashed,
                    reason: FinalizeReason::DeadlineMissed,
                };
                Ok(BondOutcome::Slashed)
            }
            BondState::Disputed { .. } => Err(ScemaDexError::Bond(format!(
                "bond {digest} is disputed; call resolve_dispute"
            ))),
        }
    }

    /// Finalize every bond whose window has elapsed or whose deadline has passed,
    /// returning the `(digest, outcome)` of each transition. A driver loop calls
    /// this on a tick to settle matured bonds in bulk. Disputed bonds are left
    /// untouched.
    pub fn sweep(&self) -> Result<Vec<(String, BondOutcome)>> {
        let now = self.clock.unix_now();
        let mut bonds = self.lock()?;
        let mut settled = Vec::new();
        for (digest, entry) in bonds.iter_mut() {
            let matured = match &entry.state {
                BondState::Provisional {
                    window_closes_unix, ..
                } => now >= *window_closes_unix,
                BondState::Escrowed => entry.bond.deadline_unix != 0 && now >= entry.bond.deadline_unix,
                _ => false,
            };
            if !matured {
                continue;
            }
            let (outcome, reason) = match &entry.state {
                BondState::Provisional { outcome, .. } => (*outcome, FinalizeReason::WindowElapsed),
                BondState::Escrowed => (BondOutcome::Slashed, FinalizeReason::DeadlineMissed),
                _ => unreachable!("filtered by `matured`"),
            };
            entry.state = BondState::Finalized { outcome, reason };
            settled.push((digest.clone(), outcome));
        }
        Ok(settled)
    }

    /// Current state of a tracked bond.
    pub fn state(&self, digest: &str) -> Option<BondState> {
        self.lock().ok()?.get(digest).map(|e| e.state.clone())
    }

    /// The original escrowed bond (full collateral) for a tracked digest.
    pub fn bond(&self, digest: &str) -> Option<Bond> {
        self.lock().ok()?.get(digest).map(|e| e.bond.clone())
    }

    /// Slash distribution for a finalized-slashed bond, per the configured routing.
    /// Returns `None` unless the bond has finalized as `Slashed`.
    pub fn slash_distribution(&self, digest: &str) -> Option<SlashDistribution> {
        let bonds = self.lock().ok()?;
        let entry = bonds.get(digest)?;
        match entry.state {
            BondState::Finalized {
                outcome: BondOutcome::Slashed,
                ..
            } => Some(self.config.slash_routing.distribute(entry.bond.amount)),
            _ => None,
        }
    }

    /// Bonds that have not yet reached a terminal state.
    pub fn open_count(&self) -> usize {
        self.lock()
            .map(|b| b.values().filter(|e| !e.state.is_final()).count())
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bond(digest: &str, amount: u64, deadline_unix: u64) -> Bond {
        Bond {
            intent_digest: digest.into(),
            amount: Usdc(amount),
            min_out_raw: 1_000_000,
            deadline_unix,
        }
    }

    #[test]
    fn atomic_config_finalizes_on_provision() {
        // Zero window == today's atomic settle, for both outcomes.
        let m = SettlementMachine::new(SettlementConfig::atomic(), ManualClock::new(1_000));
        m.open(&bond("h", 500, 0)).unwrap();
        let state = m.provision("h", BondOutcome::Honored).unwrap();
        assert_eq!(state.final_outcome(), Some(BondOutcome::Honored));
        assert_eq!(m.finalize("h").unwrap(), BondOutcome::Honored);

        m.open(&bond("s", 500, 0)).unwrap();
        let state = m.provision("s", BondOutcome::Slashed).unwrap();
        assert_eq!(state.final_outcome(), Some(BondOutcome::Slashed));
        assert_eq!(m.open_count(), 0);
    }

    #[test]
    fn optimistic_honors_after_window() {
        let clock = ManualClock::new(1_000);
        let m = SettlementMachine::new(SettlementConfig::optimistic(60), clock);
        m.open(&bond("d", 500, 0)).unwrap();
        m.provision("d", BondOutcome::Honored).unwrap();
        assert!(matches!(
            m.state("d"),
            Some(BondState::Provisional { .. })
        ));
        // Window still open → finalize refuses.
        assert!(m.finalize("d").is_err());
    }

    #[test]
    fn window_elapse_finalizes_via_sweep() {
        let m = SettlementMachine::new(SettlementConfig::optimistic(60), ManualClock::new(1_000));
        m.open(&bond("d", 500, 0)).unwrap();
        m.provision("d", BondOutcome::Honored).unwrap();
        // Nothing matured yet.
        assert!(m.sweep().unwrap().is_empty());
        // Advance past the window (tests share the module, so `clock` is reachable).
        m.clock.advance(61);
        let settled = m.sweep().unwrap();
        assert_eq!(settled, vec![("d".to_string(), BondOutcome::Honored)]);
        assert_eq!(m.finalize("d").unwrap(), BondOutcome::Honored);
        assert_eq!(m.open_count(), 0);
    }

    #[test]
    fn challenge_upheld_flips_to_slashed() {
        let m = SettlementMachine::new(SettlementConfig::optimistic(60), ManualClock::new(1_000));
        m.open(&bond("d", 500, 0)).unwrap();
        m.provision("d", BondOutcome::Honored).unwrap();
        m.file_challenge("d", "skeptic").unwrap();
        assert!(matches!(m.state("d"), Some(BondState::Disputed { .. })));
        assert_eq!(m.resolve_dispute("d", true).unwrap(), BondOutcome::Slashed);
        assert!(matches!(
            m.state("d"),
            Some(BondState::Finalized {
                reason: FinalizeReason::ChallengeUpheld,
                ..
            })
        ));
    }

    #[test]
    fn challenge_rejected_keeps_honor() {
        let m = SettlementMachine::new(SettlementConfig::optimistic(60), ManualClock::new(1_000));
        m.open(&bond("d", 500, 0)).unwrap();
        m.provision("d", BondOutcome::Honored).unwrap();
        m.file_challenge("d", "skeptic").unwrap();
        assert_eq!(m.resolve_dispute("d", false).unwrap(), BondOutcome::Honored);
    }

    #[test]
    fn cannot_challenge_after_window_or_a_slash() {
        let clock = ManualClock::new(1_000);
        let m = SettlementMachine::new(SettlementConfig::optimistic(60), clock);
        m.open(&bond("late", 500, 0)).unwrap();
        m.provision("late", BondOutcome::Honored).unwrap();
        m.clock.advance(61);
        assert!(m.file_challenge("late", "x").is_err());

        let m2 = SettlementMachine::new(SettlementConfig::optimistic(60), ManualClock::new(1_000));
        m2.open(&bond("slash", 500, 0)).unwrap();
        m2.provision("slash", BondOutcome::Slashed).unwrap();
        assert!(m2.file_challenge("slash", "x").is_err());
    }

    #[test]
    fn deadline_missed_slashes_undelivered_bond() {
        let m = SettlementMachine::new(SettlementConfig::optimistic(60), ManualClock::new(1_000));
        m.open(&bond("dead", 500, 1_050)).unwrap();
        // Before the deadline, an unfilled bond can't be finalized.
        assert!(m.finalize("dead").is_err());
        m.clock.set(1_100);
        assert_eq!(m.finalize("dead").unwrap(), BondOutcome::Slashed);
        assert!(matches!(
            m.state("dead"),
            Some(BondState::Finalized {
                reason: FinalizeReason::DeadlineMissed,
                ..
            })
        ));
    }

    #[test]
    fn proof_resolves_deterministically() {
        let m = SettlementMachine::new(SettlementConfig::optimistic(3600), ManualClock::new(1_000));
        m.open(&bond("p", 500, 0)).unwrap();
        m.provision("p", BondOutcome::Honored).unwrap();
        // A proof collapses the (possibly long) window immediately.
        assert_eq!(
            m.resolve_with_proof("p", BondOutcome::Slashed).unwrap(),
            BondOutcome::Slashed
        );
        assert!(matches!(
            m.state("p"),
            Some(BondState::Finalized {
                reason: FinalizeReason::ProofVerified,
                ..
            })
        ));
    }

    #[test]
    fn slash_routing_conserves_value() {
        let routing = SlashRouting {
            to_caller_bps: 4_000,
            to_challengers_bps: 3_000,
            to_insurance_bps: 2_000,
            to_lineage_bps: 1_000,
        };
        assert!(routing.is_valid());
        let d = routing.distribute(Usdc(1_003)); // odd amount to force dust
        let total = d.caller.0 + d.challengers.0 + d.insurance.0 + d.lineage.0;
        assert_eq!(total, 1_003, "no lamport lost to rounding");
        assert_eq!(d.challengers.0, 300);
        assert_eq!(d.insurance.0, 200);
        assert_eq!(d.lineage.0, 100);
        assert_eq!(d.caller.0, 403, "caller absorbs the dust");
    }

    /// **S-04.** An invalid routing must not be able to pay out more than the bond.
    ///
    /// Two 100 % shares is the witness the audit's Lean model exhibits
    /// (`Audit.Escrow.finding_S04_invalid_routing_overdraws`): against the previous
    /// arithmetic this demanded twice the bond and reached `amount - challengers -
    /// insurance - lineage` as a plain `u64` subtraction, which underflows — a debug
    /// panic, or in release a caller share near `u64::MAX`. The on-chain twin refuses
    /// such a routing at `escrow`; off-chain nothing checked, so conservation is now a
    /// property of the split itself.
    #[test]
    fn invalid_routing_cannot_overdraw() {
        let routing = SlashRouting {
            to_caller_bps: 0,
            to_challengers_bps: 10_000,
            to_insurance_bps: 10_000,
            to_lineage_bps: 0,
        };
        assert!(!routing.is_valid());
        let d = routing.distribute(Usdc(1_000));
        let total = d.caller.0 + d.challengers.0 + d.insurance.0 + d.lineage.0;
        assert_eq!(total, 1_000, "an invalid routing still pays out exactly the bond");
        assert_eq!(d.challengers.0, 1_000, "taken in order against the remainder");
        assert_eq!(d.insurance.0, 0, "nothing left for the second full share");
        assert_eq!(d.caller.0, 0);
    }

    /// The four shares summed must not be able to wrap. Four `u32`s near the top of the
    /// range overflow a `u32` total, and a wrapped total can land on exactly 10 000 —
    /// which would make `is_valid` vouch for a routing paying out several times over.
    #[test]
    fn total_bps_does_not_wrap() {
        let routing = SlashRouting {
            to_caller_bps: u32::MAX,
            to_challengers_bps: u32::MAX,
            to_insurance_bps: u32::MAX,
            to_lineage_bps: u32::MAX,
        };
        assert!(!routing.is_valid());
        let d = routing.distribute(Usdc(1_000));
        assert_eq!(d.caller.0 + d.challengers.0 + d.insurance.0 + d.lineage.0, 1_000);
    }

    /// The checked builder refuses what the debug assertion only complains about.
    #[test]
    fn try_with_slash_routing_refuses_an_invalid_split() {
        let routing = SlashRouting {
            to_caller_bps: 5_000,
            to_challengers_bps: 4_000,
            to_insurance_bps: 0,
            to_lineage_bps: 0,
        };
        assert!(SettlementConfig::optimistic(60)
            .try_with_slash_routing(routing)
            .is_err());
        assert!(SettlementConfig::optimistic(60)
            .try_with_slash_routing(SlashRouting::default())
            .is_ok());
    }

    #[test]
    fn default_routing_sends_slash_to_caller() {
        // Backcompat: default routing == legacy "slash compensates the caller".
        let m = SettlementMachine::new(SettlementConfig::optimistic(60), ManualClock::new(1_000));
        m.open(&bond("d", 500, 0)).unwrap();
        m.provision("d", BondOutcome::Honored).unwrap();
        m.file_challenge("d", "x").unwrap();
        m.resolve_dispute("d", true).unwrap();
        let dist = m.slash_distribution("d").unwrap();
        assert_eq!(dist.caller, Usdc(500));
        assert_eq!(dist.challengers, Usdc(0));
    }
}
