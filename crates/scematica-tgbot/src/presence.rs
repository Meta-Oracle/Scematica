//! Which bot this process is polling, announced on the File-Based IPC surface.
//!
//! ## The failure this exists for
//!
//! Telegram's `getUpdates` delivers each update to **exactly one** caller. Two processes
//! holding the same bot token therefore do not both receive the operator's commands — they
//! split them at random, with no error on either side, and one of the two can sell
//! positions. `auth.rs` guards *who* may command the bot; nothing guarded *how many
//! processes were listening*.
//!
//! The Omni-Agent's cockpit already refuses to poll a token it can see belongs to this bot.
//! The words "can see" were the whole problem: it compared `SCEMA_AGENT_TG_TOKEN` against
//! `SCEMA_TG_TOKEN` **in its own process**, and the two live in separate `.env` files that
//! are never loaded together. So the check was structurally unable to fire in the exact
//! deployment it was written for — and it did not, against two processes pointed at
//! @Scematicabot. A guard that cannot fire is worse than no guard, because it is reported
//! as passing.
//!
//! ## Why a file, and why this file
//!
//! The two processes share no language, no library and no config. They do share a
//! directory: this repository's processes talk through JSON files in the bot's working
//! directory and nothing else. So this is the existing convention rather than a new
//! mechanism — write to `.tmp`, rename, and let the reader judge liveness.
//!
//! ## Why the bot id and not the token
//!
//! A Telegram token is `<bot_id>:<secret>`. Publishing the id alone identifies the bot
//! exactly, and publishing the secret would put a credential in a world-readable file
//! beside the metrics. The id is not a secret — it is the first thing `getMe` returns.
//!
//! ## Why a pid and not a timestamp
//!
//! A timestamp needs a heartbeat to mean anything, and a heartbeat is a second thing that
//! can be wrong. A pid answers "is that process still there" directly, which is the
//! question, and it is the same answer `main.rs` already gets from `tasklist` for the
//! sniper. A stale file left by a crash names a pid nobody is running, and the reader
//! treats that as absent rather than as a conflict.

use std::path::PathBuf;

use scematica_core::metrics::artifact_path;
use serde::{Deserialize, Serialize};

/// Where the announcement lives, beside the sniper's own IPC files.
pub const PRESENCE_FILE: &str = "scematica-tgbot-presence.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Presence {
    /// The numeric bot id — the part of the token before the colon. Never the secret.
    pub bot_id: i64,
    /// The @username, for a message a human can act on without decoding an id.
    pub username: String,
    /// This process. A reader asks the OS whether it is still alive.
    pub pid: u32,
    pub started_at: String,
}

/// The bot id carried by a token, without keeping the token.
///
/// Returns `None` for anything that is not `<digits>:<secret>` rather than guessing. A
/// malformed token is Telegram's problem to report, and inventing an id here would make
/// two unrelated bots look like a conflict.
pub fn bot_id_of(token: &str) -> Option<i64> {
    token.split_once(':')?.0.trim().parse().ok()
}

fn path() -> PathBuf {
    artifact_path(PRESENCE_FILE)
}

/// Announce that this process is polling this bot.
///
/// Best-effort on purpose: the bot must start and be useful on a read-only directory, and
/// failing to publish a courtesy to another process is not a reason to refuse to run. The
/// caller logs the failure and carries on.
pub fn announce(token: &str, username: &str) -> std::io::Result<()> {
    let Some(bot_id) = bot_id_of(token) else {
        return Ok(());
    };
    let presence = Presence {
        bot_id,
        username: username.to_string(),
        pid: std::process::id(),
        started_at: chrono::Utc::now().to_rfc3339(),
    };

    // Write then rename, the convention every writer here follows: a reader must never
    // catch a half-written file, and on this path a half-written file would be read as
    // "no conflict" — the answer that costs something.
    let target = path();
    let mut tmp = target.as_os_str().to_os_string();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, serde_json::to_vec_pretty(&presence)?)?;
    std::fs::rename(&tmp, &target)
}

/// Withdraw the announcement.
///
/// Only correct on a clean exit. A crash leaves the file behind, which is why the reader
/// checks the pid rather than trusting the file's existence.
pub fn withdraw() {
    let _ = std::fs::remove_file(path());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bot_id_is_the_part_before_the_colon() {
        assert_eq!(bot_id_of("8849814959:AAEg4WurVrJwvq9C"), Some(8849814959));
    }

    #[test]
    fn a_token_without_a_colon_has_no_id() {
        // Not zero, and not a guess: two unrelated malformed tokens must not collide on a
        // default id and be reported as the same bot.
        assert_eq!(bot_id_of("nonsense"), None);
        assert_eq!(bot_id_of(""), None);
    }

    #[test]
    fn a_non_numeric_prefix_has_no_id() {
        assert_eq!(bot_id_of("abc:def"), None);
    }

    #[test]
    fn the_secret_is_never_part_of_the_id() {
        // The whole reason this publishes an id rather than a token.
        let id = bot_id_of("8849814959:SECRET").unwrap();
        assert!(!id.to_string().contains("SECRET"));
    }
}
