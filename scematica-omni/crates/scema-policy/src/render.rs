//! Rendering: the only place in Rust where a [`Term`] becomes a string.
//!
//! It lives in the policy crate rather than in a front end because it is not a front-end
//! concern. There are three consumers — the `scema` CLI, the MCP server, and (as a ported
//! copy) the browser HUD — and the moment each of them formatted its own numbers, one of
//! them would get it wrong and nothing would catch it. Same reasoning as
//! `lib/mesh/view.ts::toneFor` being the only thing in the web app allowed to pick a colour:
//! a rule that encodes a claim about trust needs exactly one implementation.
//!
//! One rule governs every function here, and it is the reason this is a module rather than
//! a few `println!`s at the call sites:
//!
//! > **An unmeasured term prints as `—`, never as `0.00`.**
//!
//! A column of numbers is the most persuasive thing a program can emit. Printing the
//! neutral element of an unmeasured term as a number puts "nobody looked" and "we looked
//! and it was zero" in the same column, in the same font, and the distinction that the
//! entire type system below has been protecting is lost in the last hundred lines of the
//! program. So [`cell`] is the only thing that formats a [`Term`], and it takes the whole
//! term rather than its value.
//!
//! Colour is not used at all. Every surface in this repository treats colour as decoration
//! and never as the message; here there is nothing to decorate, and a matrix that reads
//! identically through a pipe is worth more than one that needs a terminal.

use scema_sim::Projection;
use scema_world::{Term, WorldState};

use crate::decide::{Abstention, Decision};

/// Format one term for a table cell.
pub fn cell(t: &Term) -> String {
    if t.measured {
        format!("{:>7.2}", t.value)
    } else {
        format!("{:>7}", "—")
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    let mut out: String = s.chars().take(n.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// The world header that precedes every matrix.
///
/// Extent and blind spots are on the same line as the object count on purpose: "214 files"
/// alone is a claim of completeness that the observer may not have made.
pub fn world_header(w: &WorldState) -> String {
    let extent = match w.extent.fraction() {
        Some(_) => format!("{} observed ({})", w.extent.observed, w.extent.note),
        None => format!("{} observed, EXTENT UNBOUNDED ({})", w.extent.observed, w.extent.note),
    };
    let mut s = format!(
        "WORLD    {}\n         {:?} · {:?} · observer `{}`\n         {} · legibility {:.0}%",
        w.entity.locator,
        w.entity.kind,
        w.domain,
        w.observer,
        extent,
        w.legibility() * 100.0
    );
    if !w.blind_spots.is_empty() {
        s.push_str(&format!("\n         BLIND SPOTS ({}):", w.blind_spots.len()));
        for b in w.blind_spots.iter().take(5) {
            s.push_str(&format!("\n           · {b}"));
        }
        if w.blind_spots.len() > 5 {
            s.push_str(&format!("\n           · … {} more", w.blind_spots.len() - 5));
        }
    }
    s
}

/// Counted signals, which are the only things that can ground a branch.
pub fn signals(w: &WorldState) -> String {
    signals_capped(w, usize::MAX)
}

/// As [`signals`], but capped — and it always states how many it dropped.
///
/// The cap exists for the MCP surface, where a monorepo's several hundred signals would
/// crowd out a model's context. Truncating silently would emit a wrong count, which is the
/// one thing this workspace will not do, so the tail line is not optional.
pub fn signals_capped(w: &WorldState, max: usize) -> String {
    if w.signals.is_empty() {
        return "SIGNALS  none counted".to_string();
    }
    let mut out = String::from("SIGNALS");
    for s in w.signals.iter().take(max) {
        out.push_str(&format!(
            "\n  {:<12} {:<11} {:>5.2}  {}",
            format!("{:?}", s.polarity).to_uppercase(),
            if s.measured { "counted" } else { "ESTIMATED" },
            s.magnitude,
            truncate(&s.label, 60)
        ));
        if let Some(e) = s.evidence.first() {
            out.push_str(&format!("\n  {:<12} {:<11} {:>5}  └ {}", "", "", "", truncate(e, 60)));
        }
    }
    if w.signals.len() > max {
        out.push_str(&format!(
            "\n  ... {} more signal(s) not listed",
            w.signals.len() - max
        ));
    }
    out
}

/// The simulation matrix.
pub fn matrix(decision: &Decision, projections: &[Projection]) -> String {
    let mut out = String::from("SIMULATION MATRIX\n");
    out.push_str(&format!(
        "  {:<3} {:<44} {:>7} {:>7} {:>7} {:>7} {:>7} {:>9} {:>9}\n",
        "#", "BRANCH", "GAIN", "RISK", "COST", "UNCERT", "REVERS", "UTILITY", "MEASURED"
    ));
    out.push_str(&format!("  {}\n", "─".repeat(105)));

    if decision.ranked.is_empty() {
        out.push_str("  (no branch was allowed to compete)\n");
    }

    for (i, r) in decision.ranked.iter().enumerate() {
        let p = projections.iter().find(|p| p.hypothesis == r.hypothesis);
        let (g, k, c, u, v) = match p {
            Some(p) => (
                cell(&p.expected_gain),
                cell(&p.risk),
                cell(&p.cost),
                cell(&p.uncertainty),
                cell(&p.reversibility),
            ),
            None => ("      ?".into(), "      ?".into(), "      ?".into(), "      ?".into(), "      ?".into()),
        };
        let chosen = if decision.chosen.as_ref() == Some(&r.hypothesis) { "▸" } else { " " };
        out.push_str(&format!(
            "{}{:>3} {:<44} {} {} {} {} {} {:>9.3} {:>9}\n",
            chosen,
            i + 1,
            truncate(&r.statement, 44),
            g,
            k,
            c,
            u,
            v,
            r.utility.value,
            r.utility.coverage.label()
        ));
    }

    for e in &decision.excluded {
        out.push_str(&format!(
            "  {:>3} {:<44} {}\n",
            "—",
            truncate(&e.statement, 44),
            format_args!("EXCLUDED — {}", truncate(&e.reason, 45))
        ));
    }

    out.push_str(&format!(
        "\n  measured across the whole matrix: {} ({:.0}%)\n",
        decision.coverage.label(),
        decision.coverage.fraction() * 100.0
    ));
    out.push_str("  `—` means the term was not measured; it contributed nothing to the utility.\n");
    out
}

/// The verdict, and the reason when there is not one.
pub fn verdict(decision: &Decision) -> String {
    match (&decision.chosen, &decision.abstention) {
        (Some(id), _) => {
            let r = decision.ranked.iter().find(|r| &r.hypothesis == id);
            let mut s = format!("DECISION  {id}");
            if let Some(r) = r {
                s.push_str(&format!("\n          {}", r.statement));
                s.push_str("\n\n          because");
                for c in &r.utility.contributions {
                    if c.effect == 0.0 && !c.measured {
                        continue;
                    }
                    s.push_str(&format!(
                        "\n            {:>+7.3}  {:<2} {}",
                        c.effect,
                        c.symbol,
                        truncate(&c.note, 70)
                    ));
                }
                s.push_str(&format!("\n            {:>+7.3}  = utility", r.utility.value));
            }
            s
        }
        (None, Some(a)) => {
            let mut s = format!("ABSTAINED  {}", a.headline());
            if let Abstention::TooLittleMeasured { .. } = a {
                s.push_str(
                    "\n           This is a statement about how little was observed, not about the branches.",
                );
            }
            s
        }
        (None, None) => "ABSTAINED  no branch chosen and no reason recorded (this is a bug)".into(),
    }
}

/// Which specialists were consulted, and which declined.
pub fn evaluators(decision: &Decision) -> String {
    if decision.evaluator_status.is_empty() {
        return "EVALUATORS  none registered".into();
    }
    let mut out = String::from("EVALUATORS");
    for s in &decision.evaluator_status {
        out.push_str(&format!(
            "\n  {:<10} {:<14} {}",
            s.evaluator,
            s.applicability.label(),
            truncate(s.applicability.note(), 74)
        ));
    }
    out
}

/// The failure modes of one branch, named even when their likelihood is unknown.
pub fn failure_modes(p: &Projection) -> String {
    if p.failure_modes.is_empty() {
        return String::new();
    }
    let mut out = String::from("FAILURE MODES");
    for f in &p.failure_modes {
        out.push_str(&format!(
            "\n  {:>7}  {}\n           {}",
            if f.likelihood.measured {
                format!("{:.2}", f.likelihood.value)
            } else {
                "—".into()
            },
            truncate(&f.label, 70),
            truncate(&f.detail, 76)
        ));
    }
    out.push_str("\n  A named failure mode with an unknown likelihood is the point, not a gap.");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unmeasured_term_never_prints_as_a_number() {
        // The one rule this module exists for.
        let t = Term::absent("R", "gain", 0.0, "nothing cited");
        assert!(cell(&t).contains('—'));
        assert!(!cell(&t).contains('0'));
    }

    #[test]
    fn a_measured_zero_does_print_as_zero() {
        // And the other half of it: a real zero is a real observation.
        let t = Term::measured("R", "gain", 0.0, "counted zero");
        assert!(cell(&t).contains("0.00"));
    }

    #[test]
    fn a_capped_signal_list_says_how_many_it_dropped() {
        // A silently truncated list is a wrong count.
        use scema_world::{Domain, Entity, EntityKind, Extent, Polarity, Signal, WorldState};
        let sig = |i: usize| Signal {
            id: format!("s{i}"),
            polarity: Polarity::Risk,
            label: format!("thing {i}"),
            detail: String::new(),
            magnitude: 0.5,
            measured: true,
            targets: vec![],
            evidence: vec!["counted".into()],
        };
        let w = WorldState {
            observer: "t".into(),
            entity: Entity { kind: EntityKind::Repository, locator: "/r".into(), label: "r".into() },
            domain: Domain::Software,
            observed_at: 0,
            objects: vec![],
            facts: vec![],
            signals: (0..10).map(sig).collect(),
            extent: Extent::complete(0, "t"),
            blind_spots: vec![],
        };
        let out = signals_capped(&w, 3);
        assert!(out.contains("7 more signal(s) not listed"), "{out}");
        assert!(!signals(&w).contains("not listed"));
    }

    #[test]
    fn an_unbounded_extent_is_shouted_not_implied() {
        use scema_world::{Domain, Entity, EntityKind, Extent, WorldState};
        let w = WorldState {
            observer: "t".into(),
            entity: Entity { kind: EntityKind::Repository, locator: "/r".into(), label: "r".into() },
            domain: Domain::Software,
            observed_at: 0,
            objects: vec![],
            facts: vec![],
            signals: vec![],
            extent: Extent::partial(4000, "walk capped"),
            blind_spots: vec![],
        };
        assert!(world_header(&w).contains("EXTENT UNBOUNDED"));
    }
}
