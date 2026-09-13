//! Turning state into messages.
//!
//! One rule shapes the whole file, and it is the same one `scema_policy::render::cell`,
//! `lib/mesh/view.ts` and `lib/zero/types.ts::cell` are built on:
//!
//! **An unmeasured value prints an em dash, never `0.00`.**
//!
//! A `None` here means the sniper has not written that file. Printing `0` for it produces
//! a status report that reads as a running bot which has taken no trades and made no
//! money — a confident set of numbers describing something nobody measured. `dash()` is
//! the only place a missing value becomes text, so the rule has one implementation.
//!
//! Everything that did not originate as a literal in this crate goes through
//! `api::esc()`. Symbols come off the chain and are chosen by whoever launched the pool.

use scematica_core::config::SniperConfig;
use scematica_core::metrics::{MetricsSnapshot, TradeEvent};
use serde_json::Value;

use crate::api::esc;
use crate::state::{Controls, Liveness};

/// The one place a missing measurement becomes a string.
pub fn dash<T: std::fmt::Display>(v: Option<T>) -> String {
    match v {
        Some(v) => v.to_string(),
        None => "—".into(),
    }
}

fn f2(v: Option<f64>) -> String {
    dash(v.map(|v| format!("{v:.2}")))
}

fn f4(v: Option<f64>) -> String {
    dash(v.map(|v| format!("{v:.4}")))
}

/// A mint, shortened for a phone screen but never invented.
///
/// Truncation is marked with an ellipsis so a reader can tell a shortened address from a
/// short one, and the full mint is always available through `/positions`' detail lines.
fn short_mint(m: &str) -> String {
    if m.len() <= 12 {
        return esc(m);
    }
    esc(&format!("{}…{}", &m[..4], &m[m.len() - 4..]))
}

fn hms(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 {
        format!("{m}m {s}s")
    } else {
        format!("{s}s")
    }
}

// ── status ───────────────────────────────────────────────────────────────────

pub fn liveness_line(l: &Liveness) -> String {
    match l {
        Liveness::Live { metrics_age_secs } => {
            format!("🟢 <b>RUNNING</b> — state written {metrics_age_secs}s ago")
        }
        // The distinction that costs money if collapsed: a wedged process is still holding
        // the single-instance lock, so restarting will refuse, and the operator needs to
        // know to kill it rather than wonder why `sniper` will not start.
        Liveness::Stale { metrics_age_secs, pid } => format!(
            "🟠 <b>NOT WRITING</b> — last state {} ago{}\nThe process may be wedged. It still holds the \
             single-instance lock, so a restart will refuse until it is stopped.",
            hms(*metrics_age_secs),
            match pid {
                Some(p) => format!(" (lockfile PID {p})"),
                None => String::new(),
            }
        ),
        Liveness::Stopped => "⚫ <b>STOPPED</b> — no state files. The sniper is not running.".into(),
    }
}

pub fn status(
    live: &Liveness,
    m: Option<&MetricsSnapshot>,
    c: &Controls,
    config: &SniperConfig,
    open_positions: Option<usize>,
) -> String {
    let mut s = String::new();
    s.push_str("<b>SCEMATICA — STATUS</b>\n\n");
    s.push_str(&liveness_line(live));
    s.push_str("\n\n");

    match m {
        Some(m) => {
            s.push_str(&format!(
                "<b>PnL</b>        {:+.4} SOL\n\
                 <b>Trades</b>     {} attempted · {} confirmed · {} failed\n\
                 <b>Fill rate</b>  {:.1}%\n\
                 <b>Pools</b>      {} tracked\n\
                 <b>Uptime</b>     {}\n",
                m.total_pnl_sol(),
                m.trades_attempted,
                m.trades_confirmed,
                m.trades_failed,
                m.win_rate(),
                m.pools_tracked,
                hms(m.uptime_secs),
            ));
        }
        None => {
            // Not a row of zeroes. The file is absent and that is what gets said.
            s.push_str("<b>PnL</b>        —\n<b>Trades</b>     —\n<i>No metrics file — nothing has been measured.</i>\n");
        }
    }

    s.push_str(&format!("<b>Open</b>       {}\n\n", dash(open_positions)));

    let mode = c.rate_mode.clone().unwrap_or_else(|| config.active_mode_name.clone());
    // The numbers come from config.toml, never from the rate-mode file — see control.rs.
    let entry = crate::control::find_mode(config, &mode);
    s.push_str(&format!(
        "<b>Mode</b>       {}  (TP {} · SL {} · {} SOL)\n",
        esc(&mode),
        dash(entry.as_ref().map(|e| format!("{:.0}%", e.take_profit_pct))),
        dash(entry.as_ref().map(|e| format!("{:.0}%", e.stop_loss_pct))),
        dash(entry.as_ref().map(|e| format!("{:.4}", e.quote_amount))),
    ));

    s.push_str(&format!(
        "<b>Buys</b>       {}\n",
        if c.sell_mode {
            format!(
                "⏸ PAUSED{}",
                c.sell_mode_reason.as_deref().map(|r| format!(" (by {})", esc(r))).unwrap_or_default()
            )
        } else {
            "▶ active".into()
        }
    ));
    if c.dump_mode {
        s.push_str("<b>DUMP MODE</b>  🔴 ENGAGED — positions are being force-sold at any price\n");
    }
    if c.high_speed {
        s.push_str("<b>High speed</b> on\n");
    }
    if c.moon_chase {
        s.push_str("<b>Moon chase</b> on\n");
    }
    s
}

// ── positions ────────────────────────────────────────────────────────────────

pub fn positions(v: Option<&Value>) -> String {
    let Some(arr) = v.and_then(|v| v.as_array()) else {
        return "<b>POSITIONS</b>\n\n<i>No positions file — the sniper has not written one.</i>".into();
    };
    if arr.is_empty() {
        // A measured zero, and it says so rather than sharing the wording above.
        return "<b>POSITIONS</b>\n\nNone open.".into();
    }

    let mut s = String::from("<b>POSITIONS</b>\n");
    for p in arr {
        let mint = p["mint"].as_str().unwrap_or("?");
        let entry = p["entry_lamports"].as_f64().unwrap_or(0.0);
        let cur = p["current_value_lamports"].as_f64();
        let peak = p["peak_value_lamports"].as_f64();
        let pnl = match (cur, entry) {
            (Some(c), e) if e > 0.0 => Some((c - e) / e * 100.0),
            _ => None,
        };
        let peak_pnl = match (peak, entry) {
            (Some(pk), e) if e > 0.0 => Some((pk - e) / e * 100.0),
            _ => None,
        };
        let age = p["entry_unix_secs"]
            .as_i64()
            .map(|t| (chrono::Utc::now().timestamp() - t).max(0) as u64);
        let stale = p["last_check_unix_secs"]
            .as_i64()
            .map(|t| (chrono::Utc::now().timestamp() - t).max(0));

        s.push_str(&format!(
            "\n<code>{}</code>\n  PnL <b>{}</b>   peak {}   held {}\n  TP {}   SL {}   esc {}   declines {}\n",
            short_mint(mint),
            dash(pnl.map(|v| format!("{v:+.1}%"))),
            dash(peak_pnl.map(|v| format!("{v:+.1}%"))),
            dash(age.map(hms)),
            dash(p["dynamic_tp_pct"].as_f64().map(|v| format!("{v:.0}%"))),
            dash(p["current_sl_pct"].as_f64().map(|v| format!("{v:+.1}%"))),
            dash(p["escalations"].as_u64()),
            dash(p["decline_streak"].as_u64()),
        ));
        // A position whose price check has stopped is not one that is holding — it is one
        // nobody is evaluating, and that difference is the whole reason `/zero` has a
        // liveness model. Same distinction, surfaced here.
        if let Some(secs) = stale {
            if secs > 30 {
                s.push_str(&format!(
                    "  ⚠️ last price check {} ago — exits are NOT being evaluated\n",
                    hms(secs as u64)
                ));
            }
        }
    }
    s
}

// ── trades ───────────────────────────────────────────────────────────────────

pub fn trades(t: Option<&[TradeEvent]>) -> String {
    let Some(t) = t else {
        return "<b>TRADES</b>\n\n<i>No trade log.</i>".into();
    };
    if t.is_empty() {
        return "<b>TRADES</b>\n\nNone recorded.".into();
    }
    let mut s = String::from("<b>TRADES</b>  <i>(newest last)</i>\n");
    for e in t {
        let sym = if e.symbol.is_empty() { short_mint(&e.mint) } else { esc(&e.symbol) };
        let when = e.timestamp.format("%H:%M:%S");
        if e.kind == "SELL" {
            s.push_str(&format!(
                "\n{} <b>SELL</b> {}  {:+.4} SOL ({:+.1}%)  {}  <i>{}</i>",
                when,
                sym,
                e.pnl,
                e.pnl_pct,
                e.status,
                esc(&e.exit_reason),
            ));
        } else {
            s.push_str(&format!(
                "\n{} <b>{}</b> {}  {:.4} SOL  {}",
                when,
                esc(&e.kind),
                sym,
                e.amount,
                e.status,
            ));
        }
    }
    s
}

pub fn pnl_summary(window: usize, r: Option<(f64, usize, usize)>) -> String {
    match r {
        // `sells == 0` is the case that has to be named rather than divided by: a window
        // containing only buys has no win rate, and reporting 0% would say every trade
        // lost when none has resolved.
        Some((_, _, 0)) => format!(
            "<b>REALISED PnL</b> — last {window} events\n\nNo settled sells in the window. \
             Win rate is <b>—</b>, not 0%: nothing has resolved."
        ),
        Some((total, wins, sells)) => format!(
            "<b>REALISED PnL</b> — last {window} events\n\n\
             <b>Total</b>     {total:+.4} SOL over {sells} sells\n\
             <b>Wins</b>      {wins}/{sells}  ({:.0}%)\n\
             <b>Average</b>   {:+.4} SOL",
            wins as f64 / sells as f64 * 100.0,
            total / sells as f64,
        ),
        None => "<b>REALISED PnL</b>\n\n<i>No trade log.</i>".into(),
    }
}

// ── filters, DQ*, decisions ──────────────────────────────────────────────────

pub fn filters(v: Option<&Value>) -> String {
    let Some(obj) = v else {
        return "<b>FILTERS</b>\n\n<i>No filter-stats file.</i>".into();
    };
    let mut rows: Vec<(String, u64)> = Vec::new();
    // The file's shape has changed over time; accept either a flat map of counts or one
    // nested under `rejections`, rather than assuming and rendering an empty report.
    let src = obj.get("rejections").unwrap_or(obj);
    if let Some(map) = src.as_object() {
        for (k, v) in map {
            if let Some(n) = v.as_u64() {
                rows.push((k.clone(), n));
            }
        }
    }
    if rows.is_empty() {
        return "<b>FILTERS</b>\n\n<i>No per-filter counts in the file.</i>".into();
    }
    rows.sort_by(|a, b| b.1.cmp(&a.1));
    let total: u64 = rows.iter().map(|r| r.1).sum();
    let mut s = format!("<b>FILTERS</b> — {total} rejections\n");
    for (name, n) in rows.iter().take(20) {
        s.push_str(&format!(
            "\n<code>{:>7}</code>  {:>5.1}%  {}",
            n,
            *n as f64 / total.max(1) as f64 * 100.0,
            esc(name)
        ));
    }
    // The lesson `measure --split` exists for, stated where somebody will read it.
    s.push_str(
        "\n\n<i>An aggregate over the whole log is a claim about HISTORY, not about the bot as \
         configured now. A gate whose veto was removed still dominates this list.</i>",
    );
    s
}

pub fn dq(stats: Option<&Value>, advice: Option<&Value>) -> String {
    let mut s = String::from("<b>DEEP Q*</b>\n");
    match stats {
        Some(v) => s.push_str(&format!(
            "\n<b>ε</b>          {}\n<b>Steps</b>      {}\n<b>Replay</b>     {}\n<b>Reward</b>     {}\n",
            f4(v["epsilon"].as_f64()),
            dash(v["train_steps"].as_u64().or_else(|| v["steps"].as_u64())),
            dash(v["replay_size"].as_u64()),
            f2(v["total_reward"].as_f64()),
        )),
        None => s.push_str("\n<i>No DQ* stats file — the agent has not written one.</i>\n"),
    }
    if let Some(a) = advice {
        s.push_str(&format!(
            "\n<b>Last advice</b>  {}\n<b>Confidence</b>   {}\n<b>Coverage</b>     {}\n",
            esc(a["action"].as_str().unwrap_or("—")),
            f2(a["confidence"].as_f64()),
            dash(a["coverage"].as_str().map(esc)),
        ));
        // The rule the DQ* calibration module exists to enforce, restated where the number
        // is read: a confident argmax over invented inputs looks exactly like a real one.
        s.push_str(
            "\n<i>Coverage rides with confidence deliberately. Five finite Q-values with a clear \
             argmax look identical whether the features were measured or defaulted.</i>",
        );
    }
    s
}

pub fn decisions(v: Option<&[Value]>) -> String {
    let Some(rows) = v else {
        return "<b>POOL DECISIONS</b>\n\n<i>No decision log.</i>".into();
    };
    if rows.is_empty() {
        return "<b>POOL DECISIONS</b>\n\nNone recorded.".into();
    }
    let mut s = String::from("<b>POOL DECISIONS</b>  <i>(newest last)</i>\n");
    for r in rows {
        s.push_str(&format!(
            "\n<code>{}</code>  {}  <i>{}</i>{}",
            short_mint(r["mint"].as_str().unwrap_or("?")),
            esc(r["decision"].as_str().unwrap_or("?")),
            esc(r["reason"].as_str().or_else(|| r["stage"].as_str()).unwrap_or("")),
            r["decide_latency_ms"]
                .as_f64()
                .map(|ms| format!("  {ms:.0}ms"))
                .unwrap_or_default(),
        ));
    }
    s
}

// ── config and modes ─────────────────────────────────────────────────────────

pub fn params(c: &SniperConfig) -> String {
    format!(
        "<b>TRADING PARAMETERS</b>  <i>(config.toml)</i>\n\n\
         <b>Base size</b>       {:.4} SOL\n\
         <b>Take profit</b>     {:.1}%\n\
         <b>Stop loss</b>       {:.1}%\n\
         <b>Trailing stop</b>   {:.1}%\n\
         <b>Slippage</b>        buy {:.1}% · sell {:.1}%\n\
         <b>Max positions</b>   {}\n\n\
         <b>Momentum</b>\n\
         · escalation      ×{:.2} up to {} rounds, on {:.1}%/check\n\
         · peak floor      {:.0}%\n\
         · pullback exit   {:.1}%{}\n\
         · velocity decay  {} (from {:.0}%, drop {:.1})\n\n\
         <b>Exits</b>\n\
         · no-pump         {}s, peak under {:.1}%\n\
         · max hold        {} min\n\
         · flash crash     {}\n\
         · profit-first    {} (floor {:.0}%, target {:.2} SOL)\n\n\
         <b>Entry</b>\n\
         · pool score      ≥ {:.0}\n\
         · pool size       {:.1} – {:.1} SOL\n\
         · mint cooldown   {}s\n\
         · coherence       {}\n\
         · Kelly           {} (fraction {:.2}, lookback {})",
        c.quote_amount,
        c.take_profit_pct,
        c.stop_loss_pct,
        c.trailing_stop_loss_pct,
        c.buy_slippage_pct,
        c.sell_slippage_pct,
        if c.max_concurrent_positions == 0 { "unlimited".into() } else { c.max_concurrent_positions.to_string() },
        c.momentum_escalation_factor,
        c.momentum_max_escalations,
        c.momentum_escalation_threshold_pct,
        c.momentum_min_peak_pct,
        c.momentum_pullback_exit_pct,
        if c.adaptive_pullback { " (adaptive)" } else { "" },
        on_off(c.velocity_decay_exit),
        c.velocity_decay_min_pnl_pct,
        c.velocity_decay_drop_threshold,
        c.no_pump_timeout_secs,
        c.no_pump_min_gain_pct,
        c.max_position_hold_mins,
        if c.flash_crash_pct > 0.0 { format!("{:.0}%", c.flash_crash_pct) } else { "off".into() },
        on_off(c.profit_first_mode),
        c.profit_first_floor_pct,
        c.wallet_target_sol,
        c.min_pool_score,
        c.filters.min_pool_size,
        c.filters.max_pool_size,
        c.mint_cooldown_secs,
        on_off(c.coherence_breaker),
        on_off(c.kelly_sizing),
        c.kelly_fraction,
        c.kelly_lookback,
    )
}

fn on_off(b: bool) -> &'static str {
    if b {
        "on"
    } else {
        "off"
    }
}

pub fn modes(config: &SniperConfig, active: Option<&str>) -> String {
    let active = active.unwrap_or(&config.active_mode_name);
    let mut s = String::from("<b>RATE MODES</b>  <i>(from config.toml)</i>\n");
    for m in config.rate_modes.iter().filter(|m| m.enabled) {
        let marker = if m.name.eq_ignore_ascii_case(active) { "▶" } else { " " };
        s.push_str(&format!(
            "\n{} <b>{}</b>\n    TP {:.0}%  SL {:.0}%  {:.4} SOL  {:.1}% wallet  esc {}",
            marker,
            esc(&m.name),
            m.take_profit_pct,
            m.stop_loss_pct,
            m.quote_amount,
            m.wallet_pct,
            m.momentum_max_escalations,
        ));
    }
    s.push_str("\n\nSwitch with <code>/mode &lt;name&gt;</code>.");
    s
}

pub fn log_lines(lines: Option<&[String]>) -> String {
    let Some(l) = lines else {
        return "<b>LOG</b>\n\n<i>No log file.</i>".into();
    };
    if l.is_empty() {
        return "<b>LOG</b>\n\nEmpty.".into();
    }
    format!("<b>LOG</b>\n\n<pre>{}</pre>", esc(&l.join("\n")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unmeasured_value_is_an_em_dash_not_a_zero() {
        assert_eq!(dash(None::<f64>), "—");
        assert_eq!(f2(None), "—");
        // A MEASURED zero still prints as a number — it is a real observation.
        assert_eq!(f2(Some(0.0)), "0.00");
    }

    #[test]
    fn a_missing_metrics_file_does_not_render_as_zero_trades() {
        let out = status(
            &Liveness::Stopped,
            None,
            &Controls {
                sell_mode: false,
                sell_mode_reason: None,
                dump_mode: false,
                high_speed: false,
                moon_chase: false,
                rate_mode: None,
            },
            &SniperConfig::default(),
            None,
        );
        assert!(out.contains("nothing has been measured"));
        assert!(!out.contains("0.0000 SOL"));
        assert!(out.contains("STOPPED"));
    }

    #[test]
    fn an_empty_position_list_reads_differently_from_a_missing_file() {
        let none = positions(None);
        let empty = positions(Some(&serde_json::json!([])));
        assert_ne!(none, empty);
        assert!(none.contains("has not written"));
        assert!(empty.contains("None open"));
    }

    #[test]
    fn a_window_with_no_settled_sells_has_no_win_rate() {
        let out = pnl_summary(50, Some((0.0, 0, 0)));
        assert!(out.contains("not 0%"));
        assert!(!out.contains("0%)"));
    }

    #[test]
    fn a_markdown_hostile_symbol_survives_a_trade_row() {
        // Built by deserialising rather than by struct literal: `TradeEvent` gains
        // `#[serde(default)]` fields over time, and a literal here would have to be
        // updated for every one of them — a test that breaks on unrelated growth is a
        // test people delete.
        let e: TradeEvent = serde_json::from_value(serde_json::json!({
            "timestamp": chrono::Utc::now(),
            "kind": "SELL",
            "mint": "MintAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "symbol": "PUMP_IT<b>",
            "amount": 0.01,
            "pnl": 0.5,
            "status": "OK",
            "signature": "",
            "dex": "Raydium",
            "hops": 1,
            "pnl_pct": 50.0,
            "position_age_secs": 12.0,
            "exit_reason": "take_profit",
        }))
        .expect("the fixture matches TradeEvent");
        let out = trades(Some(&[e]));
        // The angle brackets are escaped; the underscore needs no escaping in HTML, which
        // is the entire reason this bot does not use Markdown.
        assert!(out.contains("PUMP_IT&lt;b&gt;"));
    }

    #[test]
    fn a_short_mint_is_not_falsely_ellipsised() {
        assert_eq!(short_mint("SOL"), "SOL");
        assert!(short_mint("MintAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA").contains('…'));
    }
}
