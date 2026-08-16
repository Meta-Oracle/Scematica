//! A character grid the graph is drawn onto, with box-drawing junctions resolved properly.
//!
//! Terminal-independent and ratatui-independent on purpose: `render.rs` blits a viewport
//! of this grid into a ratatui `Buffer`, so every drawing decision here is testable by
//! reading characters back out (see the tests at the bottom).
//!
//! ## Why lines are accumulated as bitmasks rather than written as characters
//!
//! Edges cross. If a horizontal run writes `─` over a cell where a vertical run already
//! wrote `│`, the crossing renders as a break in one of the two lines and the reader
//! follows the wrong edge. So line drawing records a direction mask per cell (N/E/S/W) and
//! only resolves masks to glyphs once, at the end — a crossing therefore becomes `┼`
//! because that is what it is.
//!
//! ## Why there are three stroke sets
//!
//! Colour is decoration, never the message. A blocking veto, an ordinary signal and an
//! *unreadable* veto have to stay distinguishable in a 16-colour terminal, through
//! `NO_COLOR`, and in a screenshot pasted into a monochrome document — so they use
//! different glyph families rather than different colours of the same glyph:
//!
//! ```text
//! Single   ─ │ ┌ ┐ └ ┘ ┼    ordinary flow
//! Double   ═ ║ ╔ ╗ ╚ ╝ ╬    an active block — the loudest thing on the canvas
//! Dashed   ╌ ╎              unreadable: this edge's state could not be determined
//! ```
//!
//! When two strokes meet on one cell the louder wins (`Double > Single > Dashed`), because
//! a crossing that hides an active block is the one error worth designing against.

use crate::view::Tone;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stroke {
    /// Lowest priority: an edge whose state could not be read.
    Dashed,
    Single,
    /// Highest priority: an active block.
    Double,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell {
    pub ch: char,
    /// `None` renders in the default foreground — plain text, not a claim about trust.
    pub tone: Option<Tone>,
    pub dim: bool,
    pub bold: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Cell { ch: ' ', tone: None, dim: false, bold: false }
    }
}

/// How something is drawn: a tone plus the two emphasis bits.
///
/// Bundled rather than passed as three positional parameters — `text(x, y, s, t, false,
/// true)` at a call site is unreadable, and the two bools are trivially swappable without
/// a compile error.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Ink {
    /// `None` is plain text — the default foreground, making no claim about trust.
    pub tone: Option<Tone>,
    pub dim: bool,
    pub bold: bool,
}

impl Ink {
    /// Ink that carries a trust claim.
    pub fn tone(tone: Tone) -> Self {
        Ink { tone: Some(tone), ..Ink::default() }
    }
    /// Ink with no tone: chrome, headings, labels — not a claim about any unit.
    pub fn plain() -> Self {
        Ink::default()
    }
    pub fn dim(mut self, dim: bool) -> Self {
        self.dim = dim;
        self
    }
    pub fn bold(mut self, bold: bool) -> Self {
        self.bold = bold;
        self
    }
    fn cell(self, ch: char) -> Cell {
        Cell { ch, tone: self.tone, dim: self.dim, bold: self.bold }
    }
}

const N: u8 = 1;
const E: u8 = 2;
const S: u8 = 4;
const W: u8 = 8;

#[derive(Clone, Copy, Default)]
struct LineCell {
    mask: u8,
    stroke: Option<Stroke>,
    tone: Option<Tone>,
    dim: bool,
}

pub struct Canvas {
    pub width: u16,
    pub height: u16,
    cells: Vec<Cell>,
    lines: Vec<LineCell>,
}

impl Canvas {
    pub fn new(width: u16, height: u16) -> Self {
        let n = width as usize * height as usize;
        Canvas {
            width,
            height,
            cells: vec![Cell::default(); n],
            lines: vec![LineCell::default(); n],
        }
    }

    fn idx(&self, x: u16, y: u16) -> Option<usize> {
        if x >= self.width || y >= self.height {
            return None;
        }
        Some(y as usize * self.width as usize + x as usize)
    }

    pub fn get(&self, x: u16, y: u16) -> Cell {
        self.idx(x, y).map(|i| self.cells[i]).unwrap_or_default()
    }

    /// Read a row back as a string. Test affordance, and the basis of `--once` rendering.
    pub fn row(&self, y: u16) -> String {
        (0..self.width).map(|x| self.get(x, y).ch).collect()
    }

    pub fn set(&mut self, x: u16, y: u16, cell: Cell) {
        if let Some(i) = self.idx(x, y) {
            self.cells[i] = cell;
        }
    }

    /// Write text left-to-right, clipped at the canvas edge.
    pub fn text(&mut self, x: u16, y: u16, s: &str, ink: Ink) {
        for (i, ch) in s.chars().enumerate() {
            let Some(cx) = x.checked_add(i as u16) else { return };
            if cx >= self.width {
                return;
            }
            self.set(cx, y, ink.cell(ch));
        }
    }

    fn add_mask(&mut self, x: u16, y: u16, bits: u8, stroke: Stroke, tone: Tone, dim: bool) {
        let Some(i) = self.idx(x, y) else { return };
        let c = &mut self.lines[i];
        c.mask |= bits;
        // Louder stroke wins the cell, and carries its tone with it: at a crossing the
        // colour must agree with the glyph, or a `╬` in signal-green claims the block is
        // routine.
        if c.stroke.is_none_or(|s| stroke > s) {
            c.stroke = Some(stroke);
            c.tone = Some(tone);
            c.dim = dim;
        }
    }

    /// Horizontal run, inclusive of both ends. Order of `x1`/`x2` does not matter.
    pub fn hline(&mut self, x1: u16, x2: u16, y: u16, stroke: Stroke, tone: Tone, dim: bool) {
        let (lo, hi) = (x1.min(x2), x1.max(x2));
        for x in lo..=hi {
            let mut bits = 0;
            if x > lo {
                bits |= W;
            }
            if x < hi {
                bits |= E;
            }
            // A single-cell run still has to be visible, so give it both sides.
            if lo == hi {
                bits = E | W;
            }
            self.add_mask(x, y, bits, stroke, tone, dim);
        }
    }

    /// Vertical run, inclusive of both ends.
    pub fn vline(&mut self, y1: u16, y2: u16, x: u16, stroke: Stroke, tone: Tone, dim: bool) {
        let (lo, hi) = (y1.min(y2), y1.max(y2));
        for y in lo..=hi {
            let mut bits = 0;
            if y > lo {
                bits |= N;
            }
            if y < hi {
                bits |= S;
            }
            if lo == hi {
                bits = N | S;
            }
            self.add_mask(x, y, bits, stroke, tone, dim);
        }
    }

    /// Resolve accumulated line masks into glyphs.
    ///
    /// Must be called once, after every edge is drawn and before any box or label is —
    /// boxes are opaque and are meant to paint over whatever passes beneath them.
    pub fn resolve_lines(&mut self) {
        for i in 0..self.cells.len() {
            let l = self.lines[i];
            if l.mask == 0 {
                continue;
            }
            let stroke = l.stroke.unwrap_or(Stroke::Single);
            self.cells[i] = Cell {
                ch: glyph(l.mask, stroke),
                tone: l.tone,
                dim: l.dim,
                bold: stroke == Stroke::Double,
            };
        }
    }

    /// A node box: border, then its interior lines. Opaque — it clears what is underneath.
    pub fn box_outline(&mut self, x: u16, y: u16, w: u16, h: u16, ink: Ink) {
        if w < 2 || h < 2 {
            return;
        }
        let (r, b) = (x + w - 1, y + h - 1);
        let c = |ch: char| ink.cell(ch);

        for cx in x..=r {
            self.set(cx, y, c('─'));
            self.set(cx, b, c('─'));
        }
        for cy in y..=b {
            self.set(x, cy, c('│'));
            self.set(r, cy, c('│'));
            // Clear the interior so an edge routed beneath does not show through.
            if cy > y && cy < b {
                for cx in (x + 1)..r {
                    self.set(cx, cy, Cell::default());
                }
            }
        }
        self.set(x, y, c('┌'));
        self.set(r, y, c('┐'));
        self.set(x, b, c('└'));
        self.set(r, b, c('┘'));
    }
}

/// Map a direction mask to a box-drawing character.
///
/// Dashed has no corner or junction glyphs in Unicode, so it falls back to the single-line
/// forms for anything that is not a straight run. That is the right trade: the dash
/// carries "unreadable" along the straights where the eye follows the line, and a corner
/// drawn solid is better than a corner drawn as a gap.
fn glyph(mask: u8, stroke: Stroke) -> char {
    let straight_h = mask == E || mask == W || mask == (E | W);
    let straight_v = mask == N || mask == S || mask == (N | S);

    match stroke {
        Stroke::Dashed if straight_h => return '╌',
        Stroke::Dashed if straight_v => return '╎',
        _ => {}
    }

    let double = stroke == Stroke::Double;
    match mask {
        m if m == N || m == S || m == (N | S) => if double { '║' } else { '│' },
        m if m == E || m == W || m == (E | W) => if double { '═' } else { '─' },
        m if m == (E | S) => if double { '╔' } else { '┌' },
        m if m == (S | W) => if double { '╗' } else { '┐' },
        m if m == (N | E) => if double { '╚' } else { '└' },
        m if m == (N | W) => if double { '╝' } else { '┘' },
        m if m == (N | E | S) => if double { '╠' } else { '├' },
        m if m == (N | S | W) => if double { '╣' } else { '┤' },
        m if m == (E | S | W) => if double { '╦' } else { '┬' },
        m if m == (N | E | W) => if double { '╩' } else { '┴' },
        m if m == (N | E | S | W) => if double { '╬' } else { '┼' },
        _ => ' ',
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_crossing_resolves_to_a_junction_not_a_break() {
        let mut c = Canvas::new(9, 5);
        c.hline(0, 8, 2, Stroke::Single, Tone::Live, false);
        c.vline(0, 4, 4, Stroke::Single, Tone::Live, false);
        c.resolve_lines();
        assert_eq!(c.get(4, 2).ch, '┼', "row: {:?}", c.row(2));
        assert_eq!(c.get(3, 2).ch, '─');
        assert_eq!(c.get(4, 1).ch, '│');
    }

    #[test]
    fn corners_are_drawn_where_runs_meet() {
        let mut c = Canvas::new(6, 4);
        // ┌ at (1,1): run east and run south from the same cell.
        c.hline(1, 4, 1, Stroke::Single, Tone::Live, false);
        c.vline(1, 3, 1, Stroke::Single, Tone::Live, false);
        c.resolve_lines();
        assert_eq!(c.get(1, 1).ch, '┌');
        assert_eq!(c.get(1, 3).ch, '│');
        assert_eq!(c.get(4, 1).ch, '─');
    }

    /// The rule that keeps a block visible when edges cross. A `┼` painted by a routine
    /// signal over an active veto would hide the single most important bit on the canvas.
    #[test]
    fn the_louder_stroke_wins_a_shared_cell() {
        let mut c = Canvas::new(7, 5);
        c.hline(0, 6, 2, Stroke::Single, Tone::Live, false);
        c.vline(0, 4, 3, Stroke::Double, Tone::Veto, false);
        c.resolve_lines();
        assert_eq!(c.get(3, 2).ch, '╬');
        assert_eq!(c.get(3, 2).tone, Some(Tone::Veto));

        // …and in the other draw order, which is the one that actually varies at runtime.
        let mut d = Canvas::new(7, 5);
        d.vline(0, 4, 3, Stroke::Double, Tone::Veto, false);
        d.hline(0, 6, 2, Stroke::Single, Tone::Live, false);
        d.resolve_lines();
        assert_eq!(d.get(3, 2).ch, '╬');
        assert_eq!(d.get(3, 2).tone, Some(Tone::Veto));
    }

    /// An unreadable edge must not look like a cleared one even with colour stripped.
    #[test]
    fn dashed_straights_survive_without_colour() {
        let mut c = Canvas::new(6, 3);
        c.hline(0, 5, 1, Stroke::Dashed, Tone::Absent, true);
        c.resolve_lines();
        assert_eq!(c.row(1), "╌╌╌╌╌╌");
    }

    #[test]
    fn dashed_falls_back_to_solid_at_a_corner() {
        let mut c = Canvas::new(5, 4);
        c.hline(1, 4, 1, Stroke::Dashed, Tone::Absent, true);
        c.vline(1, 3, 1, Stroke::Dashed, Tone::Absent, true);
        c.resolve_lines();
        assert_eq!(c.get(1, 1).ch, '┌', "a corner drawn as a gap is worse than a solid one");
        assert_eq!(c.get(2, 1).ch, '╌');
    }

    /// A box is opaque: an edge routed underneath must not show through its interior, or
    /// the picture asserts a connection into the middle of a unit.
    #[test]
    fn a_box_paints_over_whatever_runs_beneath_it() {
        let mut c = Canvas::new(12, 6);
        c.hline(0, 11, 2, Stroke::Single, Tone::Live, false);
        c.resolve_lines();
        c.box_outline(2, 1, 8, 4, Ink::tone(Tone::Live));
        assert_eq!(c.get(5, 2).ch, ' ', "row: {:?}", c.row(2));
        assert_eq!(c.get(2, 1).ch, '┌');
        assert_eq!(c.get(9, 4).ch, '┘');
        // Outside the box the line is untouched.
        assert_eq!(c.get(0, 2).ch, '─');
    }

    #[test]
    fn drawing_outside_the_canvas_is_clipped_not_panicked() {
        let mut c = Canvas::new(4, 3);
        c.hline(0, 100, 1, Stroke::Single, Tone::Live, false);
        c.vline(0, 100, 2, Stroke::Single, Tone::Live, false);
        c.text(2, 0, "overlong text", Ink::plain());
        c.box_outline(3, 2, 40, 40, Ink::tone(Tone::Live));
        c.resolve_lines();
        assert_eq!(c.width, 4);
    }

    #[test]
    fn text_is_clipped_at_the_right_edge() {
        let mut c = Canvas::new(6, 2);
        c.text(3, 0, "abcdef", Ink::plain());
        assert_eq!(c.row(0), "   abc");
    }

    #[test]
    fn a_single_cell_run_is_still_visible() {
        let mut c = Canvas::new(3, 3);
        c.hline(1, 1, 1, Stroke::Single, Tone::Live, false);
        c.resolve_lines();
        assert_eq!(c.get(1, 1).ch, '─');
    }
}
