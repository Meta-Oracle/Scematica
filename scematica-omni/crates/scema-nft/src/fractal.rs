//! The world as a fractal: a form that grows from what was observed, and stops where it
//! wasn't.
//!
//! The plate in `plate.rs` is an instrument — gauges, rings, a coverage meter. This is the
//! other thing the same data can be: a **growth**, where the shape itself is the reading.
//! Nothing here is decoration laid over numbers. Every property of the form is a quantity
//! somebody counted:
//!
//! | The form | Comes from |
//! |---|---|
//! | how deep it grows | `Extent` — how much of the entity the observer reached |
//! | how far it branches | the signal count |
//! | how wide the branches spread | the balance of risk against opportunity |
//! | how fast branches shorten | legibility — the share of objects that may be acted on |
//! | **severed limbs** | blind spots, one cut each |
//! | ghosted strokes | signals whose magnitude was estimated, not counted |
//! | the exact form | the world's own commitment, as the seed |
//!
//! ## Ignorance is a severed limb
//!
//! This is the honesty rule of the whole crate, in the language of growth rather than of
//! gauges. A blind spot does not fade a branch or tint it — it **cuts it off**, leaving a
//! stub and a gap where the rest of the structure should have been. A world nobody could
//! read grows into something visibly mutilated, and it should be uncomfortable to look at,
//! because that is an accurate report.
//!
//! The count is exact: **one cut per reported blind spot**, no more. The first version made
//! it a per-node *probability* of `blind_spots / observed`, which compounds down the
//! recursion — three reported blind spots cut twenty-six limbs. That is not a stylistic
//! error. It is the form asserting far more ignorance than the observer reported, which is
//! the same class of lie as rendering an unmeasured term as `0.00`, and it is why the cut
//! set is chosen up front by a counting pass rather than rolled at each node.
//!
//! ## Determinism, which fractals make harder rather than easier
//!
//! The output is compared **byte for byte** against `web/lib/omni/nft.ts`, so the usual way
//! to write this — floats, `sin`, a seeded float RNG — is unavailable. `sin` and `cos` are
//! not correctly rounded and may differ in the last place between runtimes, and a one-ULP
//! difference at the root of a recursion is a visibly different tree by the fourth level.
//!
//! So: integer milliunits throughout, angles at whole degrees through `geom`'s sine table,
//! and a **32-bit** xorshift seeded from the world digest. Thirty-two rather than sixty-four
//! because JavaScript numbers are doubles: `Math.imul` and `>>> 0` reproduce u32 wrapping
//! exactly, where u64 would need `BigInt` and an easy place to disagree.
//!
//! The recursion order is fixed and the RNG is consumed in that order. That is load-bearing:
//! draw the children in a different sequence and every subsequent draw takes a different
//! number, which is a different tree from the same world.

use scema_world::{Polarity, WorldState};

use crate::geom::{div_round, fmt, pt, step, Pt, UNIT, VIEW};
use crate::palette::Role;
use crate::raster::{Anchor, Prim};

/// Where the trunk starts, in milliunits.
const ROOT: Pt = Pt { x: (VIEW * UNIT) / 2, y: 492 * UNIT };
/// Trunk length before any decay.
const TRUNK: i64 = 108 * UNIT;
/// Shortest branch worth drawing. Below this the structure is noise.
const MIN_LEN: i64 = 4 * UNIT;
/// Hard ceiling on recursion, whatever the extent says.
const MAX_DEPTH: u32 = 9;
/// Hard ceiling on drawn segments, so a pathological world cannot emit a 40MB file.
const MAX_SEGMENTS: usize = 6_000;

/// A 32-bit xorshift, seeded from the world's commitment.
///
/// Deliberately small and boring. It is not cryptographic and does not need to be — its only
/// job is to make two different worlds grow visibly different forms while making the *same*
/// world grow the same one, in two languages.
pub struct Rng(u32);

impl Rng {
    /// Seed from the first eight hex characters of a digest.
    ///
    /// A zero seed is replaced: xorshift has a fixed point at zero and would return zero
    /// forever, which would silently collapse every world onto one form.
    pub fn from_digest(hex: &str) -> Rng {
        let mut seed: u32 = 0;
        for c in hex.chars().take(8) {
            seed = seed.wrapping_mul(16).wrapping_add(c.to_digit(16).unwrap_or(0));
        }
        Rng(if seed == 0 { 0x9E37_79B9 } else { seed })
    }

    /// The next 32-bit value.
    ///
    /// `#[allow]` rather than a rename: the TypeScript port has this method too, and a name
    /// that differs between the two is one more thing a reader has to hold in their head
    /// while checking that the two implementations agree.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// A value in `[0, n)`. `n == 0` yields 0 rather than dividing by zero.
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            self.next() % n
        }
    }

    /// A signed jitter in `[-spread, spread]`.
    pub fn jitter(&mut self, spread: i64) -> i64 {
        if spread <= 0 {
            return 0;
        }
        self.below((spread * 2 + 1) as u32) as i64 - spread
    }
}

/// The shape parameters this world implies.
///
/// Extracted before drawing so they can be tested on their own — the mapping from a world to
/// a form is the part that has to be *right*, and it is much easier to be sure of when it is
/// not tangled up with path strings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Growth {
    /// Recursion depth.
    pub depth: u32,
    /// Children per node.
    pub arity: u32,
    /// Half-angle between outermost children, in degrees.
    pub spread: i64,
    /// Length decay per level, as a percentage.
    pub decay: i64,
    /// How many limbs to cut. **Exactly** the number of blind spots the observer reported,
    /// bounded by how many limbs there are to cut.
    pub cuts: usize,
    /// True when the extent's denominator is unknown: the frontier is drawn dashed, because
    /// nobody knows how much further the structure goes.
    pub unbounded: bool,
}

/// Read the growth parameters out of a world.
///
/// Every number below traces to something counted. There is no aesthetic constant that is
/// secretly doing the work, which is the difference between a visualisation and a
/// decoration with data sprinkled on it.
pub fn growth_of(w: &WorldState) -> Growth {
    let signals = w.signals.len() as u32;
    let risks = w.risks().count() as i64;
    let opportunities = w.opportunities().count() as i64;

    // Depth from extent. An unbounded extent grows to full depth — the observer did not say
    // it had reached the end, so cutting the form short would assert something it did not.
    let depth = match w.extent.fraction() {
        None => MAX_DEPTH,
        Some(f) => {
            let d = 3.0 + f * (MAX_DEPTH as f64 - 3.0);
            (d.round() as u32).clamp(3, MAX_DEPTH)
        }
    };

    // Two children is a tree; three is a thicket. More signals earn more branching, and a
    // world with none stays a bare fork rather than pretending to complexity.
    let arity = if signals >= 12 {
        4
    } else if signals >= 5 {
        3
    } else {
        2
    };

    // Risk splays the form outward, opportunity draws it upright. A world of pure risk
    // sprawls; a world of pure opportunity reaches. Neutral when there is nothing counted —
    // not "balanced", just the base angle, because no signals is not a measurement of
    // equilibrium.
    let total = (risks + opportunities).max(1);
    let risk_share = div_round(risks * 100, total);
    let spread = 18 + div_round(risk_share * 22, 100);

    // Legibility slows the decay: a world you can act on holds its length further out.
    let decay = 66 + (w.legibility() * 10.0).round() as i64;

    Growth {
        depth,
        arity,
        spread,
        decay,
        // One cut per blind spot. Not a rate — a count.
        //
        // The first version made this a per-node probability of `blind_spots / observed`,
        // which compounds down the recursion: three reported blind spots cut twenty-six
        // limbs. That is not a stylistic error, it is the form asserting far more ignorance
        // than the observer reported, which is precisely the class of lie the rest of this
        // crate exists to prevent. A count maps one to one and cannot inflate.
        cuts: w.blind_spots.len(),
        unbounded: w.extent.fraction().is_none(),
    }
}

/// Which single level the cuts land on, and how many nodes it holds.
///
/// **One level, not a range**, and that is the fix for a subtle miscount. A cut removes its
/// own descendants, so if cuts could land at different depths one might sit inside another
/// and never be reached — three blind spots then rendered as two limbs cut. Confining every
/// cut to one level makes nesting impossible and the count exact.
///
/// The shallowest level with **room to spare** is chosen: a cut nearer the trunk removes
/// more and is therefore more visible, but the level must hold at least `SURVIVAL` times as
/// many nodes as there are cuts.
///
/// Without that headroom the form can be annihilated. Three blind spots on an arity-3 tree
/// fit exactly on level one — and cutting all three level-one limbs deletes the entire
/// canopy, rendering "three of six objects were unreadable" as "nothing was observed at
/// all". A blind spot removes a portion of the world, never the world.
fn cut_level(g: &Growth) -> (u32, usize) {
    /// How many nodes a level must hold per cut. Three, so at most a third of the structure
    /// at that level is removed and the rest visibly survives.
    const SURVIVAL: usize = 3;

    // The search runs the whole depth rather than stopping at `CUT_LEVELS`, and prefers the
    // shallowest level that fits. Shallow is more visible, so it wins when it can — but a
    // world with many blind spots needs somewhere to put them, and many small voids deep in
    // the canopy is an accurate picture where two large ones capped at a shallow level is
    // not.
    let deepest = g.depth.saturating_sub(1).max(1);
    for l in 1..=deepest {
        let size = (g.arity as usize).saturating_pow(l.min(20));
        if size >= g.cuts.saturating_mul(SURVIVAL) {
            return (l, size);
        }
    }
    (deepest, (g.arity as usize).saturating_pow(deepest.min(20)))
}

/// How many limbs will actually be cut, and whether that is fewer than were reported.
///
/// The cap exists because the deepest level a void can still be seen on holds only so many
/// nodes. When it bites, the footer says so — a form that quietly cut fewer limbs than there
/// were blind spots would understate exactly the thing it is meant to show.
fn planned_cuts(g: &Growth) -> (usize, bool) {
    let (_, size) = cut_level(g);
    // At most a third of the level, matching `SURVIVAL`. Beyond that the structure at that
    // level stops reading as a structure with holes in it and starts reading as debris.
    let room = (size / 3).max(1);
    let n = g.cuts.min(room);
    (n, n < g.cuts)
}

/// Choose which nodes to cut.
///
/// Never node 0 — severing the trunk deletes the whole form, which would report "one blind
/// spot" as "nothing was observed at all". Deterministic given the seed, and it returns at
/// most as many cuts as there are nodes to cut.
fn cut_set(g: &Growth, rng: &mut Rng) -> Vec<usize> {
    let (_, nodes) = cut_level(g);
    let (want, _) = planned_cuts(g);
    let mut chosen: Vec<usize> = Vec::with_capacity(want);
    let mut guard = 0;
    while chosen.len() < want && guard < want * 16 + 64 {
        let pick = rng.below(nodes as u32) as usize;
        if !chosen.contains(&pick) {
            chosen.push(pick);
        }
        guard += 1;
    }
    chosen.sort_unstable();
    chosen
}

struct Canvas<'a> {
    out: String,
    rng: Rng,
    g: Growth,
    world: &'a WorldState,
    segments: usize,
    /// Terminal marks, drawn after the branches so they sit on top.
    tips: Vec<(Pt, usize)>,
    severed: usize,
    /// The same drawing, as primitives.
    ///
    /// Accumulated alongside the SVG string rather than replacing it, so the raster backend
    /// walks the identical growth without the SVG output changing by a byte — which the
    /// parity fixture would catch immediately if it did.
    prims: Vec<Prim>,
    /// Positions-within-level to cut, chosen before drawing.
    cuts: Vec<usize>,
    /// The one level cuts land on.
    cut_at: u32,
}

impl Canvas<'_> {
    fn line(&mut self, a: Pt, b: Pt, width_mu: i64, role: Role, dashed: bool) {
        if self.segments >= MAX_SEGMENTS {
            return;
        }
        self.segments += 1;
        self.prims.push(Prim::Line {
            a: (a.x, a.y),
            b: (b.x, b.y),
            width_mu: width_mu.max(400),
            role,
            dashed,
        });
        let dash = if dashed { " stroke-dasharray=\"3 4\"" } else { "" };
        self.out.push_str(&format!(
            "<path d=\"M {} L {}\" stroke=\"{}\" stroke-width=\"{}\" stroke-linecap=\"round\" fill=\"none\"{dash}/>",
            pt(a),
            pt(b),
            role.hex(),
            fmt(width_mu.max(400))
        ));
    }

    /// Grow one branch, then its children.
    ///
    /// Depth-first, children in a fixed order, RNG consumed as encountered. Reordering any
    /// of that changes every subsequent draw and produces a different tree from the same
    /// world — which would break byte-parity and, worse, make the form stop being a function
    /// of the world.
    fn branch(&mut self, from: Pt, angle: i64, len: i64, depth: u32, pos: usize) {
        if depth == 0 || len < MIN_LEN || self.segments >= MAX_SEGMENTS {
            self.tips.push((from, self.tips.len()));
            return;
        }

        // A node's identity for cutting is its *position within its level*, threaded down
        // the recursion — not a visit counter. A visit counter shifts when a cut removes a
        // subtree, so later indices point at different nodes than the ones chosen, which is
        // how three blind spots came out as two limbs cut.
        let level = self.g.depth - depth;
        let me = (level == self.cut_at).then_some(pos);

        // A blind spot cuts the limb off here. The stub is drawn — the observer did reach
        // this far — and then nothing, which is the point: the missing structure is the
        // report.
        if me.is_some_and(|i| self.cuts.binary_search(&i).is_ok()) {
            let stub = step(from, len / 3, angle);
            self.line(from, stub, depth as i64 * 420, Role::Absent, true);
            self.severed += 1;
            return;
        }

        let jitter = self.rng.jitter(4);
        let to = step(from, len, angle + jitter);

        // The frontier of an unbounded extent is dashed: the observer did not say where the
        // structure ends, so its outermost growth is drawn as unknown rather than as
        // finished.
        let frontier = self.g.unbounded && depth == 1;
        let role = if frontier { Role::Stale } else { Role::Measured };
        self.line(from, to, depth as i64 * 340, role, frontier);

        let arity = self.g.arity as i64;
        let next_len = div_round(len * self.g.decay, 100);
        for i in 0..arity {
            // Symmetric spread: children fan from -spread to +spread.
            let offset = if arity == 1 {
                0
            } else {
                -self.g.spread + div_round(2 * self.g.spread * i, arity - 1)
            };
            self.branch(to, angle + offset, next_len, depth - 1, pos * arity as usize + i as usize);
        }
    }

    /// A mark per counted signal, placed on the tips the growth produced.
    ///
    /// Risk is a triangle, opportunity a disc — shape carries the message, as everywhere
    /// else here. An estimated magnitude is hollow, because a guess must not draw as a
    /// count. There are usually more tips than signals; the surplus stay bare rather than
    /// being given marks nobody measured.
    fn marks(&mut self) {
        let tips = self.tips.clone();
        if tips.is_empty() {
            return;
        }
        let n = self.world.signals.len().max(1);
        for (i, sig) in self.world.signals.iter().enumerate() {
            // Spread across the canopy rather than clustered: evenly spaced positions in the
            // tip list, which is itself in a fixed traversal order.
            let Some((p, _)) = tips.get(i * tips.len() / n) else { continue };
            let role = match sig.polarity {
                Polarity::Risk => Role::Risk,
                Polarity::Opportunity => Role::Opportunity,
            };
            let fill = if sig.measured { role.hex() } else { "none".to_string() };
            match sig.polarity {
                Polarity::Opportunity => {
                    self.prims.push(Prim::Disc {
                        c: (p.x, p.y),
                        r_mu: 2_600,
                        role,
                        filled: sig.measured,
                    });
                    self.out.push_str(&format!(
                    "<circle cx=\"{}\" cy=\"{}\" r=\"2.6\" fill=\"{fill}\" stroke=\"{}\" stroke-width=\"1.2\"/>",
                    fmt(p.x),
                    fmt(p.y),
                    role.hex()
                ))
                }
                Polarity::Risk => {
                    let a = Pt { x: p.x, y: p.y - 3 * UNIT };
                    let b = Pt { x: p.x - 2600, y: p.y + 1800 };
                    let c = Pt { x: p.x + 2600, y: p.y + 1800 };
                    self.prims.push(Prim::Tri {
                        a: (a.x, a.y),
                        b: (b.x, b.y),
                        c: (c.x, c.y),
                        role,
                        filled: sig.measured,
                    });
                    self.out.push_str(&format!(
                        "<path d=\"M {} L {} L {} Z\" fill=\"{fill}\" stroke=\"{}\" stroke-width=\"1.2\"/>",
                        pt(a),
                        pt(b),
                        pt(c),
                        role.hex()
                    ));
                }
            }
        }
    }
}

/// Draw a world as a fractal growth.
///
/// `digest_hex` seeds the form and is printed on it. It is never recomputed here — a picture
/// that derived its own commitment could not be used to check anything.
pub fn render(world: &WorldState, digest_hex: &str) -> String {
    scene(world, digest_hex).0
}

/// The same growth as a PNG.
///
/// Rasterised from the identical primitive list the SVG is built from, so the two cannot
/// depict different trees. See `raster` for why the rasteriser and the PNG encoder are
/// written here rather than taken from a library.
pub fn render_png(world: &WorldState, digest_hex: &str, size: usize) -> Vec<u8> {
    let (_, prims) = scene(world, digest_hex);
    crate::raster::render_png(&prims, VIEW, size, Role::Ground)
}

/// Draw the growth once, returning both renderings' inputs.
fn scene(world: &WorldState, digest_hex: &str) -> (String, Vec<Prim>) {
    let g = growth_of(world);
    let mut rng = Rng::from_digest(digest_hex);
    let cuts = cut_set(&g, &mut rng);
    let (cut_at, _) = cut_level(&g);
    let mut c = Canvas {
        out: String::with_capacity(16 * 1024),
        rng,
        g,
        world,
        segments: 0,
        tips: Vec::new(),
        severed: 0,
        prims: Vec::new(),
        cuts,
        cut_at,
    };

    c.branch(ROOT, 0, TRUNK, g.depth, 0);
    c.marks();

    let mut s = String::with_capacity(c.out.len() + 2048);
    s.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {VIEW} {VIEW}\" width=\"{VIEW}\" height=\"{VIEW}\" role=\"img\" aria-label=\"{}\">",
        crate::plate::esc(&format!(
            "Scematica Omni world growth for {} ({}), observed by {}",
            world.entity.label,
            world.entity.kind.as_str(),
            world.observer
        ))
    ));
    s.push_str(&format!(
        "<rect x=\"0\" y=\"0\" width=\"{VIEW}\" height=\"{VIEW}\" fill=\"{}\"/>",
        Role::Ground.hex()
    ));
    s.push_str(&format!(
        "<rect x=\"8\" y=\"8\" width=\"{}\" height=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"1\"/>",
        VIEW - 16,
        VIEW - 16,
        Role::Frame.hex()
    ));

    // The frame, as primitives too. It is chrome rather than data, but the claim this
    // module makes is that the two renderings depict the same thing — and a border present
    // in one and absent in the other is a small, avoidable way for that to be false.
    let (lo, hi) = (8 * UNIT, (VIEW - 8) * UNIT);
    for (a, b) in [
        ((lo, lo), (hi, lo)),
        ((hi, lo), (hi, hi)),
        ((hi, hi), (lo, hi)),
        ((lo, hi), (lo, lo)),
    ] {
        c.prims.push(Prim::Line { a, b, width_mu: 1_000, role: Role::Frame, dashed: false });
    }

    // The growth itself.
    s.push_str(&c.out);

    // The legend is small and factual. A form this suggestive needs its terms stated, or a
    // viewer reads whatever they already believed into it — which is also why it is built
    // through `text_pair`: the SVG element and the raster primitive are produced together,
    // so a legend cannot appear in one rendering and not the other.
    let extent = match world.extent.total {
        Some(t) => format!("EXTENT {}/{}", world.extent.observed, t),
        None => format!("EXTENT {} · UNBOUNDED", world.extent.observed),
    };
    let (planned, capped) = planned_cuts(&g);
    let _ = planned;
    let blind = if world.blind_spots.is_empty() {
        "NO BLIND SPOTS".to_string()
    } else if capped {
        format!(
            "{} BLIND SPOT(S) · {} LIMB(S) CUT (CAPPED)",
            world.blind_spots.len(),
            c.severed
        )
    } else {
        format!("{} BLIND SPOT(S) · {} LIMB(S) CUT", world.blind_spots.len(), c.severed)
    };
    let measured = world.signals.iter().filter(|s| s.measured).count();
    let coverage = if world.signals.is_empty() {
        "COVERAGE ∅".to_string()
    } else {
        format!("COVERAGE {}/{}", measured, world.signals.len())
    };

    let legend: Vec<(i64, i64, i64, Role, Anchor, String)> = vec![
        (24, 34, 17, Role::Heading, Anchor::Start, crate::plate::truncate(&world.entity.label, 30)),
        (
            24,
            52,
            10,
            Role::Label,
            Anchor::Start,
            format!(
                "{} · {} · {}",
                world.entity.kind.as_str(),
                world.domain.as_str(),
                crate::plate::truncate(&world.observer, 28)
            ),
        ),
        (24, 460, 10, Role::Label, Anchor::Start, extent),
        (
            VIEW - 24,
            460,
            10,
            if world.blind_spots.is_empty() { Role::Label } else { Role::Absent },
            Anchor::End,
            blind,
        ),
        (24, 478, 10, Role::Label, Anchor::Start, coverage),
        (
            VIEW - 24,
            478,
            10,
            Role::Label,
            Anchor::End,
            format!("depth {} · arity {} · spread {}°", g.depth, g.arity, g.spread),
        ),
        (
            24,
            496,
            10,
            Role::Claim,
            Anchor::Start,
            format!("world {}", crate::plate::short_digest(digest_hex)),
        ),
    ];
    for (x, y, size, role, anchor, body) in legend {
        let (el, prim) = text_pair(x, y, size, role, anchor, &body);
        s.push_str(&el);
        c.prims.push(prim);
    }

    s.push_str("</svg>");
    (s, c.prims)
}

/// A text element, and the primitive that mirrors it.
///
/// Returned as a pair so the caller cannot emit one without the other — a legend that
/// appeared in the SVG and not in the PNG would leave the raster looking like a picture
/// rather than a reading.
fn text_pair(x: i64, y: i64, size: i64, role: Role, anchor: Anchor, body: &str) -> (String, Prim) {
    let a = match anchor {
        Anchor::Start => "start",
        Anchor::Middle => "middle",
        Anchor::End => "end",
    };
    (
        text(x, y, size, role, a, body),
        Prim::Text { at: (x * UNIT, y * UNIT), size, role, anchor, body: body.to_string() },
    )
}

fn text(x: i64, y: i64, size: i64, role: Role, anchor: &str, body: &str) -> String {
    format!(
        "<text x=\"{x}\" y=\"{y}\" font-family=\"ui-monospace,SFMono-Regular,Menlo,Consolas,monospace\" font-size=\"{size}\" fill=\"{}\" text-anchor=\"{anchor}\">{}</text>",
        role.hex(),
        crate::plate::esc(body)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{empty_world, parity_world, rich_world, unbounded_world};

    #[test]
    fn the_same_world_grows_the_same_form() {
        let w = parity_world();
        assert_eq!(render(&w, "abcdef0123456789"), render(&w, "abcdef0123456789"));
    }

    #[test]
    fn a_different_commitment_grows_a_different_form() {
        // The seed is the world's own digest, so two worlds are visually distinct — and the
        // same world never is.
        let w = parity_world();
        assert_ne!(render(&w, "1111111111111111"), render(&w, "2222222222222222"));
    }

    #[test]
    fn a_zero_seed_does_not_collapse_the_rng() {
        // xorshift has a fixed point at zero and would return zero forever, quietly
        // collapsing every world onto one form.
        let mut r = Rng::from_digest("00000000");
        let a = r.next();
        let b = r.next();
        assert_ne!(a, 0);
        assert_ne!(a, b);
    }

    #[test]
    fn an_unbounded_extent_grows_to_full_depth_and_says_so() {
        // The observer did not say it had reached the end, so cutting the form short would
        // assert something it never claimed.
        let g = growth_of(&unbounded_world());
        assert_eq!(g.depth, MAX_DEPTH);
        assert!(g.unbounded);
        assert!(render(&unbounded_world(), "aa").contains("UNBOUNDED"));
    }

    #[test]
    fn a_partly_observed_world_grows_shallower_than_a_fully_observed_one() {
        let mut shallow = rich_world();
        shallow.extent = scema_world::Extent { observed: 1, total: Some(100), note: "x".into() };
        let mut deep = rich_world();
        deep.extent = scema_world::Extent { observed: 100, total: Some(100), note: "x".into() };
        assert!(growth_of(&shallow).depth < growth_of(&deep).depth);
    }

    #[test]
    fn blind_spots_sever_exactly_as_many_limbs_as_were_reported() {
        // Not a rate. A per-node probability compounds down the recursion — three reported
        // blind spots cut twenty-six limbs in the first version — which is the form claiming
        // more ignorance than the observer did.
        let clean = growth_of(&rich_world());
        assert_eq!(clean.cuts, 0);

        let mut holed = rich_world();
        holed.blind_spots = (0..2).map(|i| format!("spot {i}")).collect();
        holed.extent = scema_world::Extent { observed: 4, total: Some(4), note: "x".into() };
        // Exactly two blind spots, exactly two cuts. Not a rate that compounds.
        assert_eq!(growth_of(&holed).cuts, 2);

        let svg = render(&holed, "abcdef0123456789");
        assert!(svg.contains("LIMB(S) CUT"), "a severed form must say so");
    }

    #[test]
    fn risk_splays_the_form_and_opportunity_keeps_it_upright() {
        let mut risky = rich_world();
        risky.signals.retain(|s| s.polarity == Polarity::Risk);
        let mut hopeful = rich_world();
        hopeful.signals.retain(|s| s.polarity == Polarity::Opportunity);
        assert!(growth_of(&risky).spread > growth_of(&hopeful).spread);
    }

    #[test]
    fn a_world_with_no_signals_stays_a_bare_fork() {
        // No signals is not a measurement of equilibrium, so the form does not pretend to
        // complexity it has no basis for.
        let mut bare = rich_world();
        bare.signals.clear();
        assert_eq!(growth_of(&bare).arity, 2);
    }

    #[test]
    fn more_signals_earn_more_branching() {
        let mut w = rich_world();
        let one = w.signals[0].clone();
        w.signals = (0..12).map(|_| one.clone()).collect();
        assert_eq!(growth_of(&w).arity, 4);
    }

    #[test]
    fn an_empty_world_still_renders_and_says_it_is_empty() {
        let svg = render(&empty_world(), "aa");
        assert!(svg.starts_with("<svg "));
        assert!(svg.ends_with("</svg>"));
        assert!(svg.contains("COVERAGE ∅"));
    }

    #[test]
    fn an_estimated_signal_is_drawn_hollow() {
        let mut w = rich_world();
        w.signals[0].measured = true;
        let counted = render(&w, "abcdef0123456789");
        w.signals[0].measured = false;
        let guessed = render(&w, "abcdef0123456789");
        assert_ne!(counted, guessed, "a guess must not draw as a count");
    }

    #[test]
    fn a_pathological_world_cannot_emit_an_unbounded_file() {
        // Depth and arity are both data-driven, so their product has to be capped somewhere.
        let mut w = rich_world();
        let one = w.signals[0].clone();
        w.signals = (0..64).map(|_| one.clone()).collect();
        w.extent = scema_world::Extent::partial(1, "unbounded");
        let svg = render(&w, "abcdef0123456789");
        assert!(svg.len() < 2_000_000, "svg was {} bytes", svg.len());
    }

    #[test]
    fn a_hostile_label_cannot_inject_markup() {
        let mut w = rich_world();
        w.entity.label = "</svg><script>alert(1)</script>".into();
        let svg = render(&w, "aa");
        assert!(!svg.contains("<script"));
        assert_eq!(svg.matches("</svg>").count(), 1);
    }

    #[test]
    fn the_number_of_cuts_never_exceeds_the_number_of_blind_spots() {
        // The property the first version violated. Checked across seeds, because the cut set
        // is chosen by the RNG and a bug here would show up on some seeds and not others.
        for seed in ["00000000", "deadbeef", "12345678", "ffffffff", "0a0b0c0d"] {
            let mut w = rich_world();
            w.blind_spots = (0..3).map(|i| format!("spot {i}")).collect();
            let svg = render(&w, seed);
            // `stroke=`, not just the hex: the footer's blind-spot count uses the same
            // role as `fill=`, and counting both makes three cuts look like four.
            let needle = format!("stroke=\"{}\"", Role::Absent.hex());
            let cut = svg.matches(&needle).count();
            assert!(cut <= 3, "seed {seed}: {cut} cuts for 3 blind spots");
        }
    }

    #[test]
    fn the_trunk_is_never_cut() {
        // Severing node 0 deletes the whole form, which would report "one blind spot" as
        // "nothing was observed at all".
        let mut w = rich_world();
        w.blind_spots = (0..40).map(|i| format!("spot {i}")).collect();
        let svg = render(&w, "deadbeef");
        assert!(svg.contains(&Role::Measured.hex()), "some growth must survive");
    }

    #[test]
    fn the_cut_count_is_exact_across_seeds() {
        // Confining every cut to one level makes nesting impossible, so the number of limbs
        // cut equals the number of blind spots. Before that, a cut chosen inside another
        // cut's subtree was never reached and three blind spots rendered as two.
        for seed in ["00000000", "deadbeef", "12345678", "ffffffff", "0a0b0c0d", "5eed5eed"] {
            for n in [1usize, 2, 3, 5] {
                let mut w = rich_world();
                w.blind_spots = (0..n).map(|i| format!("spot {i}")).collect();
                let svg = render(&w, seed);
                let needle = format!("stroke=\"{}\"", Role::Absent.hex());
                assert_eq!(
                    svg.matches(&needle).count(),
                    n,
                    "seed {seed}, {n} blind spot(s)"
                );
                assert!(svg.contains(&format!("{n} LIMB(S) CUT")));
            }
        }
    }

    #[test]
    fn cutting_never_annihilates_the_form() {
        // Three blind spots on an arity-3 tree fit exactly on level one, and cutting all
        // three level-one limbs deletes the canopy — rendering "three of six objects were
        // unreadable" as "nothing was observed at all". The cut level must have headroom.
        for n in [1usize, 2, 3, 4, 6, 9] {
            let mut w = rich_world();
            w.blind_spots = (0..n).map(|i| format!("spot {i}")).collect();
            let svg = render(&w, "deadbeef");
            let (_, size) = cut_level(&growth_of(&w));
            let (cuts, _) = planned_cuts(&growth_of(&w));
            assert!(cuts * 3 <= size * 2, "{n} blind spots removed too much of the level");
            let grown = svg.matches(&format!("stroke=\"{}\"", Role::Measured.hex())).count();
            assert!(grown > 20, "{n} blind spots left only {grown} live branches");
        }
    }

    #[test]
    fn the_png_is_deterministic_and_depicts_the_same_growth() {
        // Rasterised from the identical primitive list the SVG is built from, so the two
        // cannot show different trees.
        let w = parity_world();
        let d = crate::world_digest(&w);
        let a = render_png(&w, &d, 128);
        let b = render_png(&w, &d, 128);
        assert_eq!(a, b);
        assert_eq!(&a[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
    }

    #[test]
    fn a_different_world_produces_a_different_png() {
        let a = parity_world();
        let mut b = parity_world();
        b.blind_spots.push("one more".into());
        assert_ne!(
            render_png(&a, &crate::world_digest(&a), 128),
            render_png(&b, &crate::world_digest(&b), 128)
        );
    }

    #[test]
    fn the_svg_and_the_png_carry_the_same_legend() {
        // `text_pair` produces the element and the primitive together, so a legend line
        // cannot reach one rendering and not the other. Asserted because that pairing is
        // easy to bypass by adding a bare `text(...)` call later.
        let w = parity_world();
        let d = crate::world_digest(&w);
        let (svg, prims) = scene(&w, &d);
        let texts: Vec<&Prim> = prims.iter().filter(|p| matches!(p, Prim::Text { .. })).collect();
        assert_eq!(texts.len(), svg.matches("<text").count());
        assert!(texts.len() >= 7, "the legend lost lines: {}", texts.len());
    }
}
