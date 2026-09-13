//! Scematica on Telegram — the sniper's controls and its Grok agent, from a phone.
//!
//! ```text
//! SCEMA_TG_TOKEN=...  SCEMA_TG_OWNERS=...  XAI_API_KEY=...  cargo run --bin scema-tgbot
//! ```
//!
//! A fourth face over the File-Based IPC surface, beside the ratatui dashboard, the HTTP
//! API and the web dashboard. It starts nothing, owns nothing, and holds no lock: the
//! sniper runs independently and this is a way to watch and steer it.
//!
//! ## The three things that make this safe to point at a live bot
//!
//! **1. Deny by default.** A bot token is a public endpoint — anyone who learns the
//! @username can message it. `auth.rs` refuses everyone until an owner is claimed from the
//! operator's own console. There is no "first message wins".
//!
//! **2. The offset advances before the work.** `getUpdates(offset)` acknowledges
//! everything below `offset`, so the choice is: commit first and risk losing a command to
//! a crash, or commit last and risk running it twice on restart. For a process that can
//! sell positions, **twice is worse than never** — the same reasoning that made the
//! treasury path answer 202 rather than retry. So the offset moves first, and a command
//! lost to a crash is a command the operator sends again.
//!
//! **3. It reads state, it does not compute it.** Every number comes from a file the
//! sniper wrote or from `config.toml`. Nothing here re-derives a threshold, a score or a
//! PnL, so there is no second implementation to drift — the lesson `/zero` cost.

mod api;
mod auth;
mod chat;
mod commands;
mod control;
mod presence;
mod render;
mod state;

use std::time::Duration;

use anyhow::{Context, Result};
use scematica_ai::chat_types::RiskLevel;
use scematica_core::config::BotConfig;
use tracing::{error, info, warn};

use api::{esc, Telegram, Update, UpdateKind};
use auth::{Access, Auth};
use commands::Action;

/// Environment variable holding the Telegram bot token.
const TOKEN_ENV: &str = "SCEMA_TG_TOKEN";

#[tokio::main]
async fn main() -> Result<()> {
    dotenv::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let token = std::env::var(TOKEN_ENV).map_err(|_| {
        anyhow::anyhow!(
            "{TOKEN_ENV} is not set.\n\n\
             Put it in the gitignored .env, never in config.toml — that file has been \
             committed with a live key once already:\n\n    {TOKEN_ENV}=123456:AA...\n"
        )
    })?;

    let config_path = std::env::var("CONFIG_PATH").unwrap_or_else(|_| "config.toml".into());
    // Read once at startup to fail early on a broken config, and re-read per command so a
    // hand edit — or a `/tp` from a second ago — is what gets reported.
    let _ = BotConfig::from_file(&config_path)
        .with_context(|| format!("reading {config_path}"))?;

    let tg = Telegram::new(&token)?;
    let me = tg.me().await.context("the bot token was refused by Telegram")?;
    info!("connected as @{me}");

    let auth = Auth::from_env();
    if auth.was_configured() {
        info!("{} owner(s) from {}", auth.owner_count(), auth::OWNERS_ENV);
    } else if let Some(code) = auth.issue_claim() {
        // To the console, never over Telegram. Proving you are the operator means having
        // this terminal — which is precisely what somebody who merely found the @username
        // does not have.
        println!("\n  ┌──────────────────────────────────────────────┐");
        println!("  │  No owner configured.                        │");
        println!("  │                                              │");
        println!("  │  Message @{me} and send:", );
        println!("  │      /claim {code}", );
        println!("  │                                              │");
        println!("  │  Valid for 15 minutes, single use.           │");
        println!("  │  Then set {}=<id>  │", auth::OWNERS_ENV);
        println!("  │  so it survives a restart.                   │");
        println!("  └──────────────────────────────────────────────┘\n");
        warn!("no owner configured — the bot will refuse every command until claimed");
    }

    // Say which bot this process is polling, so a second poller can refuse before it
    // takes a command that was meant for this one. Best-effort: a read-only directory
    // must not stop the control surface starting.
    if let Err(e) = presence::announce(&token, &me) {
        warn!("could not publish {}: {e}", presence::PRESENCE_FILE);
    }

    if let Err(e) = tg.set_commands(commands::MENU).await {
        // Cosmetic: the menu is a convenience and its absence does not stop a command
        // working, so this must not be fatal.
        warn!("could not publish the command menu: {e}");
    }

    // Grok. Absent is a first-class state: the control surface is the point of this bot
    // and it works with no model at all, so a missing key disables chat and nothing else.
    let mut grok = match chat::Chat::connect() {
        Ok(c) => {
            info!("chat: {} ({})", c.provider(), c.model());
            Some(c)
        }
        Err(e) => {
            warn!("chat disabled — {e}");
            None
        }
    };

    let mut pending = commands::Pending::default();
    let mut offset: i64 = 0;

    info!("polling");
    loop {
        // Ctrl-C withdraws the presence announcement on the way out. A crash cannot, which
        // is why the reader checks the pid rather than trusting the file to be absent.
        let updates = tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("shutting down");
                presence::withdraw();
                return Ok(());
            }
            updates = tg.updates(offset) => updates,
        };
        let updates = match updates {
            Ok(u) => u,
            Err(e) => {
                // A 409 is not a transient network failure: something else is polling this
                // same bot, and every update it takes is a command that never reaches here.
                //
                // This process keeps polling anyway, and the asymmetry is deliberate. The
                // other poller is the Omni-Agent's cockpit, which yields — losing a draft
                // approval costs a round trip. This bot holds `/dump` and `/pause`, and an
                // operator reaching for an emergency stop must not find it switched off
                // because a drafting agent turned up. So: say it loudly, once per
                // occurrence, and stay.
                if e.to_string().contains("409") {
                    error!(
                        "another process is polling @{me} — it is taking commands meant for \
                         this bot. Stop it, or give it its own bot (SCEMA_AGENT_TG_TOKEN). \
                         Still polling: this is the surface that can pause and dump."
                    );
                } else {
                    // Telegram rate-limits and drops connections routinely. Back off rather
                    // than spinning; the offset is untouched, so nothing is lost.
                    warn!("getUpdates failed: {e}");
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
                continue;
            }
        };

        for update in updates {
            // See the header: acknowledge BEFORE acting. A command that can sell positions
            // must not be replayed by a crash-restart.
            offset = offset.max(update.update_id + 1);
            if let Err(e) = handle(&tg, &auth, &mut grok, &mut pending, &config_path, update).await
            {
                error!("handling an update failed: {e}");
            }
        }

        if let Some(g) = grok.as_mut() {
            g.sweep();
        }
    }
}

async fn handle(
    tg: &Telegram,
    auth: &Auth,
    grok: &mut Option<chat::Chat>,
    pending: &mut commands::Pending,
    config_path: &str,
    update: Update,
) -> Result<()> {
    match update.kind {
        UpdateKind::Other => Ok(()),

        UpdateKind::Message { chat_id, user_id, username, text } => {
            // `/claim` is the one command reachable without authorisation, and it is the
            // only one — it cannot read state, change a parameter or spend anything.
            if let Some(code) = text.trim().strip_prefix("/claim") {
                let msg = match auth.try_claim(user_id, code) {
                    Ok(()) => format!(
                        "✅ Claimed. You are the owner (id <code>{user_id}</code>).\n\n\
                         Set <code>{}={user_id}</code> in .env so this survives a restart — \
                         until then the claim lives in memory only.\n\n/help",
                        auth::OWNERS_ENV
                    ),
                    Err(e) => esc(&e),
                };
                tg.send(chat_id, &msg).await?;
                return Ok(());
            }

            if let Access::Denied(why) = auth.authorise(user_id) {
                warn!("refused {username} (id {user_id})");
                tg.send(chat_id, &why).await?;
                return Ok(());
            }

            // Re-read per command. A cached config is how a control surface starts lying
            // about the thing it controls.
            let config = match BotConfig::from_file(config_path) {
                Ok(c) => c.sniper,
                Err(e) => {
                    tg.send(
                        chat_id,
                        &format!(
                            "❌ <code>{}</code> does not parse: {}\n\n\
                             <i>Nothing was read and nothing was changed.</i>",
                            esc(config_path),
                            esc(&e.to_string())
                        ),
                    )
                    .await?;
                    return Ok(());
                }
            };

            let action = commands::dispatch(
                &text, chat_id, &username, &config, config_path, pending,
            );

            match action {
                Action::Reply(body) if body == "__RESET__" => {
                    if let Some(g) = grok.as_mut() {
                        g.reset(chat_id);
                    }
                    tg.send(chat_id, "Chat history cleared.").await?;
                }
                Action::Reply(body) => {
                    tg.send(chat_id, &body).await?;
                }
                Action::Unknown(cmd) => {
                    tg.send(
                        chat_id,
                        &format!(
                            "No command <code>/{}</code>. /help for the list.\n\n\
                             <i>Not passed to the model: a typo answered confidently is worse \
                             than a typo refused.</i>",
                            esc(&cmd)
                        ),
                    )
                    .await?;
                }
                Action::Chat(prompt) => {
                    let Some(g) = grok.as_mut() else {
                        tg.send(
                            chat_id,
                            "Chat is not configured. Set <code>XAI_API_KEY</code> in .env for \
                             Grok, then restart.\n\n<i>The control commands work without it — \
                             /help.</i>",
                        )
                        .await?;
                        return Ok(());
                    };
                    match g.ask(chat_id, user_id, &prompt).await {
                        Ok(reply) => send_reply(tg, g, chat_id, reply).await?,
                        Err(e) => {
                            tg.send(chat_id, &format!("❌ {}", esc(&e.to_string()))).await?;
                        }
                    }
                }
            }
            Ok(())
        }

        UpdateKind::Callback { chat_id, user_id, username, callback_id, message_id, data } => {
            if let Access::Denied(_) = auth.authorise(user_id) {
                tg.answer_callback(&callback_id, "Not authorised.").await;
                warn!("refused a button press from {username} (id {user_id})");
                return Ok(());
            }
            let Some(g) = grok.as_mut() else {
                tg.answer_callback(&callback_id, "Chat is not configured.").await;
                return Ok(());
            };

            // `callback_data` is `ok:<token>` or `no:<token>`. The token is a key into this
            // process's own table and carries no instruction — a crafted callback can name
            // a token that does not exist, and nothing more.
            let (verb, token) = data.split_once(':').unwrap_or(("", ""));
            let reply = match verb {
                "ok" => {
                    tg.answer_callback(&callback_id, "Running…").await;
                    g.confirm(chat_id, user_id, token).await?
                }
                "no" => {
                    tg.answer_callback(&callback_id, "Cancelled").await;
                    g.cancel(chat_id, user_id, token)
                }
                _ => {
                    tg.answer_callback(&callback_id, "Unknown button").await;
                    return Ok(());
                }
            };

            // Strip the keyboard from the prompt the moment it is answered. A live Confirm
            // under a resolved action is an invitation to press it again, and a second
            // press would be a second trade.
            if message_id != 0 {
                tg.edit(chat_id, message_id, "<i>Answered.</i>").await?;
            }
            send_reply(tg, g, chat_id, reply).await
        }
    }
}

async fn send_reply(
    tg: &Telegram,
    grok: &mut chat::Chat,
    chat_id: i64,
    reply: chat::Reply,
) -> Result<()> {
    match reply {
        chat::Reply::Text(t) => {
            tg.send(chat_id, &esc(&t)).await?;
        }
        chat::Reply::Confirm { token, summary, risk } => {
            let badge = match risk {
                RiskLevel::Safe => "",
                RiskLevel::Moderate => "⚠️ ",
                RiskLevel::High => "🔴 ",
            };
            let body = format!(
                "{badge}<b>Confirm</b>\n\n{}\n\n<i>Expires in 5 minutes. Only you, in this chat, \
                 can answer it.</i>",
                esc(&summary)
            );
            let buttons = vec![vec![
                ("✅ Confirm".to_string(), format!("ok:{token}")),
                ("✖ Cancel".to_string(), format!("no:{token}")),
            ]];
            let id = tg.send_with_buttons(chat_id, &body, &buttons).await?;
            grok.bind_message(&token, id);
        }
        chat::Reply::Gone => {
            tg.send(chat_id, "<i>That action was already answered.</i>").await?;
        }
        chat::Reply::Expired => {
            tg.send(chat_id, "<i>That confirmation expired. Ask again if you still want it.</i>")
                .await?;
        }
        chat::Reply::WrongUser => {
            tg.send(
                chat_id,
                "<i>That confirmation belongs to someone else's request and is still waiting \
                 for them.</i>",
            )
            .await?;
        }
    }
    Ok(())
}
