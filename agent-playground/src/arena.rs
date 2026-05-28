use crate::agent::{build_client, AgentConfig};
use crate::llm::ChatMessage;
use anyhow::Result;
use chrono::{DateTime, Local};
use tokio::sync::mpsc;
use uuid::Uuid;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct ArenaMessage {
    pub id: Uuid,
    pub from_agent: String,
    pub content: String,
    pub timestamp: DateTime<Local>,
    pub tokens: Option<u32>,
    pub latency_ms: u64,
    pub round: u32,
}

#[derive(Debug, Clone)]
pub enum ArenaEvent {
    Message(ArenaMessage),
    AgentThinking { name: String },
    AgentDone { name: String, #[allow(dead_code)] tokens: u32, latency_ms: u64 },
    AgentError { name: String, error: String },
    RoundStarted(u32),
    ConversationStarted,
    ConversationPaused,
    ModelsListed { agent: String, models: Vec<String> },
    StatusUpdate(String),
}

#[derive(Debug, Clone)]
pub enum PlaygroundCommand {
    SetTopic(String),
    AddAgent(AgentConfig),
    RemoveAgent(String),
    StartConversation,
    PauseConversation,
    StepOnce,
    ListModels,
    ClearHistory,
    SetTurnDelay(u64),
    Quit,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConversationMode {
    RoundRobin,
}

pub struct ArenaConfig {
    pub topic: String,
    pub agents: Vec<AgentConfig>,
    #[allow(dead_code)]
    pub mode: ConversationMode,
    pub turn_delay_ms: u64,
}

pub async fn run_arena(
    initial: ArenaConfig,
    mut cmd_rx: mpsc::Receiver<PlaygroundCommand>,
    event_tx: mpsc::Sender<ArenaEvent>,
) -> Result<()> {
    let mut agents = initial.agents;
    let mut topic = initial.topic;
    let mut history: Vec<ArenaMessage> = Vec::new();
    let mut round = 0u32;
    let mut running = false;
    let mut agent_idx = 0usize;
    let mut turn_delay_ms = initial.turn_delay_ms;

    loop {
        // Drain all pending commands before taking a turn
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                PlaygroundCommand::SetTopic(t) => {
                    topic = t.clone();
                    let _ = event_tx.send(ArenaEvent::StatusUpdate(format!("Topic: {}", t))).await;
                }
                PlaygroundCommand::AddAgent(cfg) => {
                    let name = cfg.name.clone();
                    let label = cfg.model_label();
                    agents.push(cfg);
                    let _ = event_tx
                        .send(ArenaEvent::StatusUpdate(format!("Added {} ({})", name, label)))
                        .await;
                }
                PlaygroundCommand::RemoveAgent(name) => {
                    agents.retain(|a| a.name != name);
                    if agent_idx >= agents.len() && !agents.is_empty() {
                        agent_idx = 0;
                    }
                    let _ = event_tx
                        .send(ArenaEvent::StatusUpdate(format!("Removed agent: {}", name)))
                        .await;
                }
                PlaygroundCommand::StartConversation => {
                    if agents.is_empty() {
                        let _ = event_tx
                            .send(ArenaEvent::StatusUpdate("No agents — add one first.".to_string()))
                            .await;
                    } else {
                        running = true;
                        let _ = event_tx.send(ArenaEvent::ConversationStarted).await;
                    }
                }
                PlaygroundCommand::PauseConversation => {
                    running = false;
                    let _ = event_tx.send(ArenaEvent::ConversationPaused).await;
                }
                PlaygroundCommand::StepOnce => {
                    if !agents.is_empty() {
                        run_turn(&agents, &topic, &mut history, &mut round, &mut agent_idx, &event_tx).await;
                    }
                }
                PlaygroundCommand::ClearHistory => {
                    history.clear();
                    round = 0;
                    agent_idx = 0;
                    let _ = event_tx
                        .send(ArenaEvent::StatusUpdate("History cleared.".to_string()))
                        .await;
                }
                PlaygroundCommand::ListModels => {
                    if let Some(agent) = agents.first() {
                        match build_client(agent) {
                            Ok(client) => match client.list_models().await {
                                Ok(models) => {
                                    let _ = event_tx
                                        .send(ArenaEvent::ModelsListed {
                                            agent: agent.name.clone(),
                                            models,
                                        })
                                        .await;
                                }
                                Err(e) => {
                                    let _ = event_tx
                                        .send(ArenaEvent::StatusUpdate(format!("list_models failed: {}", e)))
                                        .await;
                                }
                            },
                            Err(e) => {
                                let _ = event_tx
                                    .send(ArenaEvent::StatusUpdate(format!("client build error: {}", e)))
                                    .await;
                            }
                        }
                    }
                }
                PlaygroundCommand::SetTurnDelay(ms) => {
                    turn_delay_ms = ms;
                }
                PlaygroundCommand::Quit => return Ok(()),
            }
        }

        if running && !agents.is_empty() {
            run_turn(&agents, &topic, &mut history, &mut round, &mut agent_idx, &event_tx).await;
            if turn_delay_ms > 0 {
                tokio::time::sleep(std::time::Duration::from_millis(turn_delay_ms)).await;
            }
        } else {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }
}

async fn run_turn(
    agents: &[AgentConfig],
    topic: &str,
    history: &mut Vec<ArenaMessage>,
    round: &mut u32,
    agent_idx: &mut usize,
    event_tx: &mpsc::Sender<ArenaEvent>,
) {
    let idx = *agent_idx % agents.len();
    let agent = &agents[idx];
    *round += 1;

    let _ = event_tx.send(ArenaEvent::RoundStarted(*round)).await;
    let _ = event_tx
        .send(ArenaEvent::AgentThinking { name: agent.name.clone() })
        .await;

    let messages = build_prompt(agent, topic, history, agents);

    let result = match build_client(agent) {
        Ok(client) => client.chat(messages, agent.max_tokens).await,
        Err(e) => Err(e),
    };

    match result {
        Ok(resp) => {
            let _ = event_tx
                .send(ArenaEvent::AgentDone {
                    name: agent.name.clone(),
                    tokens: resp.tokens_used.unwrap_or(0),
                    latency_ms: resp.latency_ms,
                })
                .await;

            let msg = ArenaMessage {
                id: Uuid::new_v4(),
                from_agent: agent.name.clone(),
                content: resp.content,
                timestamp: Local::now(),
                tokens: resp.tokens_used,
                latency_ms: resp.latency_ms,
                round: *round,
            };
            history.push(msg.clone());
            let _ = event_tx.send(ArenaEvent::Message(msg)).await;
        }
        Err(e) => {
            let _ = event_tx
                .send(ArenaEvent::AgentError {
                    name: agent.name.clone(),
                    error: e.to_string(),
                })
                .await;
        }
    }

    *agent_idx = (idx + 1) % agents.len();
}

fn build_prompt(
    agent: &AgentConfig,
    topic: &str,
    history: &[ArenaMessage],
    all_agents: &[AgentConfig],
) -> Vec<ChatMessage> {
    let others: Vec<&str> = all_agents
        .iter()
        .filter(|a| a.name != agent.name)
        .map(|a| a.name.as_str())
        .collect();

    let others_str = if others.is_empty() {
        "no other agents (you are alone in this arena)".to_string()
    } else {
        others.join(", ")
    };

    let system = format!(
        "You are {name}. {persona}\n\n\
         You are participating in a live multi-agent debate arena. Topic: \"{topic}\"\n\
         Other participants: {others}\n\n\
         Rules:\n\
         - Respond in 2-4 paragraphs max. Be sharp and concise.\n\
         - Reference and challenge what others have said when relevant.\n\
         - Stay in character at all times.\n\
         - Do NOT introduce yourself, repeat the topic, or use phrases like \"As an AI\".\n\
         - Speak directly and confidently.",
        name = agent.name,
        persona = agent.persona,
        topic = topic,
        others = others_str,
    );

    let mut messages = vec![ChatMessage::system(system)];

    if history.is_empty() {
        messages.push(ChatMessage::user(format!(
            "The arena is open. Topic: \"{}\"\n\nShare your opening position.",
            topic
        )));
    } else {
        // Keep the last 12 messages to stay within context
        let recent = history.iter().rev().take(12).collect::<Vec<_>>();
        let transcript: String = recent
            .iter()
            .rev()
            .map(|m| format!("**{}**: {}", m.from_agent, m.content))
            .collect::<Vec<_>>()
            .join("\n\n");

        messages.push(ChatMessage::user(format!(
            "Conversation so far:\n\n{}\n\n---\n\nYour turn, {}.",
            transcript, agent.name
        )));
    }

    messages
}
