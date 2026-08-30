//! How long the pipeline takes to arrive at a decision.
//!
//! The repository's own notes call the central product problem structural: **the bot arrives
//! post-pump.** No amount of filter tuning touches that, and the recorded oscillation —
//! strictness selecting for parabolic peaks, then escalation chasing the top — is what
//! happens when it is attacked from the selection side instead.
//!
//! Attacking it from the latency side needs a number that does not exist yet. The execution
//! path is already instrumented: `TxTelemetryEvent` carries `elapsed_ms`,
//! `blockhash_fetch_ms_total`, `send_confirm_ms_total`. What nothing measures is the part
//! *before* that — the gap between the listener seeing a pool and the pipeline deciding what
//! to do about it, which is where the filter round trips live.
//!
//! So this records one span: **detection to decision**. It is deliberately the smallest
//! useful thing rather than a full tracing layer, because the point is to establish a
//! baseline before changing anything, and a measurement nobody can interpret is not a
//! baseline.
//!
//! ## Why a side table rather than a parameter
//!
//! `write_pool_decision` is reached from twenty-six call sites in the buy path. Threading a
//! start time through all of them is a large diff in a live trading loop for a diagnostic,
//! and a large diff there is how a trading bug gets introduced by a measurement. A pool is
//! marked on arrival and looked up at the decision, which changes no control flow at all.
//!
//! ## What it does not claim
//!
//! An unmarked pool reports `None`, not zero. A pool the listener never marked — because the
//! process restarted mid-flight, or because it arrived by a path that does not mark — has an
//! *unmeasured* latency, and a zero there would read as "arrived instantly", which is the
//! most flattering possible reading of the exact thing under investigation.

use std::collections::HashMap;
use std::time::Instant;

use parking_lot::Mutex;

/// Most pools tracked at once.
///
/// Bounded because this is fed by an unbounded event stream and never drained on the paths
/// where a pool is dropped before any decision is written. A leak in the sniper is not a
/// leak in a report — it is a leak in the process that holds the money.
const MAX_TRACKED: usize = 4_096;

/// Entries older than this are stale: the pool was seen and never decided.
const STALE_SECS: u64 = 300;

static SEEN: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);

/// Mark a pool as seen, at the moment the listener produced it.
///
/// Called before any filtering, so the span covers the whole pipeline rather than the part
/// that happened to run.
pub fn mark_seen(pool_id: &str) {
    let mut guard = SEEN.lock();
    let map = guard.get_or_insert_with(HashMap::new);

    if map.len() >= MAX_TRACKED {
        // Drop what is certainly finished before dropping anything else. A pool seen five
        // minutes ago and never decided is not going to be decided.
        let now = Instant::now();
        map.retain(|_, t| now.duration_since(*t).as_secs() < STALE_SECS);
        if map.len() >= MAX_TRACKED {
            // Still full: the listener is outrunning the pipeline by more than the table
            // holds. Clearing loses measurements, which is the correct thing to lose —
            // the alternative is unbounded growth in the process that holds the money.
            map.clear();
        }
    }
    map.insert(pool_id.to_string(), Instant::now());
}

/// Milliseconds since this pool was marked, or `None` if it never was.
///
/// Consuming: a pool decided twice would otherwise report the span to the *first* decision
/// both times, which silently understates the second.
pub fn since_seen_ms(pool_id: &str) -> Option<u64> {
    let mut guard = SEEN.lock();
    let map = guard.as_mut()?;
    let started = map.remove(pool_id)?;
    Some(Instant::now().duration_since(started).as_millis() as u64)
}

/// How many pools are currently being tracked. For tests and diagnostics.
pub fn tracked() -> usize {
    SEEN.lock().as_ref().map_or(0, |m| m.len())
}

/// Forget everything. Tests only — the sniper never wants this.
#[cfg(test)]
fn reset() {
    *SEEN.lock() = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// These share one process-global table, so they run under one lock rather than in
    /// parallel. A shared global is the right shape here — the PID lockfile already
    /// guarantees one sniper per machine — but it does mean the tests must not interleave.
    static GUARD: Mutex<()> = Mutex::new(());

    #[test]
    fn an_unmarked_pool_is_unmeasured_and_never_zero() {
        let _g = GUARD.lock();
        reset();
        // Zero would read as "arrived instantly", which is the most flattering possible
        // reading of the exact thing under investigation.
        assert_eq!(since_seen_ms("never-seen"), None);
    }

    #[test]
    fn a_marked_pool_reports_a_span() {
        let _g = GUARD.lock();
        reset();
        mark_seen("pool-a");
        let ms = since_seen_ms("pool-a");
        assert!(ms.is_some());
        assert!(ms.unwrap() < 5_000, "a span of {ms:?} ms is not plausible in a test");
    }

    #[test]
    fn reading_a_span_consumes_it() {
        // A pool decided twice would otherwise report the span to the first decision both
        // times, silently understating the second.
        let _g = GUARD.lock();
        reset();
        mark_seen("pool-b");
        assert!(since_seen_ms("pool-b").is_some());
        assert_eq!(since_seen_ms("pool-b"), None);
    }

    #[test]
    fn the_table_is_bounded() {
        // Fed by an unbounded event stream and not drained on every path. A leak here is a
        // leak in the process that holds the money.
        let _g = GUARD.lock();
        reset();
        for i in 0..(MAX_TRACKED + 500) {
            mark_seen(&format!("pool-{i}"));
        }
        assert!(tracked() <= MAX_TRACKED, "tracked {} entries", tracked());
    }

    #[test]
    fn pools_are_tracked_independently() {
        let _g = GUARD.lock();
        reset();
        mark_seen("x");
        mark_seen("y");
        assert!(since_seen_ms("x").is_some());
        assert!(since_seen_ms("y").is_some(), "reading one must not drop the other");
    }
}
