//! Geometry, in integers, so that Rust and the browser draw the same bytes.
//!
//! ## Why this file is written the way it is
//!
//! The plate has to be reproducible. An NFT whose image depends on which runtime rendered
//! it is not an artefact, it is two artefacts with one name — and the whole point of
//! deriving it from a `WorldState` is that anybody holding the world file can regenerate
//! the image and get the same thing. So the SVG text is **compared byte for byte** between
//! `scema-nft` and `web/lib/omni/nft.ts`, and `check:omni` fails on a single differing
//! character.
//!
//! That is a much stronger requirement than "looks the same", and it rules out the obvious
//! implementation. This repository has already paid for the general version of this lesson
//! once, in `scema_verify::canonical`: **bit-exact float agreement between two runtimes is
//! engineered, not achieved by care.** The specific hazards here:
//!
//! - `sin` and `cos` are **not** correctly rounded by IEEE-754 and are free to differ in
//!   the last place between Rust's libm and a JavaScript engine. A one-ULP difference
//!   survives rounding whenever it straddles a tie, which on a plate with hundreds of
//!   coordinates is not a rare event — it is a certainty over enough worlds. So there is no
//!   trigonometry in this crate at all: [`SIN_MICRO`] is a table of integer sines at whole
//!   degrees, shared verbatim with the port, and every angle on the plate is a whole degree.
//! - Multiplication, addition, subtraction, division and `sqrt` **are** correctly rounded
//!   and may be used freely. `exp`, `log`, `pow` and the trigonometric family may not.
//! - Decimal formatting differs between `format!("{:.3}")` (ties to even) and
//!   `Number.toFixed(3)` (ties decided by the binary value). Neither is used: coordinates
//!   are integers in thousandths of a unit and [`fmt`] renders them by integer arithmetic.
//!
//! The unit of every coordinate here is a **milliunit** — one thousandth of a viewBox unit
//! — held in `i64`. The plate is 512 units across, so the whole drawing lives inside
//! ±512_000 and nothing is anywhere near an overflow.

/// The plate is square, this many units on a side.
pub const VIEW: i64 = 512;

/// One viewBox unit, in milliunits.
pub const UNIT: i64 = 1_000;

/// Centre of the plate, in milliunits.
pub const CENTER: i64 = VIEW * UNIT / 2;

/// `sin(d°) * 1_000_000`, rounded half away from zero, for `d` in `0..=90`.
///
/// Ninety-one entries rather than three hundred and sixty: the rest follow by symmetry in
/// [`sin_micro`], and a quarter table cannot disagree with itself about `sin(90°)` the way
/// four independently generated quadrants can.
pub const SIN_MICRO: [i64; 91] = [
    0, 17452, 34899, 52336, 69756, 87156, 104528, 121869, 139173, 156434, 173648, 190809,
    207912, 224951, 241922, 258819, 275637, 292372, 309017, 325568, 342020, 358368, 374607,
    390731, 406737, 422618, 438371, 453990, 469472, 484810, 500000, 515038, 529919, 544639,
    559193, 573576, 587785, 601815, 615661, 629320, 642788, 656059, 669131, 681998, 694658,
    707107, 719340, 731354, 743145, 754710, 766044, 777146, 788011, 798636, 809017, 819152,
    829038, 838671, 848048, 857167, 866025, 874620, 882948, 891007, 898794, 906308, 913545,
    920505, 927184, 933580, 939693, 945519, 951057, 956305, 961262, 965926, 970296, 974370,
    978148, 981627, 984808, 987688, 990268, 992546, 994522, 996195, 997564, 998630, 999391,
    999848, 1_000_000,
];

/// The scale [`SIN_MICRO`] is expressed in.
pub const MICRO: i64 = 1_000_000;

/// `sin(deg°) * 1_000_000`, for any integer degree, by quadrant symmetry.
pub fn sin_micro(deg: i64) -> i64 {
    let d = deg.rem_euclid(360);
    match d {
        0..=90 => SIN_MICRO[d as usize],
        91..=180 => SIN_MICRO[(180 - d) as usize],
        181..=270 => -SIN_MICRO[(d - 180) as usize],
        _ => -SIN_MICRO[(360 - d) as usize],
    }
}

/// `cos(deg°) * 1_000_000`.
pub fn cos_micro(deg: i64) -> i64 {
    sin_micro(deg + 90)
}

/// Divide, rounding half away from zero, in integers.
///
/// Half away from zero rather than half to even, and stated rather than inherited: the port
/// spells the same rule out by hand because `Math.round` in JavaScript rounds half toward
/// positive infinity, which disagrees with this on every negative tie. The plate keeps most
/// coordinates positive, but "most" is not a property a byte comparison respects.
pub fn div_round(num: i64, den: i64) -> i64 {
    debug_assert!(den > 0, "denominator must be positive");
    if num >= 0 {
        (num + den / 2) / den
    } else {
        -((-num + den / 2) / den)
    }
}

/// A point on the plate, in milliunits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pt {
    pub x: i64,
    pub y: i64,
}

/// Polar to cartesian, with 0° at twelve o'clock and angles increasing clockwise.
///
/// SVG's native zero is at three o'clock with y growing downward, which makes every
/// hand-written arc off by ninety degrees and mirrored. Absorbing that here means no call
/// site has to think about it, and the tests read as "zero is the top".
pub fn polar(radius_mu: i64, deg: i64) -> Pt {
    let a = deg - 90;
    Pt {
        x: CENTER + div_round(radius_mu * cos_micro(a), MICRO),
        y: CENTER + div_round(radius_mu * sin_micro(a), MICRO),
    }
}

/// Convert a fraction in `[0, 1]` to a span of milliunits.
///
/// `f64` multiplication is correctly rounded and therefore safe to share; the result is
/// immediately quantised, so no float ever reaches the output text.
///
/// `NaN` becomes zero because it has no position in the ordering and `clamp` would panic on
/// it; the infinities are ordered and simply clamp. A world file is JSON somebody else
/// wrote and is allowed to contain whatever its producer put there — a `NaN` in a path
/// attribute is not a wrong picture, it is no picture at all.
pub fn scale(fraction: f64, span_mu: i64) -> i64 {
    if fraction.is_nan() {
        return 0;
    }
    let f = fraction.clamp(0.0, 1.0);
    let v = f * span_mu as f64;
    if v >= 0.0 {
        (v + 0.5).floor() as i64
    } else {
        -((-v + 0.5).floor() as i64)
    }
}

/// Render a milliunit coordinate as SVG text.
///
/// Integer arithmetic end to end, trailing zeros trimmed, no decimal point when the value
/// is whole. `256_500` becomes `256.5`; `256_000` becomes `256`. The port produces the same
/// string by the same construction rather than by `toFixed`, which would disagree here on
/// values that are exactly representable in decimal but not in binary.
pub fn fmt(mu: i64) -> String {
    let neg = mu < 0;
    let a = mu.unsigned_abs();
    let whole = a / UNIT as u64;
    let frac = a % UNIT as u64;
    let sign = if neg && (whole != 0 || frac != 0) { "-" } else { "" };
    if frac == 0 {
        return format!("{sign}{whole}");
    }
    let mut f = format!("{frac:03}");
    while f.ends_with('0') {
        f.pop();
    }
    format!("{sign}{whole}.{f}")
}

/// `x,y` for a path command.
pub fn pt(p: Pt) -> String {
    format!("{} {}", fmt(p.x), fmt(p.y))
}

/// An arc path from `start_deg` to `end_deg`, clockwise, at one radius.
///
/// Returns an empty string for a sweep of zero or less, rather than a degenerate `A`
/// command. An arc whose endpoints coincide renders as nothing at all on some engines and
/// as a **complete circle** on others, and a full circle is the single worst thing a zero
/// gauge could draw — it is the picture of total coverage.
///
/// A sweep of 360° or more is drawn as two half arcs for the same reason: one `A` command
/// back to its own start point is the degenerate case again.
pub fn arc_path(radius_mu: i64, start_deg: i64, end_deg: i64) -> String {
    let sweep = end_deg - start_deg;
    if sweep <= 0 {
        return String::new();
    }
    if sweep >= 360 {
        let a = polar(radius_mu, 0);
        let b = polar(radius_mu, 180);
        let r = fmt(radius_mu);
        return format!(
            "M {} A {r} {r} 0 0 1 {} A {r} {r} 0 0 1 {}",
            pt(a),
            pt(b),
            pt(a)
        );
    }
    let a = polar(radius_mu, start_deg);
    let b = polar(radius_mu, end_deg);
    let large = if sweep > 180 { 1 } else { 0 };
    let r = fmt(radius_mu);
    format!("M {} A {r} {r} 0 {large} 1 {}", pt(a), pt(b))
}

/// A straight radial segment between two radii at one angle.
pub fn spoke_path(inner_mu: i64, outer_mu: i64, deg: i64) -> String {
    format!("M {} L {}", pt(polar(inner_mu, deg)), pt(polar(outer_mu, deg)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sine_table_agrees_with_itself_at_the_cardinals() {
        assert_eq!(sin_micro(0), 0);
        assert_eq!(sin_micro(90), MICRO);
        assert_eq!(sin_micro(180), 0);
        assert_eq!(sin_micro(270), -MICRO);
        assert_eq!(sin_micro(360), 0);
        assert_eq!(cos_micro(0), MICRO);
        assert_eq!(cos_micro(90), 0);
        assert_eq!(cos_micro(180), -MICRO);
    }

    #[test]
    fn the_table_is_accurate_enough_to_draw_with() {
        // Not a claim that the table is the true sine — a claim that it is within a
        // milliunit of it at plate radius, which is the only accuracy that can matter when
        // the output is quantised to thousandths of a unit.
        for d in 0..360 {
            let want = (d as f64).to_radians().sin();
            let got = sin_micro(d) as f64 / MICRO as f64;
            assert!((want - got).abs() < 1e-5, "sin({d}) off by {}", want - got);
        }
    }

    #[test]
    fn sine_symmetry_holds_across_quadrants() {
        for d in 0..360 {
            assert_eq!(sin_micro(d), -sin_micro(d + 180), "antisymmetry at {d}");
            assert_eq!(sin_micro(d), sin_micro(d + 360), "periodicity at {d}");
        }
    }

    #[test]
    fn rounding_is_half_away_from_zero_in_both_directions() {
        // The one place Rust and JavaScript disagree by default. Pinned in both.
        assert_eq!(div_round(5, 10), 1);
        assert_eq!(div_round(-5, 10), -1);
        assert_eq!(div_round(4, 10), 0);
        assert_eq!(div_round(-4, 10), 0);
        assert_eq!(div_round(15, 10), 2);
        assert_eq!(div_round(-15, 10), -2);
    }

    #[test]
    fn zero_is_at_twelve_oclock_and_angles_run_clockwise() {
        let top = polar(100 * UNIT, 0);
        assert_eq!(top.x, CENTER);
        assert!(top.y < CENTER, "0 degrees must be above centre");

        let right = polar(100 * UNIT, 90);
        assert!(right.x > CENTER, "90 degrees must be to the right");
        assert_eq!(right.y, CENTER);
    }

    #[test]
    fn formatting_trims_without_losing_the_value() {
        assert_eq!(fmt(256_000), "256");
        assert_eq!(fmt(256_500), "256.5");
        assert_eq!(fmt(256_050), "256.05");
        assert_eq!(fmt(256_005), "256.005");
        assert_eq!(fmt(0), "0");
        assert_eq!(fmt(-1_500), "-1.5");
        assert_eq!(fmt(-500), "-0.5");
    }

    #[test]
    fn a_zero_sweep_draws_nothing_and_a_full_sweep_draws_a_circle() {
        // The distinction the whole crate turns on. A gauge nobody measured draws a full
        // dashed sweep; a gauge measured at zero must draw *nothing*. If a zero sweep
        // silently produced a circle, those two would be the same picture.
        assert_eq!(arc_path(100 * UNIT, 0, 0), "");
        assert_eq!(arc_path(100 * UNIT, 0, -10), "");
        assert!(arc_path(100 * UNIT, 0, 360).contains('A'));
        assert_eq!(arc_path(100 * UNIT, 0, 360).matches('A').count(), 2);
    }

    #[test]
    fn the_large_arc_flag_flips_past_a_half_turn() {
        assert!(arc_path(100 * UNIT, 0, 90).contains(" 0 0 1 "));
        assert!(arc_path(100 * UNIT, 0, 181).contains(" 0 1 1 "));
    }

    #[test]
    fn a_non_finite_fraction_does_not_reach_the_output() {
        // A world file is JSON somebody else wrote. A NaN in a path attribute is not a
        // wrong picture, it is no picture at all.
        assert_eq!(scale(f64::NAN, 100), 0);
        assert_eq!(scale(f64::INFINITY, 100), 100);
        assert_eq!(scale(-1.0, 100), 0);
        assert_eq!(scale(0.5, 100), 50);
        assert_eq!(scale(1.0, 100), 100);
    }
}
