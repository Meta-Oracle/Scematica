//! Drawing. Rectangles and borders only — every decision about *meaning* was already made
//! in [`crate::view`], and every decision about *colour* in [`crate::theme`].
//!
//! The visual grammar is deliberately not the one `mesh-dashboard` uses. That surface draws
//! a topology, so it is a graph and a vortex. Omni's subject is a **ledger**: a matrix of
//! competing branches, a column of records, a commitment that either recomputes or does
//! not. So this is typographic and column-oriented, and the only "chart" anywhere is the
//! coverage meter — one cell per term, because a proportional bar would hide the
//! denominator, which is the number that matters.
//!
//! Three things this file is not allowed to do, each of which was a real bug somewhere else
//! in this repository:
//!
//! * **Format a `Term`.** `scema_policy::render::cell` does that, via [`crate::view::cell`].
//! * **Pick a colour from a value.** Roles come from `view`; provenance is asked before
//!   value, always.
//! * **Draw an empty bar for an unmeasured quantity.** An empty bar reads as "measured, and
//!   it is zero". Absent renders as nothing, or as `∅`, never as an empty container.

use ratatui::layout::{Alignment, Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use scema_policy::render as core_render;
use scema_world::Polarity;

use crate::app::{App, Focus, Mode, Status, Tab};
use crate::theme::{Role, Theme};
use crate::view;

/// Draw everything.
pub fn draw(f: &mut Frame, app: &App) {
    let t = app.theme;
    let area = f.size();
    f.render_widget(Block::default().style(t.bg(Role::Ground)), area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // header + tabs
            Constraint::Min(6),    // body
            Constraint::Length(1), // status
            Constraint::Length(1), // keys
        ])
        .split(area);

    header(f, app, rows[0]);
    match app.tab {
        Tab::World => world_tab(f, app, rows[1]),
        Tab::Simulate => simulate_tab(f, app, rows[1]),
        Tab::Records => records_tab(f, app, rows[1]),
        Tab::Memory => memory_tab(f, app, rows[1]),
        Tab::Policy => policy_tab(f, app, rows[1]),
    }
    status_bar(f, app, rows[2]);
    key_bar(f, app, rows[3]);

    if app.mode == Mode::ConfirmDecide {
        confirm_decide(f, app, area);
    }
    if app.help {
        help_overlay(f, app, area);
    }
}

// ── chrome ────────────────────────────────────────────────────────────────────

fn panel<'a>(t: &Theme, title: &'a str, focused: bool) -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Plain)
        .border_style(t.style(if focused { Role::ChromeFocus } else { Role::Chrome }))
        .title(Span::styled(
            format!(" {title} "),
            t.style(if focused { Role::HeadingActive } else { Role::Heading }),
        ))
        .style(t.bg(Role::Panel))
}

fn header(f: &mut Frame, app: &App, area: Rect) {
    let t = app.theme;
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(20), Constraint::Length(34)])
        .split(area);

    // Tab strip. The active tab is the only bright thing in the row, and the number keys
    // that select them are printed rather than hidden in the help — a five-tab console
    // where the tabs are undiscoverable is a one-tab console.
    let mut spans: Vec<Span> = vec![Span::styled(" ", t.style(Role::Label))];
    for (i, tab) in Tab::ALL.iter().enumerate() {
        let active = *tab == app.tab;
        spans.push(Span::styled(
            format!(" {}·{} ", i + 1, tab.title()),
            t.style(if active { Role::HeadingActive } else { Role::Heading }),
        ));
        spans.push(Span::styled("│", t.style(Role::Chrome)));
    }
    let strip = Paragraph::new(vec![
        Line::from(spans),
        Line::from(vec![
            Span::styled("  ", t.style(Role::Label)),
            Span::styled(app.tab.question(), t.style(Role::Label)),
        ]),
    ])
    .block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(t.style(Role::Chrome))
            .style(t.bg(Role::Ground)),
    );
    f.render_widget(strip, cols[0]);

    // Right side: runtime, path, and what the worker is doing. `busy` is explicit text and
    // not a spinner alone — a spinner says "alive", this has to say *what*.
    let busy = match app.busy {
        Some(what) => Span::styled(format!("● {what}"), t.style(Role::Working)),
        None => Span::styled("○ idle", t.style(Role::Label)),
    };
    let right = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("SCEMA OMNI ", t.style(Role::HeadingActive)),
            Span::styled(app.runtime, t.style(Role::Label)),
        ])
        .alignment(Alignment::Right),
        Line::from(vec![busy, Span::styled(format!("  {}", view::truncate(&app.path, 20)), t.style(Role::Label))])
            .alignment(Alignment::Right),
    ])
    .block(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(t.style(Role::Chrome))
            .style(t.bg(Role::Ground)),
    );
    f.render_widget(right, cols[1]);
}

fn status_bar(f: &mut Frame, app: &App, area: Rect) {
    let t = app.theme;
    let (glyph, role) = match &app.status {
        Status::Idle => (" ", Role::Label),
        Status::Note(_) => ("·", Role::Measured),
        Status::Warn(_) => ("!", Role::Abstained),
        Status::Error(_) => ("✕", Role::Invalid),
    };
    let line = Line::from(vec![
        Span::styled(format!(" {glyph} "), t.style(role)),
        Span::styled(app.status.text().to_string(), t.style(role)),
    ]);
    f.render_widget(Paragraph::new(line).style(t.bg(Role::Ground)), area);
}

fn key_bar(f: &mut Frame, app: &App, area: Rect) {
    let t = app.theme;
    let keys: &[(&str, &str)] = match (app.mode, app.tab) {
        (Mode::EditGoal, _) => &[("enter", "accept"), ("esc", "cancel")],
        (Mode::EditConstraint, _) => &[("enter", "add must-not"), ("esc", "cancel")],
        (Mode::ConfirmDecide, _) => &[("y", "seal a record"), ("n/esc", "no")],
        (_, Tab::World) => &[
            ("o", "observe"),
            ("tab", "pane"),
            ("space", "ground signal"),
            ("s", "simulate"),
            ("?", "help"),
            ("q", "quit"),
        ],
        (_, Tab::Simulate) => &[
            ("g", "goal"),
            ("m", "must-not"),
            ("enter", "simulate (writes nothing)"),
            ("D", "decide + seal"),
            ("?", "help"),
        ],
        (_, Tab::Records) => &[("r", "reload"), ("enter", "open"), ("v", "re-verify"), ("?", "help")],
        (_, Tab::Memory) => &[("r", "reload"), ("?", "help"), ("q", "quit")],
        (_, Tab::Policy) => &[("?", "help"), ("q", "quit")],
    };
    let mut spans = Vec::new();
    for (k, label) in keys {
        spans.push(Span::styled(format!(" {k} "), t.style(Role::HeadingActive)));
        spans.push(Span::styled(format!("{label}  "), t.style(Role::Hint)));
    }
    f.render_widget(Paragraph::new(Line::from(spans)).style(t.bg(Role::Ground)), area);
}

// ── WORLD ─────────────────────────────────────────────────────────────────────

fn world_tab(f: &mut Frame, app: &App, area: Rect) {
    let t = app.theme;
    let Some(w) = &app.world else {
        let msg = Paragraph::new(vec![
            Line::from(Span::styled("Nothing has been observed yet.", t.style(Role::Body))),
            Line::from(""),
            Line::from(Span::styled(
                format!("Press `o` to perceive {}", app.path),
                t.style(Role::Label),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "An empty world is not the same as an unobserved one: this console",
                t.style(Role::Hint),
            )),
            Line::from(Span::styled(
                "will show you a world with zero signals as exactly that.",
                t.style(Role::Hint),
            )),
        ])
        .block(panel(&t, "WORLD", true))
        .wrap(Wrap { trim: true });
        f.render_widget(msg, area);
        return;
    };

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(46), Constraint::Percentage(54)])
        .split(area);

    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(11), Constraint::Min(4)])
        .split(cols[0]);

    // ── the header: what was seen, and what was not ──────────────────────────
    let (extent_text, extent_role) = view::extent_line(w);
    let legibility = w.legibility();
    let mut lines = vec![
        kv(&t, "entity", &w.entity.locator),
        kv(&t, "kind", &format!("{:?} · {:?}", w.entity.kind, w.domain)),
        kv(&t, "observer", &w.observer),
        Line::from(vec![
            Span::styled(format!("{:<11}", "extent"), t.style(Role::Label)),
            Span::styled(view::truncate(&extent_text, 60), t.style(extent_role)),
        ]),
        Line::from(vec![
            Span::styled(format!("{:<11}", "legibility"), t.style(Role::Label)),
            // `legibility` is a measured ratio over observed objects, so it is a measured
            // number even when it is low. A world with no objects returns 0.0, which is
            // ignorance, not perfection — the note says which.
            Span::styled(format!("{:.0}%", legibility * 100.0), t.style(Role::Measured)),
            Span::styled(
                if w.objects.is_empty() {
                    "  (no objects — illegible, not perfect)"
                } else {
                    "  of observed objects are current"
                },
                t.style(Role::Hint),
            ),
        ]),
        Line::from(""),
    ];
    if w.blind_spots.is_empty() {
        lines.push(Line::from(vec![
            Span::styled(format!("{:<11}", "blind"), t.style(Role::Label)),
            Span::styled("none reported", t.style(Role::Label)),
        ]));
    } else {
        lines.push(Line::from(vec![
            Span::styled(format!("{:<11}", "blind"), t.style(Role::Label)),
            Span::styled(
                format!("{} thing(s) could not be read", w.blind_spots.len()),
                t.style(Role::Abstained),
            ),
        ]));
        for b in w.blind_spots.iter().take(3) {
            lines.push(Line::from(vec![
                Span::styled("           · ", t.style(Role::Chrome)),
                Span::styled(view::truncate(b, 58), t.style(Role::Abstained)),
            ]));
        }
        if w.blind_spots.len() > 3 {
            lines.push(Line::from(Span::styled(
                format!("           … {} more", w.blind_spots.len() - 3),
                t.style(Role::Hint),
            )));
        }
    }
    f.render_widget(
        Paragraph::new(lines).block(panel(&t, "PERCEPTION", false)),
        left[0],
    );

    // ── objects, coloured by provenance and never by value ───────────────────
    let obj_focus = app.focus == Focus::Left;
    let obj_area = left[1];
    let inner = obj_area.inner(&Margin { horizontal: 1, vertical: 1 });
    let visible = inner.height as usize;
    let start = app.object_sel.saturating_sub(visible.saturating_sub(1));
    let mut obj_lines = Vec::new();
    for (i, o) in w.objects.iter().enumerate().skip(start).take(visible) {
        let selected = i == app.object_sel && obj_focus;
        let prov = view::provenance_role(&o.provenance);
        let base = if selected { t.on(Role::Body, Role::Selection) } else { t.style(Role::Body) };
        obj_lines.push(Line::from(vec![
            Span::styled(
                format!("{:<11}", view::provenance_label(&o.provenance)),
                merge(t.style(prov), base),
            ),
            Span::styled(format!("{:<11}", view::truncate(&o.kind, 10)), merge(t.style(Role::Label), base)),
            Span::styled(view::truncate(&o.label, 30), base),
        ]));
    }
    if w.objects.is_empty() {
        obj_lines.push(Line::from(Span::styled(
            "no objects — the observer reached nothing it could name",
            t.style(Role::Label),
        )));
    }
    f.render_widget(
        Paragraph::new(obj_lines).block(panel(
            &t,
            &format!("OBJECTS {}/{}", (app.object_sel + 1).min(w.objects.len()), w.objects.len()),
            obj_focus,
        )),
        obj_area,
    );

    // ── signals: the only things that can ground a branch ────────────────────
    signal_pane(f, app, cols[1]);
}

fn signal_pane(f: &mut Frame, app: &App, area: Rect) {
    let t = app.theme;
    let Some(w) = &app.world else { return };
    let focused = app.focus == Focus::Right;
    let inner = area.inner(&Margin { horizontal: 1, vertical: 1 });
    let visible = (inner.height as usize).saturating_sub(1);
    let start = app.signal_sel.saturating_sub(visible.saturating_sub(1).max(1));

    let mut lines = vec![Line::from(vec![
        Span::styled("  ✓  ", t.style(Role::Label)),
        Span::styled(format!("{:<5}", "KIND"), t.style(Role::Label)),
        Span::styled(format!("{:>6}", "MAG"), t.style(Role::Label)),
        Span::styled("  LABEL", t.style(Role::Label)),
    ])];

    for (i, s) in w.signals.iter().enumerate().skip(start).take(visible) {
        let selected = i == app.signal_sel && focused;
        let base = if selected { t.on(Role::Body, Role::Selection) } else { t.style(Role::Body) };
        let ticked = app.grounded.contains(&s.id);
        let role = view::signal_role(s);
        lines.push(Line::from(vec![
            // The tick is the grounding assertion. `[x]` rather than a colour, because it
            // is a claim the operator is making and it has to survive a screenshot.
            Span::styled(
                if ticked { "  ✓  " } else { "  ·  " },
                merge(t.style(if ticked { Role::Chosen } else { Role::Chrome }), base),
            ),
            Span::styled(format!("{:<5}", view::signal_tag(s)), merge(t.style(role), base)),
            // A magnitude the observer *estimated* must not render like a counted one.
            Span::styled(
                format!("{:>6.2}", s.magnitude),
                merge(t.style(if s.measured { Role::Measured } else { Role::Estimated }), base),
            ),
            Span::styled(format!("  {}", view::truncate(&s.label, 48)), base),
        ]));
        if selected {
            if let Some(e) = s.evidence.first() {
                lines.push(Line::from(vec![
                    Span::styled("        └ ", t.style(Role::Chrome)),
                    Span::styled(view::truncate(e, 56), t.style(Role::Hint)),
                ]));
            }
        }
    }
    if w.signals.is_empty() {
        lines.push(Line::from(Span::styled(
            "none counted. Nothing can ground a branch, so the agent will abstain —",
            t.style(Role::Abstained),
        )));
        lines.push(Line::from(Span::styled(
            "which is the honest answer, not a failure.",
            t.style(Role::Hint),
        )));
    }

    let (risks, opportunities) = polarity_counts(w);
    let title = format!(
        "SIGNALS  {risks} risk · {opportunities} opportunity · {} asserted as grounds",
        app.grounded.len()
    );
    f.render_widget(Paragraph::new(lines).block(panel(&t, &title, focused)), area);
}

// ── SIMULATE ──────────────────────────────────────────────────────────────────

fn simulate_tab(f: &mut Frame, app: &App, area: Rect) {
    let t = app.theme;
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(6), Constraint::Min(8)])
        .split(area);

    // ── the ask ──────────────────────────────────────────────────────────────
    let editing_goal = app.mode == Mode::EditGoal;
    let goal_text = if app.goal.is_empty() && !editing_goal {
        Span::styled("(press `g` and state a goal)", t.style(Role::Hint))
    } else {
        Span::styled(
            format!("{}{}", app.goal, if editing_goal { "▏" } else { "" }),
            t.style(Role::Body),
        )
    };
    let mut ask = vec![
        Line::from(vec![Span::styled(format!("{:<11}", "goal"), t.style(Role::Label)), goal_text]),
        Line::from(vec![
            Span::styled(format!("{:<11}", "grounds"), t.style(Role::Label)),
            if app.grounded.is_empty() {
                Span::styled(
                    "none — an ungrounded goal branch scores ≤ 0 and the agent abstains",
                    t.style(Role::Abstained),
                )
            } else {
                Span::styled(
                    app.grounded.iter().cloned().collect::<Vec<_>>().join(", "),
                    t.style(Role::Chosen),
                )
            },
        ]),
        Line::from(vec![
            Span::styled(format!("{:<11}", "must-not"), t.style(Role::Label)),
            if app.must_not.is_empty() && app.mode != Mode::EditConstraint {
                Span::styled("none", t.style(Role::Label))
            } else {
                Span::styled(
                    format!(
                        "{}{}",
                        app.must_not.join("  "),
                        if app.mode == Mode::EditConstraint {
                            format!("  {}▏", app.constraint_draft)
                        } else {
                            String::new()
                        }
                    ),
                    t.style(Role::Body),
                )
            },
        ]),
    ];
    let dangling = app.dangling_grounds();
    if !dangling.is_empty() {
        ask.push(Line::from(vec![
            Span::styled(format!("{:<11}", "ignored"), t.style(Role::Label)),
            Span::styled(
                format!("{} — no such signal in this world", dangling.join(", ")),
                t.style(Role::Abstained),
            ),
        ]));
    }
    f.render_widget(
        Paragraph::new(ask).block(panel(&t, "ASK", app.mode == Mode::EditGoal || app.mode == Mode::EditConstraint)),
        rows[0],
    );

    // ── the matrix ───────────────────────────────────────────────────────────
    let Some(c) = &app.cycle else {
        let hint = Paragraph::new(vec![
            Line::from(Span::styled("No ranking yet.", t.style(Role::Body))),
            Line::from(""),
            Line::from(Span::styled(
                "`enter` simulates — it computes the whole cycle and writes nothing.",
                t.style(Role::Label),
            )),
            Line::from(Span::styled(
                "`D` decides — same computation, but it seals a record and appends memory.",
                t.style(Role::Label),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "They are separate keys on purpose. The two paths compute exactly the same",
                t.style(Role::Hint),
            )),
            Line::from(Span::styled(
                "thing and differ only in whether they leave a trace, so the only protection",
                t.style(Role::Hint),
            )),
            Line::from(Span::styled(
                "against a counterfactual later reading as a decision is that they are not",
                t.style(Role::Hint),
            )),
            Line::from(Span::styled("the same keystroke.", t.style(Role::Hint))),
        ])
        .block(panel(&t, "SIMULATION MATRIX", true))
        .wrap(Wrap { trim: true });
        f.render_widget(hint, rows[1]);
        return;
    };

    let split = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(6), Constraint::Length(12)])
        .split(rows[1]);

    matrix(f, app, split[0], c);
    verdict(f, app, split[1], c);
}

fn matrix(f: &mut Frame, app: &App, area: Rect, c: &scema_agent::Cycle) {
    let t = app.theme;
    let d = &c.decision;
    let mut lines = vec![Line::from(vec![
        Span::styled(format!(" {:<3} {:<38}", "#", "BRANCH"), t.style(Role::Label)),
        Span::styled(
            format!("{:>7}{:>7}{:>7}{:>7}{:>7}", "GAIN", "RISK", "COST", "UNCERT", "REVERS"),
            t.style(Role::Label),
        ),
        Span::styled(format!("{:>9}  ", "UTILITY"), t.style(Role::Label)),
        Span::styled("MEASURED", t.style(Role::Label)),
    ])];

    for (i, r) in d.ranked.iter().enumerate() {
        let selected = i == app.matrix_sel;
        let role = view::rank_role(d, r);
        let base = if selected { t.on(Role::Body, Role::Selection) } else { Style::default() };
        let mut spans = vec![
            Span::styled(
                format!("{}{:>3} ", view::rank_marker(d, r), i + 1),
                merge(t.style(role), base),
            ),
            Span::styled(format!("{:<38}", view::truncate(&r.statement, 38)), merge(t.style(role), base)),
        ];
        match view::projection_for(&c.projections, &r.hypothesis) {
            Some(p) => {
                for term in p.terms() {
                    let (text, tr) = view::cell(term);
                    spans.push(Span::styled(text, merge(t.style(tr), base)));
                }
            }
            None => {
                // A ranked row with no projection is a bug upstream, and it must look like
                // one rather than like five unmeasured terms.
                for _ in 0..5 {
                    spans.push(Span::styled(format!("{:>7}", "?"), merge(t.style(Role::Invalid), base)));
                }
            }
        }
        spans.push(Span::styled(
            format!("{:>9.3}  ", r.utility.value),
            merge(t.style(role), base),
        ));
        spans.push(Span::styled(
            view::coverage_meter(r.utility.coverage),
            merge(
                t.style(if view::coverage_is_thin(r.utility.coverage, d.config.min_coverage) {
                    Role::Abstained
                } else {
                    Role::Measured
                }),
                base,
            ),
        ));
        lines.push(Line::from(spans));
    }

    for e in &d.excluded {
        lines.push(Line::from(vec![
            Span::styled(format!(" {:>3} ", "—"), t.style(Role::Excluded)),
            Span::styled(format!("{:<38}", view::truncate(&e.statement, 38)), t.style(Role::Excluded)),
            Span::styled(
                format!("EXCLUDED — {}", view::truncate(&e.reason, 40)),
                t.style(Role::Abstained),
            ),
        ]));
    }

    if d.ranked.is_empty() && d.excluded.is_empty() {
        lines.push(Line::from(Span::styled(
            "no branch was allowed to compete",
            t.style(Role::Abstained),
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled(" measured across the whole matrix: ", t.style(Role::Label)),
        Span::styled(d.coverage.label(), t.style(Role::Measured)),
        Span::styled(
            format!("  ({:.0}%)   ", d.coverage.fraction() * 100.0),
            t.style(Role::Measured),
        ),
        Span::styled(
            format!("`{}` = not measured; it contributed nothing.", view::UNMEASURED),
            t.style(Role::Hint),
        ),
    ]));

    let title = if app.cycle_persisted {
        format!("SIMULATION MATRIX  ·  SEALED AS {}", c.record.id)
    } else {
        format!("SIMULATION MATRIX  ·  NOT WRITTEN (would seal as {})", c.record.id)
    };
    f.render_widget(Paragraph::new(lines).block(panel(&t, &title, true)), area);
}

fn verdict(f: &mut Frame, app: &App, area: Rect, c: &scema_agent::Cycle) {
    let t = app.theme;
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
        .split(area);

    let d = &c.decision;
    let mut lines = Vec::new();
    match (&d.chosen, &d.abstention) {
        (Some(id), _) => {
            let r = d.ranked.iter().find(|r| &r.hypothesis == id);
            lines.push(Line::from(vec![
                Span::styled("▸ DECISION  ", t.style(Role::Chosen)),
                Span::styled(id.clone(), t.style(Role::Chosen)),
            ]));
            if let Some(r) = r {
                lines.push(Line::from(Span::styled(
                    format!("            {}", view::truncate(&r.statement, 60)),
                    t.style(Role::Body),
                )));
                lines.push(Line::from(""));
                // The explanation has to add up on screen. A score a reader cannot
                // decompose is a score they have to take on trust, which is the opposite of
                // what a decision record is for.
                for contribution in &r.utility.contributions {
                    if contribution.effect == 0.0 && !contribution.measured {
                        continue;
                    }
                    lines.push(Line::from(vec![
                        Span::styled(
                            format!("   {:>+7.3}  ", contribution.effect),
                            t.style(if contribution.measured { Role::Measured } else { Role::Unmeasured }),
                        ),
                        Span::styled(format!("{:<3}", contribution.symbol), t.style(Role::Label)),
                        Span::styled(view::truncate(&contribution.note, 48), t.style(Role::Hint)),
                    ]));
                }
                lines.push(Line::from(vec![Span::styled(
                    format!("   {:>+7.3}  = utility", r.utility.value),
                    t.style(Role::Chosen),
                )]));
            }
        }
        (None, Some(a)) => {
            lines.push(Line::from(Span::styled(
                format!("◇ ABSTAINED  {}", a.headline()),
                t.style(Role::Abstained),
            )));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                view::abstention_advice(a),
                t.style(Role::Body),
            )));
        }
        (None, None) => lines.push(Line::from(Span::styled(
            "no branch chosen and no reason recorded — this is a bug",
            t.style(Role::Invalid),
        ))),
    }
    f.render_widget(
        Paragraph::new(lines).block(panel(&t, "VERDICT", false)).wrap(Wrap { trim: false }),
        cols[0],
    );

    // ── specialists ──────────────────────────────────────────────────────────
    let mut ev = Vec::new();
    if d.evaluator_status.is_empty() {
        ev.push(Line::from(Span::styled("none registered", t.style(Role::Label))));
    }
    for s in &d.evaluator_status {
        let role = if s.applicability.is_applicable() { Role::Measured } else { Role::Label };
        ev.push(Line::from(vec![
            Span::styled(format!("{:<9}", s.evaluator), t.style(Role::Body)),
            Span::styled(format!("{:<15}", s.applicability.label()), t.style(role)),
        ]));
        ev.push(Line::from(Span::styled(
            format!("  {}", view::truncate(s.applicability.note(), 44)),
            t.style(Role::Hint),
        )));
    }
    f.render_widget(
        Paragraph::new(ev).block(panel(&t, "SPECIALISTS", false)).wrap(Wrap { trim: true }),
        cols[1],
    );
}

// ── RECORDS ───────────────────────────────────────────────────────────────────

fn records_tab(f: &mut Frame, app: &App, area: Rect) {
    let t = app.theme;
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(46), Constraint::Percentage(54)])
        .split(area);

    let mut lines = vec![Line::from(vec![
        Span::styled(format!(" {:<9}{:<9}", "ID", "COMMIT"), t.style(Role::Label)),
        Span::styled("GOAL", t.style(Role::Label)),
    ])];
    if app.records.is_empty() {
        lines.push(Line::from(Span::styled(
            format!("no records under {}", app.root.join("decisions").display()),
            t.style(Role::Label),
        )));
        lines.push(Line::from(Span::styled(
            "press `D` on the SIMULATE tab to seal one",
            t.style(Role::Hint),
        )));
    }
    let inner = cols[0].inner(&Margin { horizontal: 1, vertical: 1 });
    let visible = (inner.height as usize).saturating_sub(1);
    let start = app.record_sel.saturating_sub(visible.saturating_sub(1).max(1));
    for (i, r) in app.records.iter().enumerate().skip(start).take(visible) {
        let selected = i == app.record_sel;
        let base = if selected { t.on(Role::Body, Role::Selection) } else { Style::default() };
        // Three states, drawn as three things. An unreadable record must not render as an
        // invalid one — the second is an accusation and the first is a gap.
        let (mark, role) = match (r.unreadable.is_some(), r.valid) {
            (true, _) => ("UNREAD", Role::Abstained),
            (_, Some(true)) => ("VALID", Role::Valid),
            (_, Some(false)) => ("INVALID", Role::Invalid),
            (_, None) => ("—", Role::Unmeasured),
        };
        lines.push(Line::from(vec![
            Span::styled(format!(" {:<9}", r.id), merge(t.style(Role::Body), base)),
            Span::styled(format!("{mark:<9}"), merge(t.style(role), base)),
            Span::styled(
                match &r.unreadable {
                    Some(e) => view::truncate(e, 34),
                    None => view::truncate(&r.goal, 34),
                },
                merge(t.style(Role::Body), base),
            ),
        ]));
        if selected && r.unreadable.is_none() {
            // The outcome under the selected row rather than in a column of its own: an
            // abstention headline is a sentence, and truncating it to a column width is how
            // "the ranking stands on 1/5 measured terms" becomes "the ranking stands on…".
            lines.push(Line::from(vec![
                Span::styled("   └ ", t.style(Role::Chrome)),
                Span::styled(
                    view::truncate(&r.outcome, 44),
                    t.style(if r.chosen.is_some() { Role::Chosen } else { Role::Abstained }),
                ),
            ]));
            lines.push(Line::from(vec![
                Span::styled("     ", t.style(Role::Chrome)),
                Span::styled(
                    format!("{}  ·  {}", stamp(r.at), view::truncate(&r.entity, 30)),
                    t.style(Role::Hint),
                ),
            ]));
        }
    }
    f.render_widget(
        Paragraph::new(lines).block(panel(&t, &format!("DECISION RECORDS  {}", app.records.len()), true)),
        cols[0],
    );

    record_detail(f, app, cols[1]);
}

fn record_detail(f: &mut Frame, app: &App, area: Rect) {
    let t = app.theme;
    let Some(pair) = &app.open_record else {
        f.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled("Select a record and press `enter`.", t.style(Role::Body))),
                Line::from(""),
                Line::from(Span::styled("What a verified commitment proves:", t.style(Role::Label))),
                Line::from(Span::styled(
                    "  · the record was not edited after it was sealed.",
                    t.style(Role::Body),
                )),
                Line::from(""),
                Line::from(Span::styled("What it does NOT prove:", t.style(Role::Label))),
                Line::from(Span::styled(
                    "  · that the world was as described — provenance carries that, not the digest;",
                    t.style(Role::Body),
                )),
                Line::from(Span::styled(
                    "  · that this is the original record. Tamper-evident, not tamper-proof,",
                    t.style(Role::Body),
                )),
                Line::from(Span::styled(
                    "    until the root is anchored somewhere the author does not control.",
                    t.style(Role::Body),
                )),
            ])
            .block(panel(&t, "RECORD", false))
            .wrap(Wrap { trim: true }),
            area,
        );
        return;
    };

    let (record, v) = (&pair.0, &pair.1);
    let mut lines = vec![
        kv(&t, "id", &record.id),
        kv(&t, "runtime", &record.runtime),
        kv(&t, "goal", &view::truncate(&record.goal.statement, 48)),
        kv(&t, "entity", &view::truncate(&record.world.entity.locator, 48)),
        kv(&t, "sealed", &stamp(record.at)),
        Line::from(""),
        Line::from(vec![
            Span::styled(format!("{:<11}", "commit"), t.style(Role::Label)),
            Span::styled(
                if v.valid { "VALID" } else { "INVALID" },
                t.style(if v.valid { Role::Valid } else { Role::Invalid }),
            ),
            Span::styled(
                format!("  root {}", view::truncate(&record.commitment.root, 24)),
                t.style(Role::Hint),
            ),
        ]),
    ];
    for m in &v.mismatches {
        lines.push(Line::from(vec![
            Span::styled(format!("  {:<12}", m.field), t.style(Role::Invalid)),
            Span::styled(
                format!(
                    "committed {}…  recomputed {}…",
                    &m.committed[..m.committed.len().min(10)],
                    &m.recomputed[..m.recomputed.len().min(10)]
                ),
                t.style(Role::Body),
            ),
        ]));
    }
    if v.root_only {
        lines.push(Line::from(Span::styled(
            "  every part verifies but the root does not — the root was edited on its own",
            t.style(Role::Invalid),
        )));
    }
    lines.push(Line::from(""));

    // The matrix, from the sealed record rather than from a live cycle. Same renderer
    // rules; the numbers are historical.
    let d = &record.decision;
    lines.push(Line::from(Span::styled("RANKING AS SEALED", t.style(Role::Label))));
    for r in &d.ranked {
        let role = view::rank_role(d, r);
        let mut spans = vec![Span::styled(
            format!("{} {:<30}", view::rank_marker(d, r), view::truncate(&r.statement, 30)),
            t.style(role),
        )];
        if let Some(p) = view::projection_for(&record.projections, &r.hypothesis) {
            for term in p.terms() {
                let (text, tr) = view::cell(term);
                spans.push(Span::styled(text, t.style(tr)));
            }
        }
        spans.push(Span::styled(format!("{:>8.3}", r.utility.value), t.style(role)));
        lines.push(Line::from(spans));
    }
    if let Some(a) = &d.abstention {
        lines.push(Line::from(Span::styled(
            format!("◇ ABSTAINED — {}", a.headline()),
            t.style(Role::Abstained),
        )));
    }

    f.render_widget(
        Paragraph::new(lines).block(panel(&t, "RECORD", false)).wrap(Wrap { trim: false }),
        area,
    );
}

// ── MEMORY ────────────────────────────────────────────────────────────────────

fn memory_tab(f: &mut Frame, app: &App, area: Rect) {
    let t = app.theme;
    let Some(m) = &app.memory else {
        f.render_widget(
            Paragraph::new(Span::styled("press `r` to read memory", t.style(Role::Label)))
                .block(panel(&t, "MEMORY", true)),
            area,
        );
        return;
    };

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    let mut lines = vec![
        Line::from(Span::styled(m.root.display().to_string(), t.style(Role::Hint))),
        Line::from(""),
    ];
    for (kind, n, corrupt) in &m.counts {
        lines.push(Line::from(vec![
            Span::styled(format!("  {:<16}", format!("{kind:?}")), t.style(Role::Body)),
            Span::styled(format!("{n:>6} record(s)"), t.style(Role::Measured)),
            if *corrupt > 0 {
                // An unreadable line is not a missing record and not a present one. It is
                // counted separately so a corrupt log cannot masquerade as a short one.
                Span::styled(format!("   {corrupt} unreadable line(s)"), t.style(Role::Invalid))
            } else {
                Span::styled(String::new(), t.style(Role::Label))
            },
        ]));
    }
    f.render_widget(Paragraph::new(lines).block(panel(&t, "FOUR MEMORIES", false)), cols[0]);

    let c = &m.calibration;
    let cal = vec![
        Line::from(vec![
            Span::styled(format!("{:<32}", "branches not taken, recorded"), t.style(Role::Label)),
            Span::styled(format!("{:>6}", c.recorded), t.style(Role::Measured)),
        ]),
        Line::from(vec![
            Span::styled(format!("{:<32}", "of those, later resolved"), t.style(Role::Label)),
            Span::styled(format!("{:>6}", c.resolved), t.style(Role::Measured)),
        ]),
        Line::from(vec![
            Span::styled(format!("{:<32}", "unresolved"), t.style(Role::Label)),
            Span::styled(format!("{:>6}", c.unresolved), t.style(Role::Measured)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(format!("{:<32}", "mean |projected − realised|"), t.style(Role::Label)),
            match c.mean_abs_error {
                Some(e) => Span::styled(format!("{e:>6.3}"), t.style(Role::Measured)),
                // `None`, not `0.000`. A perfect score and no evidence must not print
                // alike — this is the same rule as the em dash in the matrix, one layer up.
                None => Span::styled(format!("{:>6}", view::UNMEASURED), t.style(Role::Unmeasured)),
            },
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "A branch nobody ran has no outcome. Unresolved counterfactuals are counted,",
            t.style(Role::Hint),
        )),
        Line::from(Span::styled(
            "never scored — imputing one would mean the loop generating its own training",
            t.style(Role::Hint),
        )),
        Line::from(Span::styled(
            "signal, and every later decision would be tuned to a fiction.",
            t.style(Role::Hint),
        )),
    ];
    f.render_widget(
        Paragraph::new(cal).block(panel(&t, "CALIBRATION", false)).wrap(Wrap { trim: true }),
        cols[1],
    );
}

// ── POLICY ────────────────────────────────────────────────────────────────────

fn policy_tab(f: &mut Frame, app: &App, area: Rect) {
    let t = app.theme;
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    let w = app.config.weights;
    let lines = vec![
        Line::from(Span::styled("U = R − λ₁K − λ₂C − λ₃U + λ₄V", t.style(Role::HeadingActive))),
        Line::from(""),
        weight(&t, "λ₁ risk", w.risk),
        weight(&t, "λ₂ cost", w.cost),
        weight(&t, "λ₃ uncertainty", w.uncertainty),
        weight(&t, "λ₄ reversibility", w.reversibility),
        Line::from(""),
        Line::from(Span::styled(
            "A stated preference, not a fitted parameter. Hashed into every record so a",
            t.style(Role::Hint),
        )),
        Line::from(Span::styled(
            "ranking can be re-read against the preferences that produced it.",
            t.style(Role::Hint),
        )),
        Line::from(""),
        Line::from(Span::styled("GATES", t.style(Role::Label))),
        Line::from(vec![
            Span::styled(format!("  {:<22}", "min measured fraction"), t.style(Role::Body)),
            Span::styled(format!("{:.0}%", app.config.min_coverage * 100.0), t.style(Role::Measured)),
        ]),
        Line::from(vec![
            Span::styled(format!("  {:<22}", "specialist veto at"), t.style(Role::Body)),
            Span::styled(format!("≤ {:.2}", app.config.veto_at_or_below), t.style(Role::Measured)),
        ]),
    ];
    f.render_widget(
        Paragraph::new(lines).block(panel(&t, "UTILITY", false)).wrap(Wrap { trim: true }),
        cols[0],
    );

    let mut right = vec![Line::from(Span::styled("OBSERVERS", t.style(Role::Label)))];
    for (name, about) in &app.observers {
        right.push(Line::from(Span::styled(format!("  {name}"), t.style(Role::Body))));
        right.push(Line::from(Span::styled(format!("    {about}"), t.style(Role::Hint))));
    }
    right.push(Line::from(""));
    right.push(Line::from(Span::styled("SPECIALISTS", t.style(Role::Label))));
    for (name, about) in &app.evaluators {
        right.push(Line::from(Span::styled(format!("  {name}"), t.style(Role::Body))));
        right.push(Line::from(Span::styled(format!("    {about}"), t.style(Role::Hint))));
    }
    right.push(Line::from(""));
    right.push(Line::from(Span::styled(
        "A specialist may decline in two distinguishable ways: OUT-OF-DOMAIN is",
        t.style(Role::Hint),
    )));
    right.push(Line::from(Span::styled(
        "permanent and fine; INSUFFICIENT means its domain, missing inputs — which",
        t.style(Role::Hint),
    )));
    right.push(Line::from(Span::styled(
        "is something you can go and supply.",
        t.style(Role::Hint),
    )));
    f.render_widget(
        Paragraph::new(right).block(panel(&t, "REGISTRY", false)).wrap(Wrap { trim: true }),
        cols[1],
    );
}

fn weight<'a>(t: &Theme, label: &'a str, value: f64) -> Line<'a> {
    // A bar, but a bar of a *stated preference* rather than a measurement, so it is drawn
    // with the label tone rather than the measured tone. Nothing here was observed.
    let filled = ((value.clamp(0.0, 1.0)) * 16.0).round() as usize;
    Line::from(vec![
        Span::styled(format!("  {label:<18}"), t.style(Role::Body)),
        Span::styled(format!("{value:>5.2}  "), t.style(Role::Body)),
        Span::styled("█".repeat(filled), t.style(Role::Heading)),
        Span::styled("░".repeat(16 - filled), t.style(Role::Chrome)),
    ])
}

// ── overlays ──────────────────────────────────────────────────────────────────

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect { x, y, width: w.min(area.width), height: h.min(area.height) }
}

fn confirm_decide(f: &mut Frame, app: &App, area: Rect) {
    let t = app.theme;
    let r = centered(area, 66, 11);
    f.render_widget(Clear, r);
    let lines = vec![
        Line::from(Span::styled("Seal a decision record?", t.style(Role::HeadingActive))),
        Line::from(""),
        Line::from(Span::styled(
            "This writes a record under .scema/decisions/ and appends memory,",
            t.style(Role::Body),
        )),
        Line::from(Span::styled(
            "including one counterfactual per branch not taken.",
            t.style(Role::Body),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("goal  ", t.style(Role::Label)),
            Span::styled(view::truncate(&app.goal, 52), t.style(Role::Body)),
        ]),
        Line::from(vec![
            Span::styled("ground ", t.style(Role::Label)),
            Span::styled(
                if app.grounded.is_empty() {
                    "nothing asserted — expect an abstention".to_string()
                } else {
                    app.grounded.iter().cloned().collect::<Vec<_>>().join(", ")
                },
                t.style(if app.grounded.is_empty() { Role::Abstained } else { Role::Chosen }),
            ),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("  y ", t.style(Role::HeadingActive)),
            Span::styled("seal it     ", t.style(Role::Body)),
            Span::styled("  n/esc ", t.style(Role::HeadingActive)),
            Span::styled("no", t.style(Role::Body)),
        ]),
    ];
    f.render_widget(
        Paragraph::new(lines)
            .block(panel(&t, "CONFIRM", true))
            .wrap(Wrap { trim: true }),
        r,
    );
}

fn help_overlay(f: &mut Frame, app: &App, area: Rect) {
    let t = app.theme;
    let r = centered(area, 78, 26);
    f.render_widget(Clear, r);
    let k = |key: &'static str, what: &'static str| {
        Line::from(vec![
            Span::styled(format!("  {key:<12}"), t.style(Role::HeadingActive)),
            Span::styled(what, t.style(Role::Body)),
        ])
    };
    let lines = vec![
        Line::from(Span::styled("NAVIGATION", t.style(Role::Label))),
        k("1..5 / ←→", "switch tab"),
        k("tab", "move focus between panes"),
        k("↑↓ / jk", "move the selection"),
        k("? ", "this overlay"),
        k("q / ctrl-c", "quit"),
        Line::from(""),
        Line::from(Span::styled("PERCEIVING", t.style(Role::Label))),
        k("o", "observe the path again"),
        k("space", "assert the selected signal as a ground for the goal"),
        Line::from(""),
        Line::from(Span::styled("DECIDING", t.style(Role::Label))),
        k("g", "edit the goal"),
        k("m", "add a must-not constraint (subject[:detail])"),
        k("enter", "simulate — computes everything, writes nothing"),
        k("D", "decide — same computation, seals a record"),
        Line::from(""),
        Line::from(Span::styled("READING BACK", t.style(Role::Label))),
        k("r", "reload records / memory"),
        k("v", "re-verify the open record"),
        Line::from(""),
        Line::from(Span::styled(
            "An em dash means nobody measured that term. It is not a zero, and it",
            t.style(Role::Hint),
        )),
        Line::from(Span::styled(
            "contributed nothing to the utility beside it.",
            t.style(Role::Hint),
        )),
    ];
    f.render_widget(
        Paragraph::new(lines).block(panel(&t, "KEYS", true)).wrap(Wrap { trim: true }),
        r,
    );
}

// ── small helpers ─────────────────────────────────────────────────────────────

/// Unix seconds as a readable stamp, without pulling in `chrono`.
///
/// `scema-world` deliberately takes two dependencies so a reimplementer has two to match,
/// and a date formatter in the TUI would be a third in the tree for one column. This is
/// civil-from-days arithmetic (Howard Hinnant's `civil_from_days`), which is exact and
/// about fifteen lines.
fn stamp(unix: i64) -> String {
    if unix <= 0 {
        return "—".into();
    }
    let days = unix.div_euclid(86_400);
    let secs = unix.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}Z",
        secs / 3_600,
        (secs % 3_600) / 60
    )
}

fn kv<'a>(t: &Theme, key: &'a str, value: &str) -> Line<'a> {
    Line::from(vec![
        Span::styled(format!("{key:<11}"), t.style(Role::Label)),
        Span::styled(value.to_string(), t.style(Role::Body)),
    ])
}

/// Overlay a foreground style onto a background style, keeping the background.
///
/// Selection is a background; every role is a foreground. Setting them independently means
/// a selected row keeps its per-cell meaning instead of flattening to one highlight colour,
/// which would erase the measured/unmeasured distinction on exactly the row the operator is
/// looking at.
fn merge(fg: Style, bg: Style) -> Style {
    match bg.bg {
        Some(c) => fg.bg(c),
        None => fg,
    }
}

/// Re-exported so a caller that wants the plain-text matrix (for `--once`, for a pipe) uses
/// the same formatter the CLI does rather than scraping the buffer.
pub fn plain_matrix(c: &scema_agent::Cycle) -> String {
    let mut out = String::new();
    out.push_str(&core_render::world_header(&c.world));
    out.push_str("\n\n");
    out.push_str(&core_render::signals(&c.world));
    out.push_str("\n\n");
    out.push_str(&core_render::matrix(&c.decision, &c.projections));
    out.push('\n');
    out.push_str(&core_render::evaluators(&c.decision));
    out.push_str("\n\n");
    out.push_str(&core_render::verdict(&c.decision));
    out.push('\n');
    out
}

/// Count signals by polarity, for a header. Kept here rather than in `view` because it is
/// presentation arithmetic with no trust claim in it.
pub fn polarity_counts(w: &scema_world::WorldState) -> (usize, usize) {
    (
        w.signals.iter().filter(|s| s.polarity == Polarity::Risk).count(),
        w.signals.iter().filter(|s| s.polarity == Polarity::Opportunity).count(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use scema_agent::Agent;
    use scema_world::{
        Domain, Entity, EntityKind, Extent, Goal, Object, Polarity, Provenance, Signal, WorldState,
    };

    /// Draw one frame and return it as text, symbols only.
    ///
    /// Symbols rather than styles, deliberately. Everything this console claims has to
    /// survive `NO_COLOR`, so the *text* is what must be right; and a snapshot carrying
    /// escape sequences would churn on every palette tweak until nobody read it any more.
    fn frame(app: &App, w: u16, h: u16) -> String {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| draw(f, app)).unwrap();
        let buffer = term.backend().buffer();
        let area = buffer.area();
        let mut out = String::new();
        for y in 0..area.height {
            for x in 0..area.width {
                out.push_str(buffer.get(x, y).symbol());
            }
            out.push('\n');
        }
        out
    }

    fn scratch() -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!(
            "scema-tui-render-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    /// Always `Depth::Mono`, so every assertion below is an assertion about text.
    fn app_with(world: Option<WorldState>) -> (App, std::path::PathBuf) {
        let dir = scratch();
        let agent = Agent::new(&dir, None);
        let mut a = App::new(
            crate::theme::Theme::new(crate::theme::Depth::Mono),
            dir.clone(),
            ".".into(),
            &agent,
        );
        a.world = world;
        (a, dir)
    }

    fn world() -> WorldState {
        WorldState {
            observer: "test".into(),
            entity: Entity {
                kind: EntityKind::Repository,
                locator: "/repo".into(),
                label: "repo".into(),
            },
            domain: Domain::Software,
            observed_at: 1_700_000_000,
            objects: vec![
                Object::new("a", "crate", "alpha", Provenance::Live { age_secs: 3 }),
                Object::new(
                    "b",
                    "crate",
                    "beta",
                    Provenance::Stale { age_secs: 900, budget_secs: 60 },
                ),
                Object::new("c", "crate", "gamma", Provenance::Absent),
            ],
            facts: vec![],
            signals: vec![
                Signal {
                    id: "untested:alpha".into(),
                    polarity: Polarity::Risk,
                    label: "`alpha` has no tests".into(),
                    detail: String::new(),
                    magnitude: 0.42,
                    measured: true,
                    targets: vec!["a".into()],
                    evidence: vec!["counted 0 test attributes".into()],
                },
                Signal {
                    id: "guessed:beta".into(),
                    polarity: Polarity::Opportunity,
                    label: "beta might be worth extracting".into(),
                    detail: String::new(),
                    magnitude: 0.30,
                    measured: false,
                    targets: vec![],
                    evidence: vec![],
                },
            ],
            extent: Extent::partial(3, "walk capped at 3"),
            blind_spots: vec!["/repo/secret: permission denied".into()],
        }
    }

    #[test]
    fn every_tab_draws_without_panicking_at_several_sizes() {
        // A panic here is the whole reason this test exists. ratatui's layout arithmetic
        // underflows on small rectangles, and the production failure mode is the console
        // dying on somebody's 80-column terminal with a backtrace instead of a screen.
        for tab in Tab::ALL {
            for (w, h) in [(60u16, 16u16), (80, 24), (120, 40), (200, 60)] {
                let (mut a, dir) = app_with(Some(world()));
                a.tab = tab;
                let text = frame(&a, w, h);
                assert!(!text.is_empty());
                std::fs::remove_dir_all(&dir).ok();
            }
        }
    }

    #[test]
    fn an_unobserved_world_says_so_rather_than_drawing_an_empty_one() {
        // "nothing was observed" and "a world with nothing in it" are different claims, and
        // the console must not render them alike — the same tri-state discipline `/mesh`
        // enforces on its edges.
        let (a, dir) = app_with(None);
        let text = frame(&a, 100, 30);
        assert!(text.contains("Nothing has been observed yet"), "{text}");
        assert!(!text.contains("OBJECTS 0/0"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn provenance_is_spelled_out_in_the_object_list() {
        // With `Depth::Mono` there is nothing but the text carrying it, which is exactly
        // the condition the palette promises to survive.
        let (a, dir) = app_with(Some(world()));
        let text = frame(&a, 120, 32);
        assert!(text.contains("LIVE"), "{text}");
        assert!(text.contains("STALE"), "{text}");
        assert!(text.contains("ABSENT"), "{text}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_estimated_signal_is_tagged_as_one_on_screen() {
        let (mut a, dir) = app_with(Some(world()));
        a.focus = Focus::Right;
        let text = frame(&a, 120, 32);
        assert!(text.contains("EST?"), "an estimated magnitude must not read as counted:\n{text}");
        assert!(text.contains("RISK"), "{text}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_unbounded_extent_is_shouted_on_screen() {
        let (a, dir) = app_with(Some(world()));
        let text = frame(&a, 200, 32);
        assert!(text.contains("EXTENT UNBOUNDED"), "{text}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn blind_spots_are_on_the_first_screen_and_not_buried() {
        // The most useful thing an observer knows is what it could not read. Putting it
        // behind a keystroke would make the default screen a claim of completeness.
        let (a, dir) = app_with(Some(world()));
        let text = frame(&a, 120, 32);
        assert!(text.contains("could not be read"), "{text}");
        assert!(text.contains("permission denied"), "{text}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_unmeasured_term_reaches_the_screen_as_an_em_dash() {
        // The end-to-end version of the one rule: `Term` -> `render::cell` -> `view::cell`
        // -> a rectangle on a terminal. Every layer between has its own test; this asserts
        // the cell that a human actually looks at.
        //
        // The branch is ungrounded, so `StructuralSimulator` refuses to score an expected
        // gain for it. That column must print an em dash and never `0.00`.
        let (mut a, dir) = app_with(Some(world()));
        let mut agent = Agent::new(&dir, None);
        agent.persist = false;
        let cycle = agent
            .cycle_over(world(), Goal::new("g", "do something nothing supports"))
            .unwrap();

        let ungrounded = cycle
            .projections
            .iter()
            .any(|p| !p.expected_gain.measured);
        assert!(ungrounded, "the fixture must contain at least one unmeasured gain");

        a.tab = Tab::Simulate;
        a.goal = "do something nothing supports".into();
        a.cycle = Some(cycle);
        a.cycle_persisted = false;

        let text = frame(&a, 130, 44);
        assert!(
            text.contains('\u{2014}'),
            "an unmeasured term must reach the screen as an em dash:\n{text}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_simulation_never_claims_to_have_written_a_record() {
        // The one sentence on this screen that is a claim about the filesystem, and the
        // reason `simulate` and `decide` are different keys.
        let (mut a, dir) = app_with(Some(world()));
        let mut agent = Agent::new(&dir, None);
        agent.persist = false;
        a.cycle = Some(agent.cycle_over(world(), Goal::new("g", "tidy up")).unwrap());
        a.cycle_persisted = false;
        a.tab = Tab::Simulate;

        let text = frame(&a, 130, 44);
        assert!(!text.contains("SEALED AS"), "{text}");
        assert!(text.contains("NOT WRITTEN"), "{text}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_coverage_meter_is_beside_every_utility_it_qualifies() {
        // `measured_fraction` is never separated from the score it qualifies — the same
        // rule `/mesh` enforces on Psi. A utility standing on two terms out of five and one
        // standing on five out of five would otherwise print identically.
        let (mut a, dir) = app_with(Some(world()));
        let mut agent = Agent::new(&dir, None);
        agent.persist = false;
        a.cycle = Some(agent.cycle_over(world(), Goal::new("g", "tidy up")).unwrap());
        a.tab = Tab::Simulate;

        let text = frame(&a, 140, 44);
        assert!(text.contains('\u{25B0}') || text.contains('\u{25B1}'), "{text}");
        assert!(text.contains("measured across the whole matrix"), "{text}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_record_pane_states_what_a_verified_commitment_does_not_prove() {
        // Not only in a comment. A verifier that teaches its reader to over-trust it is
        // worse than no verifier.
        let (mut a, dir) = app_with(Some(world()));
        a.tab = Tab::Records;
        let text = frame(&a, 140, 40);
        assert!(text.contains("does NOT prove") || text.contains("NOT prove"), "{text}");
        assert!(text.contains("Tamper-evident") || text.contains("tamper-evident"), "{text}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_stamp_of_zero_is_an_em_dash_rather_than_the_epoch() {
        // `1970-01-01` is a real-looking date and it is what an unset timestamp renders as
        // if nobody thinks about it. It would read as a decision made 56 years ago.
        assert_eq!(stamp(0), "\u{2014}");
        assert_eq!(stamp(-1), "\u{2014}");
        assert_eq!(stamp(1_700_000_000), "2023-11-14 22:13Z");
    }
}
