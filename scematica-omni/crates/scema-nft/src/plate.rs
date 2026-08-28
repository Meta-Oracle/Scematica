//! The plate: one `WorldState`, drawn.
//!
//! ## What this is trying to be
//!
//! Not decoration with data sprinkled on it. The plate is an **instrument**, in the same
//! sense as Scylar's sigil and the console's coverage meter: every mark on it is a
//! measurement or the absence of one, and a viewer who learns four rules can read a world
//! off it without a legend. The rules are the ones this whole workspace is built on, and
//! here they become shapes:
//!
//! 1. **An unmeasured gauge must not look like a measured zero.** The em-dash rule in
//!    vector form. A gauge nobody measured draws its **full sweep, dashed**, and labels
//!    itself with an em dash; a gauge measured at zero draws **nothing at all** and labels
//!    itself `0.00`. Different shape, different text. The trap is that both would otherwise
//!    be a zero-length arc, which is to say the same picture.
//! 2. **Ignorance is a hole, not an absence of ink.** Every blind spot the observer
//!    reported cuts a visible notch through the extent ring. A world nobody could read is
//!    not a clean plate — it is a perforated one, and it should be uncomfortable to look
//!    at.
//! 3. **Coverage is one cell per term, never a proportional bar.** A bar renders 2/5 and
//!    4/10 identically and the denominator is the number that matters.
//! 4. **Shape carries the message; colour agrees with it.** Risk is a triangle, opportunity
//!    is a disc, estimated is hollow, absent is dashed. Render the plate in greyscale and
//!    nothing is lost. This is the SVG form of the console's "colour is decoration, never
//!    the message" — with more force, because an SVG has no `NO_COLOR` and no fallback: it
//!    will be rendered once, by a wallet, at a size nobody consulted us about.
//!
//! ## The one thing it deliberately does not do
//!
//! It does not score the world. There is no beauty function, no rarity roll, no aggregate
//! "quality" out of ten. Every quantity drawn is one the observer actually reported, and
//! the plate's only editorial act is choosing what to put where. A generated trait that
//! ranked worlds against each other would be a number of exactly the right shape with
//! nothing behind it — which is the failure mode this repository has spent fourteen
//! versions building machinery to refuse.

use scema_world::{Polarity, Provenance, WorldState};

use crate::geom::{
    arc_path, div_round, fmt, polar, pt, scale, spoke_path, CENTER, UNIT, VIEW,
};
use crate::palette::Role;

// ── layout ────────────────────────────────────────────────────────────────────
//
// All radii in milliunits. The dial is centred on the plate; the text bands sit above and
// below it, and `layout_bands_do_not_overlap_the_dial` pins that they do not collide.

const R_EXTENT: i64 = 180 * UNIT;
const R_NOTCH_IN: i64 = 168 * UNIT;
const R_NOTCH_OUT: i64 = 192 * UNIT;
const R_SPOKE_IN: i64 = 100 * UNIT;
const R_SPOKE_MAX: i64 = 158 * UNIT;
const R_PROVENANCE: i64 = 88 * UNIT;
const R_LEGIBILITY_MAX: i64 = 58 * UNIT;

// The dial's bounding box. Nothing draws with these — they exist so the layout constraint
// is stated as an expression the test can check, rather than as a comment claiming the
// numbers below were chosen carefully. Text over the ring is unreadable at thumbnail size,
// which is the size this will usually be seen at.
/// Top of the dial's bounding box, in milliunits — the lowest any header text may sit.
#[allow(dead_code)]
const DIAL_TOP: i64 = CENTER - R_NOTCH_OUT;
/// Bottom of the dial's bounding box — the highest any footer text may sit.
#[allow(dead_code)]
const DIAL_BOTTOM: i64 = CENTER + R_NOTCH_OUT;

const Y_TITLE: i64 = 34 * UNIT;
const Y_SUBTITLE: i64 = 52 * UNIT;
const Y_FOOT_1: i64 = 460 * UNIT;
const Y_FOOT_2: i64 = 478 * UNIT;
const Y_FOOT_3: i64 = 496 * UNIT;

const X_MARGIN: i64 = 24 * UNIT;

/// Most signals drawn — as spokes on the ring, and as cells in the footer.
///
/// **One cap for both**, deliberately, so that a single disclosure covers the whole plate.
/// Two caps would mean two numbers to state and a reader who counted spokes and cells and
/// got different answers from the same picture.
///
/// Past this the ring is a solid disc and tells the reader nothing. Whenever it bites, the
/// footer says so: truncating silently would emit a wrong count, which is the one thing
/// this workspace does not do — the same rule as the tail line on `render::signals_capped`.
const MAX_SIGNALS: usize = 32;
/// Most notches drawn. Disclosed the same way.
const MAX_NOTCHES: usize = 32;

/// Draw a world.
///
/// `digest_hex` is the world's canonical commitment, computed by the caller so this module
/// stays free of `scema-verify` and testable on a hand-built world. It is rendered verbatim
/// and never recomputed here — a picture that derives its own digest could not be used to
/// check anything.
pub fn render(world: &WorldState, digest_hex: &str) -> String {
    let mut s = String::with_capacity(8 * 1024);

    s.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {VIEW} {VIEW}\" width=\"{VIEW}\" height=\"{VIEW}\" role=\"img\" aria-label=\"{}\">",
        esc(&aria_label(world))
    ));

    // Ground and frame.
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

    s.push_str(&header(world));
    s.push_str(&extent_ring(world));
    s.push_str(&blind_spot_notches(world));
    s.push_str(&signal_spokes(world));
    s.push_str(&provenance_ring(world));
    s.push_str(&legibility_core(world));
    s.push_str(&footer(world, digest_hex));

    s.push_str("</svg>");
    s
}

// ── header ────────────────────────────────────────────────────────────────────

fn header(w: &WorldState) -> String {
    let mut s = String::new();
    s.push_str(&text(
        X_MARGIN,
        Y_TITLE,
        18,
        Role::Heading,
        "start",
        &truncate(&w.entity.label, 30),
    ));
    s.push_str(&text(
        X_MARGIN,
        Y_SUBTITLE,
        10,
        Role::Label,
        "start",
        &format!(
            "{} · {} · {}",
            w.entity.kind.as_str(),
            w.domain.as_str(),
            truncate(&w.observer, 28)
        ),
    ));
    s
}

// ── the extent ring ───────────────────────────────────────────────────────────

/// How much of the entity the observer reached.
///
/// The rule that shapes this function: `Extent { total: None }` is **not** a zero and not a
/// full ring. It means the observer does not know the denominator — a depth-limited walk, a
/// paginated API, a partially rendered page — so the ring draws its whole sweep **dashed**
/// and the footer says `UNBOUNDED`. Drawing a complete solid ring there would claim total
/// coverage, which is the exact inverse of what the producer said.
fn extent_ring(w: &WorldState) -> String {
    let mut s = String::new();

    // The track is always drawn, so the ring is legible as a ring even at zero.
    s.push_str(&format!(
        "<circle cx=\"{c}\" cy=\"{c}\" r=\"{r}\" fill=\"none\" stroke=\"{}\" stroke-width=\"5\"/>",
        Role::Chrome.hex(),
        c = fmt(CENTER),
        r = fmt(R_EXTENT)
    ));

    match w.extent.fraction() {
        None => {
            // Unmeasured denominator: full sweep, dashed, in the staleness role.
            s.push_str(&format!(
                "<circle cx=\"{c}\" cy=\"{c}\" r=\"{r}\" fill=\"none\" stroke=\"{}\" stroke-width=\"5\" stroke-dasharray=\"6 8\" stroke-linecap=\"butt\"/>",
                Role::Stale.hex(),
                c = fmt(CENTER),
                r = fmt(R_EXTENT)
            ));
        }
        Some(f) => {
            let sweep = scale(f, 360);
            let d = arc_path(R_EXTENT, 0, sweep);
            if !d.is_empty() {
                s.push_str(&format!(
                    "<path d=\"{d}\" fill=\"none\" stroke=\"{}\" stroke-width=\"5\" stroke-linecap=\"butt\"/>",
                    Role::Measured.hex()
                ));
            }
        }
    }
    s
}

/// One notch per blind spot, cut through the extent ring.
///
/// Deliberately drawn *over* the ring rather than beside it. A blind spot is not an
/// annotation on the observation, it is a piece missing from it, and the picture should say
/// so without a caption.
fn blind_spot_notches(w: &WorldState) -> String {
    if w.blind_spots.is_empty() {
        return String::new();
    }
    let shown = w.blind_spots.len().min(MAX_NOTCHES);
    let mut s = String::new();
    for i in 0..shown {
        let deg = div_round(360 * i as i64, shown as i64);
        let d = spoke_path(R_NOTCH_IN, R_NOTCH_OUT, deg);
        s.push_str(&format!(
            "<path d=\"{d}\" stroke=\"{}\" stroke-width=\"3\" stroke-dasharray=\"3 3\"/>",
            Role::Absent.hex()
        ));
    }
    s
}

// ── signals ───────────────────────────────────────────────────────────────────

/// One spoke per counted signal.
///
/// Polarity is a **shape**: a risk ends in a triangle, an opportunity in a disc. Whether the
/// magnitude was counted or estimated is also a shape — a counted signal is solid and its
/// cap is filled, an estimated one is dashed and its cap is hollow. `Signal::measured` is
/// the flag the observer sets when it counted rather than guessed, and a guess that drew
/// identically to a count would put an estimate into the record wearing a measurement's
/// clothes.
fn signal_spokes(w: &WorldState) -> String {
    if w.signals.is_empty() {
        return String::new();
    }
    let shown = w.signals.len().min(MAX_SIGNALS);
    let mut s = String::new();
    for (i, sig) in w.signals.iter().take(shown).enumerate() {
        let deg = div_round(360 * i as i64, shown as i64);
        let span = R_SPOKE_MAX - R_SPOKE_IN;
        let outer = R_SPOKE_IN + scale(sig.magnitude, span);
        let role = match sig.polarity {
            Polarity::Risk => Role::Risk,
            Polarity::Opportunity => Role::Opportunity,
        };

        let dash = if sig.measured { String::new() } else { " stroke-dasharray=\"4 3\"".into() };
        let d = spoke_path(R_SPOKE_IN, outer, deg);
        s.push_str(&format!(
            "<path d=\"{d}\" stroke=\"{}\" stroke-width=\"2\"{dash}/>",
            role.hex()
        ));

        // The cap. Filled when counted, hollow when estimated.
        let fill = if sig.measured { role.hex() } else { "none".to_string() };
        let cap = polar(outer, deg);
        match sig.polarity {
            Polarity::Opportunity => {
                s.push_str(&format!(
                    "<circle cx=\"{}\" cy=\"{}\" r=\"3.5\" fill=\"{fill}\" stroke=\"{}\" stroke-width=\"1.5\"/>",
                    fmt(cap.x),
                    fmt(cap.y),
                    role.hex()
                ));
            }
            Polarity::Risk => {
                let a = polar(outer + 5 * UNIT, deg);
                let b = polar(outer - 2 * UNIT, deg - 2);
                let c = polar(outer - 2 * UNIT, deg + 2);
                s.push_str(&format!(
                    "<path d=\"M {} L {} L {} Z\" fill=\"{fill}\" stroke=\"{}\" stroke-width=\"1.5\"/>",
                    pt(a),
                    pt(b),
                    pt(c),
                    role.hex()
                ));
            }
        }
    }
    s
}

// ── provenance ────────────────────────────────────────────────────────────────

/// The provenance mix of the observed objects, as a composition ring.
///
/// `Absent` is drawn **dashed**, never as a filled segment: nothing is known in that arc,
/// and a solid band would render "unread" in the same language as "read and found empty".
/// Simulated is dashed for the same reason it is a distinct arm of the enum — simulated
/// output is labelled at every point it surfaces.
fn provenance_ring(w: &WorldState) -> String {
    if w.objects.is_empty() {
        return String::new();
    }
    let (live, stale, absent, simulated) = provenance_counts(w);
    let total = w.objects.len() as i64;
    let mut s = String::new();
    let mut at: i64 = 0;

    // Ordered so the arcs are laid out identically for the same counts every time.
    for (count, role, dashed) in [
        (live, Role::Opportunity, false),
        (stale, Role::Stale, false),
        (simulated, Role::Claim, true),
        (absent, Role::Absent, true),
    ] {
        if count == 0 {
            continue;
        }
        let sweep = div_round(360 * count, total);
        let d = arc_path(R_PROVENANCE, at, at + sweep);
        if !d.is_empty() {
            let dash = if dashed { " stroke-dasharray=\"4 4\"" } else { "" };
            s.push_str(&format!(
                "<path d=\"{d}\" fill=\"none\" stroke=\"{}\" stroke-width=\"3\"{dash}/>",
                role.hex()
            ));
        }
        at += sweep;
    }
    s
}

fn provenance_counts(w: &WorldState) -> (i64, i64, i64, i64) {
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
    (live, stale, absent, simulated)
}

// ── legibility ────────────────────────────────────────────────────────────────

/// The share of observed objects that may be acted on, as a disc.
///
/// This is where the crate's central distinction is at its sharpest, because
/// `WorldState::legibility` returns `0.0` for **two different worlds**: one where objects
/// were observed and none of them are actionable, and one where there were no objects at
/// all. The number cannot tell them apart — `world.rs` says so in its own doc comment — so
/// the picture has to.
///
/// - No objects: a dashed ghost outline at full radius and the glyph `∅`. Nothing was read
///   because there was nothing to read.
/// - Objects, none actionable: **no disc at all** and the text `0.00`. A real measurement
///   that happens to be zero.
fn legibility_core(w: &WorldState) -> String {
    let mut s = String::new();
    if w.objects.is_empty() {
        s.push_str(&format!(
            "<circle cx=\"{c}\" cy=\"{c}\" r=\"{r}\" fill=\"none\" stroke=\"{}\" stroke-width=\"2\" stroke-dasharray=\"5 6\"/>",
            Role::Ghost.hex(),
            c = fmt(CENTER),
            r = fmt(R_LEGIBILITY_MAX)
        ));
        s.push_str(&text(CENTER, CENTER + 7 * UNIT, 22, Role::Ghost, "middle", "∅"));
        return s;
    }

    let f = w.legibility();
    let r = scale(f, R_LEGIBILITY_MAX);
    if r > 0 {
        s.push_str(&format!(
            "<circle cx=\"{c}\" cy=\"{c}\" r=\"{}\" fill=\"{}\" fill-opacity=\"0.18\" stroke=\"{}\" stroke-width=\"1.5\"/>",
            fmt(r),
            Role::Measured.hex(),
            Role::Measured.hex(),
            c = fmt(CENTER)
        ));
    }
    s.push_str(&text(
        CENTER,
        CENTER + 5 * UNIT,
        16,
        Role::Body,
        "middle",
        &fixed2(f),
    ));
    s
}

// ── footer ────────────────────────────────────────────────────────────────────

fn footer(w: &WorldState, digest_hex: &str) -> String {
    let mut s = String::new();

    // Line 1: extent, stated the way the observer stated it.
    let extent = match w.extent.total {
        Some(t) => format!("EXTENT {}/{}", w.extent.observed, t),
        None => format!("EXTENT {} · UNBOUNDED", w.extent.observed),
    };
    let blind = if w.blind_spots.is_empty() {
        "NO BLIND SPOTS".to_string()
    } else if w.blind_spots.len() > MAX_NOTCHES {
        format!("{} BLIND SPOT(S) · {MAX_NOTCHES} DRAWN", w.blind_spots.len())
    } else {
        format!("{} BLIND SPOT(S)", w.blind_spots.len())
    };
    s.push_str(&text(X_MARGIN, Y_FOOT_1, 10, Role::Label, "start", &extent));
    s.push_str(&text(
        (VIEW * UNIT) - X_MARGIN,
        Y_FOOT_1,
        10,
        if w.blind_spots.is_empty() { Role::Label } else { Role::Absent },
        "end",
        &blind,
    ));

    // Line 2: coverage, one cell per signal.
    s.push_str(&coverage_cells(w));

    // Line 3: the commitment. Azure, because it is a claim.
    s.push_str(&text(
        X_MARGIN,
        Y_FOOT_3,
        10,
        Role::Claim,
        "start",
        &format!("world {}", short_digest(digest_hex)),
    ));
    let (live, stale, absent, simulated) = provenance_counts(w);
    s.push_str(&text(
        (VIEW * UNIT) - X_MARGIN,
        Y_FOOT_3,
        10,
        Role::Label,
        "end",
        &format!("L{live} S{stale} A{absent} M{simulated}"),
    ));
    s
}

/// One cell per signal: filled if the magnitude was counted, hollow if it was estimated.
///
/// Never a proportional bar. A bar renders 2/5 and 4/10 identically, and the denominator is
/// the number that matters — the same reason the console draws `▰▰▱▱▱` and the sigil draws
/// one cell per term. An empty signal set is `∅`, not an empty meter, because a meter with
/// nothing in it is indistinguishable from a meter measured at zero.
fn coverage_cells(w: &WorldState) -> String {
    let mut s = String::new();
    let measured = w.signals.iter().filter(|s| s.measured).count();
    let total = w.signals.len();

    let label = if total == 0 {
        "COVERAGE ∅".to_string()
    } else if total > MAX_SIGNALS {
        format!("COVERAGE {measured}/{total} · {MAX_SIGNALS} DRAWN")
    } else {
        format!("COVERAGE {measured}/{total}")
    };
    s.push_str(&text(X_MARGIN, Y_FOOT_2, 10, Role::Label, "start", &label));

    if total == 0 {
        return s;
    }

    let shown = total.min(MAX_SIGNALS);
    let cell = 7 * UNIT;
    let gap = 2 * UNIT;
    let right = (VIEW * UNIT) - X_MARGIN;
    let width = shown as i64 * (cell + gap) - gap;
    let x0 = right - width;
    let y = Y_FOOT_2 - 8 * UNIT;

    for (i, sig) in w.signals.iter().take(shown).enumerate() {
        let x = x0 + i as i64 * (cell + gap);
        if sig.measured {
            s.push_str(&format!(
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\"/>",
                fmt(x),
                fmt(y),
                fmt(cell),
                fmt(cell),
                Role::Measured.hex()
            ));
        } else {
            s.push_str(&format!(
                "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"1\"/>",
                fmt(x),
                fmt(y),
                fmt(cell),
                fmt(cell),
                Role::Ghost.hex()
            ));
        }
    }
    s
}

// ── primitives ────────────────────────────────────────────────────────────────

fn text(x: i64, y: i64, size: i64, role: Role, anchor: &str, body: &str) -> String {
    format!(
        "<text x=\"{}\" y=\"{}\" font-family=\"ui-monospace,SFMono-Regular,Menlo,Consolas,monospace\" font-size=\"{size}\" fill=\"{}\" text-anchor=\"{anchor}\">{}</text>",
        fmt(x),
        fmt(y),
        role.hex(),
        esc(body)
    )
}

/// XML-escape. All five, and `&` first or the escapes escape each other.
pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // Control characters are not legal in XML 1.0 text at all, and a producer that
            // put one in a label would otherwise emit an SVG no parser will open.
            c if (c as u32) < 0x20 && c != '\t' && c != '\n' && c != '\r' => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// Truncate to `n` **code points**, appending an ellipsis.
///
/// Code points rather than UTF-16 units, and the port says so too: JavaScript's `length`
/// and `slice` count UTF-16 units, so a label containing an emoji would truncate at a
/// different place there and the two SVGs would differ by a byte.
pub fn truncate(s: &str, n: usize) -> String {
    let count = s.chars().count();
    if count <= n {
        return s.to_string();
    }
    let mut out: String = s.chars().take(n.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Two decimal places, by integer arithmetic.
///
/// Not `format!("{:.2}")`: that rounds ties to even and `Number.toFixed(2)` does not, so the
/// two runtimes would disagree on a value like `0.125` and the byte comparison would fail on
/// a number that is correct in both.
pub fn fixed2(v: f64) -> String {
    if v.is_nan() {
        return "—".to_string();
    }
    let scaled = v * 100.0;
    let n = if scaled >= 0.0 {
        (scaled + 0.5).floor() as i64
    } else {
        -((-scaled + 0.5).floor() as i64)
    };
    let neg = n < 0;
    let a = n.unsigned_abs();
    format!("{}{}.{:02}", if neg { "-" } else { "" }, a / 100, a % 100)
}

/// The first twelve hex characters of the commitment, grouped.
///
/// Twelve rather than the whole sixty-four because the plate is a picture, and rather than
/// eight because eight is short enough that a collision is a thing a person could arrange.
/// It is an index into the record, never a substitute for `scema verify`.
pub fn short_digest(hex: &str) -> String {
    let head: String = hex.chars().take(12).collect();
    if head.chars().count() < 12 {
        return head;
    }
    let c: Vec<char> = head.chars().collect();
    format!(
        "{}{}{}{}·{}{}{}{}·{}{}{}{}",
        c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7], c[8], c[9], c[10], c[11]
    )
}

fn aria_label(w: &WorldState) -> String {
    format!(
        "Scematica Omni world plate for {} ({}), observed by {}",
        w.entity.label,
        w.entity.kind.as_str(),
        w.observer
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{empty_world, rich_world, unbounded_world};

    #[test]
    // Clippy is right that these are constants, and that is the entire point: the layout is
    // a set of numbers somebody will nudge, and this is the thing that fails when a nudge
    // puts text on top of the ring. A runtime assertion over constants is the cheapest
    // available form of "these numbers are related, not independent".
    #[allow(clippy::assertions_on_constants)]
    fn layout_bands_do_not_overlap_the_dial() {
        // Text over the ring is unreadable at thumbnail size, which is the size this will
        // usually be seen at. Pinned rather than eyeballed.
        assert!(Y_SUBTITLE < DIAL_TOP, "subtitle sits on the dial");
        assert!(Y_FOOT_1 > DIAL_BOTTOM, "first footer line sits on the dial");
        assert!(Y_FOOT_1 < Y_FOOT_2 && Y_FOOT_2 < Y_FOOT_3, "footer lines out of order");
        assert!(Y_FOOT_3 < VIEW * UNIT, "footer runs off the plate");
    }

    #[test]
    fn an_unmeasured_extent_draws_a_dashed_full_sweep_and_says_unbounded() {
        let svg = render(&unbounded_world(), "abcdef0123456789");
        assert!(svg.contains("stroke-dasharray"), "unbounded extent must be dashed");
        assert!(svg.contains("UNBOUNDED"), "unbounded extent must say so");
    }

    #[test]
    fn a_measured_zero_extent_draws_no_arc_at_all() {
        // The distinction the crate exists for. Zero observed out of a known total is a
        // measurement; it draws the track and nothing on it. If this ever starts drawing a
        // dashed sweep, a measured zero has become indistinguishable from an unmeasured one.
        let mut w = rich_world();
        w.extent.observed = 0;
        w.extent.total = Some(10);
        let svg = render(&w, "abcdef0123456789");
        assert!(svg.contains("EXTENT 0/10"));
        assert!(!svg.contains("UNBOUNDED"));
    }

    #[test]
    fn an_empty_world_and_an_illegible_one_do_not_draw_the_same_picture() {
        // `WorldState::legibility` returns 0.0 for both. This is the whole reason the core
        // is drawn rather than merely printed.
        let empty = render(&empty_world(), "aa");
        let mut illegible = rich_world();
        for o in illegible.objects.iter_mut() {
            o.provenance = Provenance::Absent;
        }
        let illegible = render(&illegible, "aa");

        assert!(empty.contains('∅'), "an empty world must say nothing-to-read");
        assert!(!illegible.contains('∅'), "an illegible world measured zero, not nothing");
        assert!(illegible.contains("0.00"), "a measured zero prints as a number");
        assert_ne!(empty, illegible);
    }

    #[test]
    fn a_blind_spot_cuts_a_notch_and_is_counted() {
        let clean = render(&rich_world(), "aa");
        let mut w = rich_world();
        w.blind_spots = vec!["could not read .git".into(), "permission denied".into()];
        let holed = render(&w, "aa");
        assert!(clean.contains("NO BLIND SPOTS"));
        assert!(holed.contains("2 BLIND SPOT(S)"));
        assert_ne!(clean, holed, "blind spots must change the picture");
    }

    #[test]
    fn an_estimated_signal_is_drawn_differently_from_a_counted_one() {
        let mut w = rich_world();
        w.signals[0].measured = true;
        let counted = render(&w, "aa");
        w.signals[0].measured = false;
        let guessed = render(&w, "aa");
        assert_ne!(counted, guessed, "an estimate must not draw as a count");
    }

    #[test]
    fn coverage_is_cells_and_an_empty_set_is_the_empty_glyph() {
        let mut w = rich_world();
        let svg = render(&w, "aa");
        let cells = svg.matches("<rect").count();
        assert!(cells >= w.signals.len(), "one cell per signal, plus ground and frame");
        assert!(svg.contains("COVERAGE "));

        w.signals.clear();
        assert!(render(&w, "aa").contains("COVERAGE ∅"));
    }

    #[test]
    fn a_capped_plate_says_how_many_it_drew() {
        // The plate can only hold so many spokes before the ring is a solid disc. Capping is
        // fine; capping *silently* is not — a reader who counts marks would come away with a
        // wrong count, which is the failure `render::signals_capped` grew its tail line for.
        let mut w = rich_world();
        let one = w.signals[0].clone();
        w.signals = (0..MAX_SIGNALS + 9).map(|_| one.clone()).collect();
        w.blind_spots = (0..MAX_NOTCHES + 4).map(|i| format!("spot {i}")).collect();

        let svg = render(&w, "aa");
        assert!(svg.contains(&format!("COVERAGE {}/{}", MAX_SIGNALS + 9, MAX_SIGNALS + 9)));
        assert!(svg.contains(&format!("· {MAX_SIGNALS} DRAWN")), "the signal cap must be disclosed");
        assert!(svg.contains(&format!("{} BLIND SPOT(S) · {MAX_NOTCHES} DRAWN", MAX_NOTCHES + 4)));

        // And an uncapped plate must not carry the disclosure, or it becomes noise nobody
        // reads and the capped case stops standing out.
        assert!(!render(&rich_world(), "aa").contains("DRAWN"));
    }

    #[test]
    fn rendering_is_deterministic() {
        let w = rich_world();
        assert_eq!(render(&w, "aa"), render(&w, "aa"));
    }

    #[test]
    fn the_output_is_well_formed_enough_to_open() {
        let svg = render(&rich_world(), "aa");
        assert!(svg.starts_with("<svg "));
        assert!(svg.ends_with("</svg>"));
        assert!(svg.contains("viewBox=\"0 0 512 512\""));
        // No stray unescaped markup from a label.
        assert_eq!(svg.matches("<svg").count(), 1);
    }

    #[test]
    fn a_hostile_label_cannot_inject_markup() {
        let mut w = rich_world();
        w.entity.label = "</svg><script>alert(1)</script>".into();
        w.observer = "a & b \"c\"".into();
        let svg = render(&w, "aa");
        assert!(!svg.contains("<script"));
        assert_eq!(svg.matches("</svg>").count(), 1);
        assert!(svg.contains("&amp;"));
    }

    #[test]
    fn fixed2_matches_the_javascript_rounding_rule() {
        assert_eq!(fixed2(0.0), "0.00");
        assert_eq!(fixed2(1.0), "1.00");
        assert_eq!(fixed2(0.125), "0.13");
        assert_eq!(fixed2(0.005), "0.01");
        assert_eq!(fixed2(0.666_666), "0.67");
        assert_eq!(fixed2(f64::NAN), "—");
    }

    #[test]
    fn the_short_digest_is_grouped_and_never_padded() {
        assert_eq!(short_digest("0123456789abcdef"), "0123·4567·89ab");
        // A short input is returned as-is rather than padded — inventing digits in a
        // commitment is worse than printing a stub.
        assert_eq!(short_digest("abc"), "abc");
    }

    #[test]
    fn truncation_counts_code_points() {
        assert_eq!(truncate("abcdef", 3), "ab…");
        assert_eq!(truncate("abc", 3), "abc");
        // An astral character is one code point here and two UTF-16 units in JavaScript.
        // The port must agree with this, which is why the rule is stated in both files.
        assert_eq!(truncate("😀😀😀😀", 3).chars().count(), 3);
    }
}
