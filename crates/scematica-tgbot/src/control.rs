//! Changing the bot's behaviour — the only module here that writes.
//!
//! Everything goes through the File-Based IPC surface the sniper already watches. No new
//! channel is invented, per the workspace rule: the dashboard, the HTTP API and this bot
//! are three faces over one mechanism.
//!
//! ## Which file actually reaches the running sniper
//!
//! This is the part that decides whether a command is real or decorative, and it is not
//! obvious from the file names. Traced through `crates/scematica-sniper/src/main.rs`:
//!
//! | want | write | picked up |
//! |---|---|---|
//! | pause buys | `scematica-sell-mode.json` exists | ≤ 5 s |
//! | force-sell all | `scematica-dump-mode.json` exists | ≤ 5 s |
//! | rate mode | `scematica-rate-mode.json` `mode` | ≤ 5 s |
//! | a specific TP / SL | **`config.toml`** | ≤ 30 s |
//!
//! Two traps in that table, both found by reading the watchers rather than the filenames:
//!
//! **The rate-mode file's numbers are ignored.** When `mode` names a `[[sniper.rate_modes]]`
//! entry, the sniper applies *that entry's* TP, SL, size and escalations and discards
//! `tp_pct`/`sl_pct` from the file — its own comment says config is the single source of
//! truth. So `/mode` sends a name and this module reads the numbers back out of
//! `config.toml` to show the operator what will actually run. It also **refuses a name not
//! in the config**, because an unknown name falls into the sniper's fallback branch where
//! the file's numbers *do* apply — a typo would silently become a live TP.
//!
//! **Writing `tp_pct` into the rate-mode file does nothing.** The watcher dedups on
//! `mode:wallet_pct`, so a change to any other field never re-triggers; and even if it
//! did, a known mode name overwrites TP/SL from config on the next line. The only channel
//! that moves TP or SL on their own is `config.toml`, which the sniper polls by mtime. So
//! `/tp` and `/sl` edit that file — surgically, and never leaving it unparseable.

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use scematica_core::config::{BotConfig, RateMode, SniperConfig};
use scematica_core::metrics::{
    artifact_path, DUMP_MODE_FILE, HIGH_SPEED_FILE, MOON_CHASE_FILE, RATE_MODE_FILE, SELL_MODE_FILE,
};
use serde_json::{json, Value};

/// Write a JSON file the way every other writer in this workspace does: to `<file>.tmp`,
/// then rename. A reader must never see a half-written control file — the sniper polls
/// these every five seconds and a truncated read is a mode that silently fails to engage.
fn atomic_write(name: &str, value: &Value) -> Result<()> {
    let path = artifact_path(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let body = serde_json::to_string_pretty(value)?;
    std::fs::write(&tmp, body).with_context(|| format!("writing {name}.tmp"))?;
    std::fs::rename(&tmp, &path).with_context(|| format!("renaming {name}.tmp into place"))?;
    Ok(())
}

fn remove(name: &str) -> Result<()> {
    match std::fs::remove_file(artifact_path(name)) {
        Ok(()) => Ok(()),
        // Already gone is the desired state, not a failure.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("removing {name}")),
    }
}

// ── pause / resume ───────────────────────────────────────────────────────────

/// Pause buys. Open positions keep running their exit rules.
///
/// Deliberately not called "stop": it halts *entries* and never touches existing risk.
/// Conflating the two is how an operator ends up unable to stop buying without also being
/// forced to sell — the same split `/zero`'s kill switch makes.
pub fn pause(by: &str) -> Result<()> {
    atomic_write(SELL_MODE_FILE, &json!({ "enabled": true, "paused_by": by }))
}

pub fn resume() -> Result<()> {
    remove(SELL_MODE_FILE)
}

// ── dump ─────────────────────────────────────────────────────────────────────

/// Force-sell every open position at `min_out = 0`.
///
/// The most destructive control in the system: it accepts *any* price, so a thin pool
/// returns approximately nothing and the loss is unrecoverable. `commands.rs` requires a
/// typed confirmation before this is ever called; the function itself stays dumb, because
/// a guard living inside the action is a guard that the next caller forgets to want.
pub fn dump(by: &str) -> Result<()> {
    atomic_write(DUMP_MODE_FILE, &json!({ "enabled": true, "triggered_by": by }))
}

pub fn dump_off() -> Result<()> {
    remove(DUMP_MODE_FILE)
}

// ── presence-flag modes ──────────────────────────────────────────────────────

pub fn set_high_speed(on: bool) -> Result<()> {
    if on {
        atomic_write(HIGH_SPEED_FILE, &json!({ "enabled": true }))
    } else {
        remove(HIGH_SPEED_FILE)
    }
}

pub fn set_moon_chase(on: bool) -> Result<()> {
    if on {
        atomic_write(MOON_CHASE_FILE, &json!({ "enabled": true }))
    } else {
        remove(MOON_CHASE_FILE)
    }
}

// ── rate mode ────────────────────────────────────────────────────────────────

/// Find a rate mode by name, case-insensitively, among the enabled ones.
pub fn find_mode(config: &SniperConfig, name: &str) -> Option<RateMode> {
    config
        .rate_modes
        .iter()
        .find(|m| m.name.eq_ignore_ascii_case(name.trim()) && m.enabled)
        .cloned()
}

/// Switch rate mode.
///
/// Returns the config entry that will actually apply, so the caller reports the real
/// numbers rather than repeating what the operator asked for.
pub fn set_rate_mode(config: &SniperConfig, name: &str) -> Result<RateMode> {
    let Some(mode) = find_mode(config, name) else {
        let available: Vec<&str> = config
            .rate_modes
            .iter()
            .filter(|m| m.enabled)
            .map(|m| m.name.as_str())
            .collect();
        // Refused rather than passed through. An unknown name reaches the sniper's
        // fallback branch, where the file's own tp_pct/sl_pct become live parameters —
        // so a typo would not fail, it would quietly trade on whatever this file said.
        bail!(
            "no enabled rate mode called {:?}. Available: {}",
            name.trim(),
            available.join(", ")
        );
    };

    // Carry the numbers for the dashboard and the web UI, which read this file to display
    // the active mode. The sniper ignores them and re-derives from config; they are here
    // so a reader of the file is not left guessing.
    atomic_write(
        RATE_MODE_FILE,
        &json!({
            "mode":        mode.name,
            "tp_pct":      mode.take_profit_pct,
            "sl_pct":      mode.stop_loss_pct,
            "quote_amount": mode.quote_amount,
            "wallet_pct":  mode.wallet_pct,
            "multiplier":  1.0,
            "high_speed":  artifact_path(HIGH_SPEED_FILE).exists(),
            "set_by":      "telegram",
        }),
    )?;
    Ok(mode)
}

// ── TP / SL, via config.toml ─────────────────────────────────────────────────

/// Which `[sniper]` key to rewrite.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Param {
    TakeProfit,
    StopLoss,
}

impl Param {
    fn key(self) -> &'static str {
        match self {
            Param::TakeProfit => "take_profit_pct",
            Param::StopLoss => "stop_loss_pct",
        }
    }

    /// Bounds that keep a typo from becoming a position.
    ///
    /// A stop of 0 sells instantly and a stop of 100 never sells; a take-profit under 1%
    /// exits inside the AMM spread. These are sanity rails, not opinions about strategy —
    /// the wide upper bound on TP is deliberate, since the momentum escalator legitimately
    /// rides a target into the thousands.
    fn bounds(self) -> (f64, f64) {
        match self {
            Param::TakeProfit => (1.0, 100_000.0),
            Param::StopLoss => (1.0, 99.0),
        }
    }
}

/// The result of a parameter edit.
pub struct ParamChange {
    pub key: &'static str,
    pub from: f64,
    pub to: f64,
}

/// Rewrite one key inside `config.toml`'s `[sniper]` table.
///
/// Three properties, each load-bearing:
///
/// 1. **Scoped to the `[sniper]` table.** `take_profit_pct` also appears in every
///    `[[sniper.rate_modes]]` entry further down the file. A first-match replace would
///    silently retune a rate mode instead — and the change would only surface the next
///    time somebody selected it.
/// 2. **The file is re-parsed before the rename.** If the edit produced something
///    `BotConfig::from_file` cannot read, nothing is committed. An unparseable
///    `config.toml` does not merely fail the hot-reload; it stops the sniper starting at
///    all, and a Telegram command must not be able to do that.
/// 3. **Comments survive.** This is a line rewrite, not a serialise round-trip. Every
///    threshold in that file carries a comment recording what it cost to learn, and a
///    `toml::to_string` would erase all of them.
pub fn set_param(config_path: &str, param: Param, value: f64) -> Result<ParamChange> {
    let (lo, hi) = param.bounds();
    if !value.is_finite() || value < lo || value > hi {
        bail!("{} must be between {lo} and {hi} (got {value})", param.key());
    }

    let original = std::fs::read_to_string(config_path)
        .with_context(|| format!("reading {config_path}"))?;

    let before = BotConfig::from_file(config_path)
        .with_context(|| format!("{config_path} does not currently parse — refusing to edit it"))?;
    let from = match param {
        Param::TakeProfit => before.sniper.take_profit_pct,
        Param::StopLoss => before.sniper.stop_loss_pct,
    };

    let mut out = String::with_capacity(original.len() + 16);
    let mut in_sniper = false;
    let mut replaced = false;

    for line in original.lines() {
        let trimmed = line.trim_start();
        // A new table header ends the `[sniper]` section — including `[sniper.filters]`
        // and `[[sniper.rate_modes]]`, which are different tables that happen to share
        // the prefix and contain the same key names.
        if trimmed.starts_with('[') {
            in_sniper = trimmed.starts_with("[sniper]");
        } else if in_sniper && !replaced {
            let is_key = trimmed
                .split('=')
                .next()
                .map(|k| k.trim() == param.key())
                .unwrap_or(false);
            if is_key {
                // Keep whatever trailing comment the line carried: it is the record of why
                // the old value was what it was, and it stays true about the old value.
                let comment = line.find('#').map(|i| format!("  {}", &line[i..])).unwrap_or_default();
                out.push_str(&format!("{} = {}{}\n", param.key(), fmt(value), comment));
                replaced = true;
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
    }

    if !replaced {
        bail!(
            "could not find `{}` in the [sniper] table of {config_path}",
            param.key()
        );
    }

    // Write, verify, then commit. The verification reads the TEMP file, so a bad edit is
    // never visible to the sniper even momentarily.
    let path = PathBuf::from(config_path);
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tgbot.tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, &out).with_context(|| format!("writing {config_path}.tgbot.tmp"))?;

    let verified = match BotConfig::from_file(&tmp) {
        Ok(c) => c,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            bail!("the edit would have made {config_path} unparseable ({e}) — nothing was changed");
        }
    };
    let landed = match param {
        Param::TakeProfit => verified.sniper.take_profit_pct,
        Param::StopLoss => verified.sniper.stop_loss_pct,
    };
    if (landed - value).abs() > 1e-9 {
        let _ = std::fs::remove_file(&tmp);
        // The line that was rewritten was not the one that feeds `[sniper]`. Refusing is
        // the only safe answer: something else in the file now holds the value.
        bail!(
            "the edit did not take effect as intended (expected {value}, config reads {landed}) \
             — nothing was changed"
        );
    }

    std::fs::rename(&tmp, &path).with_context(|| format!("renaming into {config_path}"))?;
    Ok(ParamChange { key: param.key(), from, to: value })
}

/// Format a float back into TOML without gaining or losing precision visually.
///
/// `175` must be written `175.0`: the field is an `f64`, and TOML types an unsuffixed
/// integer as an integer, which fails to deserialise into one.
fn fmt(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{v:.1}")
    } else {
        format!("{v}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_whole_number_keeps_its_toml_float_form() {
        // `take_profit_pct = 175` is an integer to TOML and fails to deserialise as f64.
        assert_eq!(fmt(175.0), "175.0");
        assert_eq!(fmt(12.5), "12.5");
    }

    #[test]
    fn bounds_reject_the_values_that_break_a_position() {
        assert!(Param::StopLoss.bounds().0 >= 1.0);
        assert!(Param::StopLoss.bounds().1 <= 99.0);
        // A take-profit ceiling has to clear the escalation ladder, which legitimately
        // reaches the thousands — a "sensible" cap of 1000 would refuse a real config.
        assert!(Param::TakeProfit.bounds().1 >= 10_000.0);
    }

    /// The defect the section-scoping exists to prevent, exercised on a miniature file.
    #[test]
    fn the_edit_is_scoped_to_the_sniper_table() {
        let src = "\
[sniper]
take_profit_pct = 175.0   # baseline
stop_loss_pct = 10.0

[[sniper.rate_modes]]
name = \"Micro\"
take_profit_pct = 50.0
";
        // Reproduce the scoping loop over the fixture. A naive first-match replace would
        // be correct here too (the [sniper] key comes first), so the case that matters is
        // the second occurrence staying untouched.
        let mut out = String::new();
        let mut in_sniper = false;
        let mut replaced = false;
        for line in src.lines() {
            let trimmed = line.trim_start();
            if trimmed.starts_with('[') {
                in_sniper = trimmed.starts_with("[sniper]");
            } else if in_sniper && !replaced {
                if trimmed.split('=').next().map(|k| k.trim() == "take_profit_pct").unwrap_or(false)
                {
                    let comment =
                        line.find('#').map(|i| format!("  {}", &line[i..])).unwrap_or_default();
                    out.push_str(&format!("take_profit_pct = {}{}\n", fmt(220.0), comment));
                    replaced = true;
                    continue;
                }
            }
            out.push_str(line);
            out.push('\n');
        }
        assert!(replaced);
        assert!(out.contains("take_profit_pct = 220.0   # baseline"));
        // The rate mode's own value is a different table and must not have moved.
        assert!(out.contains("take_profit_pct = 50.0"));
        // And the comment recording why the old value was chosen survived.
        assert!(out.contains("# baseline"));
    }

    #[test]
    fn a_subtable_header_ends_the_sniper_section() {
        // `[sniper.filters]` shares the prefix and is a different table. Testing
        // `starts_with("[sniper")` instead of `"[sniper]"` would keep the section open
        // across it and rewrite the wrong key.
        assert!(!"[sniper.filters]".starts_with("[sniper]"));
        assert!(!"[[sniper.rate_modes]]".starts_with("[sniper]"));
        assert!("[sniper]".starts_with("[sniper]"));
    }
}
