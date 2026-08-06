//! The Scematica equations, as running instrumentation.
//!
//! Reference: `EQUATIONS.md` (statements) and `EQUATIONS-ANALYSIS.md` (derivations).
//!
//! Three definitional relations govern the agent:
//!
//! ```text
//!   I.    E    = E_Σ[ Y · (b − L) · (AI · ε) ]     the edge
//!   II.   AI   = E_Σ · ε · ν                        capability
//!   III.  AI²  = I · q · ε                          value identity
//! ```
//!
//! II and III both determine `AI`, which over-determines the system. Substituting one
//! into the other predicts the intelligence ratio:
//!
//! ```text
//!   I_predicted = (E_Σ² · ε · ν²) / q
//! ```
//!
//! But `I` is *also* defined independently, as the normalised dispersion of Q* across
//! the evaluated population. Two routes to one quantity is a constraint, and the
//! residual between them is what this module computes.
//!
//! # Why this exists
//!
//! The failure it was written for: on 2026-08-05 the agent returned `SellPartial` for 25
//! consecutive pools at Q* ≈ 26.5 against a best-buy Q of ≈ 13.5. Every candidate was
//! vetoed. The deployed guard tested whether the bearish Q exceeded the best buy Q by a
//! relative margin — and a collapsed policy passes a margin test *trivially*, because
//! collapse produces a large, stable gap on every input. The guard read the collapse as
//! maximum conviction, which on its own terms it was.
//!
//! Magnitude cannot detect collapse. Only dispersion can. A model returning the same
//! value for every input has `Var[Q*] = 0` and therefore `I = 0`, however large that
//! value is and however decisively it beats the alternatives. Confidence is a property
//! of the model; information is a property of the relationship between the model and its
//! input, and it is measured by variance.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Episodes normalisation constant `N₀` from the symbol table.
pub const REFERENCE_EPISODES: f64 = 10_000.0;

/// Window over which Q* dispersion is measured.
///
/// Long enough that a genuine run of similar pools does not read as collapse, short
/// enough to detect one inside a single session. The observed collapse held for 25
/// consecutive evaluations, so a 32-sample window sees it fully formed.
pub const DISPERSION_WINDOW: usize = 32;

/// Minimum samples before the intelligence ratio is considered meaningful.
///
/// Variance over a handful of points is dominated by sampling noise, and reporting it
/// would hand the veto logic a number that swings on its own arithmetic.
pub const MIN_DISPERSION_SAMPLES: usize = 12;

/// Below this intelligence ratio the policy is treated as non-discriminating.
///
/// `I` is a squared coefficient of variation, so 1e-4 corresponds to a relative spread
/// of about 1% around the mean Q*. A policy whose valuations vary by less than one
/// percent across genuinely different pools is not reading the pools.
pub const COLLAPSE_THRESHOLD: f64 = 1e-4;

/// A single evaluation's contribution to the population statistics.
#[derive(Debug, Clone, Copy)]
pub struct Evaluation {
    /// `Q* = max_a Q(s,a)` for this input.
    pub q_star: f64,
    /// Mean Q across all actions — the value of acting at random, `Q̄_rand`.
    pub q_mean: f64,
}

impl Evaluation {
    /// Build from a full Q-vector. Returns `None` for an empty or non-finite vector,
    /// which is what an untrained network emits and must not be counted as a sample.
    pub fn from_q_values(q_values: &[f64]) -> Option<Self> {
        if q_values.is_empty() || !q_values.iter().all(|q| q.is_finite()) {
            return None;
        }
        let q_star = q_values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let q_mean = q_values.iter().sum::<f64>() / q_values.len() as f64;
        Some(Self { q_star, q_mean })
    }

    /// Policy advantage `ΔQ = Q* − Q̄_rand`, the value the policy adds over random action.
    pub fn advantage(&self) -> f64 {
        self.q_star - self.q_mean
    }
}

/// What the constraint says about the agent right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Verdict {
    /// Too few samples to judge. The agent keeps whatever authority it already had.
    Indeterminate,
    /// Measured and predicted dispersion agree. Components are mutually consistent.
    Consistent,
    /// `ρ < 0` — measured dispersion below prediction. Confident but uninformative.
    Collapsed,
    /// `ρ > 0` — measured dispersion above prediction. Value estimates unstable
    /// relative to accumulated experience; argues for more training, not less authority.
    Unstable,
}

impl Verdict {
    /// May the agent veto a buy outright on this verdict?
    ///
    /// A collapsed policy may not. It keeps position-sizing influence and keeps
    /// training — it simply cannot hold the gate shut while saying the same thing about
    /// every pool it sees. `Unstable` retains the veto: a noisy net is still reading its
    /// input, which is the property the veto depends on.
    pub fn may_veto(self) -> bool {
        !matches!(self, Verdict::Collapsed)
    }

    pub fn label(self) -> &'static str {
        match self {
            Verdict::Indeterminate => "indeterminate",
            Verdict::Consistent => "consistent",
            Verdict::Collapsed => "collapsed",
            Verdict::Unstable => "unstable",
        }
    }
}

/// Snapshot of the equation terms, serialised into `scematica-nn-stats.json`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EquationStats {
    /// Measured intelligence ratio `I = Var_p[Q*] / E_p[Q*]²`.
    pub intelligence_ratio: f64,
    /// Predicted `I = (E_Σ² · ε · ν²) / q` from equations II and III.
    pub intelligence_predicted: f64,
    /// Consistency residual `ρ = measured − predicted`.
    pub residual: f64,
    /// Mean policy advantage `ΔQ` over the window.
    pub advantage: f64,
    /// Optimal exploration rate `ε* = (2/3)(Q*/ΔQ)`, `None` where `ΔQ ≤ 0`.
    pub epsilon_star: Option<f64>,
    /// Capture coefficient `Y` — confirmed buys / attempted buys.
    pub capture: f64,
    /// Samples currently in the dispersion window.
    pub samples: usize,
    pub verdict: String,
}

/// Rolling computation of the equation terms over recent evaluations.
#[derive(Debug, Clone)]
pub struct EquationMonitor {
    window: VecDeque<Evaluation>,
    capacity: usize,
    buys_attempted: u64,
    buys_confirmed: u64,
}

impl Default for EquationMonitor {
    fn default() -> Self {
        Self::new(DISPERSION_WINDOW)
    }
}

impl EquationMonitor {
    pub fn new(capacity: usize) -> Self {
        Self {
            window: VecDeque::with_capacity(capacity.max(1)),
            capacity: capacity.max(1),
            buys_attempted: 0,
            buys_confirmed: 0,
        }
    }

    /// Record one evaluation of the network against one candidate.
    pub fn observe(&mut self, eval: Evaluation) {
        if self.window.len() == self.capacity {
            self.window.pop_front();
        }
        self.window.push_back(eval);
    }

    /// Record a Q-vector directly. Non-finite or empty vectors are ignored.
    pub fn observe_q_values(&mut self, q_values: &[f64]) {
        if let Some(eval) = Evaluation::from_q_values(q_values) {
            self.observe(eval);
        }
    }

    /// Record the outcome of a buy attempt, for the capture coefficient `Y`.
    pub fn record_buy(&mut self, confirmed: bool) {
        self.buys_attempted += 1;
        if confirmed {
            self.buys_confirmed += 1;
        }
    }

    pub fn samples(&self) -> usize {
        self.window.len()
    }

    /// Mean of `Q*` over the window.
    pub fn mean_q_star(&self) -> f64 {
        if self.window.is_empty() {
            return 0.0;
        }
        self.window.iter().map(|e| e.q_star).sum::<f64>() / self.window.len() as f64
    }

    /// Population variance of `Q*` over the window.
    pub fn variance_q_star(&self) -> f64 {
        let n = self.window.len();
        if n < 2 {
            return 0.0;
        }
        let mean = self.mean_q_star();
        self.window
            .iter()
            .map(|e| {
                let d = e.q_star - mean;
                d * d
            })
            .sum::<f64>()
            / n as f64
    }

    /// Mean policy advantage `ΔQ = Q* − Q̄_rand`.
    pub fn advantage(&self) -> f64 {
        if self.window.is_empty() {
            return 0.0;
        }
        self.window.iter().map(|e| e.advantage()).sum::<f64>() / self.window.len() as f64
    }

    /// Measured intelligence ratio `I = Var_p[Q*] / E_p[Q*]²`.
    ///
    /// Returns 0.0 when the mean is ~0, which is the correct reading rather than a
    /// division blow-up: a network centred on zero with no spread carries no
    /// information either.
    pub fn intelligence_ratio(&self) -> f64 {
        let mean = self.mean_q_star();
        if mean.abs() < f64::EPSILON {
            return 0.0;
        }
        self.variance_q_star() / (mean * mean)
    }

    /// Predicted intelligence ratio `I = (E_Σ² · ε · ν²) / q`, from equations II and III.
    ///
    /// `expected_return` is `E_Σ`, `episodes` is `N` (normalised internally by `N₀`).
    /// `q` is taken as the window's mean `Q*`, the population's normalised value term.
    pub fn intelligence_predicted(&self, expected_return: f64, epsilon: f64, episodes: usize) -> f64 {
        let q = self.mean_q_star();
        if q.abs() < f64::EPSILON {
            return 0.0;
        }
        let nu = episodes as f64 / REFERENCE_EPISODES;
        (expected_return * expected_return * epsilon * nu * nu) / q
    }

    /// Consistency residual `ρ = I_measured − I_predicted`.
    pub fn residual(&self, expected_return: f64, epsilon: f64, episodes: usize) -> f64 {
        self.intelligence_ratio() - self.intelligence_predicted(expected_return, epsilon, episodes)
    }

    /// Optimal exploration rate `ε* = (2/3)(Q*/ΔQ)`.
    ///
    /// `None` where `ΔQ ≤ 0` — a policy with no advantage over random action has no
    /// interior optimum, and the formal limit `ε* → ∞` is the statement that it should
    /// be exploring rather than acting.
    pub fn epsilon_star(&self) -> Option<f64> {
        let advantage = self.advantage();
        if advantage <= f64::EPSILON {
            return None;
        }
        Some((2.0 / 3.0) * (self.mean_q_star() / advantage))
    }

    /// Capture coefficient `Y` — the fraction of attempted buys that confirmed.
    ///
    /// This is the multiplier on the whole of equation I. With no attempts yet it reads
    /// 1.0, so an idle system is not reported as a failing one.
    pub fn capture(&self) -> f64 {
        if self.buys_attempted == 0 {
            return 1.0;
        }
        self.buys_confirmed as f64 / self.buys_attempted as f64
    }

    /// Classify the agent against the constraint.
    ///
    /// The collapse test is on the *measured* ratio directly rather than on the residual
    /// sign alone. The residual can be driven negative by a large predicted term while
    /// the network is still discriminating perfectly well, and demoting a working policy
    /// because its expected return rose would be exactly the wrong response.
    pub fn verdict(&self, expected_return: f64, epsilon: f64, episodes: usize) -> Verdict {
        if self.window.len() < MIN_DISPERSION_SAMPLES {
            return Verdict::Indeterminate;
        }
        if self.intelligence_ratio() < COLLAPSE_THRESHOLD {
            return Verdict::Collapsed;
        }
        let rho = self.residual(expected_return, epsilon, episodes);
        if rho > 0.0 {
            Verdict::Unstable
        } else {
            Verdict::Consistent
        }
    }

    /// Full snapshot for serialisation.
    pub fn stats(&self, expected_return: f64, epsilon: f64, episodes: usize) -> EquationStats {
        EquationStats {
            intelligence_ratio: self.intelligence_ratio(),
            intelligence_predicted: self.intelligence_predicted(expected_return, epsilon, episodes),
            residual: self.residual(expected_return, epsilon, episodes),
            advantage: self.advantage(),
            epsilon_star: self.epsilon_star(),
            capture: self.capture(),
            samples: self.window.len(),
            verdict: self.verdict(expected_return, epsilon, episodes).label().to_string(),
        }
    }
}

/// Kelly fraction `f* = W − (1−W)/R` from win rate and payoff ratio.
///
/// Clamped to `[0, 1]`: a negative Kelly means no edge, and the correct size is zero
/// rather than a short position the sniper cannot express.
pub fn kelly_fraction(win_rate: f64, payoff_ratio: f64) -> f64 {
    if payoff_ratio <= 0.0 {
        return 0.0;
    }
    (win_rate - (1.0 - win_rate) / payoff_ratio).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor_with(q_stars: &[f64], q_mean_offset: f64) -> EquationMonitor {
        let mut m = EquationMonitor::new(DISPERSION_WINDOW);
        for &q in q_stars {
            m.observe(Evaluation { q_star: q, q_mean: q - q_mean_offset });
        }
        m
    }

    /// The exact production signature: 25 evaluations at Q* ≈ 26.5 with no spread.
    #[test]
    fn constant_q_star_is_classified_as_collapsed() {
        let m = monitor_with(&[26.5; 25], 13.0);
        assert_eq!(m.intelligence_ratio(), 0.0, "constant output must have zero dispersion");
        assert_eq!(m.verdict(0.5, 0.068, 20_500), Verdict::Collapsed);
        assert!(!m.verdict(0.5, 0.068, 20_500).may_veto(), "a collapsed policy must not veto");
    }

    /// The failure the margin guard could not see: a huge, stable Q-gap is not information.
    #[test]
    fn large_confident_gap_does_not_rescue_a_collapsed_policy() {
        // sell_q 26.5 against buy_q 13.5 clears any relative margin test by ~96%.
        let m = monitor_with(&[26.5; 25], 13.0);
        assert!(m.advantage() > 12.0, "the gap is genuinely large");
        assert_eq!(
            m.verdict(0.5, 0.068, 20_500),
            Verdict::Collapsed,
            "magnitude must not launder a constant function into a signal"
        );
    }

    /// A network that reads its input keeps its veto.
    #[test]
    fn discriminating_policy_retains_veto_authority() {
        let varied: Vec<f64> = (0..24).map(|i| 10.0 + i as f64 * 1.5).collect();
        let m = monitor_with(&varied, 4.0);
        assert!(m.intelligence_ratio() > COLLAPSE_THRESHOLD);
        assert!(m.verdict(0.5, 0.068, 20_500).may_veto());
    }

    #[test]
    fn insufficient_samples_are_indeterminate_not_collapsed() {
        let m = monitor_with(&[26.5; 4], 13.0);
        assert_eq!(m.verdict(0.5, 0.068, 20_500), Verdict::Indeterminate);
        assert!(m.verdict(0.5, 0.068, 20_500).may_veto(), "silence must not disarm the veto");
    }

    #[test]
    fn epsilon_star_follows_the_advantage_ratio() {
        // ΔQ = 10, Q* = 20  ->  ε* = (2/3)(20/10) = 4/3
        let m = monitor_with(&[20.0; 16], 10.0);
        let eps = m.epsilon_star().expect("positive advantage yields an optimum");
        assert!((eps - 4.0 / 3.0).abs() < 1e-9, "got {eps}");
    }

    #[test]
    fn no_advantage_yields_no_interior_optimum() {
        // ΔQ = 0: the policy does not beat random, so ε* diverges.
        let m = monitor_with(&[20.0; 16], 0.0);
        assert!(m.epsilon_star().is_none());
    }

    #[test]
    fn capture_tracks_fill_rate() {
        let mut m = EquationMonitor::default();
        assert_eq!(m.capture(), 1.0, "an idle system is not a failing one");
        m.record_buy(true);
        m.record_buy(false);
        assert!((m.capture() - 0.5).abs() < 1e-12, "1 of 2 confirmed is Y = 0.5");
    }

    #[test]
    fn kelly_matches_the_measured_constants() {
        // W = 0.30986, R = 14.4647 from 639 closed positions -> f* = 0.26215
        let f = kelly_fraction(0.30986, 14.4647);
        assert!((f - 0.26215).abs() < 1e-4, "got {f}");
    }

    #[test]
    fn kelly_floors_at_zero_without_an_edge() {
        assert_eq!(kelly_fraction(0.10, 1.2), 0.0);
        assert_eq!(kelly_fraction(0.5, 0.0), 0.0);
    }

    #[test]
    fn non_finite_q_values_are_never_sampled() {
        let mut m = EquationMonitor::default();
        m.observe_q_values(&[f64::NAN, 1.0]);
        m.observe_q_values(&[]);
        assert_eq!(m.samples(), 0, "an untrained net must not pollute the window");
    }

    #[test]
    fn window_is_bounded() {
        let mut m = EquationMonitor::new(8);
        for i in 0..50 {
            m.observe(Evaluation { q_star: i as f64, q_mean: 0.0 });
        }
        assert_eq!(m.samples(), 8);
    }
}
