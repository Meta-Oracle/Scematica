//! [`WorldFeatures`] — a world as a fixed-width vector, without losing what was not measured.
//!
//! Every specialist in this runtime that could learn something is currently blind to three of
//! the four producers. `scema_policy::dqstar` declines on anything non-trading because
//! `TradeState` is pool-and-position shaped, so a repository, a DOM and a set of oracle feeds
//! reach the policy layer with nothing able to score them. This is the perception input that
//! is the same shape for all four.
//!
//! ## Why not the growth parameters
//!
//! The obvious candidate was `scema_nft::Growth` — six integers, already domain-agnostic,
//! already derived from any world. It is not usable, and the reason is instructive rather
//! than incidental: `growth_of` maps a world where **nothing was counted** and a world where
//! **everything counted was an opportunity** onto the same `spread`, because `risk_share` is
//! zero in both. `scema-nft`'s own tests now pin that. It is not a bug — a drawing is allowed
//! to lose detail — but it makes `Growth` a picture, not a perception.
//!
//! That collapse is `Scema.contribution_collapses` in `scema-lean`: summation provably cannot
//! carry the distinction, so whatever needs it must carry it *alongside*. Which is what
//! [`Term`] does, and why every field below is one.
//!
//! ## The two corrections made at this boundary
//!
//! [`WorldState`] has two accessors that are correct for rendering and wrong for learning,
//! and both are corrected here rather than changed there — the callers of those methods are
//! fine, and a feature extractor is where the distinction starts to matter:
//!
//! * `legibility()` returns `0.0` for a world with no objects. That is the same number an
//!   entirely stale world returns, and the two are opposite situations.
//! * `extent.fraction()` returns `Some(1.0)` when the total is `Some(0)` — "nothing of
//!   nothing observed" reported as complete coverage.
//!
//! Both become [`Term::absent`] here. Nothing downstream is entitled to read them as
//! observations.
//!
//! ## Missingness is a feature, and that is not a metaphor
//!
//! [`WorldFeatures::to_vec`] substitutes the declared neutral for an unmeasured term, which
//! on its own is exactly the failure `scematica_nn::TradeState` was carrying: a model given a
//! vector cannot tell a substituted neutral from a measurement. So [`WorldFeatures::mask`]
//! exists and [`WorldFeatures::to_vec_with_mask`] returns both. Indicator variables for
//! missingness are the standard statistical treatment and the only one consistent with the
//! rest of this workspace — a consumer that ignores the mask is making a claim it cannot
//! support, and now has to ignore something to do it.

use crate::term::{Coverage, Term};
use crate::provenance::Provenance;
use crate::world::WorldState;

/// A world reduced to numbers, each carrying whether anybody measured it.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldFeatures {
    /// How much of the entity was seen. Absent when the denominator is unknown.
    pub extent_fraction: Term,
    /// Share of objects whose values may be acted on. Absent when there are no objects.
    pub legibility: Term,
    /// Risks as a share of all signals. Absent when nothing was counted — the distinction
    /// `Growth::spread` cannot make.
    pub risk_share: Term,
    /// Share of signals whose magnitude was counted rather than estimated.
    pub signal_confidence: Term,
    /// Mean magnitude of counted risks. Absent when none were counted.
    pub mean_risk: Term,
    /// Mean magnitude of counted opportunities. Absent when none were counted.
    pub mean_opportunity: Term,
    /// Share of objects that are live / stale / absent / simulated. Absent with no objects.
    pub live_share: Term,
    pub stale_share: Term,
    pub absent_share: Term,
    pub simulated_share: Term,
    /// Things the observer reported it could not read. Always measured: reporting them is
    /// the producer contract, so an empty list is an observation of none, not a silence.
    pub blind_spots: Term,
    /// How many objects were perceived at all. Always measured, for the same reason.
    pub object_count: Term,
}

/// Neutral element for every feature, in normalised space.
///
/// Additive zero throughout, because every consumer of this vector sums or weights. Unlike
/// `scematica_nn::NEUTRAL` there is no asymmetric encoding here to trip over — every share is
/// already `[0, 1]` with a meaningful zero, and the two counts saturate from zero. The
/// uniformity is a property of the design, not an assumption: if a feature is ever added
/// whose zero is extremal rather than neutral, it needs its own entry and its own reason.
pub const NEUTRAL: f64 = 0.0;

impl WorldFeatures {
    /// Number of features. Fixed, and asserted against [`Self::names`].
    pub const DIM: usize = 12;

    /// Feature names, in the order [`Self::to_vec`] emits them.
    pub const fn names() -> [&'static str; Self::DIM] {
        [
            "extent_fraction",
            "legibility",
            "risk_share",
            "signal_confidence",
            "mean_risk",
            "mean_opportunity",
            "live_share",
            "stale_share",
            "absent_share",
            "simulated_share",
            "blind_spots",
            "object_count",
        ]
    }

    /// Read the features out of a world. Pure; no clock, no IO.
    pub fn of(w: &WorldState) -> Self {
        let objects = w.objects.len();
        let signals = w.signals.len();

        // Extent. `None` is an unknown denominator; `Some(0)` is an empty one. `WorldState`
        // reports the second as complete coverage, which is right for a progress bar and
        // wrong for a feature.
        let extent_fraction = match (w.extent.total, w.extent.fraction()) {
            (Some(0), _) | (None, _) => Term::absent(
                "E",
                "extent observed",
                NEUTRAL,
                "the denominator is unknown or empty, so a fraction would be invented",
            ),
            (Some(t), Some(f)) => Term::measured(
                "E",
                "extent observed",
                f,
                format!("{} of {} observed", w.extent.observed, t),
            ),
            (Some(_), None) => Term::absent("E", "extent observed", NEUTRAL, "no fraction"),
        };

        let legibility = if objects == 0 {
            Term::absent(
                "L",
                "legibility",
                NEUTRAL,
                "no objects were perceived, so there is nothing to be legible or otherwise",
            )
        } else {
            Term::measured(
                "L",
                "legibility",
                w.legibility(),
                format!("{} of {objects} object(s) actionable", w.actionable_objects().count()),
            )
        };

        let risks = w.risks().count();
        let risk_share = if signals == 0 {
            Term::absent(
                "R",
                "risk share",
                NEUTRAL,
                "no signals were counted — which is not a measurement of equilibrium",
            )
        } else {
            Term::measured(
                "R",
                "risk share",
                risks as f64 / signals as f64,
                format!("{risks} risk(s) of {signals} signal(s)"),
            )
        };

        let counted = w.signals.iter().filter(|s| s.measured).count();
        let signal_confidence = if signals == 0 {
            Term::absent("S", "signal confidence", NEUTRAL, "no signals to be confident about")
        } else {
            Term::measured(
                "S",
                "signal confidence",
                counted as f64 / signals as f64,
                format!("{counted} of {signals} magnitude(s) counted rather than estimated"),
            )
        };

        let mean_risk = mean_of("Mr", "mean counted risk", w, true);
        let mean_opportunity = mean_of("Mo", "mean counted opportunity", w, false);

        let share = |kind: &'static str, sym: &'static str, name: &'static str, n: usize| {
            if objects == 0 {
                Term::absent(sym, name, NEUTRAL, "no objects were perceived")
            } else {
                Term::measured(
                    sym,
                    name,
                    n as f64 / objects as f64,
                    format!("{n} of {objects} object(s) {kind}"),
                )
            }
        };

        let mut live = 0;
        let mut stale = 0;
        let mut absent = 0;
        let mut simulated = 0;
        for o in &w.objects {
            match o.provenance {
                Provenance::Live { .. } => live += 1,
                Provenance::Stale { .. } => stale += 1,
                Provenance::Absent => absent += 1,
                Provenance::Simulated => simulated += 1,
            }
        }

        WorldFeatures {
            extent_fraction,
            legibility,
            risk_share,
            signal_confidence,
            mean_risk,
            mean_opportunity,
            live_share: share("live", "Pl", "live share", live),
            stale_share: share("stale", "Ps", "stale share", stale),
            absent_share: share("absent", "Pa", "absent share", absent),
            simulated_share: share("simulated", "Pm", "simulated share", simulated),
            // Always measured. A producer reporting no blind spots has made a claim, and
            // `conform` requires it to — that is the difference between this and a silence.
            blind_spots: Term::measured(
                "B",
                "blind spots",
                w.blind_spots.len() as f64,
                format!("{} thing(s) reported unreadable", w.blind_spots.len()),
            ),
            object_count: Term::measured(
                "N",
                "objects perceived",
                objects as f64,
                format!("{objects} object(s)"),
            ),
        }
    }

    /// The features in emission order.
    pub fn terms(&self) -> [&Term; Self::DIM] {
        [
            &self.extent_fraction,
            &self.legibility,
            &self.risk_share,
            &self.signal_confidence,
            &self.mean_risk,
            &self.mean_opportunity,
            &self.live_share,
            &self.stale_share,
            &self.absent_share,
            &self.simulated_share,
            &self.blind_spots,
            &self.object_count,
        ]
    }

    /// How much of this vector stood on an observation. Never separated from the vector.
    pub fn coverage(&self) -> Coverage {
        Coverage::of(&self.terms())
    }

    /// The normalised vector, with the neutral substituted for anything unmeasured.
    ///
    /// **Do not use this alone.** A consumer cannot tell a substituted neutral from a
    /// measurement, which is precisely the defect this type exists to avoid reproducing.
    /// Use [`Self::to_vec_with_mask`], or read [`Self::coverage`] beside it.
    pub fn to_vec(&self) -> Vec<f64> {
        self.terms()
            .iter()
            .enumerate()
            .map(|(i, t)| if t.measured { normalise(i, t.value) } else { NEUTRAL })
            .collect()
    }

    /// One flag per feature: `true` where a measurement exists.
    pub fn mask(&self) -> Vec<bool> {
        self.terms().iter().map(|t| t.measured).collect()
    }

    /// Values and missingness together — the form a model should actually consume.
    ///
    /// Indicator variables are the standard treatment for missing data and the only one
    /// consistent with the rest of this workspace. A `2 * DIM` input where the second half is
    /// the mask lets a network learn "unobserved" as its own condition instead of inferring
    /// it from a value that looks like every other zero.
    pub fn to_vec_with_mask(&self) -> Vec<f64> {
        let mut v = self.to_vec();
        v.extend(self.mask().into_iter().map(|m| if m { 1.0 } else { 0.0 }));
        v
    }
}

/// Saturating map for an unbounded count: `n / (n + 1)`.
///
/// Monotone, lands in `[0, 1)`, and — the reason it is used rather than a cutoff — it carries
/// **no arbitrary constant**. A `min(n / 8.0, 1.0)` would silently declare eight blind spots
/// to be the same as eighty, which is the kind of invented threshold this workspace spends
/// its time removing. Zero maps to zero, which is a *measured* zero here.
fn saturate(n: f64) -> f64 {
    n / (n + 1.0)
}

/// Per-feature normalisation into `[0, 1]`.
fn normalise(index: usize, value: f64) -> f64 {
    match index {
        // The two counts.
        10 | 11 => saturate(value),
        // Everything else is already a share.
        _ => value.clamp(0.0, 1.0),
    }
}

/// Mean magnitude over *counted* signals of one polarity.
///
/// Estimated magnitudes are excluded rather than averaged in: a guess and a count are not
/// interchangeable, and averaging them produces a number whose provenance is neither.
fn mean_of(symbol: &'static str, name: &'static str, w: &WorldState, risk: bool) -> Term {
    let vals: Vec<f64> = w
        .signals
        .iter()
        .filter(|s| s.measured && (s.polarity == crate::world::Polarity::Risk) == risk)
        .map(|s| s.magnitude)
        .collect();
    if vals.is_empty() {
        Term::absent(symbol, name, NEUTRAL, "none of this polarity had a counted magnitude")
    } else {
        let n = vals.len();
        Term::measured(symbol, name, vals.iter().sum::<f64>() / n as f64, format!("over {n} counted"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::*;

    fn sig(id: &str, polarity: Polarity, magnitude: f64, measured: bool) -> Signal {
        Signal {
            id: id.into(),
            polarity,
            label: id.into(),
            detail: String::new(),
            magnitude,
            measured,
            targets: vec![],
            evidence: if measured { vec!["counted".into()] } else { vec![] },
        }
    }

    fn world() -> WorldState {
        WorldState {
            schema: Some(WORLD_SCHEMA.into()),
            observer: "test".into(),
            entity: Entity {
                kind: EntityKind::Repository,
                locator: "x".into(),
                label: "x".into(),
            },
            domain: Domain::Software,
            observed_at: 0,
            objects: vec![],
            facts: vec![],
            signals: vec![],
            extent: Extent { observed: 0, total: None, note: String::new() },
            blind_spots: vec![],
        }
    }

    #[test]
    fn the_vector_is_the_declared_width_and_names_agree() {
        let f = WorldFeatures::of(&world());
        assert_eq!(f.to_vec().len(), WorldFeatures::DIM);
        assert_eq!(WorldFeatures::names().len(), WorldFeatures::DIM);
        assert_eq!(f.terms().len(), WorldFeatures::DIM);
        assert_eq!(f.to_vec_with_mask().len(), WorldFeatures::DIM * 2);
    }

    #[test]
    fn nothing_counted_is_distinguishable_from_nothing_bad() {
        // The whole reason this type exists. `scema_nft::growth_of` maps these two worlds
        // onto the same `spread`; here they differ, and the difference is in `measured`
        // rather than in the value — which is exactly the point.
        let mut nothing = world();
        nothing.signals.clear();

        let mut all_good = world();
        all_good.signals = vec![sig("a", Polarity::Opportunity, 0.9, true)];

        let a = WorldFeatures::of(&nothing);
        let b = WorldFeatures::of(&all_good);

        assert!(!a.risk_share.measured, "no signals means no risk share was measured");
        assert!(b.risk_share.measured);
        assert_eq!(a.risk_share.value, b.risk_share.value, "both are 0.0 as a number");
        assert_ne!(a.risk_share, b.risk_share, "and yet they are different observations");
        assert_ne!(a.coverage(), b.coverage(), "which the coverage reports");
    }

    #[test]
    fn an_empty_world_has_no_legibility_rather_than_zero_legibility() {
        // `WorldState::legibility()` returns 0.0 here, the same value an entirely stale
        // world returns. Those are opposite situations and must not share a feature value.
        let f = WorldFeatures::of(&world());
        assert!(!f.legibility.measured);
        assert_eq!(f.legibility.value, NEUTRAL);
    }

    #[test]
    fn an_empty_denominator_is_not_complete_coverage() {
        // `Extent::fraction()` reports `Some(1.0)` for `total: Some(0)` — fine for a progress
        // bar, an invention for a feature.
        let mut w = world();
        w.extent = Extent { observed: 0, total: Some(0), note: String::new() };
        assert_eq!(w.extent.fraction(), Some(1.0), "the accessor still says this");
        assert!(!WorldFeatures::of(&w).extent_fraction.measured, "the feature does not");
    }

    #[test]
    fn an_unknown_denominator_is_unmeasured_not_zero() {
        let f = WorldFeatures::of(&world());
        assert!(!f.extent_fraction.measured);
    }

    #[test]
    fn a_measured_extent_is_reported_as_a_fraction() {
        let mut w = world();
        w.extent = Extent { observed: 3, total: Some(4), note: String::new() };
        let f = WorldFeatures::of(&w);
        assert!(f.extent_fraction.measured);
        assert!((f.extent_fraction.value - 0.75).abs() < 1e-12);
    }

    #[test]
    fn estimated_magnitudes_are_excluded_from_the_mean_not_averaged_in() {
        // A guess and a count are not interchangeable, and their mean has neither provenance.
        let mut w = world();
        w.signals = vec![
            sig("counted", Polarity::Risk, 1.0, true),
            sig("guessed", Polarity::Risk, 0.0, false),
        ];
        let f = WorldFeatures::of(&w);
        assert!(f.mean_risk.measured);
        assert_eq!(f.mean_risk.value, 1.0, "the estimate must not drag the mean to 0.5");
    }

    #[test]
    fn no_counted_signals_of_a_polarity_leaves_that_mean_unmeasured() {
        let mut w = world();
        w.signals = vec![sig("guessed", Polarity::Risk, 0.9, false)];
        let f = WorldFeatures::of(&w);
        assert!(!f.mean_risk.measured, "an estimate alone does not make a counted mean");
        assert!(!f.mean_opportunity.measured);
    }

    #[test]
    fn blind_spots_are_always_measured_because_reporting_them_is_the_contract() {
        // An empty list is an observation of none. Treating it as unmeasured would make a
        // clean world indistinguishable from an unread one.
        let f = WorldFeatures::of(&world());
        assert!(f.blind_spots.measured);
        assert_eq!(f.blind_spots.value, 0.0);
    }

    #[test]
    fn counts_saturate_without_an_invented_cutoff() {
        assert_eq!(saturate(0.0), 0.0);
        assert!((saturate(1.0) - 0.5).abs() < 1e-12);
        assert!(saturate(99.0) < 1.0 && saturate(99.0) > 0.98);
        // Monotone, so more blind spots never reads as fewer.
        assert!(saturate(3.0) < saturate(4.0));
    }

    #[test]
    fn the_mask_marks_exactly_what_coverage_counts() {
        let f = WorldFeatures::of(&world());
        let mask = f.mask();
        assert_eq!(mask.len(), WorldFeatures::DIM);
        assert_eq!(mask.iter().filter(|m| **m).count(), f.coverage().measured);
    }

    #[test]
    fn every_normalised_value_is_inside_the_unit_interval() {
        // A model reading this vector is entitled to assume the range. A count that escaped
        // it would act as a sentinel — a mask smuggled in without being one.
        let mut w = world();
        w.blind_spots = (0..50).map(|i| format!("spot {i}")).collect();
        w.objects = vec![];
        for v in WorldFeatures::of(&w).to_vec() {
            assert!((0.0..=1.0).contains(&v), "{v} escaped the unit interval");
        }
    }

    #[test]
    fn the_features_are_pure() {
        let w = world();
        assert_eq!(WorldFeatures::of(&w), WorldFeatures::of(&w));
    }
}
