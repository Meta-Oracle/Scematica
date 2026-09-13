//! The command table and its dispatch.
//!
//! Read commands answer from `state.rs`; control commands go through `control.rs`; a
//! message that is not a command goes to Grok. Nothing here computes a trading number —
//! every threshold shown comes from `config.toml` and every measurement from a file the
//! sniper wrote.
//!
//! ## Destructive actions are typed, not tapped
//!
//! `/dump` force-sells every position at `min_out = 0`. One tap on a phone in a pocket is
//! not consent for that, so it is a two-step: the first `/dump` answers with what will
//! happen and a short code, and only `/dump <code>` inside a minute acts. Inline buttons
//! are used for the model's tool confirmations — where the action is bounded and named —
//! and deliberately not for this one.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use rand::Rng;
use scematica_core::config::SniperConfig;

use crate::api::esc;
use crate::{control, render, state};

/// How long a `/dump` code stays valid.
const DUMP_TTL: Duration = Duration::from_secs(60);

/// Published to Telegram's command menu.
pub const MENU: &[(&str, &str)] = &[
    ("status", "Is the bot running, and what has it done"),
    ("positions", "Open positions with PnL, peak, TP and SL"),
    ("trades", "Recent trades — /trades 20"),
    ("pnl", "Realised PnL over the recent window"),
    ("filters", "Per-filter rejection counts"),
    ("dq", "Deep Q* agent stats and its last advice"),
    ("pools", "Recent pool decisions"),
    ("params", "Every live trading parameter"),
    ("modes", "The rate-mode table"),
    ("log", "Tail the sniper log — /log 30"),
    ("mode", "Switch rate mode — /mode degen"),
    ("tp", "Set take-profit % — /tp 220"),
    ("sl", "Set stop-loss % — /sl 12"),
    ("pause", "Pause buys (positions keep their exits)"),
    ("resume", "Resume buys"),
    ("dump", "DANGER: force-sell everything at any price"),
    ("highspeed", "/highspeed on|off"),
    ("moonchase", "/moonchase on|off"),
    ("ask", "Ask Grok — or just send a message"),
    ("reset", "Forget the chat history"),
    ("help", "This list"),
];

/// A pending `/dump`, keyed by chat.
struct DumpArm {
    code: String,
    issued: Instant,
}

#[derive(Default)]
pub struct Pending {
    dumps: HashMap<i64, DumpArm>,
}

/// What dispatch decided the bot should do.
pub enum Action {
    /// Send this HTML back.
    Reply(String),
    /// Not a command — hand the text to the model.
    Chat(String),
    /// A command the bot does not have.
    Unknown(String),
}

pub fn help() -> String {
    let mut s = String::from("<b>SCEMATICA — TELEGRAM</b>\n\nThe sniper's own controls and its Grok agent.\n\n<b>Read</b>\n");
    for (c, d) in MENU.iter().take(10) {
        s.push_str(&format!("/{c} — {d}\n"));
    }
    s.push_str("\n<b>Control</b>\n");
    for (c, d) in MENU.iter().skip(10).take(8) {
        s.push_str(&format!("/{c} — {d}\n"));
    }
    s.push_str("\n<b>Chat</b>\nSend any message to talk to Grok. It can read the bot's state and, with a confirmation, act.\n");
    s.push_str(
        "\n<i>Trading parameters live in config.toml. /tp and /sl edit it and the sniper \
         picks the change up within 30 seconds; /mode switches the whole profile in 5.</i>",
    );
    s
}

/// Parse and execute one message.
///
/// `config` is re-read by the caller on every command rather than cached, so a value
/// changed by hand — or by `/tp` a second ago — is what gets reported. A cached config is
/// how a control surface starts lying about the thing it controls.
pub fn dispatch(
    text: &str,
    chat_id: i64,
    username: &str,
    config: &SniperConfig,
    config_path: &str,
    pending: &mut Pending,
) -> Action {
    let text = text.trim();
    if !text.starts_with('/') {
        return Action::Chat(text.to_string());
    }

    // `/command@BotName arg` — Telegram appends the bot's name in groups.
    let mut parts = text.splitn(2, char::is_whitespace);
    let head = parts.next().unwrap_or("");
    let arg = parts.next().unwrap_or("").trim();
    let cmd = head
        .trim_start_matches('/')
        .split('@')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();

    let n = |default: usize, max: usize| -> usize {
        arg.split_whitespace()
            .next()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(default)
            .clamp(1, max)
    };

    match cmd.as_str() {
        "start" | "help" => Action::Reply(help()),

        // ── read ─────────────────────────────────────────────────────────────
        "status" => {
            let live = state::liveness();
            let open = state::positions()
                .and_then(|v| v.as_array().map(|a| a.len()));
            Action::Reply(render::status(
                &live,
                state::metrics().as_ref(),
                &state::controls(),
                config,
                open,
            ))
        }
        "positions" | "pos" => Action::Reply(render::positions(state::positions().as_ref())),
        "trades" => {
            let count = n(15, 100);
            Action::Reply(render::trades(state::recent_trades(count).as_deref()))
        }
        "pnl" => {
            let count = n(100, 1000);
            Action::Reply(render::pnl_summary(count, state::realised(count)))
        }
        "filters" => Action::Reply(render::filters(state::filter_stats().as_ref())),
        "dq" => Action::Reply(render::dq(
            state::nn_stats().as_ref(),
            state::nn_advice().as_ref(),
        )),
        "pools" => {
            let count = n(15, 60);
            Action::Reply(render::decisions(state::recent_decisions(count).as_deref()))
        }
        "params" | "config" => Action::Reply(render::params(config)),
        "modes" => Action::Reply(render::modes(config, state::controls().rate_mode.as_deref())),
        "log" => {
            let count = n(25, 100);
            Action::Reply(render::log_lines(state::log_tail(count).as_deref()))
        }

        // ── control ──────────────────────────────────────────────────────────
        "mode" => {
            if arg.is_empty() {
                return Action::Reply(render::modes(
                    config,
                    state::controls().rate_mode.as_deref(),
                ));
            }
            match control::set_rate_mode(config, arg) {
                Ok(m) => Action::Reply(format!(
                    "✅ Rate mode → <b>{}</b>\n\nTP {:.0}%  ·  SL {:.0}%  ·  {:.4} SOL base  ·  \
                     {:.1}% wallet  ·  esc {}\n\n<i>Applied by the sniper within 5s. These numbers \
                     come from config.toml, which is what it will actually run.</i>",
                    esc(&m.name),
                    m.take_profit_pct,
                    m.stop_loss_pct,
                    m.quote_amount,
                    m.wallet_pct,
                    m.momentum_max_escalations,
                )),
                Err(e) => Action::Reply(format!("❌ {}", esc(&e.to_string()))),
            }
        }

        "tp" | "sl" => {
            let param = if cmd == "tp" {
                control::Param::TakeProfit
            } else {
                control::Param::StopLoss
            };
            let Ok(v) = arg.trim_end_matches('%').parse::<f64>() else {
                return Action::Reply(format!(
                    "Usage: <code>/{cmd} &lt;percent&gt;</code>  (e.g. <code>/{cmd} 175</code>)"
                ));
            };
            match control::set_param(config_path, param, v) {
                Ok(c) => Action::Reply(format!(
                    "✅ <b>{}</b>  {:.1}% → <b>{:.1}%</b>\n\n<i>Written to config.toml. The sniper \
                     polls it by mtime, so this is live within 30s — and it survives a restart, \
                     unlike a rate-mode switch.</i>\n\n⚠️ A later <code>/mode</code> overwrites \
                     this with that mode's own value.",
                    esc(c.key),
                    c.from,
                    c.to
                )),
                Err(e) => Action::Reply(format!("❌ {}", esc(&e.to_string()))),
            }
        }

        "pause" | "stop" => match control::pause(&format!("telegram:{username}")) {
            Ok(()) => Action::Reply(
                "⏸ <b>Buys paused.</b>\n\nOpen positions are untouched and still run their exit \
                 rules — this halts entries only. <code>/resume</code> to restart."
                    .into(),
            ),
            Err(e) => Action::Reply(format!("❌ {}", esc(&e.to_string()))),
        },

        "resume" => match control::resume() {
            Ok(()) => Action::Reply("▶ <b>Buys resumed.</b>".into()),
            Err(e) => Action::Reply(format!("❌ {}", esc(&e.to_string()))),
        },

        "dump" => dump_command(arg, chat_id, username, pending),

        "highspeed" | "moonchase" => {
            let on = match arg.to_ascii_lowercase().as_str() {
                "on" | "1" | "true" => Some(true),
                "off" | "0" | "false" => Some(false),
                _ => None,
            };
            let Some(on) = on else {
                return Action::Reply(format!("Usage: <code>/{cmd} on</code> or <code>/{cmd} off</code>"));
            };
            let r = if cmd == "highspeed" {
                control::set_high_speed(on)
            } else {
                control::set_moon_chase(on)
            };
            match r {
                Ok(()) => Action::Reply(format!(
                    "✅ <b>{}</b> {}",
                    esc(&cmd),
                    if on { "on" } else { "off" }
                )),
                Err(e) => Action::Reply(format!("❌ {}", esc(&e.to_string()))),
            }
        }

        // ── chat ─────────────────────────────────────────────────────────────
        "ask" => {
            if arg.is_empty() {
                Action::Reply("Ask me something: <code>/ask how did we do today?</code>".into())
            } else {
                Action::Chat(arg.to_string())
            }
        }
        "reset" => Action::Reply("__RESET__".into()),

        other => Action::Unknown(other.to_string()),
    }
}

/// The two-step `/dump`.
fn dump_command(arg: &str, chat_id: i64, username: &str, pending: &mut Pending) -> Action {
    if arg.eq_ignore_ascii_case("off") {
        return match control::dump_off() {
            Ok(()) => Action::Reply(
                "Dump mode cleared. <i>Positions already sold are not coming back.</i>".into(),
            ),
            Err(e) => Action::Reply(format!("❌ {}", esc(&e.to_string()))),
        };
    }

    // Second step: a code was offered.
    if !arg.is_empty() {
        let armed = pending.dumps.get(&chat_id);
        let ok = match armed {
            Some(a) if a.issued.elapsed() > DUMP_TTL => {
                pending.dumps.remove(&chat_id);
                return Action::Reply(
                    "That code expired. Send <code>/dump</code> again if you still mean it.".into(),
                );
            }
            Some(a) => a.code.eq_ignore_ascii_case(arg.trim()),
            None => false,
        };
        if !ok {
            return Action::Reply(
                "That is not the current code. Send <code>/dump</code> to get one.".into(),
            );
        }
        pending.dumps.remove(&chat_id);
        return match control::dump(&format!("telegram:{username}")) {
            Ok(()) => Action::Reply(
                "🔴 <b>DUMP MODE ENGAGED.</b>\n\nEvery open position is being sold at market with \
                 <code>min_out = 0</code>. Buys are not paused by this — send <code>/pause</code> \
                 if you also want entries stopped.\n\n<code>/dump off</code> clears the flag, but \
                 anything already sold is gone."
                    .into(),
            ),
            Err(e) => Action::Reply(format!("❌ {}", esc(&e.to_string()))),
        };
    }

    // First step: arm it, and say plainly what it does.
    const ALPHABET: &[u8] = b"23456789ABCDEFGHJKMNPQRSTUVWXYZ";
    let mut rng = rand::thread_rng();
    let code: String = (0..4)
        .map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char)
        .collect();
    pending
        .dumps
        .insert(chat_id, DumpArm { code: code.clone(), issued: Instant::now() });

    let open = state::positions()
        .and_then(|v| v.as_array().map(|a| a.len()));
    Action::Reply(format!(
        "⚠️ <b>DUMP — read this.</b>\n\nThis force-sells <b>{}</b> open position(s) at market with \
         <code>min_out = 0</code>: it accepts <b>any</b> price, including approximately nothing in \
         a thin pool. It is not reversible.\n\nTo go ahead, send:\n\n<code>/dump {}</code>\n\n\
         <i>Expires in 60 seconds. If you only want to stop buying, use /pause.</i>",
        render::dash(open),
        code,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> SniperConfig {
        SniperConfig::default()
    }

    #[test]
    fn a_plain_message_goes_to_the_model() {
        let mut p = Pending::default();
        assert!(matches!(
            dispatch("how are the trades going?", 1, "u", &cfg(), "config.toml", &mut p),
            Action::Chat(_)
        ));
    }

    #[test]
    fn a_group_suffixed_command_still_parses() {
        // In a group Telegram sends `/status@ScemaBot`. Failing to strip the suffix makes
        // every command in every group an unknown one.
        let mut p = Pending::default();
        assert!(matches!(
            dispatch("/status@ScemaAgentBot", 1, "u", &cfg(), "config.toml", &mut p),
            Action::Reply(_)
        ));
    }

    #[test]
    fn an_unknown_command_is_named_rather_than_sent_to_the_model() {
        // A typo must not silently become a prompt: the model would answer it confidently
        // and the operator would believe a command existed.
        let mut p = Pending::default();
        match dispatch("/psotions", 1, "u", &cfg(), "config.toml", &mut p) {
            Action::Unknown(c) => assert_eq!(c, "psotions"),
            _ => panic!("expected Unknown"),
        }
    }

    #[test]
    fn dump_needs_two_steps() {
        let mut p = Pending::default();
        let first = dispatch("/dump", 7, "u", &cfg(), "config.toml", &mut p);
        let Action::Reply(msg) = first else { panic!("expected a reply") };
        assert!(msg.contains("min_out = 0") || msg.contains("min_out"));
        assert!(p.dumps.contains_key(&7), "the first /dump must arm, not act");

        // A wrong code does not act and does not disarm.
        let Action::Reply(bad) = dispatch("/dump ZZZZ", 7, "u", &cfg(), "config.toml", &mut p)
        else {
            panic!()
        };
        assert!(bad.contains("not the current code"));
    }

    #[test]
    fn a_dump_code_is_scoped_to_its_chat() {
        // Arming in one chat must not arm another. Otherwise an owner's `/dump` in a DM
        // could be completed from a group by anybody who guessed four characters.
        let mut p = Pending::default();
        dispatch("/dump", 7, "u", &cfg(), "config.toml", &mut p);
        let code = p.dumps.get(&7).unwrap().code.clone();
        let Action::Reply(other) =
            dispatch(&format!("/dump {code}"), 8, "u", &cfg(), "config.toml", &mut p)
        else {
            panic!()
        };
        assert!(other.contains("not the current code"));
        assert!(p.dumps.contains_key(&7));
    }

    #[test]
    fn an_expired_dump_code_is_refused() {
        let mut p = Pending::default();
        p.dumps.insert(
            7,
            DumpArm { code: "ABCD".into(), issued: Instant::now() - DUMP_TTL - Duration::from_secs(1) },
        );
        let Action::Reply(msg) = dispatch("/dump ABCD", 7, "u", &cfg(), "config.toml", &mut p)
        else {
            panic!()
        };
        assert!(msg.contains("expired"));
        assert!(!p.dumps.contains_key(&7));
    }

    #[test]
    fn tp_rejects_a_non_number_rather_than_writing_one() {
        let mut p = Pending::default();
        let Action::Reply(msg) = dispatch("/tp lots", 1, "u", &cfg(), "config.toml", &mut p) else {
            panic!()
        };
        assert!(msg.contains("Usage"));
    }

    #[test]
    fn counts_are_clamped() {
        // A model or a fat finger asking for 100000 trades must not read a 40 MB log into
        // a Telegram message.
        let mut p = Pending::default();
        assert!(matches!(
            dispatch("/trades 999999", 1, "u", &cfg(), "config.toml", &mut p),
            Action::Reply(_)
        ));
    }
}
