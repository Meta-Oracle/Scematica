//! How old is this pool, and can we say at all?
//!
//! One implementation of a rule that was previously written out three times — twice in the
//! scorers and once, differently and wrongly, inline in `sniper.rs`. The two scorer copies
//! agreed; the sniper's did not, and the disagreement was expensive.
//!
//! ## What went wrong
//!
//! `CachedPool::open_time` is the pool's on-chain opening timestamp, and it is **usually
//! absent**. pump.fun migrations and whale-copy pools set it to `0` outright, and Raydium
//! frequently leaves it unset on new pools — which are precisely the pools this bot exists
//! to trade. Both scorers noticed and fall back to the moment the sniper *detected* the
//! pool, treating an unusable timestamp as unknown rather than as zero. `pool_scorer`'s own
//! doc says why: returning `0.0` velocity there *"would read as measured, and stalled, and
//! penalise the pool."*
//!
//! `sniper.rs` computed its own age with a bare `else { 0 }` and no fallback, and that value
//! — not the scorers' — is what reached the decision log, the Deep Q\* state and two gates.
//! Across 9,214 logged decisions the consequences were:
//!
//! - `pool_age_secs` carried a non-zero value **twice**.
//! - `historical_velocity_sol_per_sec`, guarded on `pool_age_secs > 0`, **never once**.
//! - The DQ\* state's `price_velocity` derives from that velocity, so one of the net's
//!   twenty-four input features was a constant.
//! - The aggressive-sizing gate's `velocity >= 2.618` disjunct could never be true.
//! - The elite gate's historical-velocity branch was unreachable.
//!
//! None of it announced itself. Every number had the right type and a plausible value, and
//! the only way to see it was to count how often the field varied — which is what
//! `crate::measure` now does as a standing report.
//!
//! ## The rule
//!
//! **Detection time is a measurement, not a guess.** The sniper genuinely observed this pool
//! at that moment, so "at most this old" is something it counted. It is a *bound* rather
//! than the true opening time, and callers that care about the difference should say so —
//! but it is real, and it is why the fallback is legitimate rather than a fabricated number.
//!
//! **`None` is not zero.** An age nobody can establish returns `None` so that a caller has
//! to decide what to do about it, rather than receiving a `0` that reads as "brand new" —
//! the most attractive possible pool.

/// A detection timestamp older than this is not evidence about a *new* pool.
///
/// Sixty seconds because the fallback's whole justification is that the sniper saw the pool
/// moments ago. Reaching further back would quietly turn a stale cache entry into a
/// confident age.
const DETECTION_FRESH_SECS: u64 = 60;

/// Clock skew beyond this means the timestamp is not describing this universe.
const SKEW_TOLERANCE_SECS: u64 = 30;

/// The timestamp to measure age from, or `None` when nothing supports one.
///
/// Prefers the on-chain `open_time`; falls back to a *recent* detection timestamp; gives up
/// otherwise. Giving up is a real outcome and the common one for pump.fun migrations.
pub fn effective_open_time(
    open_time: u64,
    detected_at_secs: u64,
    now_secs: u64,
) -> Option<u64> {
    if open_time > 0 {
        return Some(open_time);
    }
    if detected_at_secs > 0 && detected_at_secs >= now_secs.saturating_sub(DETECTION_FRESH_SECS) {
        return Some(detected_at_secs);
    }
    None
}

/// Age in seconds, or `None` when it cannot be established.
///
/// A timestamp far in the future is `None` rather than a large age: it is clock skew or a
/// fabricated value, and "this pool is very old" is a different claim from "this timestamp
/// is nonsense". Callers that want to *penalise* a skewed pool should check for `None` and
/// decide, rather than receiving a number that silently sorts alongside real ages.
pub fn age_secs(open_time: u64, detected_at_secs: u64, now_secs: u64) -> Option<u64> {
    let eff = effective_open_time(open_time, detected_at_secs, now_secs)?;
    if eff > now_secs.saturating_add(SKEW_TOLERANCE_SECS) {
        return None;
    }
    Some(now_secs.saturating_sub(eff))
}

/// SOL per second of pool growth since it opened, or `None`.
///
/// `None` when the age is unknown **or zero**: dividing by a zero age is not a very large
/// velocity, it is an undefined one, and a pool observed in the same second it opened has
/// not yet demonstrated a rate.
pub fn velocity_sol_per_sec(
    pool_size_sol: f64,
    open_time: u64,
    detected_at_secs: u64,
    now_secs: u64,
) -> Option<f64> {
    let age = age_secs(open_time, detected_at_secs, now_secs)?;
    if age == 0 || !pool_size_sol.is_finite() || pool_size_sol <= 0.0 {
        return None;
    }
    Some(pool_size_sol / age as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_000_000;

    #[test]
    fn an_on_chain_open_time_is_preferred() {
        assert_eq!(effective_open_time(NOW - 120, NOW - 5, NOW), Some(NOW - 120));
    }

    #[test]
    fn a_recent_detection_stands_in_when_the_chain_did_not_say() {
        // The case that covers essentially every pump.fun migration, and the one the
        // sniper's inline copy did not handle.
        assert_eq!(effective_open_time(0, NOW - 5, NOW), Some(NOW - 5));
        assert_eq!(age_secs(0, NOW - 5, NOW), Some(5));
    }

    #[test]
    fn a_stale_detection_is_not_evidence_about_a_new_pool() {
        // Reaching further back would turn an old cache entry into a confident age.
        assert_eq!(effective_open_time(0, NOW - 600, NOW), None);
        assert_eq!(age_secs(0, NOW - 600, NOW), None);
    }

    #[test]
    fn nothing_at_all_is_none_and_never_zero() {
        // The bug this module exists to prevent. Zero age reads as "brand new", which is
        // the most attractive possible pool — so an unknown age must not produce it.
        assert_eq!(effective_open_time(0, 0, NOW), None);
        assert_eq!(age_secs(0, 0, NOW), None);
        assert_ne!(age_secs(0, 0, NOW), Some(0));
    }

    #[test]
    fn a_future_timestamp_is_nonsense_rather_than_an_age() {
        // Clock skew or a fabricated value. Returning a large age would sort it alongside
        // real ones; `None` makes the caller decide.
        assert_eq!(age_secs(NOW + 10_000, 0, NOW), None);
        // Inside the skew tolerance it is treated as "just opened".
        assert_eq!(age_secs(NOW + 5, 0, NOW), Some(0));
    }

    #[test]
    fn velocity_is_undefined_at_zero_age_rather_than_enormous() {
        assert_eq!(velocity_sol_per_sec(10.0, NOW, 0, NOW), None);
        assert_eq!(velocity_sol_per_sec(10.0, 0, 0, NOW), None);
    }

    #[test]
    fn velocity_divides_by_a_real_age() {
        assert_eq!(velocity_sol_per_sec(10.0, NOW - 5, 0, NOW), Some(2.0));
        assert_eq!(velocity_sol_per_sec(10.0, 0, NOW - 4, NOW), Some(2.5));
    }

    #[test]
    fn an_empty_or_broken_pool_size_has_no_velocity() {
        assert_eq!(velocity_sol_per_sec(0.0, NOW - 5, 0, NOW), None);
        assert_eq!(velocity_sol_per_sec(f64::NAN, NOW - 5, 0, NOW), None);
    }
}
