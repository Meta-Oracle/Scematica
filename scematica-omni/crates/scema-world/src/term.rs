//! [`Term`]: one number entering an equation, carrying whether anybody measured it.
//!
//! This type is the honesty mechanism of the whole runtime, and it is deliberately the
//! same mechanism as `scematica_mesh::cognition::Term` in the bot workspace. The rule it
//! encodes was learned the expensive way twice in this repository — once when an
//! unmeasured perception channel scored `0` pinned the sentience Ψ at `0` forever, and
//! once when a literal reading of the agentic spec pinned a gate shut on subsystems
//! nobody had built:
//!
//! > **An unmeasured dimension is not a limiting factor.** It contributes the neutral
//! > element for its position in the equation — `1.0` multiplicative, `0.0` additive —
//! > and is flagged `measured: false`. Only measured degradation moves a verdict.
//!
//! The cost of that rule is that a utility of `0.91` may be standing on two terms out of
//! nine, so every aggregate in this workspace carries a [`Coverage`] and no renderer is
//! permitted to show the score without it. A confident number computed over an unmeasured
//! world is a statement about ignorance, and it has to look like one.

use serde::{Deserialize, Serialize};

/// A single quantity, plus the evidence behind it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Term {
    /// Symbol as written in the design, e.g. `R`, `C`, `U`.
    ///
    /// `String` rather than `&'static str` only because `Deserialize` cannot produce a
    /// borrowed static. The constructors take `&'static str`, so in-process a term can
    /// only ever cite a literal, never a runtime-assembled label.
    pub symbol: String,
    /// Human name of the quantity.
    pub name: String,
    /// The value used in the computation. For an unmeasured term this is the neutral
    /// element for its position — never a guess, never an average, never a prior.
    pub value: f64,
    /// Did anything actually observe this?
    pub measured: bool,
    /// What was measured, or what would have to exist for it to be measurable.
    ///
    /// Required, not optional. An unmeasured term whose note is empty is indistinguishable
    /// from one nobody thought about.
    pub note: String,
}

impl Term {
    /// A term backed by an observation.
    pub fn measured(symbol: &'static str, name: &'static str, value: f64, note: impl Into<String>) -> Self {
        Term { symbol: symbol.into(), name: name.into(), value, measured: true, note: note.into() }
    }

    /// A term nobody could observe. Takes the neutral element for its position in the
    /// equation, which the caller must supply because only the caller knows whether this
    /// term is multiplied or added.
    pub fn absent(symbol: &'static str, name: &'static str, neutral: f64, note: impl Into<String>) -> Self {
        Term { symbol: symbol.into(), name: name.into(), value: neutral, measured: false, note: note.into() }
    }

    /// Clamp the value into `[lo, hi]`, preserving `measured`.
    pub fn clamped(mut self, lo: f64, hi: f64) -> Self {
        self.value = self.value.clamp(lo, hi);
        self
    }
}

/// How much of an aggregate stood on real observations.
///
/// Constructed from a slice of terms and carried alongside every score in this workspace.
/// It is never optional and never separated from the number it qualifies — a utility
/// computed on 2 of 9 terms and one computed on 9 of 9 are different claims that would
/// otherwise print identically.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Coverage {
    pub measured: usize,
    pub total: usize,
}

impl Coverage {
    pub fn of(terms: &[&Term]) -> Self {
        Coverage { measured: terms.iter().filter(|t| t.measured).count(), total: terms.len() }
    }

    /// Fraction in `[0, 1]`. An empty aggregate is `0.0` — nothing was measured because
    /// there was nothing to measure, and that is still ignorance.
    pub fn fraction(&self) -> f64 {
        if self.total == 0 { 0.0 } else { self.measured as f64 / self.total as f64 }
    }

    /// Renderable as `2/9`.
    pub fn label(&self) -> String {
        format!("{}/{}", self.measured, self.total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_terms_take_the_neutral_element_not_zero() {
        // The whole point: an unmeasured multiplicative term is 1.0, so multiplying by it
        // is a no-op. The historical bug was writing 0.0 here and pinning the product.
        let t = Term::absent("U", "unknown factor", 1.0, "no source");
        assert_eq!(t.value, 1.0);
        assert!(!t.measured);
        assert_eq!(0.7 * t.value, 0.7);
    }

    #[test]
    fn coverage_distinguishes_a_confident_number_from_an_ignorant_one() {
        let a = Term::measured("A", "a", 0.5, "seen");
        let b = Term::absent("B", "b", 0.0, "unseen");
        assert_eq!(Coverage::of(&[&a, &b]).fraction(), 0.5);
        assert_eq!(Coverage::of(&[&a, &b]).label(), "1/2");
        assert_eq!(Coverage::of(&[]).fraction(), 0.0);
    }
}
