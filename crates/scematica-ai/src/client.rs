use crate::types::{
    AiProvider, AiRequest, AiResponse,
    AnthropicBlock, AnthropicContent, AnthropicMessage, AnthropicRequest, AnthropicResponse, AnthropicTool,
};
use anyhow::{Context, Result};
use std::time::Duration;
use tracing::debug;

/// HTTP client supporting both OpenAI-compatible APIs and the Anthropic Messages API
#[derive(Clone)]
pub struct AiClient {
    pub provider: AiProvider,
    pub model: String,
    http: reqwest::Client,
    api_key: String,
    base_url: String,
}

impl AiClient {
    pub fn new(provider: AiProvider) -> Result<Self> {
        dotenv::dotenv().ok();

        let api_key = match &provider {
            AiProvider::Claude => std::env::var("ANTHROPIC_API_KEY")
                .context("ANTHROPIC_API_KEY not set")?,
            AiProvider::Groq => std::env::var("GROQ_API_KEY")
                .context("GROQ_API_KEY not set")?,
            AiProvider::Grok => std::env::var("XAI_API_KEY")
                .context("XAI_API_KEY not set")?,
            AiProvider::OpenRouter => std::env::var("OPENROUTER_API_KEY")
                .context("OPENROUTER_API_KEY not set")?,
            AiProvider::Ollama => String::new(),
            AiProvider::Cerebras => std::env::var("CEREBRAS_API_KEY")
                .context("CEREBRAS_API_KEY not set")?,
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

    /// Auto-detect provider from environment. Priority: Claude > xAI > Groq > OpenRouter > Ollama
    pub fn from_env() -> Result<Self> {
        dotenv::dotenv().ok();

        if std::env::var("ANTHROPIC_API_KEY").is_ok() {
            return Self::new(AiProvider::Claude);
        }
        if std::env::var("XAI_API_KEY").is_ok() {
            return Self::new(AiProvider::Grok);
        }
        if std::env::var("GROQ_API_KEY").is_ok() {
            return Self::new(AiProvider::Groq);
        }
        if std::env::var("OPENROUTER_API_KEY").is_ok() {
            return Self::new(AiProvider::OpenRouter);
        }
        if std::env::var("OLLAMA_HOST").is_ok() || Self::ollama_available() {
            return Self::new(AiProvider::Ollama);
        }

        anyhow::bail!(
            "No AI API key found. Set ANTHROPIC_API_KEY, XAI_API_KEY, GROQ_API_KEY, or OPENROUTER_API_KEY."
        )
    }

    /// Send a chat completion request — routes to Anthropic or OpenAI-compatible path
    pub async fn chat(&self, request: AiRequest) -> Result<AiResponse> {
        if self.provider == AiProvider::Claude {
            self.chat_anthropic(request).await
        } else {
            self.chat_openai(request).await
        }
    }

    /// OpenAI-compatible path (Groq, xAI, OpenRouter, Ollama)
    async fn chat_openai(&self, request: AiRequest) -> Result<AiResponse> {
        let url = format!("{}/chat/completions", self.base_url);

        debug!(
            provider = ?self.provider,
            model = %request.model,
            messages = request.messages.len(),
            "Sending AI request (OpenAI-compat)"
        );

        let mut req = self.http.post(&url).json(&request);

        if !self.api_key.is_empty() {
            req = req.bearer_auth(&self.api_key);
        }

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
            debug!(tokens_used = usage.total_tokens, "AI request completed");
        }

        Ok(response)
    }

    /// Anthropic Messages API path — translates AiRequest/AiResponse to Anthropic format
    async fn chat_anthropic(&self, request: AiRequest) -> Result<AiResponse> {
        let url = format!("{}/messages", self.base_url);

        debug!(
            model = %request.model,
            messages = request.messages.len(),
            "Sending AI request (Anthropic Messages API)"
        );

        // Extract system prompt — Anthropic takes it as a top-level field, not a message
        let mut system: Option<String> = None;
        let mut messages: Vec<AnthropicMessage> = Vec::new();

        for msg in &request.messages {
            match msg.role.as_str() {
                "system" => {
                    system = msg.content.clone();
                }
                "tool" => {
                    // Tool result must be a user message with a tool_result content block
                    if let (Some(content), Some(call_id)) = (&msg.content, &msg.tool_call_id) {
                        messages.push(AnthropicMessage {
                            role: "user".into(),
                            content: AnthropicContent::Blocks(vec![AnthropicBlock {
                                block_type: "tool_result".into(),
                                text: None,
                                id: None,
                                name: None,
                                input: None,
                                tool_use_id: Some(call_id.clone()),
                                content: Some(serde_json::Value::String(content.clone())),
                            }]),
                        });
                    }
                }
                "assistant" => {
                    if let Some(tool_calls) = &msg.tool_calls {
                        // Assistant message with tool use — convert to block format
                        let mut blocks: Vec<AnthropicBlock> = Vec::new();
                        if let Some(text) = &msg.content {
                            if !text.is_empty() {
                                blocks.push(AnthropicBlock {
                                    block_type: "text".into(),
                                    text: Some(text.clone()),
                                    id: None, name: None, input: None,
                                    tool_use_id: None, content: None,
                                });
                            }
                        }
                        for tc in tool_calls {
                            let input: serde_json::Value = serde_json::from_str(&tc.function.arguments)
                                .unwrap_or(serde_json::Value::Object(Default::default()));
                            blocks.push(AnthropicBlock {
                                block_type: "tool_use".into(),
                                text: None,
                                id: Some(tc.id.clone()),
                                name: Some(tc.function.name.clone()),
                                input: Some(input),
                                tool_use_id: None,
                                content: None,
                            });
                        }
                        messages.push(AnthropicMessage {
                            role: "assistant".into(),
                            content: AnthropicContent::Blocks(blocks),
                        });
                    } else {
                        messages.push(AnthropicMessage {
                            role: "assistant".into(),
                            content: AnthropicContent::Text(msg.content.clone().unwrap_or_default()),
                        });
                    }
                }
                _ => {
                    // user message
                    messages.push(AnthropicMessage {
                        role: msg.role.clone(),
                        content: AnthropicContent::Text(msg.content.clone().unwrap_or_default()),
                    });
                }
            }
        }

        // Convert OpenAI tools schema to Anthropic format
        let tools: Option<Vec<AnthropicTool>> = request.tools.as_ref().and_then(|ts| {
            let converted: Vec<AnthropicTool> = ts.iter().filter_map(|t| {
                let func = t.get("function")?;
                let name = func.get("name")?.as_str()?.to_string();
                let description = func.get("description")?.as_str().unwrap_or("").to_string();
                let input_schema = func.get("parameters")?.clone();
                Some(AnthropicTool { name, description, input_schema })
            }).collect();
            if converted.is_empty() { None } else { Some(converted) }
        });

        let anthropic_req = AnthropicRequest {
            model: request.model.clone(),
            max_tokens: request.max_tokens,
            temperature: request.temperature,
            system,
            messages,
            tools,
        };

        let resp = self.http
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&anthropic_req)
            .send()
            .await
            .context("Anthropic API request failed")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Anthropic API error {}: {}", status, body);
        }

        let anthropic_resp: AnthropicResponse = resp.json().await
            .context("Failed to parse Anthropic response")?;

        debug!(
            input_tokens = anthropic_resp.usage.input_tokens,
            output_tokens = anthropic_resp.usage.output_tokens,
            stop_reason = ?anthropic_resp.stop_reason,
            "Anthropic request completed"
        );

        Ok(anthropic_resp.into_ai_response())
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

        // For Claude: instruct JSON in the user prompt; for others: use response_format
        let (sys, usr) = if self.provider == AiProvider::Claude {
            (
                format!("{}\n\nYou MUST respond with valid JSON only. No markdown, no explanation, no code fences — raw JSON object.", system),
                user.to_string(),
            )
        } else {
            (system.to_string(), user.to_string())
        };

        let mut request = AiRequest::new(
            &self.model,
            vec![
                ChatMessage::system(sys),
                ChatMessage::user(usr),
            ],
        );

        if self.provider != AiProvider::Claude {
            request = request.with_json_output();
        }

        let response = self.chat(request).await?;
        Ok(response.content().to_string())
    }

    fn ollama_available() -> bool {
        std::net::TcpStream::connect_timeout(
            &"127.0.0.1:11434".parse().unwrap(),
            Duration::from_millis(200),
        )
        .is_ok()
    }

    /// Override the model used by this client (builder pattern).
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = model.into();
        self
    }

    pub fn provider_name(&self) -> &str {
        match self.provider {
            AiProvider::Claude => "Anthropic Claude",
            AiProvider::Groq => "Groq",
            AiProvider::Grok => "xAI Grok",
            AiProvider::OpenRouter => "OpenRouter",
            AiProvider::Ollama => "Ollama (local)",
            AiProvider::Cerebras => "Cerebras",
        }
    }
}

impl std::fmt::Debug for AiClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "AiClient({}, model={})", self.provider_name(), self.model)
    }
}
