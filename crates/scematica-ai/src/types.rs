use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

/// Which AI provider to use
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AiProvider {
    /// Groq — free tier, 14,400 req/day, ultra-fast LPU inference
    Groq,
    /// OpenRouter — 50 free req/day, 200+ models
    OpenRouter,
    /// Local Ollama instance — no rate limits, requires local setup
    Ollama,
}

impl Default for AiProvider {
    fn default() -> Self {
        AiProvider::Groq
    }
}

impl AiProvider {
    pub fn base_url(&self) -> &str {
        match self {
            AiProvider::Groq => "https://api.groq.com/openai/v1",
            AiProvider::OpenRouter => "https://openrouter.ai/api/v1",
            AiProvider::Ollama => "http://localhost:11434/v1",
        }
    }

    pub fn default_model(&self) -> &str {
        match self {
            AiProvider::Groq => "llama-3.3-70b-versatile",
            AiProvider::OpenRouter => "meta-llama/llama-3.3-70b-instruct:free",
            AiProvider::Ollama => "llama3.3",
        }
    }
}

/// A chat message for the AI API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,   // "system" | "user" | "assistant"
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self { role: "system".into(), content: content.into() }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self { role: "user".into(), content: content.into() }
    }
}

/// Request to the AI API
#[derive(Debug, Clone, Serialize)]
pub struct AiRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub temperature: f32,
    pub max_tokens: u32,
    /// Request JSON output
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<ResponseFormat>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResponseFormat {
    #[serde(rename = "type")]
    pub format_type: String, // "json_object"
}

impl AiRequest {
    pub fn new(model: impl Into<String>, messages: Vec<ChatMessage>) -> Self {
        Self {
            model: model.into(),
            messages,
            temperature: 0.1, // low temp for consistent structured output
            max_tokens: 512,
            response_format: None,
        }
    }

    pub fn with_json_output(mut self) -> Self {
        self.response_format = Some(ResponseFormat { format_type: "json_object".into() });
        self
    }

    pub fn with_temperature(mut self, temp: f32) -> Self {
        self.temperature = temp;
        self
    }

    pub fn with_max_tokens(mut self, tokens: u32) -> Self {
        self.max_tokens = tokens;
        self
    }
}

/// Raw response from the AI API
#[derive(Debug, Clone, Deserialize)]
pub struct AiResponse {
    pub choices: Vec<AiChoice>,
    pub usage: Option<AiUsage>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AiChoice {
    pub message: AiMessage,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AiMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AiUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

impl AiResponse {
    pub fn content(&self) -> &str {
        self.choices
            .first()
            .map(|c| c.message.content.as_str())
            .unwrap_or("")
    }
}

// ─── Agent Output Types ───────────────────────────────────────────────────────

/// Risk assessment for a new token/pool (used by sniper)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenRiskScore {
    /// 0 = certain rug, 100 = very safe
    pub score: u8,
    /// "buy" | "skip" | "watch"
    pub recommendation: String,
    /// Human-readable reasoning
    pub reasoning: String,
    /// Specific red flags detected
    pub red_flags: Vec<String>,
    /// Timestamp of assessment
    pub timestamp: DateTime<Utc>,
}

impl TokenRiskScore {
    pub fn should_buy(&self) -> bool {
        self.recommendation == "buy" && self.score >= 60
    }

    pub fn is_high_risk(&self) -> bool {
        self.score < 40
    }
}

/// Arb path quality score (used by arb engine)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArbScore {
    /// 0–100 confidence this arb is real and executable
    pub confidence: u8,
    /// Estimated real profit after gas/slippage (in lamports)
    pub estimated_net_profit: i64,
    /// "execute" | "skip" | "monitor"
    pub recommendation: String,
    pub reasoning: String,
}

impl ArbScore {
    pub fn should_execute(&self) -> bool {
        self.recommendation == "execute" && self.confidence >= 70
    }
}

/// Dynamic strategy adjustment from the AI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrategyAdjustment {
    /// Suggested take-profit % (None = keep current)
    pub take_profit_pct: Option<f64>,
    /// Suggested stop-loss % (None = keep current)
    pub stop_loss_pct: Option<f64>,
    /// Suggested buy amount multiplier (1.0 = no change)
    pub amount_multiplier: f64,
    /// "aggressive" | "conservative" | "neutral"
    pub market_regime: String,
    pub reasoning: String,
}

impl Default for StrategyAdjustment {
    fn default() -> Self {
        Self {
            take_profit_pct: None,
            stop_loss_pct: None,
            amount_multiplier: 1.0,
            market_regime: "neutral".into(),
            reasoning: "No adjustment".into(),
        }
    }
}

/// Periodic market report generated by the AI
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketReport {
    pub summary: String,
    pub pnl_commentary: String,
    pub market_conditions: String,
    pub recommendations: Vec<String>,
    pub alerts: Vec<String>,
    pub timestamp: DateTime<Utc>,
}

/// Result of a debate between two AI personas
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebateResult {
    pub bull_opinion: DebateOpinion,
    pub bear_opinion: DebateOpinion,
    pub consensus_score: u8, // 0-100
    pub final_recommendation: String, // "execute" | "skip"
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebateOpinion {
    pub stance: String,
    pub reasoning: String,
    pub confidence: u8,
}
