use crate::agent::{AgentConfig, BackendConfig};
use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaygroundConfig {
    #[serde(default = "default_topic")]
    pub topic: String,
    #[serde(default)]
    pub agents: Vec<AgentConfig>,
    #[serde(default = "default_turn_delay")]
    pub turn_delay_ms: u64,
}

fn default_topic() -> String {
    "What is the nature of consciousness?".to_string()
}

fn default_turn_delay() -> u64 {
    800
}

impl Default for PlaygroundConfig {
    fn default() -> Self {
        Self {
            topic: default_topic(),
            agents: vec![
                AgentConfig {
                    name: "Alpha".to_string(),
                    backend: BackendConfig::Ollama {
                        base_url: "http://localhost:11434".to_string(),
                        model: "phi3".to_string(),
                    },
                    persona: "You are a pragmatic empiricist who values evidence, data, and logical rigor. You challenge vague claims and demand precision.".to_string(),
                    max_tokens: 350,
                },
                AgentConfig {
                    name: "Omega".to_string(),
                    backend: BackendConfig::Ollama {
                        base_url: "http://localhost:11434".to_string(),
                        model: "mistral".to_string(),
                    },
                    persona: "You are a philosophical idealist who explores the deeper meaning behind ideas and challenges materialist assumptions. You think in metaphors and first principles.".to_string(),
                    max_tokens: 350,
                },
            ],
            turn_delay_ms: default_turn_delay(),
        }
    }
}

impl PlaygroundConfig {
    pub fn from_file(path: &str) -> Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let ext = std::path::Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let config = match ext {
            "toml" => toml::from_str::<Self>(&content)?,
            _ => serde_json::from_str::<Self>(&content)?,
        };
        Ok(config)
    }

    pub fn save_example(path: &str) -> Result<()> {
        let config = Self::default();
        let json = serde_json::to_string_pretty(&config)?;
        std::fs::write(path, json)?;
        Ok(())
    }
}
