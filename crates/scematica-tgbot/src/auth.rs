//! Who may talk to this bot.
//!
//! ## The threat this file exists for
//!
//! A Telegram bot token is a **public endpoint**. Anyone who learns the bot's @username
//! can message it — and the token has appeared in a chat log, a screenshot or a commit
//! more than once in this project's history. Without an allow-list, `/dump` from a
//! stranger force-sells every open position at `min_out = 0`.
//!
//! So: **deny by default**. An empty owner list authorises nobody, not everybody, and not
//! "the first person to say hello". That last one is the tempting design and it is wrong —
//! it hands the bot to whoever finds it first, and the operator cannot tell it happened.
//!
//! ## Claiming, and why the code goes to the console
//!
//! Nobody knows their own Telegram numeric user id, so an allow-list keyed on it is
//! unusable without a bootstrap. `/claim <code>` is that bootstrap, and the code is
//! printed **to the operator's terminal** — never sent over Telegram. That is the whole
//! security argument: the channel proving you are the operator is the machine the bot runs
//! on, which is a thing an attacker who merely found the @username does not have.
//!
//! The code is single-use, expires, and claiming is refused once an owner exists. A second
//! owner is added by editing `SCEMA_TG_OWNERS`, deliberately: adding one should require
//! the same access that starting the bot did.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use rand::Rng;

/// How long a printed claim code stays valid.
const CLAIM_TTL: Duration = Duration::from_secs(15 * 60);

/// Environment variable holding the comma-separated allow-list of Telegram user ids.
pub const OWNERS_ENV: &str = "SCEMA_TG_OWNERS";

pub struct Auth {
    /// Telegram user ids permitted to command the bot.
    owners: Mutex<HashSet<i64>>,
    /// The live claim code, if the bot started with no owners.
    claim: Mutex<Option<Claim>>,
    /// True when the allow-list came from the environment rather than a claim. A claimed
    /// owner is in memory only, so the operator has to be told to persist it.
    configured: bool,
}

struct Claim {
    code: String,
    issued: Instant,
}

/// What `authorise` decided, and why.
#[derive(Debug, Clone, PartialEq)]
pub enum Access {
    /// This user is on the allow-list.
    Owner,
    /// Refused. The message is safe to send back — it says nothing about who IS allowed.
    Denied(String),
}

impl Auth {
    /// Read the allow-list from the environment.
    ///
    /// Malformed entries are **skipped with a warning rather than ignored silently**: an
    /// id with a stray space that quietly fails to parse produces a bot that refuses its
    /// own operator and gives no reason.
    pub fn from_env() -> Self {
        let raw = std::env::var(OWNERS_ENV).unwrap_or_default();
        let mut owners = HashSet::new();
        for part in raw.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            match part.parse::<i64>() {
                Ok(id) => {
                    owners.insert(id);
                }
                Err(_) => {
                    tracing::warn!(
                        "{OWNERS_ENV} entry {part:?} is not a Telegram user id and was skipped"
                    );
                }
            }
        }
        let configured = !owners.is_empty();
        Self {
            owners: Mutex::new(owners),
            claim: Mutex::new(None),
            configured,
        }
    }

    pub fn owner_count(&self) -> usize {
        self.owners.lock().len()
    }

    pub fn was_configured(&self) -> bool {
        self.configured
    }

    /// Mint a claim code. Returns `None` when an owner already exists.
    pub fn issue_claim(&self) -> Option<String> {
        if !self.owners.lock().is_empty() {
            return None;
        }
        // Six digits from an unambiguous alphabet: no 0/O, no 1/I/l. The code is read off
        // a terminal and typed into a phone, and a claim that fails because someone read a
        // zero as an O teaches them to paste the token somewhere instead.
        const ALPHABET: &[u8] = b"23456789ABCDEFGHJKMNPQRSTUVWXYZ";
        let mut rng = rand::thread_rng();
        let code: String = (0..6)
            .map(|_| ALPHABET[rng.gen_range(0..ALPHABET.len())] as char)
            .collect();
        *self.claim.lock() = Some(Claim { code: code.clone(), issued: Instant::now() });
        Some(code)
    }

    /// Attempt a claim. On success the caller becomes the sole owner.
    pub fn try_claim(&self, user_id: i64, offered: &str) -> Result<(), String> {
        if !self.owners.lock().is_empty() {
            return Err(
                "This bot already has an owner. Add another by setting SCEMA_TG_OWNERS and \
                 restarting — which needs access to the machine, exactly as claiming did."
                    .into(),
            );
        }
        let mut guard = self.claim.lock();
        let Some(claim) = guard.as_ref() else {
            return Err("No claim code is active. Restart the bot to print a new one.".into());
        };
        if claim.issued.elapsed() > CLAIM_TTL {
            *guard = None;
            return Err("That claim code has expired. Restart the bot to print a new one.".into());
        }
        // Case-insensitive because the alphabet is upper-case and phones capitalise.
        if !claim.code.eq_ignore_ascii_case(offered.trim()) {
            // Deliberately not "wrong code" with a retry counter: the code is single-use
            // and short-lived, and the honest failure is the same either way.
            return Err("That code is not valid.".into());
        }
        *guard = None;
        self.owners.lock().insert(user_id);
        Ok(())
    }

    /// May this user command the bot?
    pub fn authorise(&self, user_id: i64) -> Access {
        if self.owners.lock().contains(&user_id) {
            return Access::Owner;
        }
        if self.owners.lock().is_empty() {
            return Access::Denied(
                "This bot has no owner yet. Run it and read the claim code from the \
                 operator's console, then send:\n\n<code>/claim CODE</code>"
                    .into(),
            );
        }
        // Says nothing about who the owner is, and offers no path in. A refusal that
        // hints at the allow-list is a refusal that helps.
        Access::Denied("Not authorised.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty() -> Auth {
        Auth {
            owners: Mutex::new(HashSet::new()),
            claim: Mutex::new(None),
            configured: false,
        }
    }

    fn owned(id: i64) -> Auth {
        let mut s = HashSet::new();
        s.insert(id);
        Auth { owners: Mutex::new(s), claim: Mutex::new(None), configured: true }
    }

    #[test]
    fn an_empty_allow_list_authorises_nobody() {
        let a = empty();
        assert!(matches!(a.authorise(1), Access::Denied(_)));
        assert!(matches!(a.authorise(999), Access::Denied(_)));
    }

    #[test]
    fn a_stranger_is_refused_when_an_owner_exists() {
        let a = owned(7);
        assert_eq!(a.authorise(7), Access::Owner);
        assert!(matches!(a.authorise(8), Access::Denied(_)));
    }

    #[test]
    fn a_refusal_never_names_the_owner() {
        let a = owned(4242);
        let Access::Denied(msg) = a.authorise(1) else { panic!("expected a refusal") };
        assert!(!msg.contains("4242"));
    }

    #[test]
    fn claiming_needs_the_printed_code() {
        let a = empty();
        let code = a.issue_claim().expect("a bot with no owner issues a code");
        assert!(a.try_claim(5, "WRONG1").is_err());
        assert!(a.try_claim(5, &code).is_ok());
        assert_eq!(a.authorise(5), Access::Owner);
    }

    #[test]
    fn a_code_is_single_use() {
        let a = empty();
        let code = a.issue_claim().unwrap();
        assert!(a.try_claim(5, &code).is_ok());
        // The second attempt is refused because an owner now exists — which is also what
        // stops a leaked code being replayed by somebody else.
        assert!(a.try_claim(6, &code).is_err());
        assert!(matches!(a.authorise(6), Access::Denied(_)));
    }

    #[test]
    fn no_code_is_issued_once_an_owner_exists() {
        assert!(owned(1).issue_claim().is_none());
    }

    #[test]
    fn an_expired_code_is_refused() {
        let a = empty();
        let code = a.issue_claim().unwrap();
        *a.claim.lock() = Some(Claim {
            code: code.clone(),
            issued: Instant::now() - CLAIM_TTL - Duration::from_secs(1),
        });
        assert!(a.try_claim(5, &code).is_err());
        assert!(matches!(a.authorise(5), Access::Denied(_)));
    }

    #[test]
    fn a_malformed_owner_entry_does_not_authorise_zero() {
        // The trap: `"".parse::<i64>()` fails, but a sloppy parser using `unwrap_or(0)`
        // would insert 0 — and a Telegram user id is never 0, so nothing would break
        // visibly while the list silently held a junk entry.
        std::env::set_var(OWNERS_ENV, "not-an-id, 42 ,");
        let a = Auth::from_env();
        assert_eq!(a.owner_count(), 1);
        assert_eq!(a.authorise(42), Access::Owner);
        assert!(matches!(a.authorise(0), Access::Denied(_)));
        std::env::remove_var(OWNERS_ENV);
    }
}
