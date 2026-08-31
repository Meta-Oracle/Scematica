//! Did the projection hold? — the only honest learning signal this runtime has.
//!
//! Every attempt so far to score omni against reality has run into the same wall, and it is
//! the wall the whole design puts there deliberately: **a branch nobody took has no outcome.**
//! `Calibration::mean_abs_error` in the bot is `None` rather than `0.0` for exactly this
//! reason, and imputing results for untaken branches would mean the system generating its own
//! training signal — a number of the right shape with nothing behind it.
//!
//! There is one signal that is not imputed, and this module computes it.
//!
//! ## Observe, decide, act, **observe again**
//!
//! A sealed record contains the world as it was at T0. Re-observe the same entity at T1 and
//! the difference is measured, not modelled. That difference is a fact about what actually
//! happened, and it is available for exactly one branch — the one that was chosen — which is
//! the correct number of branches to have an outcome for.
//!
//! ## What is scored, and what is only counted
//!
//! The projection made five claims (`R`, `K`, `C`, `U`, `V`). Only some of them are checkable
//! against a later observation of the same world, and pretending otherwise is how a
//! calibration number stops meaning anything:
//!
//! * **`expected_gain` is checkable.** It was a claim about signals being resolved, and a
//!   later observation counts how many actually were.
//! * **`uncertainty` is checkable.** It was a claim about ignorance, and blind spots are
//!   counted at both ends.
//! * **`risk`, `cost` and `reversibility` are not.** They describe what *could* have gone
//!   wrong and what it would have taken to undo — counterfactuals about a path not walked.
//!   A world that came out fine does not retire the risk; it is one sample of a branch that
//!   was taken once.
//!
//! So [`Resolution`] scores two terms and reports the other three as unresolved. That is a
//! worse-looking result than scoring all five and it is the only one that is true.
//!
//! ## Every way this refuses to resolve
//!
//! An unresolvable pair is [`Resolution::Unresolved`] with a reason, never a zero error.
//! `mean_abs_error` over an empty set is `None`. A run of abstentions produces no calibration
//! at all rather than a perfect one — the identical trap `scematica_nn::calibration` names,
//! where a policy improves its score by refusing to act.

use scema_world::{Coverage, Term, WorldState};
use serde::{Deserialize, Serialize};

use crate::record::DecisionRecord;

/// Why a decision could not be scored against a later observation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum Unresolvable {
    /// The agent abstained. There is no chosen branch, so nothing was claimed.
    Abstained,
    /// The later observation is of a different entity. Comparing them would be comparing
    /// two things, and the number would look exactly like a calibration result.
    DifferentEntity { sealed: String, observed: String },
    /// The later observation is not later. Ordering is the whole claim: a projection scored
    /// against something that happened first is not a prediction.
    NotLater { sealed_at: i64, observed_at: i64 },
    /// The chosen branch has no projection in the record. A malformed record, not a result.
    NoProjection { hypothesis: String },
    /// The projection measured neither of the two checkable terms, so there is no claim to
    /// check. Distinct from a claim that turned out wrong.
    NothingWasClaimed,
}

impl Unresolvable {
    pub fn explain(&self) -> String {
        match self {
            Unresolvable::Abstained =>
                "the agent abstained, so it claimed nothing and cannot be wrong".into(),
            Unresolvable::DifferentEntity { sealed, observed } =>
                format!("the record is about {sealed} and the observation is of {observed}"),
            Unresolvable::NotLater { sealed_at, observed_at } =>
                format!("the observation is stamped {observed_at}, before the record's {sealed_at} — a projection scored against an earlier world is not a prediction"),
            Unresolvable::NoProjection { hypothesis } =>
                format!("the record chose `{hypothesis}` but carries no projection for it"),
            Unresolvable::NothingWasClaimed =>
                "neither expected gain nor uncertainty was measured, so nothing was claimed".into(),
        }
    }
}

/// One projected term against what a later observation found.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Checked {
    pub symbol: String,
    /// What was projected at T0.
    pub projected: f64,
    /// What the world showed at T1.
    pub realised: f64,
    /// `|projected − realised|`.
    pub abs_error: f64,
}

/// A decision, scored against a later observation of the same entity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Resolution {
    Unresolved { record: String, why: Unresolvable },
    Resolved {
        record: String,
        chosen: String,
        /// Seconds between sealing and re-observation. A projection checked eight seconds
        /// later and one checked eight days later are different evidence.
        elapsed_secs: i64,
        checked: Vec<Checked>,
        /// The three terms that describe a path not walked. Counted, never scored.
        unscored: Vec<String>,
    },
}

impl Resolution {
    pub fn mean_abs_error(&self) -> Option<f64> {
        match self {
            Resolution::Unresolved { .. } => None,
            Resolution::Resolved { checked, .. } if checked.is_empty() => None,
            Resolution::Resolved { checked, .. } => {
                Some(checked.iter().map(|c| c.abs_error).sum::<f64>() / checked.len() as f64)
            }
        }
    }
}

/// The terms a later observation of the same world genuinely speaks to.
///
/// `K`, `C` and `V` are absent by construction, not by omission — see the module note.
pub const CHECKABLE: [&str; 2] = ["R", "U"];

/// Terms that describe a branch not walked, and are therefore counted rather than scored.
pub const UNSCORED: [&str; 3] = ["K", "C", "V"];

/// Score one sealed record against a later observation of the same entity.
pub fn resolve(record: &DecisionRecord, later: &WorldState) -> Resolution {
    let id = record.id.clone();

    if record.world.entity.locator != later.entity.locator {
        return Resolution::Unresolved {
            record: id,
            why: Unresolvable::DifferentEntity {
                sealed: record.world.entity.locator.clone(),
                observed: later.entity.locator.clone(),
            },
        };
    }
    if later.observed_at <= record.world.observed_at {
        return Resolution::Unresolved {
            record: id,
            why: Unresolvable::NotLater {
                sealed_at: record.world.observed_at,
                observed_at: later.observed_at,
            },
        };
    }
    let Some(chosen) = record.decision.chosen.clone() else {
        return Resolution::Unresolved { record: id, why: Unresolvable::Abstained };
    };
    let Some(p) = record.projections.iter().find(|p| p.hypothesis == chosen) else {
        return Resolution::Unresolved {
            record: id,
            why: Unresolvable::NoProjection { hypothesis: chosen },
        };
    };

    let mut checked = Vec::new();

    // `R` — expected gain was a claim that counted signals would be resolved. The later
    // world says how many actually were. Only signals the *earlier* world counted are
    // eligible: a signal that appeared afterwards was not something the projection claimed.
    if p.expected_gain.measured {
        let before: Vec<&str> = record
            .world
            .signals
            .iter()
            .filter(|s| s.measured)
            .map(|s| s.id.as_str())
            .collect();
        if !before.is_empty() {
            let still: usize = later
                .signals
                .iter()
                .filter(|s| before.contains(&s.id.as_str()))
                .count();
            let resolved = before.len().saturating_sub(still) as f64 / before.len() as f64;
            checked.push(Checked {
                symbol: "R".into(),
                projected: p.expected_gain.value,
                realised: resolved,
                abs_error: (p.expected_gain.value - resolved).abs(),
            });
        }
    }

    // `U` — ignorance. Blind spots are counted at both ends, so the change is measured.
    if p.uncertainty.measured {
        let before = record.world.blind_spots.len() as f64;
        let after = later.blind_spots.len() as f64;
        // Normalised the same way the feature vector saturates counts, so the two are
        // comparable and neither invents a cutoff.
        let realised = if before + after == 0.0 { 0.0 } else { after / (before + 1.0) };
        checked.push(Checked {
            symbol: "U".into(),
            projected: p.uncertainty.value,
            realised: realised.min(1.0),
            abs_error: (p.uncertainty.value - realised.min(1.0)).abs(),
        });
    }

    if checked.is_empty() {
        return Resolution::Unresolved { record: id, why: Unresolvable::NothingWasClaimed };
    }

    Resolution::Resolved {
        record: id,
        chosen,
        elapsed_secs: later.observed_at - record.world.observed_at,
        checked,
        unscored: UNSCORED.iter().map(|s| s.to_string()).collect(),
    }
}

/// A calibration record over many resolutions.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Calibration {
    pub resolved: usize,
    pub unresolved: usize,
    /// Why each unresolved one was unresolved. The distribution is usually the finding.
    pub reasons: Vec<Unresolvable>,
    errors: Vec<f64>,
}

impl Calibration {
    pub fn observe(&mut self, r: &Resolution) {
        match r {
            Resolution::Unresolved { why, .. } => {
                self.unresolved += 1;
                self.reasons.push(why.clone());
            }
            Resolution::Resolved { checked, .. } => {
                self.resolved += 1;
                self.errors.extend(checked.iter().map(|c| c.abs_error));
            }
        }
    }

    /// `None` when nothing resolved. **Never `0.0`** — a zero there says the agent was never
    /// wrong, and "never had the chance to be" is a different sentence. A run of abstentions
    /// must not read as perfect calibration.
    pub fn mean_abs_error(&self) -> Option<f64> {
        if self.errors.is_empty() {
            return None;
        }
        Some(self.errors.iter().sum::<f64>() / self.errors.len() as f64)
    }

    /// The score with its coverage, which is never separated from it.
    pub fn term(&self) -> (Term, Coverage) {
        let total = self.resolved + self.unresolved;
        let cov = Coverage { measured: self.resolved, total };
        match self.mean_abs_error() {
            Some(e) => (
                Term::measured("Cal", "calibration", e, format!("over {} claim(s)", self.errors.len())),
                cov,
            ),
            None => (
                Term::absent(
                    "Cal",
                    "calibration",
                    0.0,
                    "nothing resolved — every decision either abstained or has no later observation",
                ),
                cov,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scema_world::*;

    fn w(locator: &str, at: i64, signals: Vec<&str>, blind: usize) -> WorldState {
        WorldState {
            schema: Some(WORLD_SCHEMA.into()),
            observer: "t".into(),
            entity: Entity {
                kind: EntityKind::Repository,
                locator: locator.into(),
                label: "x".into(),
            },
            domain: Domain::Software,
            observed_at: at,
            objects: vec![],
            facts: vec![],
            signals: signals
                .into_iter()
                .map(|id| Signal {
                    id: id.into(),
                    polarity: Polarity::Risk,
                    label: id.into(),
                    detail: String::new(),
                    magnitude: 0.5,
                    measured: true,
                    targets: vec![],
                    evidence: vec!["counted".into()],
                })
                .collect(),
            extent: Extent { observed: 1, total: Some(1), note: String::new() },
            blind_spots: (0..blind).map(|i| format!("b{i}")).collect(),
        }
    }

    /// A record whose chosen branch projected `gain` and `unc`, both measured.
    fn record(world: WorldState, chosen: Option<&str>, gain: f64, unc: f64) -> DecisionRecord {
        use scema_policy::{decide, DecisionConfig};
        use scema_sim::{Simulator, StructuralSimulator};
        let goal = Goal::new("g", "do it");
        let h = Hypothesis::new("h1", "do it", HypothesisOrigin::Human);
        let mut p = StructuralSimulator::new().project(&world, &goal, &h);
        p.expected_gain = Term::measured("R", "gain", gain, "test");
        p.uncertainty = Term::measured("U", "unc", unc, "test");
        let mut d = decide(&world, &goal, &[h.clone()], &[p.clone()], &[], DecisionConfig::default());
        d.chosen = chosen.map(|c| c.to_string());
        DecisionRecord::seal("test/1", 0, world, goal, vec![h], vec![p], d)
    }

    #[test]
    fn a_later_observation_of_the_same_entity_resolves_the_claim() {
        let r = record(w("repo", 100, vec!["s1", "s2"], 2), Some("h1"), 0.5, 0.5);
        // One of two signals is gone, so half resolved.
        let res = resolve(&r, &w("repo", 200, vec!["s1"], 2));
        match &res {
            Resolution::Resolved { checked, elapsed_secs, unscored, .. } => {
                assert_eq!(*elapsed_secs, 100);
                let gain = checked.iter().find(|c| c.symbol == "R").expect("R checked");
                assert!((gain.realised - 0.5).abs() < 1e-12);
                assert_eq!(unscored.len(), 3, "K, C and V describe a path not walked");
            }
            other => panic!("expected Resolved, got {other:?}"),
        }
        assert!(res.mean_abs_error().is_some());
    }

    #[test]
    fn an_abstention_is_unresolvable_rather_than_perfectly_calibrated() {
        // The trap `scematica_nn::calibration` names: a policy that refuses to act must not
        // thereby earn a perfect score.
        let r = record(w("repo", 100, vec!["s1"], 0), None, 0.5, 0.5);
        let res = resolve(&r, &w("repo", 200, vec![], 0));
        assert_eq!(res.mean_abs_error(), None);
        match res {
            Resolution::Unresolved { why: Unresolvable::Abstained, .. } => {}
            other => panic!("expected Abstained, got {other:?}"),
        }
    }

    #[test]
    fn an_observation_of_a_different_entity_is_refused() {
        let r = record(w("repo-a", 100, vec!["s1"], 0), Some("h1"), 0.5, 0.5);
        let res = resolve(&r, &w("repo-b", 200, vec![], 0));
        assert!(matches!(
            res,
            Resolution::Unresolved { why: Unresolvable::DifferentEntity { .. }, .. }
        ));
    }

    #[test]
    fn an_earlier_observation_is_not_a_result() {
        // Ordering is the whole claim. A projection scored against something that happened
        // first is not a prediction.
        let r = record(w("repo", 100, vec!["s1"], 0), Some("h1"), 0.5, 0.5);
        let res = resolve(&r, &w("repo", 50, vec![], 0));
        assert!(matches!(res, Resolution::Unresolved { why: Unresolvable::NotLater { .. }, .. }));
    }

    #[test]
    fn the_same_instant_is_not_later_either() {
        let r = record(w("repo", 100, vec!["s1"], 0), Some("h1"), 0.5, 0.5);
        assert!(matches!(
            resolve(&r, &w("repo", 100, vec![], 0)),
            Resolution::Unresolved { why: Unresolvable::NotLater { .. }, .. }
        ));
    }

    #[test]
    fn only_the_two_checkable_terms_are_scored() {
        // Risk, cost and reversibility describe what could have gone wrong on a path taken
        // once. A world that came out fine does not retire the risk.
        let r = record(w("repo", 100, vec!["s1"], 1), Some("h1"), 0.5, 0.5);
        match resolve(&r, &w("repo", 200, vec![], 1)) {
            Resolution::Resolved { checked, .. } => {
                let symbols: Vec<&str> = checked.iter().map(|c| c.symbol.as_str()).collect();
                for s in symbols {
                    assert!(CHECKABLE.contains(&s), "{s} is not checkable against a re-observation");
                }
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn calibration_over_nothing_resolved_is_none_not_zero() {
        let mut c = Calibration::default();
        for _ in 0..5 {
            c.observe(&Resolution::Unresolved {
                record: "x".into(),
                why: Unresolvable::Abstained,
            });
        }
        assert_eq!(c.mean_abs_error(), None);
        let (t, cov) = c.term();
        assert!(!t.measured, "an unmeasured calibration must not print as a number");
        assert_eq!(cov, Coverage { measured: 0, total: 5 });
    }

    #[test]
    fn calibration_carries_why_each_one_failed_to_resolve() {
        // The distribution of reasons is usually the finding — five abstentions and five
        // missing observations call for completely different responses.
        let mut c = Calibration::default();
        c.observe(&Resolution::Unresolved { record: "a".into(), why: Unresolvable::Abstained });
        c.observe(&Resolution::Unresolved {
            record: "b".into(),
            why: Unresolvable::NotLater { sealed_at: 2, observed_at: 1 },
        });
        assert_eq!(c.reasons.len(), 2);
        assert!(c.reasons.iter().all(|r| !r.explain().is_empty()));
    }

    #[test]
    fn a_perfect_projection_scores_zero_error_and_that_is_a_measurement() {
        // The other half of the rule: a measured zero is a real result and must be reported
        // as one. Only an *absent* score prints as unmeasured.
        let r = record(w("repo", 100, vec!["s1"], 0), Some("h1"), 1.0, 0.0);
        let res = resolve(&r, &w("repo", 200, vec![], 0));
        let e = res.mean_abs_error().expect("resolved");
        assert!(e.abs() < 1e-12, "error was {e}");
    }
}
