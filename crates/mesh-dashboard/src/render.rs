//! Drawing: the graph onto a [`Canvas`], and the whole screen onto a ratatui frame.
//!
//! The one place a [`Tone`] becomes a colour is [`colour_of`]. Everything upstream names a
//! role. The hex values match `TONE_HEX` in `web/lib/mesh/view.ts` so the terminal and the
//! browser make the same claim in the same colour.

use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};
use ratatui::Frame;

use scematica_mesh::{Cognition, EdgeKind, GateVerdict, Mesh, Node, Term};

use crate::canvas::{Canvas, Cell, Ink, Stroke};
use crate::view::{self, GraphLayout, PlacedEdge, Route, Tone, Trace, NODE_H, NODE_W};

/// The only tone → colour mapping in the crate.
pub fn colour_of(tone: Tone) -> Color {
    match tone {
        Tone::Live => Color::Rgb(74, 222, 155),
        Tone::Stale => Color::Rgb(245, 181, 68),
        Tone::Absent => Color::Rgb(90, 100, 140),
        Tone::Veto => Color::Rgb(255, 93, 125),
        Tone::Simulated => Color::Rgb(124, 156, 255),
    }
}

const ACCENT: Color = Color::Rgb(129, 140, 248);
const TEXT: Color = Color::Rgb(203, 213, 245);
const MUTED: Color = Color::Rgb(140, 150, 190);
const DIM: Color = Color::Rgb(85, 92, 125);

// ── the graph ────────────────────────────────────────────────────────────────

/// How an edge should be stroked, given what is actually known about it.
///
/// Three states that must stay distinguishable without colour (see the stroke-set note in
/// `canvas.rs`), because the difference between "this veto is clear" and "nobody could
/// read this veto" is the difference between a safe system and an unexamined one.
fn edge_stroke(e: &PlacedEdge) -> (Stroke, Tone, bool) {
    if e.kind == EdgeKind::Veto {
        // Through `edge_blocking`, so the tri-state rule has one definition rather than a
        // copy here that could drift into drawing an unreadable veto as a cleared one.
        return match view::edge_blocking(e.kind, e.active) {
            Some(true) => (Stroke::Double, Tone::Veto, false),
            Some(false) => (Stroke::Single, Tone::Absent, true),
            None => (Stroke::Dashed, Tone::Absent, true),
        };
    }
    match e.active {
        Some(true) => (Stroke::Single, Tone::Live, false),
        Some(false) => (Stroke::Single, Tone::Absent, true),
        None => (Stroke::Dashed, Tone::Absent, true),
    }
}

/// How an edge's state reads in words, for the detail panel.
///
/// Same three states as the stroke families, spelled out — "cleared" and "unreadable" are
/// different claims and the panel is where an operator goes to be sure which one they are
/// looking at.
fn edge_state_word(e: &PlacedEdge) -> (&'static str, Tone) {
    match view::edge_blocking(e.kind, e.active) {
        Some(true) => ("BLOCKING", Tone::Veto),
        Some(false) if e.kind == EdgeKind::Veto => ("clear", Tone::Live),
        _ => match e.active {
            Some(true) => ("active", Tone::Live),
            Some(false) => ("inactive", Tone::Absent),
            None => ("unreadable", Tone::Absent),
        },
    }
}

/// Paint the whole graph. Edges first, then boxes on top — a box is opaque.
pub fn paint_graph(
    mesh: &Mesh,
    g: &GraphLayout,
    selected: Option<&str>,
    traced: Option<&Trace>,
) -> Canvas {
    let mut c = Canvas::new(g.width.max(1), g.height.max(1));

    // Column headings. `col.title` was resolved from the Rust layer table during layout;
    // re-deriving it here would be a second lookup able to disagree with the first.
    for col in &g.columns {
        c.text(col.x, 0, &col.title.to_uppercase(), Ink::plain().dim(true).bold(true));
    }

    let mut arrows: Vec<(u16, u16, char, Tone, bool)> = Vec::new();

    for e in &g.edges {
        let (stroke, tone, mut dim) = edge_stroke(e);
        // Tracing dims everything with no bearing on the selection. The traced set stays
        // at full strength so the path reads as a path.
        if let Some(t) = traced {
            if !t.edges.contains(&e.key) {
                dim = true;
            }
        }

        match e.route {
            Route::Direct { channel } => {
                c.hline(e.x1, channel, e.y1, stroke, tone, dim);
                c.vline(e.y1, e.y2, channel, stroke, tone, dim);
                c.hline(channel, e.x2, e.y2, stroke, tone, dim);
            }
            Route::Lane { down_x, up_x, lane_y } => {
                c.hline(e.x1, down_x, e.y1, stroke, tone, dim);
                c.vline(e.y1, lane_y, down_x, stroke, tone, dim);
                c.hline(down_x, up_x, lane_y, stroke, tone, dim);
                c.vline(lane_y, e.y2, up_x, stroke, tone, dim);
                c.hline(up_x, e.x2, e.y2, stroke, tone, dim);
            }
        }
        arrows.push((e.x2, e.y2, '▶', tone, dim));
    }

    c.resolve_lines();

    // Arrowheads after resolution: they replace a line glyph rather than joining it.
    for (x, y, ch, tone, dim) in arrows {
        c.set(x, y, Cell { ch, tone: Some(tone), dim, bold: false });
    }

    for p in &g.placed {
        let Some(node) = mesh.nodes.iter().find(|n| n.id == p.id) else { continue };
        let tone = view::tone_for(node);
        let is_selected = selected == Some(p.id.as_str());
        let dim = traced.is_some_and(|t| !t.nodes.contains(&p.id));
        paint_node(&mut c, p.x, p.y, node, tone, is_selected, dim);
    }

    c
}

fn paint_node(
    c: &mut Canvas,
    x: u16,
    y: u16,
    node: &Node,
    tone: Tone,
    selected: bool,
    dim: bool,
) {
    c.box_outline(x, y, NODE_W, NODE_H, Ink::tone(tone).dim(dim).bold(selected));
    let inner = (NODE_W - 2) as usize;
    let ix = x + 1;

    // Row 1 — the name.
    let mut label = truncate(&node.label, inner);
    if selected {
        // A marker as well as a bold border: selection must be visible in a screenshot
        // and through NO_COLOR, not only as a weight change.
        label = truncate(&format!("▸{}", node.label), inner);
    }
    c.text(ix, y + 1, &label, Ink::tone(tone).dim(dim).bold(true));

    // Row 2 — can this be believed, and what did it decide. Both words, always: a reader
    // must not have to infer provenance from a colour.
    let prov = view::provenance_word(&node.provenance);
    let age = view::age_label(&node.provenance).unwrap_or_default();
    let left = if age.is_empty() { prov.to_string() } else { format!("{prov} {age}") };
    let right = view::verdict_word(node.verdict);
    let pad = inner.saturating_sub(left.chars().count() + right.chars().count());
    c.text(ix, y + 2, &format!("{left}{}{right}", " ".repeat(pad)), Ink::tone(tone).dim(dim));

    // Row 3 — activity, and NOTHING AT ALL when it is not measurable. An empty bar would
    // read as "measured, and it is zero", which is a different and much stronger claim.
    if let Some(a) = node.activity {
        let width = 10usize;
        let filled = ((a.clamp(0.0, 1.0)) * width as f64).round() as usize;
        let bar = format!(
            "{}{} {:>3.0}%",
            "█".repeat(filled),
            "·".repeat(width - filled),
            a.clamp(0.0, 1.0) * 100.0
        );
        c.text(ix, y + 3, &truncate(&bar, inner), Ink::tone(tone).dim(dim));
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    if max <= 1 {
        return "…".to_string();
    }
    let mut out: String = s.chars().take(max - 1).collect();
    out.push('…');
    out
}

/// Blits a viewport of a [`Canvas`] into a ratatui buffer.
pub struct CanvasView<'a> {
    pub canvas: &'a Canvas,
    pub scroll_x: u16,
    pub scroll_y: u16,
}

impl Widget for CanvasView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        for row in 0..area.height {
            let sy = row + self.scroll_y;
            if sy >= self.canvas.height {
                break;
            }
            for col in 0..area.width {
                let sx = col + self.scroll_x;
                if sx >= self.canvas.width {
                    break;
                }
                let cell = self.canvas.get(sx, sy);
                if cell.ch == ' ' {
                    continue;
                }
                let mut style = Style::default().fg(cell.tone.map(colour_of).unwrap_or(TEXT));
                if cell.dim {
                    style = style.add_modifier(Modifier::DIM);
                }
                if cell.bold {
                    style = style.add_modifier(Modifier::BOLD);
                }
                let target = buf.get_mut(area.x + col, area.y + row);
                target.set_char(cell.ch);
                target.set_style(style);
            }
        }
    }
}

// ── the screen ───────────────────────────────────────────────────────────────

pub struct Screen<'a> {
    pub mesh: &'a Mesh,
    pub layout: &'a GraphLayout,
    pub canvas: &'a Canvas,
    pub root: &'a str,
    pub selected: Option<&'a str>,
    pub tracing: bool,
    pub show_terms: bool,
    pub scroll_x: u16,
    pub scroll_y: u16,
    pub interval_secs: u64,
    pub last_error: Option<&'a str>,
}

pub fn draw(f: &mut Frame, s: &Screen) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),  // header
            Constraint::Length(3),  // diagnosis
            Constraint::Min(8),     // graph
            Constraint::Length(11), // gate | detail
            Constraint::Length(1),  // footer
        ])
        .split(f.size());

    draw_header(f, chunks[0], s);
    draw_diagnosis(f, chunks[1], s.mesh);
    draw_graph(f, chunks[2], s);

    let bottom = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[3]);
    draw_gate(f, bottom[0], &s.mesh.cognition, s.show_terms);
    draw_detail(f, bottom[1], s);

    draw_footer(f, chunks[4], s);
}

fn draw_header(f: &mut Frame, area: Rect, s: &Screen) {
    let title = Line::from(vec![
        Span::styled(
            "SCEMATICA MESH",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled("  the running system's own topology", Style::default().fg(DIM)),
    ]);
    let sub = Line::from(vec![
        Span::styled(view::visibility_label(s.mesh), Style::default().fg(TEXT)),
        Span::styled(format!("  ·  {}", s.root), Style::default().fg(DIM)),
        Span::styled(
            format!("  ·  {}", s.mesh.generated_at.chars().take(19).collect::<String>()),
            Style::default().fg(DIM),
        ),
    ]);
    f.render_widget(Paragraph::new(vec![title, sub]), area);
}

fn draw_diagnosis(f: &mut Frame, area: Rect, mesh: &Mesh) {
    let s = &mesh.summary;
    let alarming = s.blocking > 0;
    let colour = if alarming { colour_of(Tone::Veto) } else { TEXT };
    let heading = if alarming { "BLOCKING" } else { "DIAGNOSIS" };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if alarming { colour_of(Tone::Veto) } else { DIM }))
        .title(Span::styled(
            format!(" {heading} "),
            Style::default().fg(colour).add_modifier(Modifier::BOLD),
        ));

    f.render_widget(
        Paragraph::new(Line::from(Span::styled(&*s.diagnosis, Style::default().fg(colour))))
            .block(block)
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn draw_graph(f: &mut Frame, area: Rect, s: &Screen) {
    let hint = if s.tracing {
        " TOPOLOGY · tracing "
    } else {
        " TOPOLOGY "
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM))
        .title(Span::styled(hint, Style::default().fg(ACCENT)));
    let inner = block.inner(area);
    f.render_widget(block, area);
    f.render_widget(
        CanvasView { canvas: s.canvas, scroll_x: s.scroll_x, scroll_y: s.scroll_y },
        inner,
    );
}

fn verdict_colour(v: GateVerdict) -> Color {
    match v {
        GateVerdict::Act => colour_of(Tone::Live),
        GateVerdict::Damp => colour_of(Tone::Stale),
        GateVerdict::Abstain => colour_of(Tone::Veto),
        GateVerdict::Unevaluated => DIM,
    }
}

fn draw_gate(f: &mut Frame, area: Rect, c: &Cognition, show_terms: bool) {
    let vc = verdict_colour(c.verdict);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM))
        .title(Span::styled(" AGENTIC COHERENCE GATE §32 ", Style::default().fg(ACCENT)));

    let mut lines = vec![
        Line::from(vec![
            Span::styled("Ψ = C · K · (1 − R)   ", Style::default().fg(MUTED)),
            Span::styled(
                format!("{:.3}", c.psi),
                Style::default().fg(vc).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("  {:?}", c.verdict).to_uppercase(), Style::default().fg(vc)),
        ]),
        // Never on its own line away from Ψ. A gate computed on a quarter of its terms is
        // a statement about ignorance and has to look like one.
        Line::from(Span::styled(
            format!("computed on {:.0}% of its terms", c.measured_fraction * 100.0),
            Style::default().fg(DIM),
        )),
        Line::from(vec![
            factor("C confidence", Some(c.confidence), false),
            Span::raw("  "),
            factor("K coherence", Some(c.coherence.value), false),
        ]),
        Line::from(vec![
            factor("R risk", Some(c.risk.value), true),
            Span::raw("  "),
            factor("Ω state", c.omega, false),
        ]),
        Line::from(Span::styled(&*c.reading, Style::default().fg(MUTED))),
    ];

    if show_terms {
        lines.push(Line::from(Span::styled("─ terms ─", Style::default().fg(DIM))));
        for t in c
            .confidence_terms
            .iter()
            .chain(c.risk.components.iter())
            .chain(c.omega_terms.iter())
        {
            lines.push(term_line(t));
        }
    }

    f.render_widget(
        Paragraph::new(lines).block(block).wrap(Wrap { trim: true }),
        area,
    );
}

fn factor(label: &str, value: Option<f64>, invert: bool) -> Span<'static> {
    match value {
        None => Span::styled(format!("{label} —"), Style::default().fg(DIM)),
        Some(v) => {
            let good = if invert { v < 0.25 } else { v > 0.75 };
            let colour = if good { colour_of(Tone::Live) } else { TEXT };
            Span::styled(format!("{label} {v:.3}"), Style::default().fg(colour))
        }
    }
}

/// A term row, leading with whether it was measured at all.
///
/// `measured: false` means the term contributed its NEUTRAL element — not a guess and not
/// a zero-with-confidence. Hiding that flag turns the gate into a number with no evidence
/// behind it, which is the failure mode the whole module is built around.
fn term_line(t: &Term) -> Line<'static> {
    let (word, colour) = if t.measured {
        ("measured  ", colour_of(Tone::Live))
    } else {
        ("unmeasured", colour_of(Tone::Absent))
    };
    Line::from(vec![
        Span::styled(word, Style::default().fg(colour)),
        Span::styled(format!(" {:<8}", t.symbol), Style::default().fg(ACCENT)),
        Span::styled(format!("{:>6.3} ", t.value), Style::default().fg(TEXT)),
        Span::styled(t.note.clone(), Style::default().fg(MUTED)),
    ])
}

fn draw_detail(f: &mut Frame, area: Rect, s: &Screen) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM));

    let Some(id) = s.selected else {
        let block = block.title(Span::styled(" NO SELECTION ", Style::default().fg(ACCENT)));
        let help = vec![
            Line::from(Span::styled(
                "Pick a unit to see what it last decided.",
                Style::default().fg(MUTED),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "↑↓←→ / hjkl  move between units",
                Style::default().fg(DIM),
            )),
            Line::from(Span::styled("t  trace what reaches it and what it reaches", Style::default().fg(DIM))),
            Line::from(Span::styled("g  expand the gate's terms", Style::default().fg(DIM))),
            Line::from(""),
            Line::from(Span::styled(
                "A dark node is unseen, not idle — there is no source",
                Style::default().fg(DIM),
            )),
            Line::from(Span::styled("on disk for it, so there is nothing to report.", Style::default().fg(DIM))),
        ];
        f.render_widget(Paragraph::new(help).block(block).wrap(Wrap { trim: true }), area);
        return;
    };

    let Some(node) = s.mesh.nodes.iter().find(|n| n.id == id) else {
        f.render_widget(block, area);
        return;
    };

    let tone = view::tone_for(node);
    let colour = colour_of(tone);
    let block = block.title(Span::styled(
        format!(" {} ", node.label.to_uppercase()),
        Style::default().fg(colour).add_modifier(Modifier::BOLD),
    ));

    let age = view::age_label(&node.provenance)
        .map(|a| format!(" · {a} old"))
        .unwrap_or_default();

    let mut lines = vec![
        Line::from(Span::styled(&*node.blurb, Style::default().fg(MUTED))),
        Line::from(vec![
            Span::styled(
                format!("{}{}", view::provenance_word(&node.provenance), age),
                Style::default().fg(colour),
            ),
            Span::styled(
                format!("   verdict {}", view::verdict_word(node.verdict)),
                Style::default().fg(TEXT),
            ),
            Span::styled(format!("   {}", node.id), Style::default().fg(DIM)),
        ]),
    ];

    if let Some(r) = &node.reason {
        lines.push(Line::from(Span::styled(r.clone(), Style::default().fg(colour))));
    }

    if node.detail.is_empty() {
        lines.push(Line::from(Span::styled(
            "No values — this unit has no source on disk, so there is nothing to report \
             rather than nothing happening.",
            Style::default().fg(DIM),
        )));
    } else {
        for (k, v) in &node.detail {
            lines.push(Line::from(vec![
                Span::styled(format!("{k:<18}"), Style::default().fg(DIM)),
                Span::styled(v.clone(), Style::default().fg(TEXT)),
            ]));
        }
    }

    // Wiring, in words. The graph shows that an edge exists; this says what it carries
    // ("171/474 passed") and whether it is doing anything — which is where the edge
    // labels live, since a 7-column gutter has no room to print them.
    let wiring: Vec<Line> = s
        .layout
        .edges
        .iter()
        .filter(|e| e.from == id || e.to == id)
        .map(|e| {
            let outgoing = e.from == id;
            let other = if outgoing { &e.to } else { &e.from };
            let (word, tone) = edge_state_word(e);
            let mut spans = vec![
                Span::styled(
                    format!("{} {:<22}", if outgoing { "→" } else { "←" }, other),
                    Style::default().fg(MUTED),
                ),
                Span::styled(format!("{:<11}", format!("{:?}", e.kind).to_lowercase()), Style::default().fg(DIM)),
                Span::styled(format!("{word:<11}"), Style::default().fg(colour_of(tone))),
            ];
            if let Some(l) = &e.label {
                spans.push(Span::styled(l.clone(), Style::default().fg(TEXT)));
            }
            Line::from(spans)
        })
        .collect();

    if !wiring.is_empty() {
        lines.push(Line::from(Span::styled("─ wiring ─", Style::default().fg(DIM))));
        lines.extend(wiring);
    }

    f.render_widget(Paragraph::new(lines).block(block).wrap(Wrap { trim: true }), area);
}

fn draw_footer(f: &mut Frame, area: Rect, s: &Screen) {
    if let Some(err) = s.last_error {
        f.render_widget(
            Paragraph::new(Span::styled(
                format!(" collect failed: {err} — showing the last good reading "),
                Style::default().fg(colour_of(Tone::Veto)),
            )),
            area,
        );
        return;
    }

    let legend = vec![
        Span::styled("live", Style::default().fg(colour_of(Tone::Live))),
        Span::raw(" "),
        Span::styled("stale", Style::default().fg(colour_of(Tone::Stale))),
        Span::raw(" "),
        Span::styled("unseen", Style::default().fg(colour_of(Tone::Absent))),
        Span::raw(" "),
        Span::styled("═veto", Style::default().fg(colour_of(Tone::Veto))),
        Span::styled("  ╌unreadable", Style::default().fg(colour_of(Tone::Absent))),
        Span::styled(
            format!(
                "   │ q quit · r refresh ({}s) · t trace · g terms · c clear · ←→↑↓ move · </> scroll",
                s.interval_secs
            ),
            Style::default().fg(DIM),
        ),
    ];
    f.render_widget(Paragraph::new(Line::from(legend)).alignment(Alignment::Left), area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use scematica_mesh::{Edge, Node, NodeKind, Provenance, Verdict};

    fn node(id: &str, kind: NodeKind, p: Provenance, v: Verdict, activity: Option<f64>) -> Node {
        Node {
            id: id.into(),
            kind,
            label: id.into(),
            blurb: String::new(),
            provenance: p,
            verdict: v,
            activity,
            detail: vec![],
            reason: None,
        }
    }

    const LIVE: Provenance = Provenance::Live { age_secs: 2 };

    /// The three edge states must differ in *glyph*, not only in colour — a monochrome
    /// terminal has to be able to tell a cleared veto from an unreadable one.
    #[test]
    fn the_three_edge_states_use_three_stroke_families() {
        let mk = |active| PlacedEdge {
            key: "k".into(),
            from: "a".into(),
            to: "b".into(),
            kind: EdgeKind::Veto,
            active,
            label: None,
            route: Route::Direct { channel: 1 },
            x1: 0,
            y1: 0,
            x2: 2,
            y2: 0,
        };
        assert_eq!(edge_stroke(&mk(Some(true))).0, Stroke::Double);
        assert_eq!(edge_stroke(&mk(Some(false))).0, Stroke::Single);
        assert_eq!(edge_stroke(&mk(None)).0, Stroke::Dashed);
        // …and only the live block is allowed to look alarming.
        assert_eq!(edge_stroke(&mk(Some(true))).1, Tone::Veto);
        assert_eq!(edge_stroke(&mk(Some(false))).1, Tone::Absent);
    }

    /// `activity: None` renders NOTHING. An empty bar would claim the value was measured
    /// and found to be zero, which is a much stronger statement than "not measurable".
    #[test]
    fn unmeasurable_activity_draws_no_bar() {
        let mesh = Mesh::new(
            vec![
                node("a", NodeKind::Listener, LIVE, Verdict::Pass, None),
                node("b", NodeKind::Filter, LIVE, Verdict::Pass, Some(0.0)),
            ],
            vec![],
            "t".into(),
        );
        let g = view::layout(&mesh);
        let c = paint_graph(&mesh, &g, None, None);

        let a = g.find("a").unwrap();
        let bar_row: String = (a.x..a.x + NODE_W).map(|x| c.get(x, a.y + 3).ch).collect();
        assert!(
            !bar_row.contains('█') && !bar_row.contains('·'),
            "an unmeasurable node drew a bar: {bar_row:?}"
        );

        // A measured zero, by contrast, is a real reading and must be drawn.
        let b = g.find("b").unwrap();
        let zero_row: String = (b.x..b.x + NODE_W).map(|x| c.get(x, b.y + 3).ch).collect();
        assert!(zero_row.contains('·'), "a measured zero must render: {zero_row:?}");
    }

    /// Provenance and verdict are always spelled out inside the box. A reader must never
    /// have to infer "can I trust this" from a colour alone.
    #[test]
    fn every_node_states_its_provenance_in_words() {
        let mesh = Mesh::new(
            vec![node(
                "dq",
                NodeKind::Learner,
                Provenance::Stale { age_secs: 432_000, budget_secs: 120 },
                Verdict::Veto,
                None,
            )],
            vec![],
            "t".into(),
        );
        let g = view::layout(&mesh);
        let c = paint_graph(&mesh, &g, None, None);
        let p = g.find("dq").unwrap();
        let row: String = (p.x..p.x + NODE_W).map(|x| c.get(x, p.y + 2).ch).collect();
        assert!(row.contains("STALE"), "{row:?}");
        assert!(row.contains("5d"), "{row:?}");
        assert!(row.contains("VETO"), "{row:?}");
    }

    #[test]
    fn a_selected_node_is_marked_in_text_not_only_in_style() {
        let mesh = Mesh::new(
            vec![node("a", NodeKind::Listener, LIVE, Verdict::Pass, None)],
            vec![],
            "t".into(),
        );
        let g = view::layout(&mesh);
        let c = paint_graph(&mesh, &g, Some("a"), None);
        let p = g.find("a").unwrap();
        let row: String = (p.x..p.x + NODE_W).map(|x| c.get(x, p.y + 1).ch).collect();
        assert!(row.contains('▸'), "{row:?}");
    }

    #[test]
    fn an_edge_ends_in_an_arrowhead_at_its_target() {
        let mesh = Mesh::new(
            vec![
                node("a", NodeKind::Listener, LIVE, Verdict::Pass, None),
                node("b", NodeKind::Filter, LIVE, Verdict::Pass, None),
            ],
            vec![Edge::signal("a", "b").with_active(Some(true))],
            "t".into(),
        );
        let g = view::layout(&mesh);
        let c = paint_graph(&mesh, &g, None, None);
        let e = &g.edges[0];
        assert_eq!(c.get(e.x2, e.y2).ch, '▶');
    }

    #[test]
    fn column_headings_sit_above_their_columns() {
        let mesh = Mesh::new(
            vec![
                node("a", NodeKind::Listener, LIVE, Verdict::Pass, None),
                node("x", NodeKind::Executor, LIVE, Verdict::Idle, None),
            ],
            vec![],
            "t".into(),
        );
        let g = view::layout(&mesh);
        let c = paint_graph(&mesh, &g, None, None);
        let head = c.row(0);
        assert!(head.contains("INGEST"), "{head:?}");
        assert!(head.contains("EXECUTION"), "{head:?}");
    }

    #[test]
    fn painting_an_empty_mesh_does_not_panic() {
        let mesh = Mesh::new(vec![], vec![], "t".into());
        let g = view::layout(&mesh);
        let _ = paint_graph(&mesh, &g, None, None);
    }

    #[test]
    fn truncation_keeps_the_box_width() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("a-very-long-node-label", 8).chars().count(), 8);
    }

    /// Drive the whole frame through ratatui's `TestBackend`.
    ///
    /// The pure tests above cover what is *drawn*; this covers whether drawing it
    /// completes at all. A panic inside `draw` is the one failure that leaves a real
    /// terminal in raw mode on the alternate screen, and the sizes below are where it
    /// would happen: a pane shorter than its own constraints, and a width narrower than a
    /// single node box.
    #[test]
    fn the_frame_renders_at_hostile_terminal_sizes() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mesh = Mesh::new(
            vec![
                node("l", NodeKind::Listener, LIVE, Verdict::Pass, Some(0.5)),
                node("f", NodeKind::Filter, LIVE, Verdict::Veto, None),
                node("x", NodeKind::Executor, LIVE, Verdict::Idle, None),
            ],
            vec![Edge::veto("f", "x").with_active(Some(true)), Edge::signal("l", "f")],
            "2026-08-16T00:00:00Z".into(),
        );
        let g = view::layout(&mesh);

        // 8x4 is smaller than the chrome alone; 200x60 is a maximised terminal.
        for (w, h) in [(8, 4), (20, 10), (40, 18), (120, 40), (200, 60)] {
            let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
            for (selected, tracing, terms) in
                [(None, false, false), (Some("x"), true, true), (Some("f"), false, true)]
            {
                let traced = tracing.then(|| view::trace(&mesh, selected.unwrap()));
                let canvas = paint_graph(&mesh, &g, selected, traced.as_ref());
                term.draw(|f| {
                    draw(
                        f,
                        &Screen {
                            mesh: &mesh,
                            layout: &g,
                            canvas: &canvas,
                            root: ".",
                            selected,
                            tracing,
                            show_terms: terms,
                            scroll_x: 0,
                            scroll_y: 0,
                            interval_secs: 4,
                            last_error: None,
                        },
                    )
                })
                .unwrap_or_else(|e| panic!("draw failed at {w}x{h}: {e}"));
            }
        }
    }

    /// A scroll offset past the end of the canvas must clip, not index out of bounds.
    #[test]
    fn scrolling_past_the_canvas_renders_empty_rather_than_panicking() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mesh = Mesh::new(
            vec![node("a", NodeKind::Listener, LIVE, Verdict::Pass, None)],
            vec![],
            "t".into(),
        );
        let g = view::layout(&mesh);
        let canvas = paint_graph(&mesh, &g, None, None);
        let mut term = Terminal::new(TestBackend::new(60, 30)).unwrap();
        term.draw(|f| {
            draw(
                f,
                &Screen {
                    mesh: &mesh,
                    layout: &g,
                    canvas: &canvas,
                    root: ".",
                    selected: None,
                    tracing: false,
                    show_terms: false,
                    scroll_x: 9_000,
                    scroll_y: 9_000,
                    interval_secs: 4,
                    last_error: Some("a structural problem"),
                },
            )
        })
        .unwrap();
    }
}
