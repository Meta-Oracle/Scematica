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
//!
//! # The second failure, and why one dispersion measure was not enough
//!
//! Observed 2026-08-11, three days of it in `scematica-pool-decisions.jsonl`: 264 buys
//! vetoed on `dq_advice`, verdict reported as `consistent` throughout. The Q-vector was
//! `[Hold 42.98, BuyStandard 5.21, BuyAggressive 12.72, SellPartial 43.03, SellAll -7.44]`
//! and the measured `I = 4.8e-3`, comfortably 48× above [`COLLAPSE_THRESHOLD`].
//!
//! The policy was not a constant function — `Q*` really did move about 7% from pool to
//! pool, which is what `I` measures. What never moved was the **argmax**. `SellPartial`
//! won every single evaluation, and the buy actions sat structurally 3–8× below it, so
//! the relative-margin veto in the sniper fired on 100% of candidates.
//!
//! `I` is dispersion of the *value* the policy assigns. That is not the same quantity as
//! dispersion of the *decision* the policy makes, and only the second one is what a gate
//! is made of. A policy can wobble its valuations by 7% while its ranking is frozen; the
//! wobble buys it a passing grade on `I` and the frozen ranking is what actually reaches
//! the trading path. So the window now also carries the argmax, and
//! [`EquationMonitor::action_entropy`] measures whether the decision varies at all.
//!
//! The general lesson, which cost two incidents to learn: **measure the quantity the
//! downstream consumer acts on.** The consumer here acts on the argmax, so the argmax is
//! what has to be shown to carry information.

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

/// Below this normalised argmax entropy the *decision* is treated as frozen.
///
/// Entropy is normalised by `ln(n_actions)`, so the scale is 0 (the same action every
/// time) to 1 (uniform over actions). The arithmetic that sets the value, for a
/// 32-sample window over 5 actions: one dissenting sample scores 0.086, two score 0.145,
/// and three score 0.19. At 0.15 the guard fires when at most two evaluations in a full
/// window chose differently — "effectively always the same answer" — and clears as soon
/// as the policy is genuinely picking between actions.
///
/// Deliberately not zero. A test for *exactly* constant argmax is trivially defeated by
/// a single exploratory action per window, which is the same mistake as testing Q* for
/// exact constancy: it catches the textbook case and misses the real one.
pub const ACTION_ENTROPY_THRESHOLD: f64 = 0.15;

/// A single evaluation's contribution to the population statistics.
#[derive(Debug, Clone, Copy)]
pub struct Evaluation {
    /// `Q* = max_a Q(s,a)` for this input.
    pub q_star: f64,
    /// Mean Q across all actions — the value of acting at random, `Q̄_rand`.
    pub q_mean: f64,
    /// `argmax_a Q(s,a)` — the action this evaluation actually chose.
    ///
    /// Carried because the downstream veto acts on the choice, not on the value. See
    /// the module docs for the incident that made the distinction expensive.
    pub argmax: usize,
    /// How many actions the Q-vector ranked, for entropy normalisation.
    pub n_actions: usize,
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
        let argmax = q_values
            .iter()
            .enumerate()
            .fold(
                (0usize, f64::NEG_INFINITY),
                |(bi, bq), (i, &q)| if q > bq { (i, q) } else { (bi, bq) },
            )
            .0;
        Some(Self { q_star, q_mean, argmax, n_actions: q_values.len() })
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
    /// The *decision* is frozen even though the values move: argmax entropy below
    /// [`ACTION_ENTROPY_THRESHOLD`]. The policy ranks the same action first on
    /// essentially every input, so its output carries no per-pool information however
    /// well its valuations disperse. Distinct from [`Verdict::Collapsed`] so the stats
    /// file names which of the two failures occurred.
    ActionCollapsed,
    /// `ρ > 0` — measured dispersion above prediction. Value estimates unstable
    /// relative to accumulated experience; argues for more training, not less authority.
    Unstable,
}

impl Verdict {
    /// May the agent veto a buy outright on this verdict?
    ///
    /// Neither collapse may. Such a policy keeps position-sizing influence and keeps
    /// training — it simply cannot hold the gate shut while saying the same thing about
    /// every pool it sees. `Unstable` retains the veto: a noisy net is still reading its
    /// input, which is the property the veto depends on.
    pub fn may_veto(self) -> bool {
        !matches!(self, Verdict::Collapsed | Verdict::ActionCollapsed)
    }

    pub fn label(self) -> &'static str {
        match self {
            Verdict::Indeterminate => "indeterminate",
            Verdict::Consistent => "consistent",
            Verdict::Collapsed => "collapsed",
            Verdict::ActionCollapsed => "action_collapsed",
            Verdict::Unstable => "unstable",
        }
    }
}

/// Snapshot of the equation terms, serialised into `scematica-nn-stats.json`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EquationStats {
    /// Measured intelligence ratio `I = Var_p[Q*] / E_p[Q*]²`.
    pub intelligence_ratio: f64,
    /// Normalised argmax entropy over the window — dispersion of the *decision*.
    ///
    /// Reported alongside `intelligence_ratio` because the two failed independently:
    /// on 2026-08-11 this was ~0 while `intelligence_ratio` read a healthy 4.8e-3.
    #[serde(default)]
    pub action_entropy: f64,
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

    /// Normalised Shannon entropy of the argmax distribution over the window, in `[0, 1]`.
    ///
    /// This is the dispersion of the *decision*, where [`Self::intelligence_ratio`] is the
    /// dispersion of the *value*. `0.0` means the policy chose the same action on every
    /// evaluation in the window; `1.0` means it spread choices uniformly across actions.
    ///
    /// Normalisation is by `ln(n_actions)` from the observed vectors rather than by the
    /// number of *distinct* actions seen — dividing by the latter would rescale a frozen
    /// policy's entropy to look healthy, since a policy that only ever picks one action
    /// has `ln(1) = 0` in the denominator.
    pub fn action_entropy(&self) -> f64 {
        if self.window.is_empty() {
            return 0.0;
        }
        let n_actions = self.window.iter().map(|e| e.n_actions).max().unwrap_or(1);
        if n_actions < 2 {
            // One action is not a choice, so there is no decision to disperse. Report the
            // maximum rather than 0.0: the network is not withholding information here,
            // there is none to withhold, and a 0.0 would brand it collapsed forever.
            return 1.0;
        }
        let mut counts = vec![0usize; n_actions];
        for e in &self.window {
            if let Some(c) = counts.get_mut(e.argmax) {
                *c += 1;
            }
        }
        let total = self.window.len() as f64;
        let entropy: f64 = counts
            .iter()
            .filter(|&&c| c > 0)
            .map(|&c| {
                let p = c as f64 / total;
                -p * p.ln()
            })
            .sum();
        entropy / (n_actions as f64).ln()
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
        // Checked after the value test and before the residual, because a frozen decision
        // is a stronger disqualification than a residual of either sign: `ρ` compares two
        // estimates of how much the *values* spread, and neither of them is evidence that
        // the policy ever picks differently.
        if self.action_entropy() < ACTION_ENTROPY_THRESHOLD {
            return Verdict::ActionCollapsed;
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
            action_entropy: self.action_entropy(),
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

    /// Value-dispersion fixture. The argmax is *rotated* across the five actions so these
    /// cases exercise the `I` path only — a frozen argmax would trip the action guard and
    /// mask whatever the test was actually about.
    fn monitor_with(q_stars: &[f64], q_mean_offset: f64) -> EquationMonitor {
        let mut m = EquationMonitor::new(DISPERSION_WINDOW);
        for (i, &q) in q_stars.iter().enumerate() {
            m.observe(Evaluation {
                q_star: q,
                q_mean: q - q_mean_offset,
                argmax: i % 5,
                n_actions: 5,
            });
        }
        m
    }

    /// Decision-dispersion fixture: every evaluation ranks `argmax` first.
    fn monitor_frozen_argmax(q_stars: &[f64], q_mean_offset: f64, argmax: usize) -> EquationMonitor {
        let mut m = EquationMonitor::new(DISPERSION_WINDOW);
        for &q in q_stars {
            m.observe(Evaluation { q_star: q, q_mean: q - q_mean_offset, argmax, n_actions: 5 });
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

    /// The exact 2026-08-11 production Q-vector, which the value test cleared.
    ///
    /// Q* moves ~7% pool to pool, so `I` lands 48× above the collapse threshold and the
    /// old classifier returned `Consistent` — granting unlimited veto authority to a
    /// policy that had ranked `SellPartial` first on 264 consecutive candidates.
    #[test]
    fn frozen_argmax_is_caught_even_when_values_disperse() {
        let mut m = EquationMonitor::new(DISPERSION_WINDOW);
        for i in 0..32 {
            // Vary the vector by the same ~7% observed in the field, without ever
            // letting a buy action overtake SellPartial.
            let k = 1.0 + (i as f64 % 8.0) * 0.01;
            let q = [42.98 * k, 5.21 * k, 12.72 * k, 43.03 * k, -7.44 * k];
            m.observe_q_values(&q);
        }

        assert!(
            m.intelligence_ratio() > COLLAPSE_THRESHOLD,
            "fixture must reproduce the field condition: values disperse (I = {:.3e})",
            m.intelligence_ratio()
        );
        assert_eq!(m.action_entropy(), 0.0, "the decision never varied");
        assert_eq!(
            m.verdict(0.5, 0.05, 35_964),
            Verdict::ActionCollapsed,
            "a frozen ranking must not be laundered into veto authority by value wobble"
        );
        assert!(!m.verdict(0.5, 0.05, 35_964).may_veto());
    }

    /// The guard must not fire on a policy that merely *prefers* one action while still
    /// picking others — otherwise it would strip the veto from a working net.
    #[test]
    fn a_strong_but_genuine_preference_keeps_its_veto() {
        let mut m = EquationMonitor::new(DISPERSION_WINDOW);
        for i in 0..32 {
            // SellPartial wins 3 of every 4, a buy wins the fourth.
            let q = if i % 4 == 3 {
                [10.0, 30.0 + i as f64, 12.0, 9.0, -5.0]
            } else {
                [10.0, 5.0, 12.0, 40.0 + i as f64, -5.0]
            };
            m.observe_q_values(&q);
        }
        assert!(
            m.action_entropy() > ACTION_ENTROPY_THRESHOLD,
            "entropy {} should clear the threshold",
            m.action_entropy()
        );
        assert!(m.verdict(0.5, 0.05, 35_964).may_veto());
    }

    /// A single dissenting evaluation must not buy back veto authority for a frozen net.
    #[test]
    fn one_dissenting_sample_does_not_clear_the_action_guard() {
        let mut m = EquationMonitor::new(DISPERSION_WINDOW);
        for i in 0..32 {
            let q = if i == 17 {
                [10.0, 50.0, 12.0, 9.0, -5.0]
            } else {
                [10.0, 5.0, 12.0, 43.0 + i as f64 * 0.3, -5.0]
            };
            m.observe_q_values(&q);
        }
        assert!(m.action_entropy() < ACTION_ENTROPY_THRESHOLD, "got {}", m.action_entropy());
        assert_eq!(m.verdict(0.5, 0.05, 35_964), Verdict::ActionCollapsed);
    }

    #[test]
    fn from_q_values_records_the_argmax() {
        let e = Evaluation::from_q_values(&[42.98, 5.21, 12.72, 43.03, -7.44]).unwrap();
        assert_eq!(e.argmax, 3, "SellPartial is index 3");
        assert_eq!(e.n_actions, 5);
    }

    /// Frozen argmax is not judged before the window is populated, same as the value test.
    #[test]
    fn action_guard_respects_the_sample_floor() {
        let m = monitor_frozen_argmax(&[26.5, 27.1, 25.9, 26.8], 13.0, 3);
        assert_eq!(m.verdict(0.5, 0.05, 35_964), Verdict::Indeterminate);
        assert!(m.verdict(0.5, 0.05, 35_964).may_veto(), "silence must not disarm the veto");
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
            m.observe(Evaluation { q_star: i as f64, q_mean: 0.0, argmax: i % 5, n_actions: 5 });
        }
        assert_eq!(m.samples(), 8);
    }
}
