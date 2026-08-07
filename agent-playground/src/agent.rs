use crate::llm::{ollama::OllamaClient, openai_compat::OpenAICompatClient, LlmClient};
use anyhow::Result;
use ratatui::style::Color;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const AGENT_COLORS: &[Color] = &[
    Color::Cyan,
    Color::Yellow,
    Color::Green,
    Color::Magenta,
    Color::LightBlue,
    Color::LightRed,
    Color::LightGreen,
    Color::LightYellow,
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BackendConfig {
    Ollama {
        #[serde(default = "default_ollama_url")]
        base_url: String,
        model: String,
    },
    Groq {
        api_key: String,
        model: String,
    },
    OpenRouter {
        api_key: String,
        model: String,
    },
    LmStudio {
        #[serde(default = "default_lmstudio_url")]
        base_url: String,
        model: String,
    },
    Cerebras {
        api_key: String,
        model: String,
    },
}

fn default_ollama_url() -> String {
    "http://localhost:11434".to_string()
}

fn default_lmstudio_url() -> String {
    "http://localhost:1234/v1".to_string()
}

impl BackendConfig {
    pub fn label(&self) -> String {
        match self {
            Self::Ollama { model, .. } => format!("ollama/{}", model),
            Self::Groq { model, .. } => format!("groq/{}", model),
            Self::OpenRouter { model, .. } => format!("openrouter/{}", model),
            Self::LmStudio { model, .. } => format!("lmstudio/{}", model),
            Self::Cerebras { model, .. } => format!("cerebras/{}", model),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub name: String,
    pub backend: BackendConfig,
    pub persona: String,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
}

fn default_max_tokens() -> u32 {
    350
}

impl AgentConfig {
    pub fn model_label(&self) -> String {
        self.backend.label()
    }
}

#[derive(Debug, Clone)]
pub struct AgentStatus {
    pub name: String,
    pub is_thinking: bool,
    pub messages_sent: u32,
    pub total_tokens: u32,
    pub last_latency_ms: u64,
    pub is_online: bool,
    pub last_error: Option<String>,
}

impl AgentStatus {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            is_thinking: false,
            messages_sent: 0,
            total_tokens: 0,
            last_latency_ms: 0,
            is_online: true,
            last_error: None,
        }
    }
}

pub fn build_client(config: &AgentConfig) -> Result<Arc<dyn LlmClient>> {
    let client: Arc<dyn LlmClient> = match &config.backend {
        BackendConfig::Ollama { base_url, model } => {
            Arc::new(OllamaClient::new(base_url, model))
        }
        BackendConfig::Groq { api_key, model } => {
            Arc::new(OpenAICompatClient::groq(api_key, model)?)
        }
        BackendConfig::OpenRouter { api_key, model } => {
            Arc::new(OpenAICompatClient::openrouter(api_key, model)?)
        }
        BackendConfig::LmStudio { base_url, model } => {
            Arc::new(OpenAICompatClient::lm_studio(base_url, model)?)
        }
        BackendConfig::Cerebras { api_key, model } => {
            Arc::new(OpenAICompatClient::cerebras(api_key, model)?)
        }
    };
    Ok(client)
}
