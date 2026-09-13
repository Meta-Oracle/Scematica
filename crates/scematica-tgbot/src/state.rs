//! Reading the bot's state — the File-Based IPC surface, and nothing else.
//!
//! This module is **read-only**. It opens no socket, takes no lock, and writes nothing;
//! it is safe against a live sniper by construction, the same posture as `measure` and
//! `mesh-dashboard`. Everything that writes lives in `control.rs`, alone.
//!
//! ## Absent is not zero
//!
//! Every reader here returns an `Option` and never a default. A missing
//! `scematica-metrics.json` means the sniper has not written one; it does not mean zero
//! trades, zero PnL and a zero win rate. Rendering those as `0` would put a confident set
//! of numbers on screen for a bot that is not running, which is the failure mode this
//! repository has paid for in `Term`, in `FeatureMask`, in the mesh's tri-state edges and
//! in `/escrow`'s three-way verdict. `render.rs` prints `—` for a `None`.
//!
//! ## Freshness is a separate question from existence
//!
//! `scematica-metrics.json` is rewritten every five seconds. A file that exists says the
//! sniper *was* here; only its age says whether it is here *now*. Both are reported,
//! because "the bot is stopped" and "the bot is running and losing money" are answered by
//! the same file and must not look alike.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use scematica_core::metrics::{
    artifact_path, MetricsSnapshot, StrategySnapshot, TradeEvent, DUMP_MODE_FILE,
    FILTER_STATS_FILE, HIGH_SPEED_FILE, LOCK_FILE, LOG_FILE, METRICS_FILE, MOON_CHASE_FILE,
    NN_ADVICE_FILE, NN_STATS_FILE, POOL_DECISIONS_FILE, POSITIONS_FILE, RATE_MODE_FILE,
    SELL_MODE_FILE, STRATEGY_FILE, TRADES_FILE,
};
use serde_json::Value;

/// How stale `scematica-metrics.json` may be before the sniper counts as not running.
///
/// The writer's period is 5s. Six times that leaves room for a slow disk and a busy
/// machine without letting a stopped bot read as live for a minute.
pub const STALE_AFTER_SECS: u64 = 30;

fn read_string(name: &str) -> Option<String> {
    std::fs::read_to_string(artifact_path(name)).ok()
}

fn read_json(name: &str) -> Option<Value> {
    serde_json::from_str(&read_string(name)?).ok()
}

fn path_of(name: &str) -> PathBuf {
    artifact_path(name)
}

/// Seconds since a file was last written. `None` when it does not exist.
fn age_secs(name: &str) -> Option<u64> {
    let meta = std::fs::metadata(path_of(name)).ok()?;
    let modified = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
    Some(now.as_secs().saturating_sub(modified.as_secs()))
}

/// Whether the sniper is running, and how confident that answer is.
#[derive(Debug, Clone, PartialEq)]
pub enum Liveness {
    /// State is being written now.
    Live { metrics_age_secs: u64 },
    /// A lock file or state exists but nothing has been written recently. The bot was
    /// here; it is not writing. Distinct from `Stopped` because a wedged process and an
    /// absent one need different actions from the operator.
    Stale { metrics_age_secs: u64, pid: Option<u32> },
    /// No state at all.
    Stopped,
}

impl Liveness {
    pub fn is_live(&self) -> bool {
        matches!(self, Liveness::Live { .. })
    }
}

pub fn liveness() -> Liveness {
    let pid = read_string(LOCK_FILE).and_then(|s| s.trim().parse::<u32>().ok());
    match age_secs(METRICS_FILE) {
        Some(age) if age <= STALE_AFTER_SECS => Liveness::Live { metrics_age_secs: age },
        Some(age) => Liveness::Stale { metrics_age_secs: age, pid },
        None => Liveness::Stopped,
    }
}

pub fn metrics() -> Option<MetricsSnapshot> {
    serde_json::from_value(read_json(METRICS_FILE)?).ok()
}

pub fn strategy() -> Option<StrategySnapshot> {
    StrategySnapshot::load_from_file(STRATEGY_FILE)
}

pub fn filter_stats() -> Option<Value> {
    read_json(FILTER_STATS_FILE)
}

pub fn nn_stats() -> Option<Value> {
    read_json(NN_STATS_FILE)
}

pub fn nn_advice() -> Option<Value> {
    read_json(NN_ADVICE_FILE)
}

pub fn positions() -> Option<Value> {
    read_json(POSITIONS_FILE)
}

/// The three control files, as the sniper sees them.
///
/// Presence is the signal for sell and dump mode — the sniper's watchers test for the
/// file, so a file holding `{"enabled": false}` is still an engaged mode. Reading only the
/// `enabled` field would report a paused bot as running.
#[derive(Debug, Clone)]
pub struct Controls {
    pub sell_mode: bool,
    pub sell_mode_reason: Option<String>,
    pub dump_mode: bool,
    pub high_speed: bool,
    pub moon_chase: bool,
    /// The rate mode NAME from the file. The numbers behind it come from `config.toml`,
    /// never from here — see `control.rs`.
    pub rate_mode: Option<String>,
}

pub fn controls() -> Controls {
    let sell = read_json(SELL_MODE_FILE);
    Controls {
        sell_mode: path_of(SELL_MODE_FILE).exists(),
        sell_mode_reason: sell
            .as_ref()
            .and_then(|v| v.get("paused_by").or_else(|| v.get("reason")))
            .and_then(|v| v.as_str())
            .map(str::to_string),
        dump_mode: path_of(DUMP_MODE_FILE).exists(),
        high_speed: path_of(HIGH_SPEED_FILE).exists(),
        moon_chase: path_of(MOON_CHASE_FILE).exists(),
        rate_mode: read_json(RATE_MODE_FILE)
            .and_then(|v| v.get("mode").and_then(|m| m.as_str()).map(str::to_string)),
    }
}

/// The last `n` lines of a JSONL file, newest last.
///
/// Reads the whole file. These are append-only logs that reach tens of megabytes over a
/// long session, so this is deliberately capped by the caller and never used on a timer —
/// a Telegram command is a human pressing a button, not a poll.
fn tail_jsonl(name: &str, n: usize) -> Option<Vec<Value>> {
    let data = read_string(name)?;
    let mut out: Vec<Value> = data
        .lines()
        .rev()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|l| serde_json::from_str(l).ok())
        .take(n)
        .collect();
    out.reverse();
    Some(out)
}

pub fn recent_trades(n: usize) -> Option<Vec<TradeEvent>> {
    let raw = tail_jsonl(TRADES_FILE, n)?;
    Some(raw.into_iter().filter_map(|v| serde_json::from_value(v).ok()).collect())
}

pub fn recent_decisions(n: usize) -> Option<Vec<Value>> {
    tail_jsonl(POOL_DECISIONS_FILE, n)
}

/// The last `n` lines of the sniper log.
pub fn log_tail(n: usize) -> Option<Vec<String>> {
    let data = read_string(LOG_FILE)?;
    let mut out: Vec<String> = data
        .lines()
        .rev()
        .filter(|l| !l.trim().is_empty())
        .take(n)
        .map(str::to_string)
        .collect();
    out.reverse();
    Some(out)
}

/// Realised PnL over the last `n` settled sells, and the count that produced it.
///
/// Sells only. A buy has no realised PnL and including one at 0.0 would drag the average
/// toward zero in proportion to how *active* the bot is rather than how well it did.
pub fn realised(n: usize) -> Option<(f64, usize, usize)> {
    let trades = recent_trades(n)?;
    let sells: Vec<&TradeEvent> = trades.iter().filter(|t| t.kind == "SELL").collect();
    if sells.is_empty() {
        return Some((0.0, 0, 0));
    }
    let total: f64 = sells.iter().map(|t| t.pnl).sum();
    let wins = sells.iter().filter(|t| t.pnl > 0.0).count();
    Some((total, wins, sells.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn liveness_distinguishes_stopped_from_stale() {
        // Not a filesystem test — the point is that the three arms are distinct values.
        // Collapsing `Stale` into `Stopped` is the tempting simplification and it loses
        // the one case an operator has to act on differently: a process that is still
        // holding the lock and no longer writing.
        let stopped = Liveness::Stopped;
        let stale = Liveness::Stale { metrics_age_secs: 900, pid: Some(1234) };
        let live = Liveness::Live { metrics_age_secs: 2 };
        assert_ne!(stopped, stale);
        assert_ne!(stale, live);
        assert!(live.is_live());
        assert!(!stale.is_live());
        assert!(!stopped.is_live());
    }

    #[test]
    fn absent_state_is_none_rather_than_a_default() {
        // Reading a file that certainly does not exist must not manufacture a snapshot.
        // A `MetricsSnapshot::default()` here would report a stopped bot as one that has
        // taken zero trades for zero profit, which is a claim nobody measured.
        let missing = serde_json::from_str::<MetricsSnapshot>("").ok();
        assert!(missing.is_none());
    }
}
