//! Grok, over Telegram.
//!
//! Thin on purpose. `scematica-ai`'s `ChatAgent` already owns the model call, the tool
//! definitions, the conversation history and — the part that matters here — the risk
//! classification and the confirmation gate. This module wires it to a chat id and does
//! nothing else. A second LLM client would be a second place for the tool allow-list to
//! drift, which is the failure the whole previous pass of work existed to remove.
//!
//! ## What a confirmation means when the transport is Telegram
//!
//! `classify_risk` marks `swap_token` and `x402_fetch` High and `set_bot_mode` Moderate;
//! everything else is Safe and runs without asking. That split was written for a TUI where
//! the operator is sitting at the machine. Over Telegram the same split is doing more
//! work, because the request arrives from a phone that may be unlocked on a table — so:
//!
//! * a pending action is bound to **one chat and one user**, and a press from anyone else
//!   is refused even if they are also an owner (a shared group must not let one owner
//!   confirm another's trade);
//! * it **expires**, because an unanswered "sell 2 SOL?" from an hour ago is not consent;
//! * the keyboard is **removed on the first press**, so a resolved prompt cannot be
//!   pressed twice — a second press would be a second trade, and the treasury path already
//!   taught this project what a double-paid action costs.
//!
//! ## One conversation per chat
//!
//! History is per chat id, not global. Two owners in two DMs share a bot and must not
//! share a context: the model would answer one from the other's positions, and the
//! provenance of an answer is exactly what this project refuses to leave ambiguous.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use anyhow::Result;
use scematica_ai::chat_agent::ChatAgent;
use scematica_ai::chat_types::{AgentOutput, PendingToolCall, RiskLevel};
use scematica_ai::client::AiClient;
use scematica_ai::conversation::ConversationHistory;
use scematica_ai::prompts::CHAT_AGENT_SYSTEM;
use scematica_ai::tool_dispatcher::ToolDispatcher;
use scematica_ai::types::{AiProvider, ChatMessage};

/// How long an unanswered confirmation stays pressable.
const PENDING_TTL: Duration = Duration::from_secs(5 * 60);

/// Turns kept per chat. The same cap the dashboard uses.
const HISTORY_TURNS: usize = 50;

pub struct Pending {
    pub call: PendingToolCall,
    /// The chat and user the prompt was shown to. Both must match on the press.
    pub chat_id: i64,
    pub user_id: i64,
    pub message_id: i64,
    issued: Instant,
}

impl Pending {
    pub fn expired(&self) -> bool {
        self.issued.elapsed() > PENDING_TTL
    }
}

pub struct Chat {
    /// One agent per chat id. Absent until the chat's first message.
    agents: HashMap<i64, ChatAgent>,
    /// Live confirmations, keyed by the short token that rides in `callback_data`.
    pending: HashMap<String, Pending>,
    provider: String,
    model: String,
    next_token: u64,
}

impl Chat {
    /// Connect to a provider.
    ///
    /// Grok first when `XAI_API_KEY` is set, because that is what was asked for; otherwise
    /// `AiClient::from_env`'s own order decides. Refusing outright when no key exists is
    /// deliberate — the alternative is a bot that answers trading questions from a model
    /// nobody configured, or worse, silently answers nothing.
    pub fn connect() -> Result<Self> {
        let client = if std::env::var("XAI_API_KEY").is_ok() {
            AiClient::new(AiProvider::Grok)?
        } else {
            AiClient::from_env()?
        };
        let provider = client.provider_name().to_string();
        let model = client.model.clone();
        // The client is rebuilt per chat below; this one existed to report what connected.
        drop(client);
        Ok(Self {
            agents: HashMap::new(),
            pending: HashMap::new(),
            provider,
            model,
            next_token: 1,
        })
    }

    pub fn provider(&self) -> &str {
        &self.provider
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    fn agent_for(&mut self, chat_id: i64) -> Result<&mut ChatAgent> {
        if !self.agents.contains_key(&chat_id) {
            let client = if std::env::var("XAI_API_KEY").is_ok() {
                AiClient::new(AiProvider::Grok)?
            } else {
                AiClient::from_env()?
            };
            let history =
                ConversationHistory::new(ChatMessage::system(CHAT_AGENT_SYSTEM), HISTORY_TURNS);
            self.agents
                .insert(chat_id, ChatAgent::new(client, history, ToolDispatcher::new()));
        }
        Ok(self.agents.get_mut(&chat_id).expect("just inserted"))
    }

    /// What the bot should do with a chat message.
    pub async fn ask(&mut self, chat_id: i64, user_id: i64, text: &str) -> Result<Reply> {
        let out = self.agent_for(chat_id)?.process(text).await?;
        Ok(self.absorb(chat_id, user_id, out))
    }

    /// Run a pending call after the operator pressed Confirm.
    pub async fn confirm(&mut self, chat_id: i64, user_id: i64, token: &str) -> Result<Reply> {
        // Removed from the table before it runs, not after: the token is single-use, and a
        // second press arriving while the first is still in flight must find nothing.
        let Some(p) = self.pending.remove(token) else {
            return Ok(Reply::Gone);
        };
        if p.chat_id != chat_id || p.user_id != user_id {
            // Put it back — this press was from the wrong person, and refusing it must not
            // also destroy the prompt the right person is still looking at.
            self.pending.insert(token.to_string(), p);
            return Ok(Reply::WrongUser);
        }
        if p.expired() {
            return Ok(Reply::Expired);
        }
        let out = self.agent_for(chat_id)?.confirm_pending().await?;
        Ok(self.absorb(chat_id, user_id, out))
    }

    /// Drop a pending call because the operator pressed Cancel.
    pub fn cancel(&mut self, chat_id: i64, user_id: i64, token: &str) -> Reply {
        let Some(p) = self.pending.remove(token) else {
            return Reply::Gone;
        };
        if p.chat_id != chat_id || p.user_id != user_id {
            self.pending.insert(token.to_string(), p);
            return Reply::WrongUser;
        }
        match self.agents.get_mut(&chat_id) {
            Some(a) => Reply::Text(a.reject_pending()),
            None => Reply::Text("Cancelled.".into()),
        }
    }

    /// Forget a chat's history.
    pub fn reset(&mut self, chat_id: i64) {
        self.agents.remove(&chat_id);
        self.pending.retain(|_, p| p.chat_id != chat_id);
    }

    /// Drop confirmations nobody answered.
    ///
    /// Called from the poll loop rather than on a timer of its own: this is bookkeeping,
    /// and it must never be the thing that decides anything. Expiry is also checked at the
    /// point of use, so a missed sweep cannot resurrect a stale prompt.
    pub fn sweep(&mut self) {
        self.pending.retain(|_, p| !p.expired());
    }

    fn absorb(&mut self, chat_id: i64, user_id: i64, out: AgentOutput) -> Reply {
        match out {
            AgentOutput::Reply(r) => Reply::Text(r.message),
            AgentOutput::NeedsConfirmation(call) => {
                let token = format!("c{}", self.next_token);
                self.next_token += 1;
                let summary = call.summary.clone();
                let risk = call.risk.clone();
                self.pending.insert(
                    token.clone(),
                    Pending {
                        call,
                        chat_id,
                        user_id,
                        message_id: 0,
                        issued: Instant::now(),
                    },
                );
                Reply::Confirm { token, summary, risk }
            }
        }
    }

    /// Record which message carries a prompt, so its keyboard can be removed on answer.
    pub fn bind_message(&mut self, token: &str, message_id: i64) {
        if let Some(p) = self.pending.get_mut(token) {
            p.message_id = message_id;
        }
    }

    pub fn message_of(&self, token: &str) -> Option<i64> {
        self.pending.get(token).map(|p| p.message_id)
    }
}

/// What the caller should send back.
pub enum Reply {
    Text(String),
    Confirm {
        token: String,
        summary: String,
        risk: RiskLevel,
    },
    /// The token is unknown — already answered, or swept.
    Gone,
    Expired,
    WrongUser,
}

#[cfg(test)]
mod tests {
    use super::*;
    use scematica_ai::chat_types::ToolCall;

    fn pending(chat: i64, user: i64, age: Duration) -> Pending {
        Pending {
            call: PendingToolCall {
                call_id: "1".into(),
                call: ToolCall::GetBalance,
                summary: "check balance".into(),
                risk: RiskLevel::Safe,
            },
            chat_id: chat,
            user_id: user,
            message_id: 0,
            issued: Instant::now() - age,
        }
    }

    #[test]
    fn a_fresh_confirmation_is_pressable_and_an_old_one_is_not() {
        assert!(!pending(1, 1, Duration::from_secs(1)).expired());
        assert!(pending(1, 1, PENDING_TTL + Duration::from_secs(1)).expired());
    }

    #[test]
    fn a_press_from_the_wrong_user_does_not_consume_the_prompt() {
        // The property that matters in a shared group: refusing the wrong presser must
        // leave the prompt intact for the right one, so a stray tap cannot cancel somebody
        // else's trade by making the token vanish.
        let mut c = Chat {
            agents: HashMap::new(),
            pending: HashMap::new(),
            provider: "test".into(),
            model: "test".into(),
            next_token: 1,
        };
        c.pending.insert("c1".into(), pending(10, 20, Duration::from_secs(0)));
        let r = c.cancel(10, 999, "c1");
        assert!(matches!(r, Reply::WrongUser));
        assert!(c.pending.contains_key("c1"), "the prompt must survive a wrong-user press");
    }

    #[test]
    fn a_press_from_the_wrong_chat_is_refused() {
        let mut c = Chat {
            agents: HashMap::new(),
            pending: HashMap::new(),
            provider: "test".into(),
            model: "test".into(),
            next_token: 1,
        };
        c.pending.insert("c1".into(), pending(10, 20, Duration::from_secs(0)));
        assert!(matches!(c.cancel(11, 20, "c1"), Reply::WrongUser));
    }

    #[test]
    fn an_unknown_token_is_gone_rather_than_an_error() {
        let mut c = Chat {
            agents: HashMap::new(),
            pending: HashMap::new(),
            provider: "test".into(),
            model: "test".into(),
            next_token: 1,
        };
        assert!(matches!(c.cancel(1, 1, "nope"), Reply::Gone));
    }

    #[test]
    fn sweeping_drops_only_the_expired() {
        let mut c = Chat {
            agents: HashMap::new(),
            pending: HashMap::new(),
            provider: "test".into(),
            model: "test".into(),
            next_token: 1,
        };
        c.pending.insert("fresh".into(), pending(1, 1, Duration::from_secs(0)));
        c.pending.insert("old".into(), pending(1, 1, PENDING_TTL + Duration::from_secs(1)));
        c.sweep();
        assert!(c.pending.contains_key("fresh"));
        assert!(!c.pending.contains_key("old"));
    }

    #[test]
    fn resetting_a_chat_drops_its_prompts_but_not_another_chats() {
        let mut c = Chat {
            agents: HashMap::new(),
            pending: HashMap::new(),
            provider: "test".into(),
            model: "test".into(),
            next_token: 1,
        };
        c.pending.insert("a".into(), pending(1, 1, Duration::from_secs(0)));
        c.pending.insert("b".into(), pending(2, 2, Duration::from_secs(0)));
        c.reset(1);
        assert!(!c.pending.contains_key("a"));
        assert!(c.pending.contains_key("b"));
    }
}
