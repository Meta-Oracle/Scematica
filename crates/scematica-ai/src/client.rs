use crate::types::{AiProvider, AiRequest, AiResponse};
use anyhow::{Context, Result};
use std::time::Duration;
use tracing::debug;

/// HTTP client for OpenAI-compatible AI APIs (Groq, OpenRouter, Ollama)
#[derive(Clone)]
pub struct AiClient {
    pub provider: AiProvider,
    pub model: String,
    http: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl AiClient {
    /// Create a new client. API key is read from environment:
    /// - Groq: GROQ_API_KEY
    /// - OpenRouter: OPENROUTER_API_KEY
    /// - Ollama: no key needed
    pub fn new(provider: AiProvider) -> Result<Self> {
        dotenv::dotenv().ok();

        let api_key = match &provider {
            AiProvider::Groq => std::env::var("GROQ_API_KEY")
                .context("GROQ_API_KEY not set. Get a free key at https://console.groq.com")?,
            AiProvider::Grok => std::env::var("XAI_API_KEY")
                .context("XAI_API_KEY not set. Get a key at https://console.x.ai")?,
            AiProvider::OpenRouter => std::env::var("OPENROUTER_API_KEY")
                .context("OPENROUTER_API_KEY not set. Get a free key at https://openrouter.ai")?,
            AiProvider::Ollama => String::new(),
        };

        let model = std::env::var("AI_MODEL")
            .unwrap_or_else(|_| provider.default_model().to_string());

        let base_url = std::env::var("AI_BASE_URL")
            .unwrap_or_else(|_| provider.base_url().to_string());

        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()?;

        Ok(Self { provider, model, http, api_key, base_url })
    }

    /// Create from environment — auto-detects provider based on which key is set
    pub fn from_env() -> Result<Self> {
        dotenv::dotenv().ok();

        // Try Grok first
        if std::env::var("XAI_API_KEY").is_ok() {
            return Self::new(AiProvider::Grok);
        }
        // Try Groq
        if std::env::var("GROQ_API_KEY").is_ok() {
            return Self::new(AiProvider::Groq);
        }
        // Fall back to OpenRouter
        if std::env::var("OPENROUTER_API_KEY").is_ok() {
            return Self::new(AiProvider::OpenRouter);
        }
        // Fall back to local Ollama
        if std::env::var("OLLAMA_HOST").is_ok() || Self::ollama_available() {
            return Self::new(AiProvider::Ollama);
        }

        anyhow::bail!(
            "No AI API key found. Set XAI_API_KEY (https://console.x.ai), \
             GROQ_API_KEY (https://console.groq.com), or OPENROUTER_API_KEY (https://openrouter.ai)"
        )
    }

    /// Send a chat completion request
    pub async fn chat(&self, request: AiRequest) -> Result<AiResponse> {
        let url = format!("{}/chat/completions", self.base_url);

        debug!(
            provider = ?self.provider,
            model = %request.model,
            messages = request.messages.len(),
            "Sending AI request"
        );

        let mut req = self.http.post(&url).json(&request);

        if !self.api_key.is_empty() {
            req = req.bearer_auth(&self.api_key);
        }

        // OpenRouter requires these headers
        if self.provider == AiProvider::OpenRouter {
            req = req
                .header("HTTP-Referer", "https://github.com/scematica")
                .header("X-Title", "Scematica Trading Bot");
        }

        let resp = req.send().await.context("AI API request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("AI API error {}: {}", status, body);
        }

        let response: AiResponse = resp.json().await.context("Failed to parse AI response")?;

        if let Some(usage) = &response.usage {
            debug!(
                tokens_used = usage.total_tokens,
                "AI request completed"
            );
        }

        Ok(response)
    }

    /// Convenience: send a simple user message with the default model
    pub async fn ask(&self, system: &str, user: &str) -> Result<String> {
        use crate::types::ChatMessage;

        let request = AiRequest::new(
            &self.model,
            vec![
                ChatMessage::system(system),
                ChatMessage::user(user),
            ],
        );

        let response = self.chat(request).await?;
        Ok(response.content().to_string())
    }

    /// Convenience: ask for JSON output
    pub async fn ask_json(&self, system: &str, user: &str) -> Result<String> {
        use crate::types::ChatMessage;

        let request = AiRequest::new(
            &self.model,
            vec![
                ChatMessage::system(system),
                ChatMessage::user(user),
            ],
        )
        .with_json_output();

        let response = self.chat(request).await?;
        Ok(response.content().to_string())
    }

    /// Check if Ollama is running locally
    fn ollama_available() -> bool {
        // Quick sync check — just see if the port is open
        std::net::TcpStream::connect_timeout(
            &"127.0.0.1:11434".parse().unwrap(),
            Duration::from_millis(200),
        )
        .is_ok()
    }

    pub fn provider_name(&self) -> &str {
        match self.provider {
            AiProvider::Groq => "Groq",
            AiProvider::Grok => "xAI Grok",
            AiProvider::OpenRouter => "OpenRouter",
            AiProvider::Ollama => "Ollama (local)",
        }
    }
}

impl std::fmt::Debug for AiClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "AiClient({}, model={})", self.provider_name(), self.model)
    }
}
