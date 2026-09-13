//! The Telegram Bot API, by hand.
//!
//! Four endpoints: `getUpdates` (long poll), `sendMessage`, `editMessageText` and
//! `answerCallbackQuery`, plus `setMyCommands` once at startup. See the Cargo.toml note
//! for why this is not teloxide.
//!
//! ## HTML, not Markdown, and it is not a style preference
//!
//! Telegram's legacy `Markdown` parse mode rejects a message with unbalanced entities —
//! and it does so with a `400`, which means **the message simply never arrives**. Every
//! interesting string this bot prints comes off a chain: mint addresses, and token symbols
//! chosen by whoever launched the pool. A token called `PUMP_IT` contains one underscore
//! and takes the whole status report with it; `*` and `[` do the same. MarkdownV2 escapes
//! eighteen characters and is easy to get half-right.
//!
//! HTML has three: `&`, `<`, `>`. `esc()` handles them and is used on **every** value that
//! did not originate in this crate. A silent send failure on a status report is bad; a
//! silent send failure on the reply confirming a dump is worse.

use std::time::Duration;

use anyhow::{Context, Result};
use serde_json::{json, Value};
use tracing::{debug, warn};

/// Escape text for `parse_mode: HTML`.
///
/// Apply to anything from the chain, the config, an LLM or the operator. The only strings
/// that may skip it are literals in this crate's own source.
pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Telegram's hard cap on one message.
const MAX_MESSAGE: usize = 4096;

/// Long-poll timeout. The HTTP client's own timeout must exceed this or every poll ends
/// as a client-side error and the bot looks broken while working perfectly.
const POLL_SECS: u64 = 30;

#[derive(Clone)]
pub struct Telegram {
    token: String,
    http: reqwest::Client,
}

/// One inbound update, narrowed to what this bot acts on.
#[derive(Debug, Clone)]
pub struct Update {
    pub update_id: i64,
    pub kind: UpdateKind,
}

#[derive(Debug, Clone)]
pub enum UpdateKind {
    /// A text message. `user_id` is the sender; `chat_id` is where to reply.
    Message {
        chat_id: i64,
        user_id: i64,
        username: String,
        text: String,
    },
    /// An inline-keyboard press. Carries the button's `data`, which this bot writes.
    Callback {
        chat_id: i64,
        user_id: i64,
        username: String,
        callback_id: String,
        message_id: i64,
        data: String,
    },
    /// Anything else — a photo, a join event, a poll. Skipped, but still consumes its
    /// `update_id`: an update the bot cannot act on must not be re-fetched forever.
    Other,
}

impl Telegram {
    pub fn new(token: impl Into<String>) -> Result<Self> {
        let http = reqwest::Client::builder()
            // Comfortably above POLL_SECS. A client timeout at or below the long-poll
            // window turns every idle poll into a spurious error.
            .timeout(Duration::from_secs(POLL_SECS + 20))
            .build()
            .context("building the Telegram HTTP client")?;
        Ok(Self { token: token.into(), http })
    }

    fn url(&self, method: &str) -> String {
        format!("https://api.telegram.org/bot{}/{}", self.token, method)
    }

    async fn call(&self, method: &str, body: &Value) -> Result<Value> {
        let res = self
            .http
            .post(self.url(method))
            .json(body)
            .send()
            .await
            .with_context(|| format!("calling {method}"))?;

        let status = res.status();
        let value: Value = res
            .json()
            .await
            .with_context(|| format!("decoding the {method} response"))?;

        if !status.is_success() || value["ok"].as_bool() != Some(true) {
            // The description is Telegram's and is safe to log. The URL is NOT — it
            // carries the bot token in its path, so it never appears in an error.
            let desc = value["description"].as_str().unwrap_or("no description");
            anyhow::bail!("{method} failed ({status}): {desc}");
        }
        Ok(value["result"].clone())
    }

    /// Confirm the token works and report who the bot is.
    pub async fn me(&self) -> Result<String> {
        let r = self.call("getMe", &json!({})).await?;
        Ok(r["username"].as_str().unwrap_or("unknown").to_string())
    }

    /// Publish the command list, so Telegram's own menu matches what the bot answers.
    pub async fn set_commands(&self, commands: &[(&str, &str)]) -> Result<()> {
        let list: Vec<Value> = commands
            .iter()
            .map(|(c, d)| json!({ "command": c, "description": d }))
            .collect();
        self.call("setMyCommands", &json!({ "commands": list })).await?;
        Ok(())
    }

    /// Long-poll for updates from `offset`.
    pub async fn updates(&self, offset: i64) -> Result<Vec<Update>> {
        let body = json!({
            "offset": offset,
            "timeout": POLL_SECS,
            // Everything else — channel posts, edits, inline queries — is noise this bot
            // does not act on, and asking for it only widens what has to be ignored.
            "allowed_updates": ["message", "callback_query"],
        });
        let result = self.call("getUpdates", &body).await?;
        let Some(items) = result.as_array() else { return Ok(vec![]) };
        Ok(items.iter().map(parse_update).collect())
    }

    /// Send a message, splitting it across Telegram's 4096-character cap.
    ///
    /// Splits on line boundaries. Splitting mid-line would be tidier arithmetic and would
    /// cut an HTML entity in half, which fails the whole send — the same class of silent
    /// failure the HTML choice exists to avoid.
    pub async fn send(&self, chat_id: i64, html: &str) -> Result<i64> {
        let mut last = 0;
        for chunk in split_message(html) {
            let body = json!({
                "chat_id": chat_id,
                "text": chunk,
                "parse_mode": "HTML",
                "disable_web_page_preview": true,
            });
            let r = self.call("sendMessage", &body).await?;
            last = r["message_id"].as_i64().unwrap_or(0);
        }
        Ok(last)
    }

    /// Send a message carrying an inline keyboard.
    ///
    /// `buttons` is rows of `(label, callback_data)`. Telegram caps `callback_data` at 64
    /// **bytes**, so what this bot puts there is always a short opaque key into its own
    /// pending-action table — never a command with arguments, and never anything a
    /// stranger's crafted callback could turn into an instruction.
    pub async fn send_with_buttons(
        &self,
        chat_id: i64,
        html: &str,
        buttons: &[Vec<(String, String)>],
    ) -> Result<i64> {
        let keyboard: Vec<Vec<Value>> = buttons
            .iter()
            .map(|row| {
                row.iter()
                    .map(|(label, data)| json!({ "text": label, "callback_data": data }))
                    .collect()
            })
            .collect();
        let body = json!({
            "chat_id": chat_id,
            "text": truncate(html),
            "parse_mode": "HTML",
            "disable_web_page_preview": true,
            "reply_markup": { "inline_keyboard": keyboard },
        });
        let r = self.call("sendMessage", &body).await?;
        Ok(r["message_id"].as_i64().unwrap_or(0))
    }

    /// Replace a message's text and drop its keyboard.
    ///
    /// Used the moment a confirmation is answered: leaving a live "Confirm" button under a
    /// resolved prompt is an invitation to press it again, and the second press would be a
    /// second trade.
    pub async fn edit(&self, chat_id: i64, message_id: i64, html: &str) -> Result<()> {
        let body = json!({
            "chat_id": chat_id,
            "message_id": message_id,
            "text": truncate(html),
            "parse_mode": "HTML",
            "disable_web_page_preview": true,
        });
        // A failed edit must not abort the caller: the action it describes has already
        // happened, and the reply that follows is what the operator actually reads.
        if let Err(e) = self.call("editMessageText", &body).await {
            warn!("editMessageText failed: {e}");
        }
        Ok(())
    }

    /// Acknowledge a button press so Telegram stops showing its spinner.
    pub async fn answer_callback(&self, callback_id: &str, text: &str) {
        let body = json!({ "callback_query_id": callback_id, "text": text });
        if let Err(e) = self.call("answerCallbackQuery", &body).await {
            debug!("answerCallbackQuery failed: {e}");
        }
    }
}

fn parse_update(v: &Value) -> Update {
    let update_id = v["update_id"].as_i64().unwrap_or(0);

    if let Some(m) = v.get("message") {
        let chat_id = m["chat"]["id"].as_i64().unwrap_or(0);
        let user_id = m["from"]["id"].as_i64().unwrap_or(0);
        let username = m["from"]["username"]
            .as_str()
            .or_else(|| m["from"]["first_name"].as_str())
            .unwrap_or("unknown")
            .to_string();
        if let Some(text) = m["text"].as_str() {
            return Update {
                update_id,
                kind: UpdateKind::Message {
                    chat_id,
                    user_id,
                    username,
                    text: text.to_string(),
                },
            };
        }
        return Update { update_id, kind: UpdateKind::Other };
    }

    if let Some(c) = v.get("callback_query") {
        return Update {
            update_id,
            kind: UpdateKind::Callback {
                chat_id: c["message"]["chat"]["id"].as_i64().unwrap_or(0),
                user_id: c["from"]["id"].as_i64().unwrap_or(0),
                username: c["from"]["username"]
                    .as_str()
                    .or_else(|| c["from"]["first_name"].as_str())
                    .unwrap_or("unknown")
                    .to_string(),
                callback_id: c["id"].as_str().unwrap_or("").to_string(),
                message_id: c["message"]["message_id"].as_i64().unwrap_or(0),
                data: c["data"].as_str().unwrap_or("").to_string(),
            },
        };
    }

    Update { update_id, kind: UpdateKind::Other }
}

fn truncate(s: &str) -> String {
    if s.len() <= MAX_MESSAGE {
        return s.to_string();
    }
    // The ellipsis is part of the message, so it comes out of the budget rather than
    // being added on top of it. `…` is THREE bytes in UTF-8, not one: reserving a single
    // byte for it — which this did — produces a message two bytes over the cap, and
    // Telegram answers 400 on the one path that exists to stop it doing that.
    const ELLIPSIS: char = '…';
    let mut end = MAX_MESSAGE - ELLIPSIS.len_utf8();
    // Cut on a char boundary, not a byte one — a status report can carry a multi-byte
    // symbol and a byte slice through one panics.
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{ELLIPSIS}", &s[..end])
}

/// Split on line boundaries, never mid-line.
fn split_message(s: &str) -> Vec<String> {
    if s.len() <= MAX_MESSAGE {
        return vec![s.to_string()];
    }
    let mut out = Vec::new();
    let mut current = String::new();
    for line in s.lines() {
        // A single line longer than the cap cannot be split safely on a line boundary, so
        // it is truncated on its own rather than dragging the rest of the report with it.
        if line.len() >= MAX_MESSAGE {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            out.push(truncate(line));
            continue;
        }
        if current.len() + line.len() + 1 > MAX_MESSAGE {
            out.push(std::mem::take(&mut current));
        }
        current.push_str(line);
        current.push('\n');
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_the_three_html_characters() {
        assert_eq!(esc("a<b>&c"), "a&lt;b&gt;&amp;c");
    }

    #[test]
    fn a_markdown_hostile_symbol_survives() {
        // The case that motivates HTML: a launcher-chosen symbol full of Markdown syntax
        // passes through untouched, because none of it is HTML syntax.
        let symbol = "PUMP_IT*[NOW]";
        assert_eq!(esc(symbol), symbol);
    }

    #[test]
    fn short_messages_are_not_split() {
        assert_eq!(split_message("one\ntwo").len(), 1);
    }

    #[test]
    fn long_messages_split_on_line_boundaries() {
        let body = "0123456789\n".repeat(600); // ~6.6 KB
        let parts = split_message(&body);
        assert!(parts.len() > 1);
        for p in &parts {
            assert!(p.len() <= MAX_MESSAGE, "chunk of {} bytes", p.len());
            // Every chunk must be whole lines, or an HTML tag could be cut in half.
            assert!(p.ends_with('\n'));
        }
        let rejoined: String = parts.concat();
        assert_eq!(rejoined, body);
    }

    #[test]
    fn an_overlong_single_line_is_truncated_alone() {
        let body = format!("short\n{}\nshort", "x".repeat(MAX_MESSAGE + 100));
        let parts = split_message(&body);
        assert!(parts.iter().all(|p| p.len() <= MAX_MESSAGE));
    }

    #[test]
    fn truncation_respects_char_boundaries() {
        // A multi-byte symbol straddling the cap must not panic.
        let body = "é".repeat(MAX_MESSAGE);
        let cut = truncate(&body);
        assert!(cut.len() <= MAX_MESSAGE);
    }
}
