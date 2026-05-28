pub mod ollama;
pub mod openai_compat;

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self { role: "system".to_string(), content: content.into() }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: "user".to_string(), content: content.into() }
    }
}

#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub content: String,
    pub tokens_used: Option<u32>,
    pub latency_ms: u64,
}

#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn chat(&self, messages: Vec<ChatMessage>, max_tokens: u32) -> Result<LlmResponse>;
    async fn list_models(&self) -> Result<Vec<String>>;
    #[allow(dead_code)]
    fn backend_name(&self) -> &str;
    #[allow(dead_code)]
    fn model_name(&self) -> &str;
}
