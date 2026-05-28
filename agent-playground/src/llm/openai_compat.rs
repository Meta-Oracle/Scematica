use super::{ChatMessage, LlmClient, LlmResponse};
use anyhow::Result;
use async_trait::async_trait;
use reqwest::{header, Client};
use serde::{Deserialize, Serialize};
use std::time::Instant;

/// Works with any OpenAI-compatible endpoint: Groq, LM Studio, OpenRouter, Cerebras, etc.
pub struct OpenAICompatClient {
    client: Client,
    base_url: String,
    model: String,
    backend: String,
}

impl OpenAICompatClient {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
        backend_name: impl Into<String>,
    ) -> Result<Self> {
        let api_key = api_key.into();
        let mut headers = header::HeaderMap::new();
        if !api_key.is_empty() {
            let auth = header::HeaderValue::from_str(&format!("Bearer {}", api_key))
                .map_err(|e| anyhow::anyhow!("invalid api key: {}", e))?;
            headers.insert(header::AUTHORIZATION, auth);
        }

        let client = Client::builder()
            .default_headers(headers)
            .timeout(std::time::Duration::from_secs(90))
            .build()?;

        Ok(Self {
            client,
            base_url: base_url.into(),
            model: model.into(),
            backend: backend_name.into(),
        })
    }

    /// Groq free tier: llama3-8b-8192, llama3-70b-8192, mixtral-8x7b-32768
    pub fn groq(api_key: impl Into<String>, model: impl Into<String>) -> Result<Self> {
        Self::new("https://api.groq.com/openai/v1", api_key, model, "groq")
    }

    /// OpenRouter free models: meta-llama/llama-3-8b-instruct:free, etc.
    pub fn openrouter(api_key: impl Into<String>, model: impl Into<String>) -> Result<Self> {
        Self::new("https://openrouter.ai/api/v1", api_key, model, "openrouter")
    }

    /// LM Studio local: base_url typically http://localhost:1234/v1
    pub fn lm_studio(base_url: impl Into<String>, model: impl Into<String>) -> Result<Self> {
        Self::new(base_url, "", model, "lmstudio")
    }

    /// Cerebras free tier
    pub fn cerebras(api_key: impl Into<String>, model: impl Into<String>) -> Result<Self> {
        Self::new("https://api.cerebras.ai/v1", api_key, model, "cerebras")
    }
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: &'a [ChatMessage],
    max_tokens: u32,
    temperature: f32,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Choice {
    message: RespMessage,
}

#[derive(Deserialize)]
struct RespMessage {
    content: String,
}

#[derive(Deserialize)]
struct Usage {
    total_tokens: u32,
}

#[async_trait]
impl LlmClient for OpenAICompatClient {
    async fn chat(&self, messages: Vec<ChatMessage>, max_tokens: u32) -> Result<LlmResponse> {
        let start = Instant::now();
        let url = format!("{}/chat/completions", self.base_url);

        let req = ChatRequest {
            model: &self.model,
            messages: &messages,
            max_tokens,
            temperature: 0.8,
        };

        let resp = self.client.post(&url).json(&req).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("{} API error {}: {}", self.backend, status, body));
        }

        let data = resp.json::<ChatResponse>().await?;
        let content = data
            .choices
            .into_iter()
            .next()
            .map(|c| c.message.content.trim().to_string())
            .unwrap_or_default();

        Ok(LlmResponse {
            content,
            tokens_used: data.usage.map(|u| u.total_tokens),
            latency_ms: start.elapsed().as_millis() as u64,
        })
    }

    async fn list_models(&self) -> Result<Vec<String>> {
        Ok(vec![self.model.clone()])
    }

    fn backend_name(&self) -> &str {
        &self.backend
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}
