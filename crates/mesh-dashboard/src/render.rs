//! Drawing: the graph onto a [`Canvas`], the vortex onto a [`DepthGrid`], and the whole
//! screen onto a ratatui frame.
//!
//! The one place a [`Tone`] becomes a colour is [`colour_of`]. Everything upstream names a
//! role. The hex values match `TONE_HEX` in `web/lib/mesh/view.ts` so the terminal and the
//! browser make the same claim in the same colour.
//!
//! ## The overhaul
//!
//! This dashboard now behaves like an observatory, not a single static picture. It has
//! four views — `graph`, `tornado`, `coherence`, `roster` — switchable with the number
//! keys or the tab strip. The tornado is a 3D, depth-buffered, truecolor vortex of the
//! *real* mesh, ported from the hatcher-terminal observatory; the coherence view expands
//! the §32 gate into its terms with a truecolor Ψ meter; the roster is a fully interactive
//! list of every unit with its tone, verdict and activity. All of it is driven by the same
//! `Mesh`, and all of it obeys the same honesty rules (absent nodes never enter the lit
//! vortex; a stale node is dimmed, never green; an unreadable veto is not drawn as open).

use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};
use ratatui::Frame;

use scematica_mesh::{Cognition, EdgeKind, GateVerdict, Mesh, Node, Term};

use crate::canvas::{Canvas, Cell, Ink, Stroke};
use crate::view::{self, GraphLayout, PlacedEdge, Route, Tone, Trace, NODE_H, NODE_W};
use crate::vortex::{Camera, DepthGrid, Vortex, FRAME, heat};

/// Which view the dashboard is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// The layered topology graph (the original view).
    Graph,
    /// The active-node tornado — a 3D vortex of the live mesh.
    Tornado,
    /// The §32 agentic coherence gate, expanded into its terms.
    Coherence,
    /// A scrollable roster of every unit with its tone and activity.
    Roster,
}

impl View {
    pub const ALL: [View; 4] = [View::Graph, View::Tornado, View::Coherence, View::Roster];

    pub fn parse(value: &str) -> Option<View> {
        match value.trim().to_ascii_lowercase().as_str() {
            "graph" | "topology" => Some(View::Graph),
            "tornado" | "vortex" => Some(View::Tornado),
            "coherence" | "gate" | "psi" => Some(View::Coherence),
            "roster" | "list" | "nodes" => Some(View::Roster),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            View::Graph => "graph",
            View::Tornado => "tornado",
            View::Coherence => "coherence",
            View::Roster => "roster",
        }
    }

    /// The question this view answers.
    pub fn question(self) -> &'static str {
        match self {
            View::Graph => "who is connected to whom, and what are they deciding?",
            View::Tornado => "is the mesh live, and who is carrying it?",
            View::Coherence => "do the subsystems agree, and is acting safe?",
            View::Roster => "every unit, what it last decided, how active it is",
        }
    }
}

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

const ACCENT_C: Color = Color::Rgb(129, 140, 248);
const TEXT_C: Color = Color::Rgb(203, 213, 245);
const MUTED_C: Color = Color::Rgb(140, 150, 190);
const DIM_C: Color = Color::Rgb(85, 92, 125);

// ── the graph ────────────────────────────────────────────────────────────────

/// How an edge should be stroked, given what is actually known about it.
fn edge_stroke(e: &PlacedEdge) -> (Stroke, Tone, bool) {
    if e.kind == EdgeKind::Veto {
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

    for col in &g.columns {
        c.text(col.x, 0, &col.title.to_uppercase(), Ink::plain().dim(true).bold(true));
    }

    let mut arrows: Vec<(u16, u16, char, Tone, bool)> = Vec::new();

    for e in &g.edges {
        let (stroke, tone, mut dim) = edge_stroke(e);
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

    let mut label = truncate(&node.label, inner);
    if selected {
        label = truncate(&format!("▸{label}"), inner);
    }
    c.text(ix, y + 1, &label, Ink::tone(tone).dim(dim).bold(true));

    let prov = view::provenance_word(&node.provenance);
    let age = view::age_label(&node.provenance).unwrap_or_default();
    let left = if age.is_empty() { prov.to_string() } else { format!("{prov} {age}") };
    let right = view::verdict_word(node.verdict);
    let pad = inner.saturating_sub(left.chars().count() + right.chars().count());
    c.text(ix, y + 2, &format!("{left}{}{right}", " ".repeat(pad)), Ink::tone(tone).dim(dim));

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
                let mut style = Style::default().fg(cell.tone.map(colour_of).unwrap_or(TEXT_C));
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

/// Blits a [`DepthGrid`] (the vortex) into a ratatui buffer as truecolor.
pub struct VortexView<'a> {
    pub mesh: &'a Mesh,
    pub camera: &'a Camera,
    pub time: f64,
}

impl Widget for VortexView<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut grid = DepthGrid::new(area.width as usize, area.height as usize);
        grid.frame(0, 0, area.width as usize, area.height as usize, "ACTIVE NODE TORNADO", FRAME);
        let inner = (1, 1, (area.width as usize).saturating_sub(2), (area.height as usize).saturating_sub(2));
        let clip = grid.set_clip(inner.0, inner.1, inner.2, inner.3);
        let vortex = Vortex::from_mesh(self.mesh);
        vortex.draw_fitted(
            &mut grid,
            self.camera,
            (area.width as f64) / 2.0,
            (area.height as f64) / 2.0,
            inner.2,
            inner.3,
            self.time,
        );
        grid.restore_clip(clip);

        for row in 0..area.height as usize {
            for col in 0..area.width as usize {
                let ch = grid.cells_char(col, row);
                if ch == ' ' {
                    continue;
                }
                let fg = grid.fg_at(col, row);
                let target = buf.get_mut(area.x + col as u16, area.y + row as u16);
                target.set_char(ch);
                target.set_style(Style::default().fg(fg.to_ratatui()));
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
    pub view: View,
    pub time: f64,
    pub camera: &'a Camera,
}

pub fn draw(f: &mut Frame, s: &Screen) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Length(3), Constraint::Min(8), Constraint::Length(1)])
        .split(f.size());

    draw_header(f, chunks[0], s);
    draw_diagnosis(f, chunks[1], s.mesh);

    match s.view {
        View::Graph => {
            let body = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(8), Constraint::Length(11)])
                .split(chunks[2]);
            draw_graph(f, body[0], s);
            let bottom = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(body[1]);
            draw_gate(f, bottom[0], &s.mesh.cognition, s.show_terms);
            draw_detail(f, bottom[1], s);
        }
        View::Tornado => {
            f.render_widget(VortexView { mesh: s.mesh, camera: s.camera, time: s.time }, chunks[2]);
        }
        View::Coherence => {
            draw_coherence(f, chunks[2], &s.mesh.cognition);
        }
        View::Roster => {
            let split = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
                .split(chunks[2]);
            draw_roster(f, split[0], s);
            draw_detail(f, split[1], s);
        }
    }

    draw_footer(f, chunks[3], s);
}

fn draw_header(f: &mut Frame, area: Rect, s: &Screen) {
    let title = Line::from(vec![
        Span::styled("SCEMATICA MESH", Style::default().fg(ACCENT_C).add_modifier(Modifier::BOLD)),
        Span::styled("  the running system's own topology", Style::default().fg(DIM_C)),
    ]);
    let sub = Line::from(vec![
        Span::styled(view::visibility_label(s.mesh), Style::default().fg(TEXT_C)),
        Span::styled(format!("  ·  {}", s.root), Style::default().fg(DIM_C)),
        Span::styled(
            format!("  ·  {}", s.mesh.generated_at.chars().take(19).collect::<String>()),
            Style::default().fg(DIM_C),
        ),
    ]);

    // A tab strip across the top right: the four views, the active one lit.
    let tab_area = Rect { x: area.x + (area.width.saturating_sub(46)), y: area.y, width: 46.min(area.width), height: area.height };
    let tabs: Vec<Line> = scematica_mesh_tabs(s.view)
        .into_iter()
        .map(|(label, active)| {
            if active {
                Line::from(Span::styled(format!("[{label}]"), Style::default().fg(ACCENT_C).add_modifier(Modifier::BOLD)))
            } else {
                Line::from(Span::styled(format!(" {label} "), Style::default().fg(DIM_C)))
            }
        })
        .collect();
    let tab_line = Line::from(
        tabs.into_iter()
            .flat_map(|l| l.spans)
            .collect::<Vec<_>>(),
    );

    f.render_widget(Paragraph::new(vec![title, sub]), area);
    f.render_widget(Paragraph::new(tab_line).alignment(Alignment::Right), tab_area);
}

/// `(label, active)` for each view tab.
fn scematica_mesh_tabs(view: View) -> Vec<(&'static str, bool)> {
    View::ALL
        .iter()
        .map(|v| (v.as_str(), *v == view))
        .collect()
}

fn draw_diagnosis(f: &mut Frame, area: Rect, mesh: &Mesh) {
    let s = &mesh.summary;
    let alarming = s.blocking > 0;
    let colour = if alarming { colour_of(Tone::Veto) } else { TEXT_C };
    let heading = if alarming { "BLOCKING" } else { "DIAGNOSIS" };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if alarming { colour_of(Tone::Veto) } else { DIM_C }))
        .title(Span::styled(format!(" {heading} "), Style::default().fg(colour).add_modifier(Modifier::BOLD)));

    f.render_widget(
        Paragraph::new(Line::from(Span::styled(&*s.diagnosis, Style::default().fg(colour))))
            .block(block)
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn draw_graph(f: &mut Frame, area: Rect, s: &Screen) {
    let hint = if s.tracing { " TOPOLOGY · tracing " } else { " TOPOLOGY " };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM_C))
        .title(Span::styled(hint, Style::default().fg(ACCENT_C)));
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
        GateVerdict::Unevaluated => DIM_C,
    }
}

/// A truecolor Ψ meter: `width` filled cells, coloured by the heat ramp.
fn psi_meter(fraction: f64, width: usize) -> Line<'static> {
    let fraction = fraction.clamp(0.0, 1.0);
    let full = (fraction * width as f64).round() as usize;
    let mut spans = Vec::new();
    for i in 0..width {
        let local = (fraction * width as f64 - i as f64).clamp(0.0, 1.0);
        let t = (i as f64 + 0.5) / width as f64;
        let colour = if local <= 0.0 { FRAME } else { heat(t) };
        spans.push(Span::styled("█", Style::default().fg(colour.to_ratatui())));
    }
    let _ = full;
    Line::from(spans)
}

fn draw_coherence(f: &mut Frame, area: Rect, c: &Cognition) {
    let vc = verdict_colour(c.verdict);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM_C))
        .title(Span::styled(" AGENTIC COHERENCE GATE §32 ", Style::default().fg(ACCENT_C)));

    let mut lines = vec![
        Line::from(vec![
            Span::styled("Ψ = C · K · (1 − R)   ", Style::default().fg(MUTED_C)),
            Span::styled(format!("{:.3}", c.psi), Style::default().fg(vc).add_modifier(Modifier::BOLD)),
            Span::styled(format!("  {:?}", c.verdict).to_uppercase(), Style::default().fg(vc)),
        ]),
        psi_meter(c.psi, 28),
        Line::from(Span::styled(
            format!("computed on {:.0}% of its terms — a gate on a quarter of its inputs is a statement about ignorance", c.measured_fraction * 100.0),
            Style::default().fg(DIM_C),
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
        Line::from(Span::styled(&*c.reading, Style::default().fg(MUTED_C))),
        Line::from(Span::styled("─ confidence terms ─", Style::default().fg(DIM_C))),
    ];
    for t in &c.confidence_terms {
        lines.push(term_line(t));
    }
    lines.push(Line::from(Span::styled("─ risk field ─", Style::default().fg(DIM_C))));
    for t in &c.risk.components {
        lines.push(term_line(t));
    }
    if !c.omega_terms.is_empty() {
        lines.push(Line::from(Span::styled("─ Ω terms (unbuilt subsystems) ─", Style::default().fg(DIM_C))));
        for t in &c.omega_terms {
            lines.push(term_line(t));
        }
    }

    f.render_widget(Paragraph::new(lines).block(block).wrap(Wrap { trim: true }), area);
}

fn factor(label: &str, value: Option<f64>, invert: bool) -> Span<'static> {
    match value {
        None => Span::styled(format!("{label} —"), Style::default().fg(DIM_C)),
        Some(v) => {
            let good = if invert { v < 0.25 } else { v > 0.75 };
            let colour = if good { colour_of(Tone::Live) } else { TEXT_C };
            Span::styled(format!("{label} {v:.3}"), Style::default().fg(colour))
        }
    }
}

/// A term row, leading with whether it was measured at all.
fn term_line(t: &Term) -> Line<'static> {
    let (word, colour) = if t.measured {
        ("measured  ", colour_of(Tone::Live))
    } else {
        ("unmeasured", colour_of(Tone::Absent))
    };
    Line::from(vec![
        Span::styled(word, Style::default().fg(colour)),
        Span::styled(format!(" {:<8}", t.symbol), Style::default().fg(ACCENT_C)),
        Span::styled(format!("{:>6.3} ", t.value), Style::default().fg(TEXT_C)),
        Span::styled(t.note.clone(), Style::default().fg(MUTED_C)),
    ])
}

fn draw_roster(f: &mut Frame, area: Rect, s: &Screen) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM_C))
        .title(Span::styled(" ROSTER ", Style::default().fg(ACCENT_C)));

    let mut lines: Vec<Line> = Vec::new();
    for node in &s.mesh.nodes {
        let tone = view::tone_for(node);
        let colour = colour_of(tone);
        let selected = s.selected == Some(node.id.as_str());
        let mark = if selected { "▸" } else { " " };
        let prov = view::provenance_word(&node.provenance);
        let verdict = view::verdict_word(node.verdict);

        // Activity bar.
        let bar = match node.activity {
            Some(a) => {
                let width = 8usize;
                let filled = ((a.clamp(0.0, 1.0)) * width as f64).round() as usize;
                format!("{}{}", "█".repeat(filled), "·".repeat(width - filled))
            }
            None => "        ".to_string(),
        };

        let mut spans = vec![
            Span::styled(mark, Style::default().fg(ACCENT_C)),
            Span::styled(format!("{:<16}", truncate(&node.label, 16)), Style::default().fg(colour).add_modifier(if selected { Modifier::BOLD } else { Modifier::empty() })),
            Span::styled(format!("{:<7}", prov), Style::default().fg(colour)),
            Span::styled(format!("{:<9}", verdict), Style::default().fg(TEXT_C)),
        ];
        if node.activity.is_some() {
            spans.push(Span::styled(bar, Style::default().fg(colour)));
        }
        lines.push(Line::from(spans));
    }

    f.render_widget(Paragraph::new(lines).block(block).wrap(Wrap { trim: false }), area);
}

fn draw_gate(f: &mut Frame, area: Rect, c: &Cognition, show_terms: bool) {
    let vc = verdict_colour(c.verdict);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM_C))
        .title(Span::styled(" AGENTIC COHERENCE GATE §32 ", Style::default().fg(ACCENT_C)));

    let mut lines = vec![
        Line::from(vec![
            Span::styled("Ψ = C · K · (1 − R)   ", Style::default().fg(MUTED_C)),
            Span::styled(format!("{:.3}", c.psi), Style::default().fg(vc).add_modifier(Modifier::BOLD)),
            Span::styled(format!("  {:?}", c.verdict).to_uppercase(), Style::default().fg(vc)),
        ]),
        Line::from(Span::styled(
            format!("computed on {:.0}% of its terms", c.measured_fraction * 100.0),
            Style::default().fg(DIM_C),
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
        Line::from(Span::styled(&*c.reading, Style::default().fg(MUTED_C))),
    ];

    if show_terms {
        lines.push(Line::from(Span::styled("─ terms ─", Style::default().fg(DIM_C))));
        for t in c
            .confidence_terms
            .iter()
            .chain(c.risk.components.iter())
            .chain(c.omega_terms.iter())
        {
            lines.push(term_line(t));
        }
    }

    f.render_widget(Paragraph::new(lines).block(block).wrap(Wrap { trim: true }), area);
}

fn draw_detail(f: &mut Frame, area: Rect, s: &Screen) {
    let block = Block::default().borders(Borders::ALL).border_style(Style::default().fg(DIM_C));

    let Some(id) = s.selected else {
        let block = block.title(Span::styled(" NO SELECTION ", Style::default().fg(ACCENT_C)));
        let help = vec![
            Line::from(Span::styled("Pick a unit to see what it last decided.", Style::default().fg(MUTED_C))),
            Line::from(""),
            Line::from(Span::styled("1/2/3/4  switch view", Style::default().fg(DIM_C))),
            Line::from(Span::styled("↑↓←→ / hjkl  move between units", Style::default().fg(DIM_C))),
            Line::from(Span::styled("t  trace · g  terms · r  refresh", Style::default().fg(DIM_C))),
            Line::from(""),
            Line::from(Span::styled("A dark node is unseen, not idle — there is no source", Style::default().fg(DIM_C))),
            Line::from(Span::styled("on disk for it, so there is nothing to report.", Style::default().fg(DIM_C))),
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
    let block = block.title(Span::styled(format!(" {} ", node.label.to_uppercase()), Style::default().fg(colour).add_modifier(Modifier::BOLD)));

    let age = view::age_label(&node.provenance).map(|a| format!(" · {a} old")).unwrap_or_default();

    let mut lines = vec![
        Line::from(Span::styled(&*node.blurb, Style::default().fg(MUTED_C))),
        Line::from(vec![
            Span::styled(format!("{}{}", view::provenance_word(&node.provenance), age), Style::default().fg(colour)),
            Span::styled(format!("   verdict {}", view::verdict_word(node.verdict)), Style::default().fg(TEXT_C)),
            Span::styled(format!("   {}", id), Style::default().fg(DIM_C)),
        ]),
    ];

    if let Some(r) = &node.reason {
        lines.push(Line::from(Span::styled(r.clone(), Style::default().fg(colour))));
    }

    if node.detail.is_empty() {
        lines.push(Line::from(Span::styled(
            "No values — this unit has no source on disk, so there is nothing to report rather than nothing happening.",
            Style::default().fg(DIM_C),
        )));
    } else {
        for (k, v) in &node.detail {
            lines.push(Line::from(vec![
                Span::styled(format!("{k:<18}"), Style::default().fg(DIM_C)),
                Span::styled(v.clone(), Style::default().fg(TEXT_C)),
            ]));
        }
    }

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
                Span::styled(format!("{} {:<22}", if outgoing { "→" } else { "←" }, other), Style::default().fg(MUTED_C)),
                Span::styled(format!("{:<11}", format!("{:?}", e.kind).to_lowercase()), Style::default().fg(DIM_C)),
                Span::styled(format!("{word:<11}"), Style::default().fg(colour_of(tone))),
            ];
            if let Some(l) = &e.label {
                spans.push(Span::styled(l.clone(), Style::default().fg(TEXT_C)));
            }
            Line::from(spans)
        })
        .collect();

    if !wiring.is_empty() {
        lines.push(Line::from(Span::styled("─ wiring ─", Style::default().fg(DIM_C))));
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

    let legend = match s.view {
        View::Tornado => {
            let c = &s.mesh.cognition;
            vec![
                Span::styled("Ψ ", Style::default().fg(ACCENT_C)),
                Span::styled(format!("{:.3}", c.psi), verdict_colour(c.verdict)),
                Span::styled(format!(" {:?}", c.verdict).to_uppercase(), verdict_colour(c.verdict)),
                Span::styled("   intensity ", Style::default().fg(DIM_C)),
                Span::styled(format!("{:.2}", s.mesh.cognition.measured_fraction), Style::default().fg(TEXT_C)),
                Span::styled(
                    "   │ q quit · 1-4 view · ←→ rotate · r refresh · t trace · g terms · </> scroll",
                    Style::default().fg(DIM_C),
                ),
            ]
        }
        _ => vec![
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
                    "   │ q quit · 1-4 view · r refresh ({}s) · t trace · g terms · c clear · ←→↑↓ move · </> scroll",
                    s.interval_secs
                ),
                Style::default().fg(DIM_C),
            ),
        ],
    };
    let question = Line::from(vec![
        Span::styled("  ›  ", Style::default().fg(ACCENT_C)),
        Span::styled(s.view.question(), Style::default().fg(MUTED_C)),
    ]);
    f.render_widget(Paragraph::new(Line::from(legend)).alignment(Alignment::Left), area);
    // The question each view answers sits under the legend so the operator always knows
    // what the current picture is for.
    let question_area = Rect { x: area.x, y: area.y.saturating_sub(1), width: area.width, height: 1 };
    if area.y > 0 {
        f.render_widget(Paragraph::new(question).alignment(Alignment::Left), question_area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vortex::Camera;
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

    /// The three edge states must differ in *glyph*, not only in colour.
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
        assert_eq!(edge_stroke(&mk(Some(true))).1, Tone::Veto);
        assert_eq!(edge_stroke(&mk(Some(false))).1, Tone::Absent);
    }

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
        assert!(!bar_row.contains('█') && !bar_row.contains('·'), "an unmeasurable node drew a bar: {bar_row:?}");

        let b = g.find("b").unwrap();
        let zero_row: String = (b.x..b.x + NODE_W).map(|x| c.get(x, b.y + 3).ch).collect();
        assert!(zero_row.contains('·'), "a measured zero must render: {zero_row:?}");
    }

    #[test]
    fn every_node_states_its_provenance_in_words() {
        let mesh = Mesh::new(
            vec![node("dq", NodeKind::Learner, Provenance::Stale { age_secs: 432_000, budget_secs: 120 }, Verdict::Veto, None)],
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
        let mesh = Mesh::new(vec![node("a", NodeKind::Listener, LIVE, Verdict::Pass, None)], vec![], "t".into());
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

    /// Drive the whole frame through ratatui's `TestBackend`, across every view.
    #[test]
    fn the_frame_renders_at_hostile_terminal_sizes_and_every_view() {
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
        let camera = Camera::default();

        for view in View::ALL {
            for (w, h) in [(8, 4), (20, 10), (40, 18), (120, 40), (200, 60)] {
                let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
                for (selected, tracing, terms) in [(None, false, false), (Some("x"), true, true), (Some("f"), false, true)] {
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
                                view,
                                time: 1.0,
                                camera: &camera,
                            },
                        )
                    })
                    .unwrap_or_else(|e| panic!("draw failed at {w}x{h} {view:?}: {e}"));
                }
            }
        }
    }

    #[test]
    fn scrolling_past_the_canvas_renders_empty_rather_than_panicking() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mesh = Mesh::new(vec![node("a", NodeKind::Listener, LIVE, Verdict::Pass, None)], vec![], "t".into());
        let g = view::layout(&mesh);
        let canvas = paint_graph(&mesh, &g, None, None);
        let camera = Camera::default();
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
                    view: View::Graph,
                    time: 0.0,
                    camera: &camera,
                },
            )
        })
        .unwrap();
    }

    #[test]
    fn the_psi_meter_is_all_cells() {
        let line = psi_meter(0.5, 10);
        assert_eq!(line.spans.len(), 10);
    }
}
