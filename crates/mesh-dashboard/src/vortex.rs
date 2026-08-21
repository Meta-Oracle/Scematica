//! The active-node tornado: a 3D depth-buffered truecolor vortex of the real mesh.
//!
//! This is the headline view of the overhauled dashboard. It is a faithful port of the
//! observatory canvas from `hatcher-terminal` — a character grid with a z-buffer that
//! emits truecolor — re-pointed at `scematica-mesh`'s own data. The physics is the same
//! literal mapping that made the hatcher vortex legible:
//!
//! * **Radius** — inverse capability. A node carrying live throughput is drawn toward the
//!   axis; one that is doing nothing flings out to the rim, so the eye lands on the core of
//!   the mesh first.
//! * **Height** — standing. Hubs (the DQ* agent, the strategy agent, the executor) ride
//!   high in the funnel; ingest and the filter floor sit low.
//! * **Angular speed** — activation. A node carrying live signal spins faster.
//! * **Amplitude** — how much of the mesh is alive. A mesh where only a couple of units are
//!   reporting stands a short, dim funnel; one that is fully live builds a tall bright
//!   vortex.
//!
//! An idle mesh looks idle: with no live activation the funnel decays to a slow, dim ring,
//! and only spins up when work is actually flowing.
//!
//! ## Honesty is preserved
//!
//! The vortex is built from the *same* nodes the topology panel draws, and it obeys the
//! same rules:
//!
//! * An **absent** node (no source on disk — unseen, not idle) is **excluded** from the lit
//!   vortex. Spinning up a particle for a unit that does not exist would claim the system is
//!   doing something it cannot be seen to be doing.
//! * A **live veto** (the DQ* agent actively blocking buys) glows red in the vortex — the
//!   same alarm colour the topology and the gate use, so a blocking agent is red in every
//!   view at once.
//! * Stale nodes render dimmed, because a number that was true an hour ago is not a current
//!   signal.

use std::fmt::Write as _;

use scematica_mesh::{Mesh, NodeKind, Provenance};

use crate::view::{tone_for, Tone};

// ── colour ──────────────────────────────────────────────────────────────────

/// A 24-bit terminal colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// Linear blend, `t` clamped to `[0, 1]`.
    pub fn mix(self, other: Rgb, t: f64) -> Rgb {
        let t = t.clamp(0.0, 1.0);
        let lerp = |a: u8, b: u8| (a as f64 + (b as f64 - a as f64) * t).round() as u8;
        Rgb(lerp(self.0, other.0), lerp(self.1, other.1), lerp(self.2, other.2))
    }

    /// Scale toward black. Used for depth cueing in the 3D views.
    pub fn dim(self, factor: f64) -> Rgb {
        let f = factor.clamp(0.0, 1.0);
        Rgb(
            (self.0 as f64 * f).round() as u8,
            (self.1 as f64 * f).round() as u8,
            (self.2 as f64 * f).round() as u8,
        )
    }

    /// Perceptual weight in `[0, 1]`, used by the TUI blit to decide contrast.
    #[allow(dead_code)]
    pub fn luma(self) -> f64 {
        (0.2126 * self.0 as f64 + 0.7152 * self.1 as f64 + 0.0722 * self.2 as f64) / 255.0
    }

    /// Convert to a ratatui colour for the TUI backend.
    pub fn to_ratatui(self) -> ratatui::style::Color {
        ratatui::style::Color::Rgb(self.0, self.1, self.2)
    }
}

pub const BG: Rgb = Rgb(6, 8, 14);
pub const FRAME: Rgb = Rgb(38, 52, 74);
pub const LABEL: Rgb = Rgb(126, 148, 178);
/// Default foreground for prose.
#[allow(dead_code)]
pub const TEXT: Rgb = Rgb(198, 214, 232);
pub const ACCENT: Rgb = Rgb(129, 140, 248);
pub const WARN: Rgb = Rgb(245, 181, 68);
pub const BAD: Rgb = Rgb(255, 93, 125);
pub const GOOD: Rgb = Rgb(74, 222, 155);

/// Cold → hot ramp. The crate's primary scalar encoding: use it for any value
/// already normalized to `[0, 1]`.
pub fn heat(t: f64) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    const STOPS: [(f64, Rgb); 5] = [
        (0.00, Rgb(18, 26, 58)),
        (0.30, Rgb(37, 99, 235)),
        (0.55, Rgb(45, 212, 191)),
        (0.78, Rgb(250, 204, 21)),
        (1.00, Rgb(244, 63, 94)),
    ];
    for pair in STOPS.windows(2) {
        let (t0, c0) = pair[0];
        let (t1, c1) = pair[1];
        if t <= t1 {
            let span = t1 - t0;
            let local = if span <= f64::EPSILON { 0.0 } else { (t - t0) / span };
            return c0.mix(c1, local);
        }
    }
    STOPS[STOPS.len() - 1].1
}

/// Glyph ramp by density, coarse → solid. Shared by every field renderer so a
/// denser glyph always means more of whatever the panel is measuring.
pub const DENSITY: [char; 10] = [' ', '.', ':', '-', '=', '+', '*', '#', '%', '@'];

/// Pick a density glyph for `t` in `[0, 1]`.
pub fn glyph(t: f64) -> char {
    let t = t.clamp(0.0, 1.0);
    let index = (t * (DENSITY.len() - 1) as f64).round() as usize;
    DENSITY[index.min(DENSITY.len() - 1)]
}

/// The truecolor a tone earns — matches `Tone`'s place in the honesty model.
impl Tone {
    pub fn rgb(self) -> Rgb {
        match self {
            Tone::Live => GOOD,
            Tone::Stale => WARN,
            Tone::Absent => Rgb(90, 100, 140),
            Tone::Veto => BAD,
            Tone::Simulated => Rgb(124, 156, 255),
        }
    }
}

// ── 3D ─────────────────────────────────────────────────────────────────────

/// Character cells are about twice as tall as wide; scale x to compensate.
pub const CELL_ASPECT: f64 = 2.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }
    pub fn scale(self, k: f64) -> Vec3 {
        Vec3::new(self.x * k, self.y * k, self.z * k)
    }
}

impl std::ops::Add for Vec3 {
    type Output = Vec3;
    fn add(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}
impl std::ops::Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}
impl std::ops::Mul<f64> for Vec3 {
    type Output = Vec3;
    fn mul(self, k: f64) -> Vec3 {
        self.scale(k)
    }
}

/// A projected point in grid space.
#[derive(Debug, Clone, Copy)]
pub struct Projected {
    pub x: i64,
    pub y: i64,
    /// Camera-space depth, smaller is nearer.
    pub depth: f64,
    /// `1/depth`-style scale in `(0, 1]`, for size / brightness cueing.
    pub scale: f64,
}

/// An orbit camera looking at the origin.
#[derive(Debug, Clone, Copy)]
pub struct Camera {
    pub yaw: f64,
    pub pitch: f64,
    pub distance: f64,
    pub focal: f64,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: 0.32,
            distance: 9.0,
            focal: 14.0,
        }
    }
}

impl Camera {
    pub fn to_camera(self, point: Vec3) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let x = point.x * cy + point.z * sy;
        let z = -point.x * sy + point.z * cy;

        let (sp, cp) = self.pitch.sin_cos();
        let y = point.y * cp - z * sp;
        let z = point.y * sp + z * cp;

        Vec3::new(x, y, z + self.distance.max(0.1))
    }

    /// Project a world point into grid cells centred on `(cx, cy)`.
    pub fn project(&self, point: Vec3, cx: f64, cy: f64) -> Option<Projected> {
        let cs = self.to_camera(point);
        const NEAR: f64 = 0.35;
        if cs.z <= NEAR {
            return None;
        }
        let inverse = self.focal / cs.z;
        let sx = cx + cs.x * inverse * CELL_ASPECT;
        let sy = cy - cs.y * inverse;
        if !sx.is_finite() || !sy.is_finite() {
            return None;
        }
        Some(Projected {
            x: sx.round() as i64,
            y: sy.round() as i64,
            depth: cs.z,
            scale: (inverse / self.focal).clamp(0.0, 1.0),
        })
    }
}

// ── depth grid ───────────────────────────────────────────────────────────────

/// One character cell of the vortex canvas.
#[derive(Debug, Clone, Copy)]
struct Cell {
    ch: char,
    fg: Rgb,
    depth: f64,
}

impl Cell {
    const EMPTY: Cell = Cell {
        ch: ' ',
        fg: BG,
        depth: f64::INFINITY,
    };
}

/// A fixed-size, depth-buffered drawing surface that emits truecolor or plain text.
///
/// `put_depth` honours the z-buffer so the vortex occludes itself instead of painting the
/// last-drawn particle on top. `put` writes at the front of the buffer (for UI chrome and
/// labels, which must never be occluded by geometry).
pub struct DepthGrid {
    width: usize,
    height: usize,
    cells: Vec<Cell>,
    clip: (i64, i64, i64, i64),
}

impl DepthGrid {
    pub fn new(width: usize, height: usize) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        Self {
            width,
            height,
            cells: vec![Cell::EMPTY; width * height],
            clip: (0, 0, width as i64 - 1, height as i64 - 1),
        }
    }

    /// Resize, clearing. Used when the terminal changes size between frames.
    #[allow(dead_code)]
    pub fn resize(&mut self, width: usize, height: usize) {
        *self = DepthGrid::new(width, height);
    }

    pub fn set_clip(&mut self, x: i64, y: i64, w: usize, h: usize) -> (i64, i64, i64, i64) {
        let previous = self.clip;
        let x1 = x + w as i64 - 1;
        let y1 = y + h as i64 - 1;
        self.clip = (x.max(0), y.max(0), x1.min(self.width as i64 - 1), y1.min(self.height as i64 - 1));
        previous
    }
    pub fn restore_clip(&mut self, prev: (i64, i64, i64, i64)) {
        self.clip = prev;
    }
    pub fn reset_clip(&mut self) {
        self.clip = (0, 0, self.width as i64 - 1, self.height as i64 - 1);
    }
    fn in_clip(&self, x: i64, y: i64) -> bool {
        let (x0, y0, x1, y1) = self.clip;
        x >= x0 && x <= x1 && y >= y0 && y <= y1
    }

    pub fn width(&self) -> usize {
        self.width
    }
    pub fn height(&self) -> usize {
        self.height
    }
    pub fn clear(&mut self) {
        self.cells.fill(Cell::EMPTY);
        self.reset_clip();
    }
    fn index(&self, x: usize, y: usize) -> Option<usize> {
        if x < self.width && y < self.height {
            Some(y * self.width + x)
        } else {
            None
        }
    }

    /// Draw a fragment, honouring the depth buffer.
    pub fn put_depth(&mut self, x: i64, y: i64, ch: char, fg: Rgb, depth: f64) {
        if x < 0 || y < 0 || depth.is_nan() || !self.in_clip(x, y) {
            return;
        }
        let Some(index) = self.index(x as usize, y as usize) else {
            return
        };
        if depth <= self.cells[index].depth {
            self.cells[index] = Cell { ch, fg, depth };
        }
    }

    /// Draw at the front of the buffer — never occluded.
    pub fn put(&mut self, x: i64, y: i64, ch: char, fg: Rgb) {
        self.put_depth(x, y, ch, fg, f64::NEG_INFINITY);
    }

    pub fn text(&mut self, x: i64, y: i64, s: &str, fg: Rgb) {
        for (offset, ch) in s.chars().enumerate() {
            self.put(x + offset as i64, y, ch, fg);
        }
    }

    pub fn text_clipped(&mut self, x: i64, y: i64, s: &str, fg: Rgb, max_width: usize) {
        self.text_clipped_depth(x, y, s, fg, max_width, f64::NEG_INFINITY);
    }

    pub const LABEL_DEPTH_BIAS: f64 = 1_000.0;

    /// Draw a node label atomically, or draw nothing. An occluded label is dropped whole
    /// rather than fragmented — two names a few columns apart must not weld into a third.
    pub fn text_label(&mut self, x: i64, y: i64, s: &str, fg: Rgb, max_width: usize, depth: f64) -> bool {
        if max_width == 0 || depth.is_nan() {
            return false;
        }
        let span = s.chars().count().min(max_width) as i64;
        let contested = (0..span).any(|offset| {
            let (cell_x, cell_y) = (x + offset, y);
            if cell_x < 0 || cell_y < 0 || !self.in_clip(cell_x, cell_y) {
                return false;
            }
            match self.index(cell_x as usize, cell_y as usize) {
                Some(index) => depth > self.cells[index].depth,
                None => false,
            }
        });
        if contested {
            return false;
        }
        self.text_clipped_depth(x, y, s, fg, max_width, depth);
        true
    }

    pub fn text_clipped_depth(&mut self, x: i64, y: i64, s: &str, fg: Rgb, max_width: usize, depth: f64) {
        if max_width == 0 {
            return;
        }
        let count = s.chars().count();
        let keep = if count <= max_width { count } else { max_width.saturating_sub(1) };
        for (offset, ch) in s.chars().take(keep).enumerate() {
            self.put_depth(x + offset as i64, y, ch, fg, depth);
        }
        if count > max_width {
            self.put_depth(x + keep as i64, y, '…', fg, depth);
        }
    }

    /// Bresenham line at constant depth.
    #[allow(clippy::too_many_arguments)]
    pub fn line(&mut self, x0: i64, y0: i64, x1: i64, y1: i64, ch: char, fg: Rgb, depth: f64) {
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        let (mut x, mut y) = (x0, y0);
        loop {
            self.put_depth(x, y, ch, fg, depth);
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                if x == x1 {
                    break;
                }
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                if y == y1 {
                    break;
                }
                err += dx;
                y += sy;
            }
        }
    }

    /// A single-line box with an optional title in the top rule.
    pub fn frame(&mut self, x: i64, y: i64, w: usize, h: usize, title: &str, fg: Rgb) {
        if w < 2 || h < 2 {
            return;
        }
        let right = x + w as i64 - 1;
        let bottom = y + h as i64 - 1;
        for column in x + 1..right {
            self.put(column, y, '─', fg);
            self.put(column, bottom, '─', fg);
        }
        for row in y + 1..bottom {
            self.put(x, row, '│', fg);
            self.put(right, row, '│', fg);
        }
        self.put(x, y, '╭', fg);
        self.put(right, y, '╮', fg);
        self.put(x, bottom, '╰', fg);
        self.put(right, bottom, '╯', fg);
        if !title.is_empty() && w > 6 {
            let label = format!(" {title} ");
            self.text_clipped(x + 2, y, &label, ACCENT, w - 4, );
        }
    }

    /// A horizontal meter: `filled` fraction of `w` cells, coloured by ramp.
    pub fn bar(&mut self, x: i64, y: i64, w: usize, filled: f64, ramp: impl Fn(f64) -> Rgb) {
        let filled = filled.clamp(0.0, 1.0);
        let full = filled * w as f64;
        for column in 0..w {
            let local = (full - column as f64).clamp(0.0, 1.0);
            let ch = if local <= 0.0 { '░' } else { glyph(local) };
            let colour = if local <= 0.0 { FRAME } else { ramp((column as f64 + 0.5) / w as f64) };
            self.put(x + column as i64, y, ch, colour);
        }
    }

    /// Read a cell's foreground colour (for the TUI blit).
    pub fn fg_at(&self, x: usize, y: usize) -> Rgb {
        self.index(x, y).map(|i| self.cells[i].fg).unwrap_or(BG)
    }

    /// Read a cell's glyph (for the TUI blit).
    pub fn cells_char(&self, x: usize, y: usize) -> char {
        self.index(x, y).map(|i| self.cells[i].ch).unwrap_or(' ')
    }

    /// Render to an ANSI truecolor string. Colour changes are emitted only when the colour
    /// actually changes, keeping a full-screen frame to a few KB.
    pub fn render_ansi(&self) -> String {
        let mut out = String::with_capacity(self.width * self.height * 4);
        let mut current: Option<Rgb> = None;
        for row in 0..self.height {
            for column in 0..self.width {
                let cell = self.cells[row * self.width + column];
                if current != Some(cell.fg) {
                    let Rgb(r, g, b) = cell.fg;
                    let _ = write!(out, "\x1b[38;2;{r};{g};{b}m");
                    current = Some(cell.fg);
                }
                out.push(cell.ch);
            }
            out.push_str("\x1b[0m");
            current = None;
            if row + 1 < self.height {
                out.push('\n');
            }
        }
        out
    }

    /// Render without escape codes — used by `--once` plain output and snapshots.
    pub fn render_plain(&self) -> String {
        let mut out = String::with_capacity(self.width * self.height);
        for row in 0..self.height {
            for column in 0..self.width {
                out.push(self.cells[row * self.width + column].ch);
            }
            if row + 1 < self.height {
                out.push('\n');
            }
        }
        out
    }
}

// ── the vortex ───────────────────────────────────────────────────────────────

/// Funnel geometry.
const FUNNEL_HEIGHT: f64 = 5.0;
const FUNNEL_RADIUS: f64 = 3.4;
const TOUCHDOWN_RADIUS: f64 = 0.35;
const TRACERS: usize = 320;
const IDLE_SPIN: f64 = 0.18;

/// Standing (height in the funnel) by node kind. Hubs ride high; ingest and the
/// filter floor sit low. This is a *display* heuristic, not a measurement — it is the
/// funnel's version of "who matters", and it is purely a function of role, never of a
/// number that was not read.
fn standing(kind: NodeKind) -> f64 {
    match kind {
        NodeKind::Learner => 0.9,
        NodeKind::Reasoner => 0.85,
        // An observing decision runtime rides highest of all: it is the only unit that sees
        // the whole system rather than one stage of it. Display heuristic, as above — role,
        // never a number.
        NodeKind::Agent => 0.95,
        NodeKind::Gate => 0.8,
        NodeKind::Scorer => 0.7,
        NodeKind::Breaker => 0.6,
        NodeKind::Executor => 0.55,
        NodeKind::Peer => 0.5,
        NodeKind::Filter => 0.35,
        NodeKind::Listener => 0.2,
    }
}

/// One agent, resolved into vortex coordinates. Built only from nodes that are actually
/// visible — an absent node is never spun into the vortex.
pub struct VortexNode {
    pub label: String,
    pub tone: Tone,
    /// Normalized capability in `[0, 1]`: `1 − activity`, so a busy node sits near the
    /// axis and a quiet one flings to the rim. Falls back to a mid value when no activity
    /// is reported (an unmeasurable node is not thrown to the rim — that would claim it is
    /// the least capable thing in the mesh).
    pub capability: f64,
    /// Standing in `[0, 1]` from role.
    pub standing: f64,
    /// Live activation in `[0, 1]`, driving spin and brightness.
    pub activation: f64,
    /// Phase offset so the cohort is spread around the funnel.
    pub phase: f64,
}

/// The whole vortex, built from a live [`Mesh`].
pub struct Vortex {
    pub nodes: Vec<VortexNode>,
    /// Amplitude in `[0, 1]`: how tall and wide the funnel stands. Driven by how alive the
    /// mesh is — mean visibility of the live cohort.
    pub amplitude: f64,
    /// Mean activation across the visible cohort — the "is the mesh live" signal.
    pub intensity: f64,
    /// Whether the mesh is currently doing anything observable. A dormant mesh does not
    /// spin up.
    pub active: bool,
}

impl Vortex {
    /// Build the vortex from a mesh, excluding any node that is `Absent` (unseen).
    pub fn from_mesh(mesh: &Mesh) -> Self {
        let mut nodes = Vec::new();
        let mut activation_sum = 0.0;
        let mut live_activation = 0.0;
        let mut live_count = 0usize;

        for (index, node) in mesh.nodes.iter().filter(|n| !matches!(n.provenance, Provenance::Absent)).enumerate() {
            let tone = tone_for(node);
            let capability = 1.0 - node.activity.unwrap_or(0.5);
            let activation = node.activity.unwrap_or(0.0).clamp(0.0, 1.0);
            let is_live = matches!(node.provenance, Provenance::Live { .. });
            if is_live {
                live_activation += activation;
                live_count += 1;
            }
            activation_sum += activation;
            nodes.push(VortexNode {
                label: node.label.clone(),
                tone,
                capability: capability.clamp(0.0, 1.0),
                standing: standing(node.kind),
                activation,
                phase: index as f64 * std::f64::consts::TAU / mesh.nodes.len().max(1) as f64,
            });
        }

        let intensity = if nodes.is_empty() {
            0.0
        } else {
            (activation_sum / nodes.len() as f64).clamp(0.0, 1.0)
        };
        // Amplitude rides a little above the live mean so a fully-live mesh stands tall.
        let amplitude = if nodes.is_empty() {
            0.0
        } else {
            (0.25 + 0.75 * (if live_count > 0 { live_activation / live_count as f64 } else { 0.0 })).clamp(0.0, 1.0)
        };
        let active = mesh.summary.blocking > 0
            || intensity > 0.05
            || mesh.nodes.iter().any(|n| matches!(n.provenance, Provenance::Live { .. }) && n.activity.unwrap_or(0.0) > 0.0);

        Vortex { nodes, amplitude, intensity, active }
    }

    /// Radius of the funnel wall at normalized height `t` in `[0, 1]`.
    pub fn radius_at(&self, t: f64) -> f64 {
        let t = t.clamp(0.0, 1.0);
        let amplitude = self.amplitude.clamp(0.0, 1.0);
        let profile = TOUCHDOWN_RADIUS + (FUNNEL_RADIUS - TOUCHDOWN_RADIUS) * t * t;
        profile * (0.45 + 0.55 * amplitude)
    }
    pub fn height(&self) -> f64 {
        FUNNEL_HEIGHT * (0.5 + 0.5 * self.amplitude.clamp(0.0, 1.0))
    }
    pub fn spin_at(&self, t: f64) -> f64 {
        let base = if self.active { IDLE_SPIN + 2.4 * self.intensity.clamp(0.0, 1.0) } else { IDLE_SPIN };
        base * (1.9 - 0.9 * t.clamp(0.0, 1.0))
    }

    /// World position of a node at time `time`.
    pub fn node_position(&self, node: &VortexNode, time: f64) -> Vec3 {
        let t = (1.0 - node.capability) * 0.75 + (1.0 - node.standing) * 0.25;
        let radius = self.radius_at(t);
        let angle = node.phase + time * self.spin_at(t) * (0.6 + 0.8 * node.activation);
        let y = -self.height() * 0.5 + self.height() * t;
        Vec3::new(radius * angle.cos(), y, radius * angle.sin())
    }

    /// Draw the vortex into `grid`, centred on `(cx, cy)` in cells. `focal` is derived from
    /// the panel size so the funnel fills whatever space it is given.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_fitted(&self, grid: &mut DepthGrid, camera: &Camera, cx: f64, cy: f64, view_w: usize, view_h: usize, time: f64) {
        let mut fitted = *camera;
        fitted.focal = self.fit_focal(camera, view_w, view_h);
        self.draw(grid, &fitted, cx, cy, time);
    }

    fn fit_focal(&self, camera: &Camera, view_w: usize, view_h: usize) -> f64 {
        let radius = self.radius_at(1.0).max(0.05);
        let half_height = (self.height() * 0.5).max(0.05);
        let distance = camera.distance.max(0.1);
        let usable_w = (view_w as f64 * 0.44).max(1.0);
        let usable_h = (view_h as f64 * 0.46).max(1.0);
        let focal_w = usable_w * distance / (radius * CELL_ASPECT);
        let focal_h = usable_h * distance / half_height;
        focal_w.min(focal_h).clamp(4.0, 80.0)
    }

    pub fn draw(&self, grid: &mut DepthGrid, camera: &Camera, cx: f64, cy: f64, time: f64) {
        self.draw_tracers(grid, camera, cx, cy, time);
        self.draw_axis(grid, camera, cx, cy);
        self.draw_nodes(grid, camera, cx, cy, time);
    }

    fn draw_tracers(&self, grid: &mut DepthGrid, camera: &Camera, cx: f64, cy: f64, time: f64) {
        let budget = if self.active { TRACERS } else { TRACERS / 3 };
        let rings = if self.active { 22 } else { 10 };
        let per_ring = (budget / rings).max(4);

        for ring in 0..rings {
            let t = ((ring as f64 + 0.5) / rings as f64).powf(0.85);
            let y = -self.height() * 0.5 + self.height() * t;
            let base_radius = self.radius_at(t);
            let spin = self.spin_at(t);
            let ring_offset = ring as f64 * 2.399_963_23;

            for slot in 0..per_ring {
                let seed = (ring * per_ring + slot) as f64;
                let angle = ring_offset + slot as f64 * std::f64::consts::TAU / per_ring as f64 + time * spin;
                let jitter = 0.86 + 0.14 * ((seed * 0.754_877_666) % 1.0);
                let radius = base_radius * jitter;

                let point = Vec3::new(radius * angle.cos(), y, radius * angle.sin());
                let Some(p) = camera.project(point, cx, cy) else { continue };

                let depth_cue = (p.scale * 1.6).clamp(0.18, 1.0);
                let energy = (self.intensity * 0.6 + t * 0.4).clamp(0.0, 1.0);
                let colour = heat(energy).dim(depth_cue);
                let ch = if self.active {
                    glyph(0.15 + 0.85 * (0.35 * energy + 0.65 * depth_cue))
                } else {
                    '·'
                };
                grid.put_depth(p.x, p.y, ch, colour, p.depth);
            }
        }
    }

    fn draw_axis(&self, grid: &mut DepthGrid, camera: &Camera, cx: f64, cy: f64) {
        let half = self.height() * 0.5;
        let top = camera.project(Vec3::new(0.0, half, 0.0), cx, cy);
        let bottom = camera.project(Vec3::new(0.0, -half, 0.0), cx, cy);
        if let (Some(a), Some(b)) = (top, bottom) {
            grid.line(a.x, a.y, b.x, b.y, '│', FRAME, (a.depth + b.depth) * 0.5);
        }
    }

    fn draw_nodes(&self, grid: &mut DepthGrid, camera: &Camera, cx: f64, cy: f64, time: f64) {
        let mut projected: Vec<(Projected, &VortexNode)> = self
            .nodes
            .iter()
            .filter_map(|node| Some((camera.project(self.node_position(node, time), cx, cy)?, node)))
            .collect();

        for (p, node) in &projected {
            let activation = node.activation.clamp(0.0, 1.0);
            // A live veto glows red; everything else uses its tone colour, dimmed by depth.
            let colour: Rgb = if node.tone == Tone::Veto {
                BAD
            } else {
                node.tone.rgb().dim((p.scale * 1.8).clamp(0.35, 1.0))
            };
            let marker = if activation > 0.66 {
                '◉'
            } else if activation > 0.33 {
                '◍'
            } else {
                '○'
            };
            grid.put_depth(p.x, p.y, marker, colour, p.depth - 0.01);
        }

        // Labels last, nearest first.
        projected.sort_by(|(a, _), (b, _)| a.depth.total_cmp(&b.depth));
        for (p, node) in &projected {
            if node.activation.clamp(0.0, 1.0) <= 0.25 && node.standing < 0.6 {
                continue;
            }
            let label: String = node.label.chars().take(10).collect();
            let label_depth = p.depth - DepthGrid::LABEL_DEPTH_BIAS;
            grid.text_label(p.x + 2, p.y, &label, LABEL, 12, label_depth);
        }
    }
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

    #[test]
    fn the_funnel_narrows_toward_touchdown() {
        let v = Vortex {
            nodes: vec![],
            amplitude: 0.7,
            intensity: 0.5,
            active: true,
        };
        assert!(v.radius_at(0.0) < v.radius_at(0.5));
        assert!(v.radius_at(0.5) < v.radius_at(1.0));
    }

    #[test]
    fn an_active_vortex_spins_faster_than_an_idle_one() {
        let active = Vortex { nodes: vec![], amplitude: 0.7, intensity: 0.9, active: true };
        let idle = Vortex { nodes: vec![], amplitude: 0.0, intensity: 0.0, active: false };
        assert!(active.spin_at(0.5) > idle.spin_at(0.5));
        assert!(idle.spin_at(0.5) > 0.0);
    }

    #[test]
    fn touchdown_whips_faster_than_the_rim() {
        let v = Vortex { nodes: vec![], amplitude: 0.6, intensity: 0.6, active: true };
        assert!(v.spin_at(0.0) > v.spin_at(1.0));
    }

    /// The honesty rule that matters most here: an absent node must never enter the vortex.
    #[test]
    fn absent_nodes_are_excluded_from_the_vortex() {
        let mesh = Mesh::new(
            vec![
                node("a", NodeKind::Listener, LIVE, Verdict::Pass, Some(0.5)),
                Node::absent("b", NodeKind::Learner, "B", "unseen"),
                node("c", NodeKind::Executor, LIVE, Verdict::Idle, None),
            ],
            vec![],
            "t".into(),
        );
        let v = Vortex::from_mesh(&mesh);
        assert_eq!(v.nodes.len(), 2, "the unseen node must not be spun up");
        assert!(v.nodes.iter().all(|n| n.label != "b"));
    }

    #[test]
    fn a_live_veto_carries_the_veto_tone_into_the_vortex() {
        let mesh = Mesh::new(
            vec![node("dq", NodeKind::Learner, LIVE, Verdict::Veto, Some(0.8))],
            vec![],
            "t".into(),
        );
        let v = Vortex::from_mesh(&mesh);
        assert_eq!(v.nodes[0].tone, Tone::Veto);
    }

    #[test]
    fn an_emtpy_mesh_yields_an_inactive_flat_vortex() {
        let mesh = Mesh::new(
            vec![Node::absent("x", NodeKind::Executor, "X", "unseen")],
            vec![],
            "t".into(),
        );
        let v = Vortex::from_mesh(&mesh);
        assert!(v.nodes.is_empty());
        assert!(!v.active);
        assert_eq!(v.amplitude, 0.0);
    }

    #[test]
    fn a_live_mesh_is_active_and_tall() {
        let mesh = Mesh::new(
            vec![
                node("l", NodeKind::Listener, LIVE, Verdict::Pass, Some(0.6)),
                node("f", NodeKind::Filter, LIVE, Verdict::Pass, Some(0.4)),
                node("x", NodeKind::Executor, LIVE, Verdict::Idle, Some(0.1)),
            ],
            vec![Edge::signal("l", "f"), Edge::signal("f", "x")],
            "t".into(),
        );
        let v = Vortex::from_mesh(&mesh);
        assert!(v.active);
        assert!(v.amplitude > 0.2);
        assert!(v.intensity > 0.0);
    }

    #[test]
    fn depth_grid_keeps_the_nearest_fragment() {
        let mut g = DepthGrid::new(3, 1);
        g.put_depth(0, 0, 'f', TEXT, 10.0);
        g.put_depth(0, 0, 'n', TEXT, 1.0);
        g.put_depth(0, 0, 'x', TEXT, 50.0);
        assert_eq!(g.cells[0].ch, 'n');
    }

    #[test]
    fn out_of_bounds_writes_are_ignored() {
        let mut g = DepthGrid::new(4, 3);
        g.put(-1, 0, 'x', TEXT);
        g.put(0, -1, 'x', TEXT);
        g.put(99, 0, 'x', TEXT);
        assert_eq!(g.render_plain(), "    \n    \n    ");
    }

    #[test]
    fn a_fitted_vortex_fills_its_viewport() {
        let mesh = Mesh::new(
            vec![
                node("l", NodeKind::Listener, LIVE, Verdict::Pass, Some(0.6)),
                node("f", NodeKind::Filter, LIVE, Verdict::Veto, Some(0.5)),
                node("b", NodeKind::Breaker, LIVE, Verdict::Pass, Some(0.3)),
                node("d", NodeKind::Learner, LIVE, Verdict::Veto, Some(0.9)),
                node("x", NodeKind::Executor, LIVE, Verdict::Idle, Some(0.2)),
            ],
            vec![Edge::signal("l", "f"), Edge::veto("d", "x")],
            "t".into(),
        );
        let v = Vortex::from_mesh(&mesh);
        let camera = Camera::default();
        let mut grid = DepthGrid::new(60, 24);
        v.draw_fitted(&mut grid, &camera, 30.0, 12.0, 60, 24, 1.0);
        let plain = grid.render_plain();
        let (mut min_x, mut max_x, mut min_y, mut max_y) = (usize::MAX, 0usize, usize::MAX, 0usize);
        for (y, line) in plain.lines().enumerate() {
            for (x, ch) in line.chars().enumerate() {
                if !ch.is_whitespace() {
                    min_x = min_x.min(x);
                    max_x = max_x.max(x);
                    min_y = min_y.min(y);
                    max_y = max_y.max(y);
                }
            }
        }
        assert!(min_x != usize::MAX, "the fitted vortex drew nothing");
        assert!(max_x - min_x >= 30, "vortex only spans {max_x} of 60 columns");
        assert!(max_y - min_y >= 12, "vortex only spans {max_y} of 24 rows");
    }

    #[test]
    fn the_frame_changes_as_the_vortex_turns() {
        let mesh = Mesh::new(
            vec![
                node("a", NodeKind::Learner, LIVE, Verdict::Pass, Some(0.8)),
                node("b", NodeKind::Reasoner, LIVE, Verdict::Pass, Some(0.6)),
            ],
            vec![],
            "t".into(),
        );
        let v = Vortex::from_mesh(&mesh);
        let camera = Camera::default();
        let mut a = DepthGrid::new(100, 30);
        let mut b = DepthGrid::new(100, 30);
        v.draw_fitted(&mut a, &camera, 50.0, 15.0, 100, 30, 0.0);
        v.draw_fitted(&mut b, &camera, 50.0, 15.0, 100, 30, 2.0);
        assert_ne!(a.render_plain(), b.render_plain());
    }
}
