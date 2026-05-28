use super::{ChatMessage, LlmClient, LlmResponse};
use anyhow::Result;
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Instant;

pub struct OllamaClient {
    client: Client,
    base_url: String,
    model: String,
}

impl OllamaClient {
    pub fn new(base_url: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(180))
                .build()
                .expect("reqwest client"),
            base_url: base_url.into(),
            model: model.into(),
        }
    }
}

#[derive(Serialize)]
struct OllamaChatRequest<'a> {
    model: &'a str,
    messages: &'a [ChatMessage],
    stream: bool,
    options: OllamaOptions,
}

#[derive(Serialize)]
struct OllamaOptions {
    num_predict: u32,
    temperature: f32,
}

#[derive(Deserialize)]
struct OllamaChatResponse {
    message: OllamaMsg,
    eval_count: Option<u32>,
}

#[derive(Deserialize)]
struct OllamaMsg {
    content: String,
}

#[derive(Deserialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaModelEntry>,
}

#[derive(Deserialize)]
struct OllamaModelEntry {
    name: String,
}

#[async_trait]
impl LlmClient for OllamaClient {
    async fn chat(&self, messages: Vec<ChatMessage>, max_tokens: u32) -> Result<LlmResponse> {
        let start = Instant::now();
        let url = format!("{}/api/chat", self.base_url);

        let req = OllamaChatRequest {
            model: &self.model,
            messages: &messages,
            stream: false,
            options: OllamaOptions {
                num_predict: max_tokens,
                temperature: 0.8,
            },
        };

        let resp = self
            .client
            .post(&url)
            .json(&req)
            .send()
            .await?
            .error_for_status()?
            .json::<OllamaChatResponse>()
            .await?;

        Ok(LlmResponse {
            content: resp.message.content.trim().to_string(),
            tokens_used: resp.eval_count,
            latency_ms: start.elapsed().as_millis() as u64,
        })
    }

    async fn list_models(&self) -> Result<Vec<String>> {
        let url = format!("{}/api/tags", self.base_url);
        let resp = self
            .client
            .get(&url)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await?
            .error_for_status()?
            .json::<OllamaTagsResponse>()
            .await?;
        Ok(resp.models.into_iter().map(|m| m.name).collect())
    }

    fn backend_name(&self) -> &str {
        "ollama"
    }

    fn model_name(&self) -> &str {
        &self.model
    }
}
