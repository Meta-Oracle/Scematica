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
use scema_world::{Goal, Signal, Term, WorldState, GOAL_HYPOTHESIS_ID};

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

/// The agent decided — but not the thing that was asked for.
///
/// The second place a first run reads as the tool malfunctioning, and it is quieter than an
/// abstention because it looks like success. The operator types a goal, a branch is chosen,
/// and it is a different branch: `scema simulate "fix the flaky tests"` against this
/// workspace decides to clear a marker backlog instead. Every step is correct — the goal
/// branch is ungrounded, so it has no measured expected gain, so a grounded branch outranks
/// it — but nothing on screen connected the answer to the question, and the honest reading
/// of silence there is that the goal was ignored.
///
/// It is not a warning. Ranking the operator's instruction below observed evidence is the
/// design, and the note says so rather than apologising for it.
fn chose_something_else(goal: &Goal, decision: &Decision) -> String {
    let Some(chosen) = &decision.chosen else {
        return String::new();
    };
    if chosen == GOAL_HYPOTHESIS_ID || goal.statement.trim().is_empty() {
        return String::new();
    }
    // Only worth saying if the goal branch actually competed and lost.
    let Some(g) = decision.ranked.iter().find(|r| r.hypothesis == GOAL_HYPOTHESIS_ID) else {
        return String::new();
    };

    let mut s = String::from("NOTE");
    s.push_str(&format!(
        "\n  The chosen branch is not the one you asked for. Yours ranked below it at\n  \
         {:+.3}.\n",
        g.utility.value
    ));
    if goal.grounded_in.is_empty() {
        s.push_str(
            "\n  It is ungrounded: nothing observed here supports it, so no expected gain\n  \
             could be measured for it, and a branch with a measured gain outranks one\n  \
             without. An instruction is not evidence.\n\n  \
             If it does address something that was counted, say which with `--ground <id>`\n  \
             (`scema observe` lists the ids). Omni will not infer it.",
        );
    } else {
        s.push_str(
            "\n  It is grounded, and still ranked lower — the costs and risks outweigh its\n  \
             projected gain under the current weights. `scema policy` prints them.",
        );
    }
    s
}

/// What the operator can actually do about this outcome.
///
/// The single largest thing standing between a newcomer and this runtime, and it is not a
/// concept — it is a flag they have not heard of.
///
/// A first run is `scema simulate "fix the flaky tests"`. The goal branch is ungrounded, so
/// `scema-sim` refuses to project a gain for it, so it scores at or below zero, so the agent
/// abstains. Every step of that is correct and the design depends on all of it: **an
/// instruction is not evidence**, and inferring grounding from wording is a bug this
/// repository has already shipped once and removed. But the output said only that the agent
/// had abstained, which reads as the tool disagreeing, refusing, or being broken. What was
/// actually being asked for was one flag.
///
/// So the reason for abstaining is rendered as a next command wherever there is one. Each
/// abstention arm is a *different* instruction to the operator, which is why they are five
/// arms and not one, and printing the same sentence under all of them would throw that away.
///
/// It suggests and never acts: grounding is asserted by a human, and a runtime that filled
/// in `--ground` because it looked plausible would be the keyword-overlap bug again with a
/// friendlier face.
pub fn next_steps(world: &WorldState, goal: &Goal, decision: &Decision) -> String {
    let Some(a) = &decision.abstention else {
        return chose_something_else(goal, decision);
    };

    let mut s = String::from("NEXT");
    match a {
        // The common first-run case, and the only one where the fix is a flag.
        Abstention::NoPositiveUtility { .. } if goal.grounded_in.is_empty() => {
            let counted: Vec<&Signal> = world.signals.iter().filter(|s| s.measured).collect();
            if counted.is_empty() {
                s.push_str(
                    "\n  Nothing counted was observed here, so there is no evidence any branch\n  \
                     could stand on. This is a statement about the observation, not the goal.\n\n  \
                     Try `scema observe <path>` to see what the observer could and could not read.",
                );
            } else {
                s.push_str(
                    "\n  The goal branch is ungrounded: nothing observed here supports it, so no\n  \
                     expected gain could be measured for it. An instruction is not evidence.\n\n  \
                     If this goal does address something that was counted, say which — omni will\n  \
                     not infer it, because inferring it once grounded a goal in an unrelated\n  \
                     crate that merely shared a substring of its name:\n",
                );
                for sig in counted.iter().take(6) {
                    s.push_str(&format!(
                        "\n    --ground {:<28}  {}",
                        sig.id,
                        truncate(&sig.label, 44)
                    ));
                }
                if counted.len() > 6 {
                    s.push_str(&format!(
                        "\n    … {} more; `scema observe` lists them all",
                        counted.len() - 6
                    ));
                }
                s.push_str(&format!(
                    "\n\n    scema simulate \"{}\" --ground {}",
                    goal.statement, counted[0].id
                ));
                s.push_str(
                    "\n\n  If it does not address any of them, the abstention is the right answer\n  \
                     and there is nothing to fix.",
                );
            }
        }
        Abstention::NoPositiveUtility { best } => {
            s.push_str(&format!(
                "\n  The goal is grounded, and the best branch still scores {best:.3}: the costs\n  \
                 and risks outweigh the projected gain under the current weights.\n\n  \
                 `scema policy` prints those weights. They are a stated preference, not a\n  \
                 fitted parameter — changing them is a legitimate answer, and it is recorded\n  \
                 in every record so a later reader can see which preferences produced this."
            ));
        }
        Abstention::NoCandidates => {
            s.push_str(
                "\n  No branch was proposed at all. With an empty goal there is nothing for the\n  \
                 goal hypothesiser to propose, and with no counted signals there is nothing for\n  \
                 the signal hypothesiser to propose either.\n\n  \
                 Run `scema observe <path>` — if it reports blind spots, the observer could not\n  \
                 read what it needed rather than finding nothing there.",
            );
        }
        Abstention::AllForbidden { count } => {
            s.push_str(&format!(
                "\n  All {count} branch(es) were excluded by a constraint you set, so the agent\n  \
                 never ranked anything. This is the constraint working.\n"
            ));
            for c in &goal.constraints {
                s.push_str(&format!("\n    --must-not {}", c.subject));
            }
            s.push_str(
                "\n\n  A constraint matches by substring, so a short subject forbids more than it\n  \
                 looks like it does.",
            );
        }
        Abstention::TooLittleMeasured { coverage, floor } => {
            s.push_str(&format!(
                "\n  {} of the ranking's terms were measured, under the {:.0}% floor. This is a\n  \
                 statement about how little was observed, not about the branches — the agent\n  \
                 is refusing to rank on mostly-nothing rather than reporting a confident score\n  \
                 over it.\n\n  \
                 `scema observe <path>` shows the blind spots. An unreadable thing is what\n  \
                 lowers this; a genuinely empty one does not.",
                coverage.label(),
                floor * 100.0
            ));
        }
        Abstention::Contested { by, utility, note } => {
            s.push_str(&format!(
                "\n  `{by}` is qualified on this world and scores the top branch {utility:.3}, so\n  \
                 it vetoed rather than being averaged into the ranking — a utility and a\n  \
                 specialist's normalised score are not the same quantity, and blending them\n  \
                 would hide exactly this.\n\n    {}\n\n  \
                 `scema policy` lists the registered specialists.",
                truncate(note, 74)
            ));
        }
    }
    s
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
    use crate::decide::{DecisionConfig, Ranked};
    use crate::utility::Utility;
    use scema_world::Coverage;

    // ── the two places a first run reads as the tool being broken ──────────────────
    //
    // Both are correct behaviour that looked like a malfunction, which makes them a
    // rendering problem rather than a logic one. They are pinned here because the fix is
    // prose, and prose is the layer with no type system under it.

    fn abstaining_world() -> WorldState {
        use scema_world::{Domain, Entity, EntityKind, Extent, Polarity, Signal};
        WorldState {
            schema: Some(scema_world::WORLD_SCHEMA.into()),
            observer: "t".into(),
            entity: Entity {
                kind: EntityKind::Repository,
                locator: "/r".into(),
                label: "r".into(),
            },
            domain: Domain::Software,
            observed_at: 0,
            objects: vec![],
            facts: vec![],
            signals: vec![Signal {
                id: "markers:core".into(),
                polarity: Polarity::Opportunity,
                label: "11 marker(s)".into(),
                detail: String::new(),
                magnitude: 0.2,
                measured: true,
                targets: vec![],
                evidence: vec!["counted 11".into()],
            }],
            extent: Extent::complete(1, "walked"),
            blind_spots: vec![],
        }
    }

    fn cov() -> Coverage {
        Coverage { measured: 4, total: 5 }
    }

    fn util(value: f64) -> Utility {
        Utility { value, contributions: vec![], coverage: cov() }
    }

    fn ranked(id: &str, statement: &str, value: f64) -> Ranked {
        Ranked {
            hypothesis: id.into(),
            statement: statement.into(),
            utility: util(value),
            evaluations: vec![],
        }
    }

    fn abstained(reason: Abstention) -> Decision {
        Decision {
            chosen: None,
            ranked: vec![],
            excluded: vec![],
            abstention: Some(reason),
            config: DecisionConfig::default(),
            evaluator_status: vec![],
            coverage: cov(),
        }
    }

    #[test]
    fn an_ungrounded_abstention_names_the_signals_it_could_have_been_grounded_in() {
        // The single largest cliff in front of a newcomer, and it is a flag rather than a
        // concept. Before this, the output said only that the agent had abstained, which
        // reads as the tool disagreeing or being broken.
        let w = abstaining_world();
        let g = Goal::new("g", "fix the flaky tests");
        let d = abstained(Abstention::NoPositiveUtility { best: -0.095 });
        let out = next_steps(&w, &g, &d);
        assert!(out.contains("--ground markers:core"), "{out}");
        assert!(out.contains("scema simulate \"fix the flaky tests\" --ground"), "{out}");
        assert!(out.contains("an instruction is not evidence") || out.contains("not infer"), "{out}");
    }

    #[test]
    fn it_suggests_grounding_and_never_performs_it() {
        // Grounding is asserted by a human. A runtime that filled in `--ground` because it
        // looked plausible would be the keyword-overlap bug again with a friendlier face —
        // that one grounded "add tests to the scema-cli crate" in a marker backlog in a
        // different crate, because `scema` is a substring of every unit name here.
        let w = abstaining_world();
        let g = Goal::new("g", "fix the flaky tests");
        assert!(g.grounded_in.is_empty());
        let _ = next_steps(&w, &g, &abstained(Abstention::NoPositiveUtility { best: -0.1 }));
        assert!(g.grounded_in.is_empty(), "rendering must not mutate the goal");
    }

    #[test]
    fn each_abstention_reason_gives_a_different_instruction() {
        // Five reasons exist because they are five different next actions. Collapsing them
        // into one sentence would throw away the only actionable part.
        let w = abstaining_world();
        let g = Goal::new("g", "do the thing");
        let outs: Vec<String> = vec![
            next_steps(&w, &g, &abstained(Abstention::NoCandidates)),
            next_steps(&w, &g, &abstained(Abstention::AllForbidden { count: 3 })),
            next_steps(
                &w,
                &g,
                &abstained(Abstention::TooLittleMeasured {
                    coverage: cov(),
                    floor: 0.5,
                }),
            ),
            next_steps(
                &w,
                &g,
                &abstained(Abstention::Contested {
                    by: "dqstar".into(),
                    utility: -0.4,
                    note: "the net is bearish".into(),
                }),
            ),
        ];
        for o in &outs {
            assert!(!o.is_empty());
        }
        for i in 0..outs.len() {
            for j in (i + 1)..outs.len() {
                assert_ne!(outs[i], outs[j], "two abstention reasons render identically");
            }
        }
    }

    #[test]
    fn a_decision_that_is_not_the_goal_says_so() {
        // The quieter cliff: the agent decided, so it looks like success, and it chose
        // something the operator did not ask for. Correct, and silent about it until now.
        let g = Goal::new("g", "fix the flaky tests");
        let d = Decision {
            chosen: Some("h-markers-core".into()),
            ranked: vec![
                ranked("h-markers-core", "take: 11 marker(s)", 0.125),
                ranked(GOAL_HYPOTHESIS_ID, "fix the flaky tests", -0.095),
            ],
            excluded: vec![],
            abstention: None,
            config: DecisionConfig::default(),
            evaluator_status: vec![],
            coverage: cov(),
        };
        let out = next_steps(&abstaining_world(), &g, &d);
        assert!(out.contains("not the one you asked for"), "{out}");
        assert!(out.contains("--ground"), "{out}");
    }

    #[test]
    fn choosing_the_goal_branch_needs_no_explanation() {
        let g = Goal::new("g", "fix the flaky tests");
        let d = Decision {
            chosen: Some(GOAL_HYPOTHESIS_ID.into()),
            ranked: vec![ranked(GOAL_HYPOTHESIS_ID, "fix the flaky tests", 0.2)],
            excluded: vec![],
            abstention: None,
            config: DecisionConfig::default(),
            evaluator_status: vec![],
            coverage: cov(),
        };
        assert_eq!(next_steps(&abstaining_world(), &g, &d), "");
    }

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
            schema: Some(scema_world::WORLD_SCHEMA.into()),
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
            schema: Some(scema_world::WORLD_SCHEMA.into()),
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
