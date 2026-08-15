//! The agentic gate: §16, §17, §20, §22, §31, §32 and §33 of the Agentic Neural
//! Architecture spec, computed over the **observed** mesh.
//!
//! The mesh is the only thing in this system that sees every subsystem at once, which
//! makes it the only place the coherence equations can honestly be evaluated. A subsystem
//! cannot measure its own agreement with the others.
//!
//! ## The one deliberate deviation from the spec, and why
//!
//! §17 defines confidence as `C = 1 − σ(λ₁U_A + λ₂U_E + λ₃N + λ₄D)` and §34 defines the
//! composite gate as a **product** of many factors. Implemented literally, both have the
//! same defect: an unimplemented subsystem contributes either `σ(0) = 0.5` or a factor of
//! `0`, and the gate is pinned low — or shut — by dimensions nobody has built yet rather
//! than by anything observed. This repository has already paid for that lesson once, in
//! the `Perception` data ratio of `scematica-sentience`, where an unmeasured channel
//! scored `0` jammed Ψ at `0` permanently.
//!
//! So the rule here is: **an unmeasured dimension is not a limiting factor.** It
//! contributes the neutral element (1.0 multiplicative, 0.0 additive) and is flagged
//! `measured: false`. Only measured degradation moves a verdict. The cost of the rule is
//! that a gate reading 0.95 may be doing so on two inputs out of nine, so every result
//! carries [`Cognition::measured_fraction`] and the renderer is expected to show it beside
//! the number. A confident gate over an unmeasured system is a statement about ignorance,
//! and it must look like one.
//!
//! Consequently `C = 1 − tanh(Σλᵢuᵢ / 2)` rather than the spec's raw sigmoid: same shape,
//! same saturation, but anchored so that zero measured degradation yields exactly 1.0
//! instead of 0.5.
//!
//! ## Relationship to the Ψ that already exists
//!
//! `scematica-sentience` already defines a Ψ, used by `/api/sentience` and the sniper's
//! `coherence.rs` breaker. That one asks **"can this data be trusted?"** — it is a data
//! integrity measure. The Ψ here is §32's *agentic coherence gate*, which asks **"do the
//! subsystems agree, and is acting safe?"**. They are different questions and both are
//! useful; this module does not redefine the other one, and the two compose — the existing
//! Ψ is a natural input to `C` once it is wired through.

use serde::{Deserialize, Serialize};

use crate::node::{Node, Provenance, Verdict};

/// One term entering an equation, carrying whether it was actually measured.
///
/// This type is the whole honesty mechanism. Every number below is accompanied by the
/// evidence behind it, so a reader can tell a gate that is open because everything is
/// fine from a gate that is open because nothing was checked.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Term {
    /// Symbol as written in the spec, e.g. `U_E`.
    ///
    /// `String` rather than `&'static str` only because `Deserialize` cannot produce a
    /// borrowed static; the constructors still take `&'static str`, so a term can only
    /// ever cite a literal and never a runtime-assembled label.
    pub symbol: String,
    /// Spec section this term comes from, e.g. `§16`.
    pub section: String,
    pub name: String,
    /// The value used in the computation. For an unmeasured term this is the neutral
    /// element, never a guess.
    pub value: f64,
    pub measured: bool,
    /// What was measured, or what would have to exist to measure it.
    pub note: String,
}

impl Term {
    fn measured(symbol: &'static str, section: &'static str, name: &'static str, value: f64, note: impl Into<String>) -> Self {
        Term { symbol: symbol.into(), section: section.into(), name: name.into(), value, measured: true, note: note.into() }
    }

    /// An unmeasured term takes the neutral element for its position in the equation.
    fn absent(symbol: &'static str, section: &'static str, name: &'static str, neutral: f64, note: impl Into<String>) -> Self {
        Term { symbol: symbol.into(), section: section.into(), name: name.into(), value: neutral, measured: false, note: note.into() }
    }
}

/// Raw numbers the collector recovered, which the topology alone does not carry.
#[derive(Clone, Debug, Default)]
pub struct Signals {
    pub q_values: Option<Vec<f64>>,
    /// Per-variant cumulative reward from the tournament — the only ensemble this system
    /// currently has, and therefore the only source of epistemic uncertainty (§16).
    pub variant_rewards: Option<Vec<f64>>,
    /// `equations.intelligence_ratio` from the NN stats: squared coefficient of variation
    /// of Q*, i.e. whether the policy discriminates between inputs at all.
    pub intelligence_ratio: Option<f64>,
    pub trades_attempted: Option<f64>,
    pub trades_failed: Option<f64>,
    /// Whether the Dreamer-style latent world model is switched on. When it is not, the
    /// aleatoric branch of §16 has no covariance to read.
    pub world_model_active: bool,
    /// World-model reconstruction error, when the model runs. §14 gives novelty as
    /// `‖z − ẑ‖`, and this is exactly that quantity under a different name.
    pub wm_recon: Option<f64>,
    /// Whether QR-DQN distributional returns are enabled. The quantile spread of a
    /// distributional agent IS aleatoric uncertainty (§16 `E[Σ_ψ]`) — irreducible spread
    /// of the return distribution — so it becomes measurable the moment this is on.
    pub distributional: bool,
    /// Spread of the learned return distribution, when distributional mode is on.
    pub quantile_spread: Option<f64>,
    /// §20 `R_drawdown` — deepest peak-to-trough fall of the realised equity curve.
    pub drawdown: Option<f64>,
    /// §20 `R_volatility` — (σ of realised return %, mapped risk).
    pub volatility: Option<(f64, f64)>,
    /// §20 `R_liquidity` — (median pool depth in SOL, mapped risk).
    pub liquidity: Option<(f64, f64)>,
    /// §20 `R_concentration` — (open positions, Herfindahl index).
    pub concentration: Option<(usize, f64)>,
    /// Closed trades in the recent window, for term notes.
    pub closes: usize,
}

/// §16 — uncertainty decomposition, `U = U_aleatoric + U_epistemic`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Uncertainty {
    pub aleatoric: Term,
    pub epistemic: Term,
    pub total: f64,
}

/// §20 — the risk field, a weighted sum over six components normalised to [0,1].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RiskField {
    pub components: Vec<Term>,
    pub value: f64,
}

/// §31 — cognitive coherence `K = 1 − (1/N)Σ D(Yᵢ, Ȳ)`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Coherence {
    pub value: f64,
    /// Live subsystems that emitted a comparable output.
    pub subsystems: usize,
    /// Fraction of them dissenting from the majority verdict.
    pub disagreement: f64,
    /// The spec's `Yᵢ` are continuous subsystem outputs. Only discrete verdicts exist in
    /// this system today, so `D` is a discrete disagreement measure — an approximation,
    /// recorded as one rather than presented as the real thing.
    pub approximation: bool,
    pub note: String,
}

/// §22 — act, damp, or decline to act.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GateVerdict {
    /// Ψ is high: confident, coherent, low risk.
    Act,
    /// Ψ is middling — size down rather than abstain.
    Damp,
    /// Ψ is below τ_Ψ: the agent should not act.
    Abstain,
    /// Nothing was measured. **Not the same as Abstain**: a gate with no evidence has not
    /// decided against acting, it has failed to evaluate, and reporting that as a
    /// considered refusal would be inventing a judgement.
    Unevaluated,
}

/// The assembled gate: §32 `Ψ = C · K · (1 − R)`, plus §33 `Ω`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Cognition {
    /// §17 confidence, anchored so zero measured degradation is 1.0.
    pub confidence: f64,
    pub confidence_terms: Vec<Term>,
    pub uncertainty: Uncertainty,
    pub risk: RiskField,
    pub coherence: Coherence,
    /// §32 — the agentic coherence gate.
    pub psi: f64,
    pub verdict: GateVerdict,
    /// §33 — `Ω = Ψ[αH + βM + γG + δW + εP]`.
    ///
    /// `None` whenever no bracket term is measured, which is the current state: history,
    /// memory, goals, world model and planning are the five subsystems the spec adds and
    /// none of them exist yet. Emitting a number for Ω today would be emitting a number
    /// for a system that has not been built.
    pub omega: Option<f64>,
    pub omega_terms: Vec<Term>,
    /// Fraction of all terms that were actually measured, 0..1. Belongs on screen next to
    /// Ψ, always.
    pub measured_fraction: f64,
    /// One sentence a human can act on.
    pub reading: String,
}

/// Below this Ψ the agent should not act (§22 τ_Ψ).
pub const TAU_PSI: f64 = 0.45;
/// Between τ_Ψ and this, act with reduced size rather than full conviction.
pub const TAU_PSI_FULL: f64 = 0.75;

/// λ weights for §17's confidence terms.
const LAMBDA_UA: f64 = 1.0;
const LAMBDA_UE: f64 = 1.0;
const LAMBDA_N: f64 = 0.5;
const LAMBDA_D: f64 = 1.0;

/// Evaluate the gate over an observed mesh.
pub fn assess(nodes: &[Node], s: &Signals) -> Cognition {
    let uncertainty = uncertainty(s);
    let coherence = coherence(nodes);
    let risk = risk_field(s);

    // §17, in the anchored form described in the module docs.
    // §14 gives novelty as ‖z − ẑ‖. The world model's reconstruction error is precisely
    // that quantity, so novelty becomes measurable whenever the world model runs — no
    // separate perception encoder required.
    let novelty = match (s.world_model_active, s.wm_recon) {
        (true, Some(recon)) => Term::measured(
            "N_t",
            "§14",
            "novelty",
            recon.clamp(0.0, 1.0),
            format!("world-model reconstruction error {recon:.3} — this IS ‖z − ẑ‖"),
        ),
        (true, None) => Term::absent("N_t", "§14", "novelty", 0.0, "world model is on but reconstruction error is not exported"),
        _ => Term::absent(
            "N_t",
            "§14",
            "novelty",
            0.0,
            "needs the latent world model (world_model.rs) — novelty is ‖z − ẑ‖ and there is no ẑ without it",
        ),
    };
    // Disagreement among zero subsystems is not agreement — it is nothing observed. A
    // term that reports `measured` here would inflate `measured_fraction` with a
    // measurement of the empty set, which is precisely the sort of confident-looking
    // nothing this module exists to prevent.
    let disagreement = if coherence.subsystems == 0 {
        Term::absent(
            "D_t",
            "§40",
            "subsystem disagreement",
            0.0,
            "no live subsystem is expressing a verdict — there is nothing to disagree",
        )
    } else {
        Term::measured(
            "D_t",
            "§40",
            "subsystem disagreement",
            coherence.disagreement,
            format!("{} live subsystems, {:.0}% dissenting", coherence.subsystems, coherence.disagreement * 100.0),
        )
    };

    let load = LAMBDA_UA * uncertainty.aleatoric.value
        + LAMBDA_UE * uncertainty.epistemic.value
        + LAMBDA_N * novelty.value
        + LAMBDA_D * disagreement.value;
    let confidence = 1.0 - (load / 2.0).tanh();

    let confidence_terms = vec![
        uncertainty.aleatoric.clone(),
        uncertainty.epistemic.clone(),
        novelty,
        disagreement,
    ];

    // §32.
    let psi = confidence * coherence.value * (1.0 - risk.value);

    // §33. Every bracket term needs a subsystem the spec adds and this system lacks.
    let omega_terms = vec![
        Term::absent("H_t", "§3", "temporal/contextual state", 0.0, "needs the GRU recurrence of §3"),
        Term::absent("M_t", "§11", "memory", 0.0, "needs the episodic/semantic/procedural stores of §11"),
        Term::absent("G_t", "§8", "goals", 0.0, "needs the explicit goal vector of §8"),
        Term::absent("W_t", "§5", "world model", 0.0, "needs the multi-step predictor of §5"),
        Term::absent("P_t", "§23", "planning state", 0.0, "needs the planner of §23"),
    ];
    let omega = if omega_terms.iter().any(|t| t.measured) {
        Some(psi * omega_terms.iter().map(|t| t.value).sum::<f64>() / omega_terms.len() as f64)
    } else {
        None
    };

    let all: Vec<&Term> = confidence_terms
        .iter()
        .chain(risk.components.iter())
        .chain(omega_terms.iter())
        .collect();
    let measured_count = all.iter().filter(|t| t.measured).count();
    let measured_fraction = if all.is_empty() { 0.0 } else { measured_count as f64 / all.len() as f64 };

    let verdict = if measured_count == 0 || coherence.subsystems == 0 {
        GateVerdict::Unevaluated
    } else if psi < TAU_PSI {
        GateVerdict::Abstain
    } else if psi < TAU_PSI_FULL {
        GateVerdict::Damp
    } else {
        GateVerdict::Act
    };

    let reading = reading_for(verdict, psi, measured_fraction, &coherence, &risk);

    Cognition {
        confidence,
        confidence_terms,
        uncertainty,
        risk,
        coherence,
        psi,
        verdict,
        omega,
        omega_terms,
        measured_fraction,
        reading,
    }
}

/// §16 — `U = U_A + U_E`.
fn uncertainty(s: &Signals) -> Uncertainty {
    // Aleatoric uncertainty is irreducible spread of outcomes. A QR-DQN agent learns that
    // spread directly as its quantile distribution, so distributional mode makes §16's
    // `E[Σ_ψ]` measurable without the probabilistic transition model of §4 existing at all.
    let aleatoric = match (s.distributional, s.quantile_spread) {
        (true, Some(spread)) => Term::measured(
            "U_A",
            "§16",
            "aleatoric uncertainty",
            spread.clamp(0.0, 1.0),
            format!("QR-DQN quantile spread {spread:.3} — irreducible outcome variance"),
        ),
        (true, None) => Term::absent(
            "U_A",
            "§16",
            "aleatoric uncertainty",
            0.0,
            "distributional mode is on but the quantile spread is not exported yet",
        ),
        _ => Term::absent(
            "U_A",
            "§16",
            "aleatoric uncertainty",
            0.0,
            "needs QR-DQN distributional returns (distributional.rs) or the probabilistic transition model of §4",
        ),
    };

    // Epistemic uncertainty is ensemble disagreement (§16 `Var[f_ψ1..f_ψn]`). The
    // tournament variants ARE that ensemble — three agents trained on the same stream with
    // different hyperparameters — so their reward spread is a real, if coarse, estimate.
    let epistemic = match &s.variant_rewards {
        Some(r) if r.len() >= 2 => {
            let mean = r.iter().sum::<f64>() / r.len() as f64;
            let var = r.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / r.len() as f64;
            // Normalise by mean magnitude so the figure is a relative spread rather than a
            // number whose scale depends on how long the tournament has been running.
            let rel = if mean.abs() > 1e-9 { (var.sqrt() / mean.abs()).min(1.0) } else { 0.0 };
            Term::measured(
                "U_E",
                "§16",
                "epistemic uncertainty",
                rel,
                format!("{} tournament variants, relative reward spread {:.3}", r.len(), rel),
            )
        }
        _ => Term::absent(
            "U_E",
            "§16",
            "epistemic uncertainty",
            0.0,
            "needs ≥2 tournament variants reporting — the ensemble is the only estimator available",
        ),
    };

    let total = aleatoric.value + epistemic.value;
    Uncertainty { aleatoric, epistemic, total }
}

/// §31 — coherence over the live subsystems' verdicts.
fn coherence(nodes: &[Node]) -> Coherence {
    // Only units that actually decide something, and only those currently readable. A
    // stale verdict is not a present opinion, so including it would manufacture agreement
    // (or dissent) from a subsystem that has not spoken in months.
    let opinions: Vec<Verdict> = nodes
        .iter()
        .filter(|n| matches!(n.provenance, Provenance::Live { .. }))
        .filter(|n| !matches!(n.verdict, Verdict::Unknown))
        .filter(|n| {
            use crate::node::NodeKind::*;
            matches!(n.kind, Learner | Reasoner | Breaker | Gate | Scorer)
        })
        .map(|n| n.verdict)
        .collect();

    if opinions.is_empty() {
        return Coherence {
            value: 1.0,
            subsystems: 0,
            disagreement: 0.0,
            approximation: true,
            note: "no live subsystem is expressing a verdict — coherence is neutral, not high"
                .to_string(),
        };
    }

    // Reduce each verdict to permissive / restrictive, then measure dissent from the
    // majority. Two-way rather than n-way because the question §31 asks is whether the
    // subsystems are pulling in the same direction, not whether they are identical.
    let restrictive = |v: &Verdict| matches!(v, Verdict::Veto | Verdict::Damp | Verdict::Degraded);
    let n_restrictive = opinions.iter().filter(|v| restrictive(v)).count();
    let n = opinions.len();
    let minority = n_restrictive.min(n - n_restrictive);
    let disagreement = minority as f64 / n as f64; // 0 = unanimous, 0.5 = evenly split
    let value = 1.0 - 2.0 * disagreement; // map [0,0.5] onto [1,0]

    Coherence {
        value: value.clamp(0.0, 1.0),
        subsystems: n,
        disagreement,
        approximation: true,
        note: format!(
            "{n_restrictive} of {n} live subsystems are restrictive; D is a discrete majority \
             measure standing in for the spec's continuous D(Yᵢ, Ȳ)"
        ),
    }
}

/// §20 — the risk field over its six components.
fn risk_field(s: &Signals) -> RiskField {
    let mut components = Vec::new();

    // R_model: a policy that does not discriminate between inputs is a model risk, and the
    // intelligence ratio measures exactly that (squared coefficient of variation of Q*).
    components.push(match s.intelligence_ratio {
        Some(i) => {
            // Below 1e-4 the policy is treated as non-discriminating by the agent's own
            // equations module; map that onto full model risk.
            let r = if i <= 1e-4 { 1.0 } else { (1e-3 / i).min(1.0) };
            Term::measured("R_model", "§20", "model risk", r, format!("intelligence ratio {i:.2e}"))
        }
        None => Term::absent("R_model", "§20", "model risk", 0.0, "NN equations block not readable"),
    });

    // R_execution: the share of attempts that failed.
    components.push(match (s.trades_attempted, s.trades_failed) {
        (Some(a), Some(f)) if a > 0.0 => Term::measured(
            "R_exec",
            "§20",
            "execution risk",
            (f / a).clamp(0.0, 1.0),
            format!("{f:.0} failed of {a:.0} attempted"),
        ),
        _ => Term::absent("R_exec", "§20", "execution risk", 0.0, "no trades attempted — nothing to rate"),
    });

    // The remaining four come from the trade log, the pool radar and the open book — data
    // that was already on disk and had simply never been read as risk. The ATH tracker
    // still writes no state, but drawdown does not need it: the realised equity curve is
    // in `scematica-trades.jsonl`.
    components.push(match s.drawdown {
        Some(dd) => Term::measured(
            "R_dd",
            "§20",
            "drawdown risk",
            dd,
            format!("deepest peak-to-trough fall {:.1}% over {} closes", dd * 100.0, s.closes),
        ),
        None => Term::absent(
            "R_dd",
            "§20",
            "drawdown risk",
            0.0,
            "the realised equity curve never rose — no peak to fall from, which is not a drawdown of zero",
        ),
    });
    components.push(match s.liquidity {
        Some((median, r)) => Term::measured(
            "R_liq",
            "§20",
            "liquidity risk",
            r,
            format!("median depth {median:.1} SOL against a {:.0} SOL reference (anchor, not a measurement)", crate::history::DEPTH_REFERENCE_SOL),
        ),
        None => Term::absent("R_liq", "§20", "liquidity risk", 0.0, "pool radar has too few sized entries to take a median"),
    });
    components.push(match s.volatility {
        Some((sigma, r)) => Term::measured(
            "R_vol",
            "§20",
            "volatility risk",
            r,
            format!("σ {sigma:.1}% per trade against a {:.0}% reference (anchor, not a measurement)", crate::history::VOL_REFERENCE_PCT),
        ),
        None => Term::absent("R_vol", "§20", "volatility risk", 0.0, "fewer than 8 closes — dispersion would be sampling noise"),
    });
    components.push(match s.concentration {
        Some((n, h)) => Term::measured(
            "R_conc",
            "§20",
            "concentration risk",
            h,
            format!("Herfindahl {h:.3} over {n} open position{} — scale-free, no anchor", if n == 1 { "" } else { "s" }),
        ),
        None => Term::absent("R_conc", "§20", "concentration risk", 0.0, "the book is empty — no exposure at all, which is not concentration zero"),
    });

    // Mean over MEASURED components only. Averaging in the unmeasured zeros would divide a
    // real 1.0 model risk by six and report 0.17 — an unmeasured dimension must not dilute
    // a measured one any more than it may pin it.
    let measured: Vec<f64> = components.iter().filter(|t| t.measured).map(|t| t.value).collect();
    let value = if measured.is_empty() { 0.0 } else { measured.iter().sum::<f64>() / measured.len() as f64 };

    RiskField { components, value }
}

fn reading_for(v: GateVerdict, psi: f64, measured: f64, k: &Coherence, r: &RiskField) -> String {
    let coverage = format!("Ψ computed on {:.0}% of its inputs", measured * 100.0);
    match v {
        GateVerdict::Unevaluated => format!(
            "Ψ is unevaluated — no live subsystem is reporting, so this is ignorance rather than a refusal ({coverage})"
        ),
        GateVerdict::Abstain => {
            let driver = if r.value > 0.5 {
                format!("risk field at {:.2}", r.value)
            } else {
                format!("coherence at {:.2} across {} subsystems", k.value, k.subsystems)
            };
            format!("Ψ {psi:.2} is below τ_Ψ {TAU_PSI:.2} — abstain; dominant term is {driver} ({coverage})")
        }
        GateVerdict::Damp => format!(
            "Ψ {psi:.2} sits between τ_Ψ {TAU_PSI:.2} and {TAU_PSI_FULL:.2} — act with reduced size ({coverage})"
        ),
        GateVerdict::Act => format!("Ψ {psi:.2} clears τ_full {TAU_PSI_FULL:.2} — confident, coherent, low measured risk ({coverage})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node::{Node, NodeKind};

    fn live(id: &str, kind: NodeKind, verdict: Verdict) -> Node {
        Node {
            id: id.into(),
            kind,
            label: id.into(),
            blurb: String::new(),
            provenance: Provenance::Live { age_secs: 1 },
            verdict,
            activity: None,
            detail: vec![],
            reason: None,
        }
    }

    /// The load-bearing property. An unbuilt subsystem must not drag the gate down, or the
    /// gate reports on the roadmap rather than on the system — the exact failure that
    /// pinned the sentience Ψ at zero once already.
    #[test]
    fn unmeasured_dimensions_do_not_depress_psi() {
        let m = vec![live("learner.dqstar", NodeKind::Learner, Verdict::Pass)];
        let c = assess(&m, &Signals::default());
        assert_eq!(c.confidence, 1.0, "no measured degradation must yield full confidence");
        assert_eq!(c.risk.value, 0.0, "no measured risk component must yield zero risk");
        assert_eq!(c.coherence.value, 1.0);
        assert_eq!(c.psi, 1.0);
        // …but it must be loudly visible that this rests on almost nothing.
        assert!(c.measured_fraction < 0.2, "got {}", c.measured_fraction);
        assert!(c.reading.contains("%"));
    }

    /// Disagreement among zero subsystems is not a measurement of unanimity.
    #[test]
    fn disagreement_over_an_empty_set_is_unmeasured() {
        let c = assess(&[], &Signals::default());
        let d = c.confidence_terms.iter().find(|t| t.symbol == "D_t").unwrap();
        assert!(!d.measured, "an empty set cannot agree with itself");
        assert_eq!(c.verdict, GateVerdict::Unevaluated);
    }

    /// Nothing live at all is `Unevaluated`, which is NOT `Abstain`. A gate that failed to
    /// evaluate has not decided against acting.
    #[test]
    fn an_unobserved_system_is_unevaluated_not_abstaining() {
        let m = vec![Node::absent("learner.dqstar", NodeKind::Learner, "x", "y")];
        let c = assess(&m, &Signals::default());
        assert_eq!(c.verdict, GateVerdict::Unevaluated);
        assert_ne!(c.verdict, GateVerdict::Abstain);
        assert!(c.reading.contains("ignorance"));
    }

    /// Ω must stay `None` until at least one of its five subsystems exists. Emitting a
    /// number for an unbuilt architecture is the one thing this module must never do.
    #[test]
    fn omega_is_none_while_its_subsystems_do_not_exist() {
        let m = vec![live("learner.dqstar", NodeKind::Learner, Verdict::Pass)];
        let c = assess(&m, &Signals::default());
        assert!(c.omega.is_none());
        assert_eq!(c.omega_terms.len(), 5);
        assert!(c.omega_terms.iter().all(|t| !t.measured));
        // The terms double as a roadmap: each names the spec section that would supply it.
        assert!(c.omega_terms.iter().any(|t| t.note.contains("§23")));
    }

    /// §31: unanimous subsystems are coherent, an even split is not.
    #[test]
    fn coherence_falls_as_subsystems_disagree() {
        let unanimous = coherence(&[
            live("a", NodeKind::Learner, Verdict::Pass),
            live("b", NodeKind::Breaker, Verdict::Pass),
            live("c", NodeKind::Scorer, Verdict::Pass),
        ]);
        assert_eq!(unanimous.value, 1.0);
        assert_eq!(unanimous.subsystems, 3);

        let split = coherence(&[
            live("a", NodeKind::Learner, Verdict::Veto),
            live("b", NodeKind::Breaker, Verdict::Pass),
        ]);
        assert_eq!(split.value, 0.0, "an even split is total incoherence");
        assert_eq!(split.disagreement, 0.5);
    }

    /// A stale subsystem has no present opinion and must not be counted as agreeing.
    #[test]
    fn stale_subsystems_are_excluded_from_coherence() {
        let mut stale = live("b", NodeKind::Breaker, Verdict::Pass);
        stale.provenance = Provenance::Stale { age_secs: 99_999, budget_secs: 30 };
        let k = coherence(&[live("a", NodeKind::Learner, Verdict::Veto), stale]);
        assert_eq!(k.subsystems, 1, "only the live one counts");
    }

    /// §16: the tournament variants are the ensemble, and their spread is epistemic
    /// uncertainty. Without them the term is absent rather than zero-with-confidence.
    #[test]
    fn epistemic_uncertainty_comes_from_the_variant_ensemble() {
        let none = uncertainty(&Signals::default());
        assert!(!none.epistemic.measured);

        let spread = uncertainty(&Signals {
            variant_rewards: Some(vec![100.0, 200.0, 300.0]),
            ..Default::default()
        });
        assert!(spread.epistemic.measured);
        assert!(spread.epistemic.value > 0.0);
        assert!(spread.epistemic.note.contains("3 tournament variants"));
    }

    /// §20: an unmeasured component must not dilute a measured one. A real 1.0 model risk
    /// averaged over six slots would report 0.17 and read as safe.
    #[test]
    fn unmeasured_risk_components_do_not_dilute_measured_ones() {
        let r = risk_field(&Signals { intelligence_ratio: Some(1e-6), ..Default::default() });
        assert_eq!(r.value, 1.0, "a collapsed policy is full model risk, not one sixth of it");
        assert_eq!(r.components.len(), 6);
        assert_eq!(r.components.iter().filter(|t| t.measured).count(), 1);
    }

    /// The end-to-end case worth having: a collapsed policy and a split mesh should
    /// abstain, and say which term drove it.
    #[test]
    fn a_collapsed_policy_with_split_subsystems_abstains() {
        let m = vec![
            live("learner.dqstar", NodeKind::Learner, Verdict::Veto),
            live("breaker.reputation", NodeKind::Breaker, Verdict::Pass),
        ];
        let c = assess(
            &m,
            &Signals { intelligence_ratio: Some(1e-6), ..Default::default() },
        );
        assert_eq!(c.verdict, GateVerdict::Abstain);
        assert!(c.psi < TAU_PSI);
        assert!(c.reading.contains("risk field") || c.reading.contains("coherence"));
    }

    /// Execution risk is a measured ratio, and zero attempts means unmeasured rather than
    /// a flawless record.
    #[test]
    fn execution_risk_needs_attempts_to_mean_anything() {
        let none = risk_field(&Signals { trades_attempted: Some(0.0), trades_failed: Some(0.0), ..Default::default() });
        assert!(!none.components.iter().find(|t| t.symbol == "R_exec").unwrap().measured);

        let some = risk_field(&Signals { trades_attempted: Some(10.0), trades_failed: Some(3.0), ..Default::default() });
        let t = some.components.iter().find(|t| t.symbol == "R_exec").unwrap();
        assert!(t.measured);
        assert!((t.value - 0.3).abs() < 1e-9);
    }

    /// Every term must name a spec section, so a reader can check the implementation
    /// against the mathematics rather than trusting the label.
    #[test]
    fn every_term_cites_its_spec_section() {
        let m = vec![live("a", NodeKind::Learner, Verdict::Pass)];
        let c = assess(&m, &Signals::default());
        for t in c.confidence_terms.iter().chain(c.risk.components.iter()).chain(c.omega_terms.iter()) {
            assert!(t.section.starts_with('§'), "term {} has no section", t.symbol);
            assert!(!t.note.is_empty(), "term {} has no note", t.symbol);
        }
    }
}
