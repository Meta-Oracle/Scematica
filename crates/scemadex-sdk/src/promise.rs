//! **Domain-agnostic bonding** — the seam that unshackles the rail from swaps.
//!
//! Conviction Routing began as a swap product: a bond guaranteed a *minimum
//! output*, and settlement asked one hardcoded question — did the realized fill
//! clear that minimum? But nothing about "an agent stakes collateral on a promise,
//! and loses it if reality disagrees" is swap-specific. A forecast is a promise. A
//! classification is a promise. A latency SLA is a promise.
//!
//! This module names the two halves that were previously fused into the swap
//! engine:
//!
//! - [`Promise`] — *what was guaranteed*. `MinOutput` is the swap case; other
//!   variants describe other domains.
//! - [`Outcome`] — *how realized evidence judges a promise*. A swap [`Fill`] is one
//!   implementor; a [`Measurement`] (for forecasts) is another. Any type that can
//!   answer "does this evidence satisfy that promise?" plugs the rail into a new
//!   domain **without touching the settlement machine**, which already speaks only
//!   [`BondOutcome`].
//!
//! The swap path is unchanged in behavior — [`crate::bond::NoBondEngine`] and
//! [`crate::bond::EscrowBondEngine`] now compute their outcome via
//! `fill.evaluate(&bond.promise())`, i.e. through this exact seam. `Fill` really is
//! just one `Outcome` impl.

use serde::{Deserialize, Serialize};

use crate::bond::BondOutcome;
use crate::route::Fill;

/// What a bond guaranteed — the thing realized evidence is judged against.
///
/// `#[non_exhaustive]`: new domains add variants without breaking downstream
/// `match`es, so generalizing the rail never forces a breaking change.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum Promise {
    /// Realized output (base units) must be at least this. The swap guarantee.
    MinOutput(u64),
    /// A directional forecast: honors iff the realized value lands on the
    /// predicted side of `strike` (at or above when `expect_above`, else below).
    Forecast { strike: f64, expect_above: bool },
}

/// Realized evidence that judges a [`Promise`] into a [`BondOutcome`].
///
/// This is the extension point for new domains. Implement it for your evidence
/// type and the whole settlement / counter-market / scar / insurance stack works
/// unchanged — none of them know or care whether the underlying decision was a
/// swap, a forecast, or something else.
pub trait Outcome: Send + Sync {
    /// Judge this evidence against a promise. An implementor should return
    /// [`BondOutcome::Slashed`] for a promise it cannot speak to, so a
    /// mismatched (evidence, promise) pairing fails safe.
    fn evaluate(&self, promise: &Promise) -> BondOutcome;
    /// Unix second the evidence was observed — feeds the settlement window/deadline.
    fn observed_unix(&self) -> u64;
}

/// A swap fill judges a `MinOutput` promise: honored iff it cleared the guarantee.
impl Outcome for Fill {
    fn evaluate(&self, promise: &Promise) -> BondOutcome {
        match promise {
            Promise::MinOutput(min) => {
                if self.amount_out.raw >= *min {
                    BondOutcome::Honored
                } else {
                    BondOutcome::Slashed
                }
            }
            // A swap fill cannot satisfy a non-swap promise — fail safe.
            _ => BondOutcome::Slashed,
        }
    }

    fn observed_unix(&self) -> u64 {
        self.executed_unix
    }
}

/// A resolved scalar measurement — the ground truth a forecast bond is judged on
/// (a price at expiry, a realized metric, an oracle read).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Measurement {
    pub value: f64,
    pub observed_unix: u64,
}

impl Measurement {
    pub fn new(value: f64, observed_unix: u64) -> Self {
        Self {
            value,
            observed_unix,
        }
    }
}

/// A measurement judges a `Forecast` promise: honored iff it landed on the
/// predicted side of the strike.
impl Outcome for Measurement {
    fn evaluate(&self, promise: &Promise) -> BondOutcome {
        match promise {
            Promise::Forecast {
                strike,
                expect_above,
            } => {
                let landed_above = self.value >= *strike;
                if landed_above == *expect_above {
                    BondOutcome::Honored
                } else {
                    BondOutcome::Slashed
                }
            }
            _ => BondOutcome::Slashed,
        }
    }

    fn observed_unix(&self) -> u64 {
        self.observed_unix
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::Amount;

    fn fill(amount_out: u64) -> Fill {
        Fill {
            amount_out: Amount::new(amount_out, 6),
            executed_unix: 7,
        }
    }

    #[test]
    fn swap_fill_judges_min_output() {
        let promise = Promise::MinOutput(1_000);
        assert_eq!(fill(1_000).evaluate(&promise), BondOutcome::Honored);
        assert_eq!(fill(1_500).evaluate(&promise), BondOutcome::Honored);
        assert_eq!(fill(999).evaluate(&promise), BondOutcome::Slashed);
        assert_eq!(fill(1_000).observed_unix(), 7);
    }

    #[test]
    fn swap_fill_fails_safe_against_a_forecast_promise() {
        let promise = Promise::Forecast {
            strike: 100.0,
            expect_above: true,
        };
        assert_eq!(fill(999_999).evaluate(&promise), BondOutcome::Slashed);
    }

    #[test]
    fn measurement_judges_a_bullish_forecast() {
        let promise = Promise::Forecast {
            strike: 100.0,
            expect_above: true,
        };
        // Landed above → the bull was right.
        assert_eq!(
            Measurement::new(105.0, 0).evaluate(&promise),
            BondOutcome::Honored
        );
        // Landed below → slashed.
        assert_eq!(
            Measurement::new(95.0, 0).evaluate(&promise),
            BondOutcome::Slashed
        );
    }

    #[test]
    fn measurement_judges_a_bearish_forecast() {
        let promise = Promise::Forecast {
            strike: 100.0,
            expect_above: false,
        };
        assert_eq!(
            Measurement::new(95.0, 0).evaluate(&promise),
            BondOutcome::Honored
        );
        assert_eq!(
            Measurement::new(100.0, 0).evaluate(&promise),
            BondOutcome::Slashed
        );
    }

    #[test]
    fn measurement_fails_safe_against_a_swap_promise() {
        assert_eq!(
            Measurement::new(1.0, 0).evaluate(&Promise::MinOutput(0)),
            BondOutcome::Slashed
        );
    }
}
