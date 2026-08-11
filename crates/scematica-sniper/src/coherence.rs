//! Epistemic circuit breaker — halt buys when the pipeline is passing pools on
//! ignorance rather than on evidence.
//!
//! Every other breaker in this crate halts on **money**: `ath_tracker` on drawdown,
//! `grief_breaker` on a loss window, `kelly` on win rate. All of them fire *after* the
//! damage. This one fires on the condition that precedes it.
//!
//! The signal comes from a property the filter pipeline already has and does not
//! currently report. RPC-bound checks are capped at `RPC_CALL_TIMEOUT_SECS` and **fail
//! open** — when a node is slow or erroring, `check_mint_renounced`, `check_freezable`,
//! `check_burned` and the rest return `pass()` because they could not look, not because
//! they looked and approved. That is the right call for one pool: dropping every
//! candidate because a node hiccuped would forfeit the edge. It is the wrong state to
//! keep *trading* in, because past some fraction of unresolved checks the pipeline is no
//! longer a filter at all — it is a pass-through wearing a filter's name, and the safety
//! checks the operator believes are running are silently not running.
//!
//! So: count how many checks resolved with real data versus failed open, feed that to
//! the same `scematica-sentience` master equation the API's `/api/sentience` gate uses,
//! and stop buying on HOLD. One definition of Ψ across the system, not two.
//!
//! Deliberately **not** wired into the sell path. A degraded feed is a reason to stop
//! opening new risk, never a reason to stop closing existing risk — a breaker that
//! trapped positions during an RPC brownout would be worse than no breaker.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use scematica_sentience::{
    cognitive_state::CognitiveState, ethics::EthicsInputs, logic::LogicInputs,
    perception::Perception, rationality::RationalityInputs, types::Bounded, Gate, NoClient,
    Overlay,
};

/// Sliding window over which resolution is measured.
const WINDOW: Duration = Duration::from_secs(120);

/// No verdict below this many observations.
///
/// A cold start has resolved 0 of 0 checks, which is not evidence of a problem. Tripping
/// on an empty sample would halt the bot the moment it launched — the breaker would fire
/// hardest exactly when it knows least.
const MIN_SAMPLES: u64 = 20;

/// A feed with no events for this long is stalled, whatever the RPC health looks like.
const FEED_STALL_SECS: f64 = 180.0;

#[derive(Debug, Clone, Copy)]
pub struct Coherence {
    pub psi: f64,
    pub gate: Gate,
    /// Fraction of recent RPC-bound checks that returned real data.
    pub resolution_rate: f64,
    pub resolved: u64,
    pub unresolved: u64,
    /// Seconds since the listener last produced a pool.
    pub feed_age_secs: f64,
    /// False while the sample is too small to judge.
    pub decisive: bool,
}

impl Coherence {
    /// Whether buys should stop. Only a HOLD halts; CAUTION is logged, not enforced.
    pub fn should_halt(&self) -> bool {
        self.decisive && self.gate == Gate::Hold
    }

    pub fn reason(&self) -> String {
        if self.feed_age_secs > FEED_STALL_SECS {
            format!("pool feed stalled for {:.0}s", self.feed_age_secs)
        } else {
            format!(
                "only {:.0}% of {} filter checks resolved — the pipeline is passing pools \
                 it could not verify",
                self.resolution_rate * 100.0,
                self.resolved + self.unresolved
            )
        }
    }
}

struct Window {
    started: Instant,
    resolved: u64,
    unresolved: u64,
}

/// Tracks whether the pipeline's inputs can currently be believed.
pub struct CoherenceBreaker {
    window: Mutex<Window>,
    /// Millis since process start, so the "no event yet" case is distinguishable from
    /// "an event arrived at t=0".
    last_event_ms: AtomicU64,
    origin: Instant,
    enabled: bool,
}

impl CoherenceBreaker {
    pub fn new(enabled: bool) -> Self {
        Self {
            window: Mutex::new(Window { started: Instant::now(), resolved: 0, unresolved: 0 }),
            last_event_ms: AtomicU64::new(0),
            origin: Instant::now(),
            enabled,
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Record one RPC-bound filter observation: did it come back with real data?
    ///
    /// Called from the shared retry helpers rather than from each filter, so a new
    /// filter is instrumented by construction instead of by remembering to.
    pub fn record_check(&self, resolved: bool) {
        if !self.enabled {
            return;
        }
        let mut w = self.window.lock();
        if w.started.elapsed() >= WINDOW {
            // Roll rather than decay: a hard window keeps the arithmetic obvious, and the
            // breaker only needs to answer "is it bad *now*".
            w.started = Instant::now();
            w.resolved = 0;
            w.unresolved = 0;
        }
        if resolved {
            w.resolved += 1;
        } else {
            w.unresolved += 1;
        }
    }

    /// Record that the listener produced a pool.
    pub fn record_pool_seen(&self) {
        self.last_event_ms
            .store(self.origin.elapsed().as_millis() as u64, Ordering::Relaxed);
    }

    fn feed_age_secs(&self) -> f64 {
        let last = self.last_event_ms.load(Ordering::Relaxed);
        if last == 0 {
            // Nothing seen yet. Age from process start, so a listener that never connects
            // eventually reads as stalled instead of as permanently fresh.
            return self.origin.elapsed().as_secs_f64();
        }
        (self.origin.elapsed().as_millis() as u64).saturating_sub(last) as f64 / 1000.0
    }

    pub fn evaluate(&self) -> Coherence {
        let (resolved, unresolved) = {
            let w = self.window.lock();
            (w.resolved, w.unresolved)
        };
        let total = resolved + unresolved;
        let resolution_rate = if total == 0 { 1.0 } else { resolved as f64 / total as f64 };
        let feed_age_secs = self.feed_age_secs();

        let (psi, gate) = assess(resolution_rate, feed_age_secs);

        Coherence {
            psi,
            gate,
            resolution_rate,
            resolved,
            unresolved,
            feed_age_secs,
            decisive: self.enabled && total >= MIN_SAMPLES,
        }
    }
}

/// Feed the measurements to the shared master equation.
///
/// Unmeasured dimensions are 1.0, not a "modest" 0.9 — Ψ is a product of ratios, so
/// anything below 1.0 on a dimension with no instrument behind it is a standing tax that
/// drags a healthy pipeline toward the threshold. Only measured degradation may move the
/// verdict. (The same mistake, made and corrected once already in the API's gate.)
fn assess(resolution_rate: f64, feed_age_secs: f64) -> (f64, Gate) {
    let feed_health = (1.0 - (feed_age_secs / FEED_STALL_SECS)).clamp(0.0, 1.0);

    let mut state = CognitiveState::initial();
    state.perception = Perception::new(1.0, 1.0, feed_health, resolution_rate);
    state.rationality = RationalityInputs::new(resolution_rate, resolution_rate, 1.0, 0.0);
    // A pipeline reporting "passed" for checks it never completed is internally
    // inconsistent, which is exactly the logic term.
    state.logic = LogicInputs::new(1.0, resolution_rate, 1.0, 1.0);
    state.ethics = EthicsInputs::new(1.0, 1.0, 1.0, 1.0);
    state.knowledge_density = Bounded::new(resolution_rate.max(0.1));

    let readout = Overlay::new(NoClient, Some(state)).assess();
    (readout.psi, readout.gate)
}

// ── process-global instance ───────────────────────────────────────────────────
//
// The instrumentation points are the two shared RPC retry helpers in `filters.rs`, which
// are free functions with no handle to anything. Threading an `Arc<CoherenceBreaker>`
// through every filter to reach them would be a large diff across code that has nothing
// to do with this feature.
//
// A process global is honest here rather than a shortcut: the sniper refuses to start if
// another instance holds `scematica-sniper.lock`, so "one breaker per process" and "one
// breaker per bot" are the same statement. Instrumenting the shared helpers also means a
// filter added later is counted by construction instead of by remembering to.

static GLOBAL: std::sync::OnceLock<CoherenceBreaker> = std::sync::OnceLock::new();

/// Install the breaker. Called once from `Sniper::new`.
pub fn init(enabled: bool) {
    let _ = GLOBAL.set(CoherenceBreaker::new(enabled));
}

/// The installed breaker, or a disabled one.
///
/// Uninitialised means some other binary linked this crate — `backtest`, a unit test —
/// and those must neither accumulate counts nor be able to halt anything.
pub fn global() -> &'static CoherenceBreaker {
    GLOBAL.get_or_init(|| CoherenceBreaker::new(false))
}

/// Record one RPC-bound filter observation. Called from the shared retry helpers.
pub fn record_check(resolved: bool) {
    global().record_check(resolved);
}

pub fn record_pool_seen() {
    global().record_pool_seen();
}

pub fn evaluate() -> Coherence {
    global().evaluate()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_healthy_pipeline_does_not_halt() {
        let b = CoherenceBreaker::new(true);
        b.record_pool_seen();
        for _ in 0..50 {
            b.record_check(true);
        }
        let c = b.evaluate();
        assert!(!c.should_halt());
        assert_eq!(c.gate, Gate::Go);
        assert!((c.resolution_rate - 1.0).abs() < 1e-9);
    }

    #[test]
    fn a_pipeline_passing_on_ignorance_halts() {
        let b = CoherenceBreaker::new(true);
        b.record_pool_seen();
        for _ in 0..50 {
            b.record_check(false);
        }
        let c = b.evaluate();
        assert!(c.should_halt());
        assert!(c.reason().contains("could not verify"));
    }

    #[test]
    fn a_cold_start_never_halts() {
        // The breaker knows least at launch; that is not a reason to refuse to trade.
        let b = CoherenceBreaker::new(true);
        b.record_pool_seen();
        for _ in 0..(MIN_SAMPLES - 1) {
            b.record_check(false);
        }
        let c = b.evaluate();
        assert!(!c.decisive);
        assert!(!c.should_halt());
    }

    #[test]
    fn disabled_never_halts_and_records_nothing() {
        let b = CoherenceBreaker::new(false);
        for _ in 0..100 {
            b.record_check(false);
        }
        let c = b.evaluate();
        assert_eq!(c.resolved + c.unresolved, 0);
        assert!(!c.should_halt());
    }

    #[test]
    fn partial_degradation_is_ordered() {
        // Ψ must fall monotonically as resolution drops, or the verdict is arbitrary.
        let (p_good, _) = assess(1.0, 0.0);
        let (p_mid, _) = assess(0.6, 0.0);
        let (p_bad, _) = assess(0.1, 0.0);
        assert!(p_good > p_mid && p_mid > p_bad);
    }

    #[test]
    fn a_stalled_feed_is_reported_as_the_cause() {
        let c = Coherence {
            psi: 0.0,
            gate: Gate::Hold,
            resolution_rate: 1.0,
            resolved: 100,
            unresolved: 0,
            feed_age_secs: FEED_STALL_SECS + 1.0,
            decisive: true,
        };
        assert!(c.reason().contains("stalled"));
    }

    #[test]
    fn caution_does_not_halt() {
        // Only HOLD stops trading. A CAUTION band that halted would make every brief
        // RPC wobble a trading outage.
        let c = Coherence {
            psi: 0.05,
            gate: Gate::Caution,
            resolution_rate: 0.7,
            resolved: 70,
            unresolved: 30,
            feed_age_secs: 1.0,
            decisive: true,
        };
        assert!(!c.should_halt());
    }
}
