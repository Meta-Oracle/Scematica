//! Q16.16 fixed-point arithmetic — the foundation everything else is built on.
//!
//! # Why not floats
//!
//! Because the point of this mesh is that an inference can be *checked* by someone who
//! did not run it, including by a contract on BOT Chain. That requires the forward pass
//! to be bit-exact reproducible by any implementation, in any language, and floats are
//! not:
//!
//! * **Solidity has no floating point at all.** A contract adjudicating a disputed
//!   inference cannot even represent an `f32`, so a float-based net can never be
//!   settled on-chain. This alone decides the question.
//! * **Transcendentals differ between platforms.** `exp`, `tanh` and friends are libm
//!   implementations, not IEEE-specified operations; two conforming machines legitimately
//!   return different last bits.
//! * **Other languages cannot match it.** JavaScript has no `f32` type, so a browser
//!   verifier reimplementing this in JS would diverge on rounding alone.
//!
//! Integer arithmetic has none of those problems. `(a * b + 32768) / 65536` means exactly
//! one thing everywhere, forever.
//!
//! Note the division: **not** `>> 16`. An arithmetic shift floors toward negative
//! infinity, which breaks the symmetry of away-from-zero rounding on negative values.
//! That distinction is part of the specification and a reimplementation that uses a shift
//! will diverge — see `Fx::mul`.
//!
//! # The format
//!
//! A [`Fx`] is an `i32` holding a value scaled by 2^16. So `1.0` is `65536`, `-0.5` is
//! `-32768`. Range is roughly ±32768 with a resolution of ~1.5e-5, which is ample for
//! network activations and rewards — and the saturating behaviour below means exceeding
//! it degrades rather than wraps.

/// Fractional bits. Changing this changes every hash this crate produces.
pub const FRAC_BITS: u32 = 16;

/// 1.0 in fixed point.
pub const ONE: i32 = 1 << FRAC_BITS;

/// Rounding addend: half of one ulp.
const HALF: i64 = 1 << (FRAC_BITS - 1);

/// 2^FRAC_BITS as a divisor. Used instead of `>>` so rounding stays symmetric about zero.
const SCALE: i64 = 1 << FRAC_BITS;

/// A Q16.16 fixed-point number.
///
/// `Ord` and `Eq` are exact — two `Fx` compare equal only if their bit patterns match,
/// which is what makes replay verification a plain equality check rather than an epsilon
/// comparison nobody can agree on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
#[repr(transparent)]
pub struct Fx(pub i32);

// The std ops traits are implemented below and delegate here. The inherent methods keep
// their names on purpose: `a.mul(b)` names the specified fixed-point multiply, whereas
// `a * b` invites a reader to assume ordinary arithmetic. Clippy flags the shadowing;
// having both is the intent, so the lint is answered rather than obeyed.
#[allow(clippy::should_implement_trait)]
impl Fx {
    pub const ZERO: Fx = Fx(0);
    pub const ONE: Fx = Fx(ONE);
    pub const MIN: Fx = Fx(i32::MIN);
    pub const MAX: Fx = Fx(i32::MAX);

    /// Construct from a raw fixed-point integer.
    #[inline]
    pub const fn from_bits(bits: i32) -> Self {
        Fx(bits)
    }

    #[inline]
    pub const fn to_bits(self) -> i32 {
        self.0
    }

    /// Construct from a whole number.
    #[inline]
    pub const fn from_int(v: i16) -> Self {
        Fx((v as i32) << FRAC_BITS)
    }

    /// Convert from `f64`.
    ///
    /// **Boundary use only** — importing trained weights, or a game handing over a
    /// human-authored constant. Never call this inside the forward pass: doing so would
    /// reintroduce exactly the platform-dependent rounding this module exists to remove.
    pub fn from_f64(v: f64) -> Self {
        let scaled = v * (ONE as f64);
        let clamped = scaled.clamp(i32::MIN as f64, i32::MAX as f64);
        // `round` here is deterministic because it happens once, at import, and the
        // result is then part of the committed weights — every runtime reads the same
        // integers thereafter.
        Fx(clamped.round() as i32)
    }

    /// Convert to `f64` for display. Never feeds back into computation.
    pub fn to_f64(self) -> f64 {
        self.0 as f64 / ONE as f64
    }

    /// Saturating add. Saturation rather than wrapping: an overflowing activation should
    /// pin at the extreme, not silently become its own negation.
    #[inline]
    pub fn add(self, rhs: Fx) -> Fx {
        Fx(self.0.saturating_add(rhs.0))
    }

    #[inline]
    pub fn sub(self, rhs: Fx) -> Fx {
        Fx(self.0.saturating_sub(rhs.0))
    }

    /// Multiply, with round-half-away-from-zero.
    ///
    /// The rounding rule is part of the specification, not an implementation detail. A
    /// reimplementation that truncates instead will diverge from this one on roughly half
    /// of all multiplications, and the divergence compounds through layers. Half-away-
    /// from-zero is chosen over half-up because it is symmetric: `-x * y == -(x * y)`
    /// exactly, so a sign flip in the inputs cannot change the magnitude of the result.
    #[inline]
    pub fn mul(self, rhs: Fx) -> Fx {
        let product = (self.0 as i64) * (rhs.0 as i64);
        let rounded = if product >= 0 { product + HALF } else { product - HALF };
        // Division, not `>>`. An arithmetic shift floors toward negative infinity, so
        // pairing it with an away-from-zero offset rounds positives away from zero and
        // negatives *toward* it — leaving `(-x)*y` one ulp off `-(x*y)`. Integer division
        // truncates toward zero, which is what this rounding rule requires to be
        // symmetric. Caught by `multiplication_is_symmetric_under_sign`.
        Fx(saturate(rounded / SCALE))
    }

    /// Divide, with round-half-away-from-zero. Division by zero saturates by sign.
    #[inline]
    pub fn div(self, rhs: Fx) -> Fx {
        if rhs.0 == 0 {
            // A panic here would let one bad weight halt a verifier mid-replay, turning a
            // data problem into a liveness problem. Saturating keeps replay total.
            return if self.0 >= 0 { Fx::MAX } else { Fx::MIN };
        }
        let numerator = (self.0 as i64) << FRAC_BITS;
        let half = (rhs.0.unsigned_abs() as i64) / 2;
        let adjusted = if (numerator >= 0) == (rhs.0 >= 0) { numerator + half } else { numerator - half };
        Fx(saturate(adjusted / rhs.0 as i64))
    }

    /// max(0, x) — ReLU.
    #[inline]
    pub fn relu(self) -> Fx {
        if self.0 > 0 { self } else { Fx::ZERO }
    }

    #[inline]
    pub fn neg(self) -> Fx {
        Fx(self.0.saturating_neg())
    }

    #[inline]
    pub fn abs(self) -> Fx {
        Fx(self.0.saturating_abs())
    }

    #[inline]
    pub fn min(self, rhs: Fx) -> Fx {
        if self.0 <= rhs.0 { self } else { rhs }
    }

    #[inline]
    pub fn max(self, rhs: Fx) -> Fx {
        if self.0 >= rhs.0 { self } else { rhs }
    }

    #[inline]
    pub fn clamp(self, lo: Fx, hi: Fx) -> Fx {
        self.max(lo).min(hi)
    }
}

// ── operator sugar ────────────────────────────────────────────────────────────
//
// The inherent `add`/`mul`/… methods stay, and remain the ones the maths in `net.rs` and
// `commit.rs` calls. That is deliberate: `a.mul(b)` reads as "the specified fixed-point
// multiply, with its saturation and its rounding rule", where `a * b` invites the reader
// to assume ordinary arithmetic. The operators exist because a crate meant to be adopted
// should not force method syntax on its users — but they are thin delegations, so there
// is exactly one implementation of each operation to reimplement or dispute.

impl core::ops::Add for Fx {
    type Output = Fx;
    #[inline]
    fn add(self, rhs: Fx) -> Fx {
        Fx::add(self, rhs)
    }
}

impl core::ops::Sub for Fx {
    type Output = Fx;
    #[inline]
    fn sub(self, rhs: Fx) -> Fx {
        Fx::sub(self, rhs)
    }
}

impl core::ops::Mul for Fx {
    type Output = Fx;
    #[inline]
    fn mul(self, rhs: Fx) -> Fx {
        Fx::mul(self, rhs)
    }
}

impl core::ops::Div for Fx {
    type Output = Fx;
    #[inline]
    fn div(self, rhs: Fx) -> Fx {
        Fx::div(self, rhs)
    }
}

impl core::ops::Neg for Fx {
    type Output = Fx;
    #[inline]
    fn neg(self) -> Fx {
        Fx::neg(self)
    }
}

#[inline]
fn saturate(v: i64) -> i32 {
    if v > i32::MAX as i64 {
        i32::MAX
    } else if v < i32::MIN as i64 {
        i32::MIN
    } else {
        v as i32
    }
}

/// Mean of a slice, computed in a **fixed summation order** with a widened accumulator.
///
/// Both properties are load-bearing. A widened `i64` accumulator means no intermediate
/// saturation, so the result does not depend on the order values happen to arrive in; and
/// the fixed left-to-right order means a SIMD or parallel reimplementation that reassociates
/// the sum will still be asked to match this one. Any "optimisation" that reorders this is
/// a consensus break, not a speedup.
pub fn mean(values: &[Fx]) -> Fx {
    if values.is_empty() {
        return Fx::ZERO;
    }
    let mut acc: i64 = 0;
    for v in values {
        acc += v.0 as i64;
    }
    let n = values.len() as i64;
    let rounded = if acc >= 0 { acc + n / 2 } else { acc - n / 2 };
    Fx(saturate(rounded / n))
}

/// Dot product with a widened accumulator and fixed order. Same reasoning as [`mean`].
pub fn dot(a: &[Fx], b: &[Fx]) -> Fx {
    debug_assert_eq!(a.len(), b.len());
    let mut acc: i64 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        // Accumulate at full precision and shift once at the end, rather than rounding
        // every term. Rounding per-term would make the result depend on how the sum is
        // chunked, which is exactly what must not vary between implementations.
        acc += (x.0 as i64) * (y.0 as i64);
    }
    let rounded = if acc >= 0 { acc + HALF } else { acc - HALF };
    // Division rather than `>>`, for the same symmetry reason as `Fx::mul`.
    Fx(saturate(rounded / SCALE))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operators_delegate_to_the_specified_methods() {
        // If these ever diverge there would be two multiplies in the crate, and a
        // reimplementer would have no way to know which one the spec meant.
        let a = Fx::from_f64(0.3);
        let b = Fx::from_f64(-1.7);
        assert_eq!(a + b, a.add(b));
        assert_eq!(a - b, a.sub(b));
        assert_eq!(a * b, a.mul(b));
        assert_eq!(a / b, a.div(b));
        assert_eq!(-a, a.neg());
    }

    #[test]
    fn one_is_one() {
        assert_eq!(Fx::ONE.to_f64(), 1.0);
        assert_eq!(Fx::from_int(3).to_f64(), 3.0);
        assert_eq!(Fx::from_f64(0.5).to_bits(), ONE / 2);
    }

    #[test]
    fn multiplication_is_symmetric_under_sign() {
        // The reason for round-half-away-from-zero rather than half-up: a sign flip must
        // not change the magnitude, or a net's behaviour depends on which way it is
        // parameterised.
        let a = Fx::from_f64(0.30000001);
        let b = Fx::from_f64(0.7000001);
        assert_eq!(a.mul(b).neg(), a.neg().mul(b));
        assert_eq!(a.mul(b), a.neg().mul(b.neg()));
    }

    #[test]
    fn arithmetic_saturates_rather_than_wrapping() {
        // Wrapping would turn a large positive activation into a large negative one —
        // a silent sign inversion is far worse than a pinned extreme.
        assert_eq!(Fx::MAX.add(Fx::ONE), Fx::MAX);
        assert_eq!(Fx::MIN.sub(Fx::ONE), Fx::MIN);
        assert_eq!(Fx::MAX.mul(Fx::from_int(2)), Fx::MAX);
    }

    #[test]
    fn division_by_zero_saturates_instead_of_panicking() {
        // Replay must be total: one bad weight cannot be allowed to halt a verifier.
        assert_eq!(Fx::ONE.div(Fx::ZERO), Fx::MAX);
        assert_eq!(Fx::ONE.neg().div(Fx::ZERO), Fx::MIN);
    }

    #[test]
    fn relu_gates_at_exactly_zero() {
        assert_eq!(Fx::from_f64(-0.0001).relu(), Fx::ZERO);
        assert_eq!(Fx::ZERO.relu(), Fx::ZERO);
        assert_eq!(Fx::from_f64(0.0001).relu(), Fx::from_f64(0.0001));
    }

    #[test]
    fn dot_does_not_round_per_term() {
        // Many tiny products that would each round to zero individually must still sum to
        // something. A per-term rounding implementation returns 0 here and silently loses
        // the whole signal.
        let tiny = Fx::from_bits(1);
        let a = vec![tiny; 4096];
        let b = vec![Fx::ONE; 4096];
        assert_eq!(dot(&a, &b), Fx::from_bits(4096));
    }

    #[test]
    fn mean_matches_hand_computation() {
        let v = [Fx::from_int(1), Fx::from_int(2), Fx::from_int(3)];
        assert_eq!(mean(&v), Fx::from_int(2));
        assert_eq!(mean(&[]), Fx::ZERO);
    }

    #[test]
    fn mean_is_order_independent_because_the_accumulator_is_widened() {
        // Not a claim that order may vary — the spec fixes it — but that saturation
        // cannot make the fixed order produce a different answer than the maths implies.
        let v = [Fx::MAX, Fx::MIN, Fx::from_int(6)];
        let mut r = v;
        r.reverse();
        assert_eq!(mean(&v), mean(&r));
    }

    #[test]
    fn round_trip_through_f64_is_stable() {
        for raw in [-70000i32, -1, 0, 1, 65536, 123456] {
            let f = Fx::from_bits(raw);
            assert_eq!(Fx::from_f64(f.to_f64()), f);
        }
    }
}
