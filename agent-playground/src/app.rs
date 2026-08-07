use crate::agent::{AgentConfig, AgentStatus, BackendConfig, AGENT_COLORS};
use crate::arena::{ArenaConfig, ArenaEvent, ArenaMessage, ConversationMode, PlaygroundCommand};
use crate::config::PlaygroundConfig;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::style::Color;
use std::collections::HashMap;
use tokio::sync::mpsc;

#[derive(Debug, Clone, PartialEq)]
pub enum InputMode {
    Normal,
    Editing,
}

pub struct App {
    pub topic: String,
    pub agents: Vec<AgentConfig>,
    pub agent_statuses: HashMap<String, AgentStatus>,
    pub messages: Vec<ArenaMessage>,
    pub input: String,
    pub input_mode: InputMode,
    pub scroll_offset: u16,
    pub follow_mode: bool,
    pub should_quit: bool,
    pub status_msg: String,
    pub round: u32,
    pub is_running: bool,
    pub available_models: Vec<String>,
    pub total_tokens: u32,
    pub turn_delay_ms: u64,
    pub help_visible: bool,
    cmd_tx: mpsc::Sender<PlaygroundCommand>,
    msg_rx: mpsc::Receiver<ArenaEvent>,
}

impl App {
    pub fn new(
        config: PlaygroundConfig,
        cmd_tx: mpsc::Sender<PlaygroundCommand>,
        msg_rx: mpsc::Receiver<ArenaEvent>,
    ) -> Self {
        let agents = config.agents;
        let statuses = agents
            .iter()
            .map(|a| (a.name.clone(), AgentStatus::new(&a.name)))
            .collect();

        Self {
            topic: config.topic,
            agents,
            agent_statuses: statuses,
            messages: Vec::new(),
            input: String::new(),
            input_mode: InputMode::Normal,
            scroll_offset: 0,
            follow_mode: true,
            should_quit: false,
            status_msg: "Ready  |  [s] start  [i] command  [?] help  [q] quit".to_string(),
            round: 0,
            is_running: false,
            available_models: Vec::new(),
            total_tokens: 0,
            turn_delay_ms: config.turn_delay_ms,
            help_visible: false,
            cmd_tx,
            msg_rx,
        }
    }

    pub fn load_demo(&mut self) {
        self.topic = "Is artificial general intelligence inevitable, and should we fear it?".to_string();
        self.agents = vec![
            AgentConfig {
                name: "Socrates".to_string(),
                backend: BackendConfig::Ollama {
                    base_url: "http://localhost:11434".to_string(),
                    model: "phi3".to_string(),
                },
                persona: "You are Socrates, the ancient Greek philosopher. You claim to know nothing, ask probing questions, and dismantle weak arguments through careful dialectic. You speak in a measured, ironic tone.".to_string(),
                max_tokens: 400,
            },
            AgentConfig {
                name: "Turing".to_string(),
                backend: BackendConfig::Ollama {
                    base_url: "http://localhost:11434".to_string(),
                    model: "phi3".to_string(),
                },
                persona: "You are Alan Turing, the father of computer science and artificial intelligence. You think rigorously about computation, the nature of machine thought, and the imitation game. You are quietly confident and deeply analytical.".to_string(),
                max_tokens: 400,
            },
            AgentConfig {
                name: "Nietzsche".to_string(),
                backend: BackendConfig::Ollama {
                    base_url: "http://localhost:11434".to_string(),
                    model: "phi3".to_string(),
                },
                persona: "You are Friedrich Nietzsche, the philosopher of will-to-power and the eternal recurrence. You reject timid thinking, embrace contradiction, and speak with prophetic intensity. You see AGI as either the ultimate Ubermensch or the ultimate nihilism.".to_string(),
                max_tokens: 400,
            },
        ];
        self.rebuild_statuses();
        self.status_msg = "Demo loaded — 3 philosophers debate AGI. Press [s] to start!".to_string();
    }

    fn rebuild_statuses(&mut self) {
        self.agent_statuses = self
            .agents
            .iter()
            .map(|a| (a.name.clone(), AgentStatus::new(&a.name)))
            .collect();
    }

    pub fn arena_config(&self) -> ArenaConfig {
        ArenaConfig {
            topic: self.topic.clone(),
            agents: self.agents.clone(),
            mode: ConversationMode::RoundRobin,
            turn_delay_ms: self.turn_delay_ms,
        }
    }

    pub fn agent_color(&self, name: &str) -> Color {
        let idx = self.agents.iter().position(|a| a.name == name).unwrap_or(0);
        AGENT_COLORS[idx % AGENT_COLORS.len()]
    }

    pub fn poll_events(&mut self) {
        while let Ok(event) = self.msg_rx.try_recv() {
            self.apply_event(event);
        }
    }

    fn apply_event(&mut self, event: ArenaEvent) {
        match event {
            ArenaEvent::Message(msg) => {
                if let Some(s) = self.agent_statuses.get_mut(&msg.from_agent) {
                    s.messages_sent += 1;
                    s.last_latency_ms = msg.latency_ms;
                    s.is_thinking = false;
                    if let Some(t) = msg.tokens {
                        s.total_tokens += t;
                        self.total_tokens += t;
                    }
                }
                self.messages.push(msg);
                if self.follow_mode {
                    self.scroll_offset = u16::MAX;
                }
            }
            ArenaEvent::AgentThinking { name } => {
                if let Some(s) = self.agent_statuses.get_mut(&name) {
                    s.is_thinking = true;
                    s.is_online = true;
                }
            }
            ArenaEvent::AgentDone { name, tokens: _, latency_ms } => {
                if let Some(s) = self.agent_statuses.get_mut(&name) {
                    s.is_thinking = false;
                    s.last_latency_ms = latency_ms;
                }
            }
            ArenaEvent::AgentError { name, error } => {
                if let Some(s) = self.agent_statuses.get_mut(&name) {
                    s.is_thinking = false;
                    s.is_online = false;
                    s.last_error = Some(error.clone());
                }
                let short = if error.len() > 70 { &error[..70] } else { &error };
                self.status_msg = format!("[ERR] {}: {}", name, short);
            }
            ArenaEvent::RoundStarted(r) => {
                self.round = r;
            }
            ArenaEvent::ConversationStarted => {
                self.is_running = true;
                self.status_msg = "Conversation running...".to_string();
            }
            ArenaEvent::ConversationPaused => {
                self.is_running = false;
                self.status_msg = "Paused. [s] to resume, [n] to step.".to_string();
            }
            ArenaEvent::ModelsListed { agent, models } => {
                self.available_models = models.clone();
                self.status_msg = format!("[{}] models: {}", agent, models.join(", "));
            }
            ArenaEvent::StatusUpdate(msg) => {
                self.status_msg = msg;
            }
        }
    }

    pub async fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        let cmd_tx = self.cmd_tx.clone();
        match self.input_mode {
            InputMode::Normal => self.handle_normal_key(key, &cmd_tx).await?,
            InputMode::Editing => self.handle_edit_key(key, &cmd_tx).await?,
        }
        Ok(())
    }

    async fn handle_normal_key(
        &mut self,
        key: KeyEvent,
        cmd_tx: &mpsc::Sender<PlaygroundCommand>,
    ) -> Result<()> {
        match key.code {
            KeyCode::Char('q') | KeyCode::Char('Q') => {
                let _ = cmd_tx.send(PlaygroundCommand::Quit).await;
                self.should_quit = true;
            }
            KeyCode::Char('i') | KeyCode::Char(':') => {
                self.input_mode = InputMode::Editing;
                self.status_msg = "Enter command (/help for list)".to_string();
                self.help_visible = false;
            }
            KeyCode::Char('s') => {
                if self.is_running {
                    let _ = cmd_tx.send(PlaygroundCommand::PauseConversation).await;
                } else {
                    let _ = cmd_tx.send(PlaygroundCommand::StartConversation).await;
                }
            }
            KeyCode::Char('n') => {
                let _ = cmd_tx.send(PlaygroundCommand::StepOnce).await;
                self.status_msg = "Stepping...".to_string();
            }
            KeyCode::Char('m') => {
                let _ = cmd_tx.send(PlaygroundCommand::ListModels).await;
                self.status_msg = "Fetching available models...".to_string();
            }
            KeyCode::Char('c') => {
                let _ = cmd_tx.send(PlaygroundCommand::ClearHistory).await;
                self.messages.clear();
                self.scroll_offset = 0;
                self.round = 0;
                self.total_tokens = 0;
            }
            KeyCode::Char('?') | KeyCode::Char('h') => {
                self.help_visible = !self.help_visible;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.follow_mode = false;
                self.scroll_offset = self.scroll_offset.saturating_sub(3);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.follow_mode = false;
                self.scroll_offset = self.scroll_offset.saturating_add(3);
            }
            KeyCode::PageUp => {
                self.follow_mode = false;
                self.scroll_offset = self.scroll_offset.saturating_sub(20);
            }
            KeyCode::PageDown => {
                self.follow_mode = false;
                self.scroll_offset = self.scroll_offset.saturating_add(20);
            }
            KeyCode::Char('G') | KeyCode::End => {
                self.follow_mode = true;
                self.scroll_offset = u16::MAX;
                self.status_msg = "Following latest messages.".to_string();
            }
            KeyCode::Char('g') | KeyCode::Home => {
                self.follow_mode = false;
                self.scroll_offset = 0;
            }
            _ => {}
        }
        Ok(())
    }

    async fn handle_edit_key(
        &mut self,
        key: KeyEvent,
        cmd_tx: &mpsc::Sender<PlaygroundCommand>,
    ) -> Result<()> {
        match key.code {
            KeyCode::Enter => {
                let input = self.input.trim().to_string();
                self.input.clear();
                self.input_mode = InputMode::Normal;
                if !input.is_empty() {
                    self.dispatch_command(&input, cmd_tx).await?;
                }
            }
            KeyCode::Esc => {
                self.input.clear();
                self.input_mode = InputMode::Normal;
                self.status_msg = "Cancelled.".to_string();
            }
            KeyCode::Backspace => {
                self.input.pop();
            }
            KeyCode::Char(c) => {
                self.input.push(c);
            }
            _ => {}
        }
        Ok(())
    }

    async fn dispatch_command(
        &mut self,
        raw: &str,
        cmd_tx: &mpsc::Sender<PlaygroundCommand>,
    ) -> Result<()> {
        let parts: Vec<&str> = raw.splitn(2, ' ').collect();
        let verb = parts[0];
        let rest = parts.get(1).copied().unwrap_or("").trim();

        match verb {
            "/topic" | "/t" => {
                if rest.is_empty() {
                    self.status_msg = format!("Current topic: {}", self.topic);
                } else {
                    self.topic = rest.to_string();
                    let _ = cmd_tx.send(PlaygroundCommand::SetTopic(rest.to_string())).await;
                }
            }
            "/start" => {
                let _ = cmd_tx.send(PlaygroundCommand::StartConversation).await;
            }
            "/pause" | "/stop" => {
                let _ = cmd_tx.send(PlaygroundCommand::PauseConversation).await;
            }
            "/step" | "/next" | "/n" => {
                let _ = cmd_tx.send(PlaygroundCommand::StepOnce).await;
            }
            "/clear" => {
                let _ = cmd_tx.send(PlaygroundCommand::ClearHistory).await;
                self.messages.clear();
                self.round = 0;
                self.total_tokens = 0;
                self.status_msg = "History cleared.".to_string();
            }
            "/models" => {
                let _ = cmd_tx.send(PlaygroundCommand::ListModels).await;
            }
            "/delay" => {
                if let Ok(ms) = rest.parse::<u64>() {
                    self.turn_delay_ms = ms;
                    let _ = cmd_tx.send(PlaygroundCommand::SetTurnDelay(ms)).await;
                    self.status_msg = format!("Turn delay set to {}ms", ms);
                } else {
                    self.status_msg = "Usage: /delay <ms>".to_string();
                }
            }
            "/add" => {
                self.parse_add_agent(rest, cmd_tx).await;
            }
            "/remove" | "/rm" => {
                if rest.is_empty() {
                    self.status_msg = "Usage: /remove <agent-name>".to_string();
                } else {
                    self.agents.retain(|a| a.name != rest);
                    self.agent_statuses.remove(rest);
                    let _ = cmd_tx.send(PlaygroundCommand::RemoveAgent(rest.to_string())).await;
                }
            }
            "/agents" => {
                let names: Vec<String> = self.agents.iter().map(|a| format!("{} ({})", a.name, a.model_label())).collect();
                self.status_msg = if names.is_empty() {
                    "No agents.".to_string()
                } else {
                    names.join("  |  ")
                };
            }
            "/help" => {
                self.help_visible = true;
            }
            _ => {
                self.status_msg = format!("Unknown: {}  (try /help)", verb);
            }
        }
        Ok(())
    }

    async fn parse_add_agent(&mut self, rest: &str, cmd_tx: &mpsc::Sender<PlaygroundCommand>) {
        // /add <name> ollama <model> [<url>]
        // /add <name> groq <model> <api_key>
        // /add <name> openrouter <model> <api_key>
        // /add <name> cerebras <model> <api_key>
        // /add <name> lmstudio <model> [<url>]
        let parts: Vec<&str> = rest.split_whitespace().collect();
        if parts.len() < 3 {
            self.status_msg = "Usage: /add <name> <backend> <model> [url|key]".to_string();
            return;
        }
        let name = parts[0];
        let backend_str = parts[1].to_lowercase();
        let model = parts[2];
        let extra = parts.get(3).copied().unwrap_or("");

        let backend = match backend_str.as_str() {
            "ollama" => BackendConfig::Ollama {
                base_url: if extra.is_empty() { "http://localhost:11434".to_string() } else { extra.to_string() },
                model: model.to_string(),
            },
            "groq" => BackendConfig::Groq { api_key: extra.to_string(), model: model.to_string() },
            "openrouter" | "or" => BackendConfig::OpenRouter { api_key: extra.to_string(), model: model.to_string() },
            "lmstudio" | "lms" => BackendConfig::LmStudio {
                base_url: if extra.is_empty() { "http://localhost:1234/v1".to_string() } else { extra.to_string() },
                model: model.to_string(),
            },
            "cerebras" => BackendConfig::Cerebras { api_key: extra.to_string(), model: model.to_string() },
            _ => {
                self.status_msg = format!("Unknown backend '{}'. Use: ollama groq openrouter lmstudio cerebras", backend_str);
                return;
            }
        };

        let default_persona = format!(
            "You are {name}, a sharp and opinionated AI agent. You speak with confidence and engage critically with other agents' arguments.",
            name = name
        );

        let cfg = AgentConfig {
            name: name.to_string(),
            backend,
            persona: default_persona,
            max_tokens: 350,
        };

        self.agent_statuses.insert(name.to_string(), AgentStatus::new(name));
        self.agents.push(cfg.clone());
        let _ = cmd_tx.send(PlaygroundCommand::AddAgent(cfg)).await;
        self.status_msg = format!("Agent '{}' added.", name);
    }
}
