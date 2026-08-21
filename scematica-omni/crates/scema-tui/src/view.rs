//! The view model: every decision about *how something should look*, made once, in a pure
//! function, testable without a terminal.
//!
//! This is the same split `lib/mesh/view.ts` makes in the web app. `render.rs` below is
//! allowed to place rectangles and draw borders; it is not allowed to decide that an
//! unmeasured term is dim, or that a stale provenance is not green. Those are claims about
//! trust, and a claim about trust gets one implementation with a test on it.
//!
//! ## The one rule, in its third implementation
//!
//! `scema_policy::render::cell` is authoritative for Rust: **an unmeasured term prints as
//! `—`, never as `0.00`.** [`cell`] here does not re-implement it — it *calls* it, and adds
//! only the thing a terminal has that a pipe does not, which is a [`Role`]. If the two ever
//! disagree the CLI is right and this file is wrong, and [`cell`] having no arithmetic of
//! its own is what makes that hard to get wrong.
//!
//! ## Provenance outranks value, everywhere
//!
//! Copied deliberately from `lib/mesh/view.ts::toneFor`: a stale node reading PASS has not
//! passed anything recently, and painting it the same colour as a live pass is the exact
//! error the feature exists to prevent. Here the same rule applies to observed objects —
//! [`provenance_role`] is consulted before anything about the object's attributes.

use scema_policy::decide::{Abstention, Decision, Ranked};
use scema_policy::render;
use scema_sim::Projection;
use scema_world::{Coverage, Polarity, Provenance, Signal, Term, WorldState};

use crate::theme::Role;

/// The em dash used for an unmeasured term, defined once so a test can assert on it.
pub const UNMEASURED: &str = "—";

/// Format one term for a table cell, with the role it should be drawn in.
///
/// The string comes from `scema_policy::render::cell` verbatim. This function adds a
/// [`Role`] and nothing else — no rounding, no width, no fallback. That is the whole
/// design: there is one formatter and several presenters.
pub fn cell(t: &Term) -> (String, Role) {
    let text = render::cell(t);
    let role = if t.measured { Role::Measured } else { Role::Unmeasured };
    (text, role)
}

/// The role for an observed value, decided by whether it can be believed.
///
/// Note the ordering: this asks "can this be seen, and is it current?" and never looks at
/// the value. `Stale` is amber rather than any shade of the live palette because a value
/// that was true an hour ago looks exactly like one that is true now, and that resemblance
/// is the entire hazard.
pub fn provenance_role(p: &Provenance) -> Role {
    match p {
        Provenance::Live { .. } => Role::Live,
        Provenance::Stale { .. } => Role::Stale,
        Provenance::Absent => Role::Absent,
        Provenance::Simulated => Role::Simulated,
    }
}

/// A provenance rendered as text, with its age when there is one.
///
/// The label alone (`LIVE`, `STALE`) is what carries the meaning without colour; the age is
/// the detail that makes it checkable.
pub fn provenance_label(p: &Provenance) -> String {
    match p {
        Provenance::Live { age_secs } => format!("LIVE {}", short_age(*age_secs)),
        Provenance::Stale { age_secs, budget_secs } => {
            format!("STALE {} > {}", short_age(*age_secs), short_age(*budget_secs))
        }
        Provenance::Absent => "ABSENT".into(),
        Provenance::Simulated => "SIMULATED".into(),
    }
}

/// `12s`, `4m`, `3h`, `9d`. Compact because it sits in a column.
pub fn short_age(secs: u64) -> String {
    match secs {
        s if s < 60 => format!("{s}s"),
        s if s < 3_600 => format!("{}m", s / 60),
        s if s < 86_400 => format!("{}h", s / 3_600),
        s => format!("{}d", s / 86_400),
    }
}

/// The role for a counted signal.
///
/// An **estimated** signal takes its own role regardless of polarity. The observer guessed
/// its magnitude, and a guessed risk that renders identically to a counted one is a
/// magnitude that will move a utility score as if somebody had counted it.
pub fn signal_role(s: &Signal) -> Role {
    if !s.measured {
        return Role::Estimated;
    }
    match s.polarity {
        Polarity::Risk => Role::Risk,
        Polarity::Opportunity => Role::Opportunity,
    }
}

/// Four characters that say what a signal is without any colour at all.
pub fn signal_tag(s: &Signal) -> &'static str {
    match (s.measured, s.polarity) {
        (true, Polarity::Risk) => "RISK",
        (true, Polarity::Opportunity) => "OPP ",
        // The word is the message. `EST?` reads as a question because that is what an
        // estimated magnitude is.
        (false, _) => "EST?",
    }
}

/// The role for one row of the simulation matrix.
pub fn rank_role(decision: &Decision, r: &Ranked) -> Role {
    if decision.chosen.as_ref() == Some(&r.hypothesis) {
        Role::Chosen
    } else {
        Role::Runner
    }
}

/// The marker in the leftmost column of the matrix. Carries "chosen" without colour.
pub fn rank_marker(decision: &Decision, r: &Ranked) -> &'static str {
    if decision.chosen.as_ref() == Some(&r.hypothesis) {
        "▸"
    } else {
        " "
    }
}

/// A coverage meter: filled cells for measured terms, hollow for unmeasured.
///
/// Rendered as a *count*, not a percentage bar, and that is deliberate. `40%` on two terms
/// out of five and `40%` on four out of ten are different claims, and a proportional bar
/// erases the difference. Here every term gets a cell, so the width of the meter is itself
/// information: a short meter means few terms existed at all.
pub fn coverage_meter(c: Coverage) -> String {
    if c.total == 0 {
        // Not an empty bar. `Coverage::fraction` returns 0.0 for an empty aggregate on
        // purpose — "nothing was measured because there was nothing to measure" is still
        // ignorance — and drawing nothing would read as "not applicable".
        return "∅".into();
    }
    let cap = 12usize;
    if c.total <= cap {
        let mut s = String::with_capacity(c.total);
        for i in 0..c.total {
            s.push(if i < c.measured { '▰' } else { '▱' });
        }
        s
    } else {
        // Too many terms to draw one cell each. Fall back to the label rather than to a
        // proportional bar, because the whole reason for the cell-per-term design is that a
        // proportional bar hides the denominator.
        c.label()
    }
}

/// Whether a coverage is below the configured floor — the thing that turns a ranking into
/// an abstention.
pub fn coverage_is_thin(c: Coverage, floor: f64) -> bool {
    c.fraction() < floor
}

/// The abstention, split into a headline and the instruction it implies.
///
/// Every reason sends the operator somewhere different, which is why `Abstention` is an
/// enum with five arms rather than a string. Collapsing them into "the agent declined"
/// throws away the only actionable part.
pub fn abstention_advice(a: &Abstention) -> &'static str {
    match a {
        Abstention::NoCandidates => {
            "Nothing was proposed. Check that the world has counted signals — `scema observe` lists them."
        }
        Abstention::AllForbidden { .. } => {
            "Every branch violates a constraint on the goal. The goal is unsatisfiable as stated; relax a --must-not or restate it."
        }
        Abstention::NoPositiveUtility { .. } => {
            "Acting scores worse than not acting. Accept that, or lower the bar deliberately in policy — not by accident."
        }
        Abstention::TooLittleMeasured { .. } => {
            "This is a statement about how little was observed, not about the branches. Go and observe more."
        }
        Abstention::Contested { .. } => {
            "A specialist that IS qualified here disagrees with the top branch. Read its note before overriding it."
        }
    }
}

/// The projection belonging to a ranked row, matched by id.
///
/// By id and never by position: the simulator is free to reorder or drop, and a positional
/// match would silently score one branch with another's numbers.
pub fn projection_for<'a>(projections: &'a [Projection], hypothesis: &str) -> Option<&'a Projection> {
    projections.iter().find(|p| p.hypothesis == hypothesis)
}

/// A one-line summary of how much of the world was legible, and whether it is even known.
///
/// `Extent { total: None }` is rendered as an explicit shout rather than as a missing
/// denominator. An observer that hit a cap knows it hit a cap, and passing that upward is
/// the most useful thing it does.
pub fn extent_line(w: &WorldState) -> (String, Role) {
    match w.extent.fraction() {
        Some(f) => (
            format!("{} of {} observed ({:.0}%) — {}", w.extent.observed, w.extent.total.unwrap_or(0), f * 100.0, w.extent.note),
            Role::Measured,
        ),
        None => (
            format!("{} observed, EXTENT UNBOUNDED — {}", w.extent.observed, w.extent.note),
            Role::Estimated,
        ),
    }
}

/// Truncate to a character count, with an ellipsis that is included in the budget.
pub fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    if n == 0 {
        return String::new();
    }
    let mut out: String = s.chars().take(n.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use scema_world::{Coverage, Term};

    #[test]
    fn an_unmeasured_term_is_an_em_dash_and_a_recessive_role() {
        let (text, role) = cell(&Term::absent("R", "gain", 0.0, "nothing cited"));
        assert_eq!(text.trim(), UNMEASURED);
        assert!(!text.contains('0'));
        assert_eq!(role, Role::Unmeasured);
    }

    #[test]
    fn a_measured_zero_is_a_number_and_a_measured_role() {
        // The other half of the rule, and the half people delete by accident: a counted
        // zero is a real observation and must not borrow the unmeasured styling.
        let (text, role) = cell(&Term::measured("R", "gain", 0.0, "counted zero"));
        assert!(text.contains("0.00"));
        assert_eq!(role, Role::Measured);
    }

    #[test]
    fn this_module_never_formats_a_term_itself() {
        // The port-vs-copy guarantee, asserted rather than described: whatever
        // `scema_policy::render::cell` produces is what appears here, byte for byte.
        for t in [
            Term::measured("A", "a", 0.125, "x"),
            Term::absent("B", "b", 0.0, "y"),
            Term::measured("C", "c", -1.5, "z"),
        ] {
            assert_eq!(cell(&t).0, render::cell(&t));
        }
    }

    #[test]
    fn stale_never_borrows_the_live_role() {
        // `lib/mesh/view.ts::toneFor`'s rule, in Rust. Provenance is asked first and the
        // value is never consulted.
        assert_eq!(provenance_role(&Provenance::Live { age_secs: 1 }), Role::Live);
        assert_eq!(
            provenance_role(&Provenance::Stale { age_secs: 99, budget_secs: 10 }),
            Role::Stale
        );
        assert_ne!(
            provenance_role(&Provenance::Stale { age_secs: 99, budget_secs: 10 }),
            provenance_role(&Provenance::Live { age_secs: 1 })
        );
        assert_eq!(provenance_role(&Provenance::Absent), Role::Absent);
        assert_eq!(provenance_role(&Provenance::Simulated), Role::Simulated);
    }

    #[test]
    fn an_estimated_signal_does_not_render_as_a_counted_one() {
        let counted = Signal {
            id: "a".into(),
            polarity: Polarity::Risk,
            label: "x".into(),
            detail: String::new(),
            magnitude: 0.9,
            measured: true,
            targets: vec![],
            evidence: vec![],
        };
        let guessed = Signal { measured: false, ..counted.clone() };
        assert_eq!(signal_role(&counted), Role::Risk);
        assert_eq!(signal_role(&guessed), Role::Estimated);
        assert_ne!(signal_tag(&counted), signal_tag(&guessed));
    }

    #[test]
    fn the_coverage_meter_shows_the_denominator_rather_than_a_percentage() {
        // 2/5 and 4/10 are the same fraction and different claims. The meter draws one cell
        // per term precisely so they cannot render identically.
        let a = coverage_meter(Coverage { measured: 2, total: 5 });
        let b = coverage_meter(Coverage { measured: 4, total: 10 });
        assert_ne!(a, b);
        assert_eq!(a.chars().count(), 5);
        assert_eq!(b.chars().count(), 10);
        assert_eq!(a.chars().filter(|c| *c == '▰').count(), 2);
    }

    #[test]
    fn an_empty_coverage_is_not_an_empty_bar() {
        // An empty bar reads as "measured, and it is zero". `∅` reads as "there was nothing
        // here", which is the true statement. Same tri-state discipline as `/mesh`.
        assert_eq!(coverage_meter(Coverage { measured: 0, total: 0 }), "∅");
    }

    #[test]
    fn a_wide_coverage_falls_back_to_the_label_not_to_a_proportional_bar() {
        let m = coverage_meter(Coverage { measured: 30, total: 90 });
        assert_eq!(m, "30/90");
    }

    #[test]
    fn every_abstention_reason_carries_a_distinct_instruction() {
        // Five reasons, five different places to send the operator. If two collapsed, the
        // enum would be doing no work.
        let all = [
            Abstention::NoCandidates,
            Abstention::AllForbidden { count: 2 },
            Abstention::NoPositiveUtility { best: -0.1 },
            Abstention::TooLittleMeasured {
                coverage: Coverage { measured: 1, total: 5 },
                floor: 0.4,
            },
            Abstention::Contested { by: "dqstar".into(), utility: -0.2, note: "n".into() },
        ];
        let mut advice: Vec<&str> = all.iter().map(abstention_advice).collect();
        advice.sort_unstable();
        let before = advice.len();
        advice.dedup();
        assert_eq!(before, advice.len(), "two abstentions give the same instruction");
    }

    #[test]
    fn truncation_budgets_the_ellipsis() {
        assert_eq!(truncate("abcdef", 4), "abc…");
        assert_eq!(truncate("abcdef", 4).chars().count(), 4);
        assert_eq!(truncate("abc", 8), "abc");
    }

    #[test]
    fn an_unbounded_extent_is_shouted_and_not_styled_as_a_measurement() {
        use scema_world::{Domain, Entity, EntityKind, Extent};
        let mut w = WorldState {
            observer: "t".into(),
            entity: Entity { kind: EntityKind::Repository, locator: "/r".into(), label: "r".into() },
            domain: Domain::Software,
            observed_at: 0,
            objects: vec![],
            facts: vec![],
            signals: vec![],
            extent: Extent::partial(400, "walk capped"),
            blind_spots: vec![],
        };
        let (text, role) = extent_line(&w);
        assert!(text.contains("EXTENT UNBOUNDED"));
        assert_ne!(role, Role::Measured, "an unknown denominator is not a measurement");

        w.extent = Extent::complete(400, "walked");
        assert_eq!(extent_line(&w).1, Role::Measured);
    }
}
