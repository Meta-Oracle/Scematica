use scematica_core::types::TradeEntry;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ToolCall {
    SwapToken {
        from: String,
        to: String,
        amount_sol: f64,
        slippage_bps: Option<u32>,
    },
    GetQuote {
        from: String,
        to: String,
        amount_sol: f64,
    },
    GetBalance,
    SetBotMode {
        mode: String,
    },
    ScanArb,
    GetTradeHistory {
        n: Option<u32>,
    },
    GetBotStatus,
    X402Search {
        query: String,
        verified_only: Option<bool>,
        min_quality_score: Option<f64>,
        limit: Option<u32>,
    },
    X402Check {
        url: String,
        method: Option<String>,
    },
    X402Fetch {
        url: String,
        method: Option<String>,
    },
    X402Wallet,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RiskLevel {
    Safe,
    Moderate,
    High,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PendingToolCall {
    pub call_id: String,
    pub call: ToolCall,
    pub summary: String,
    pub risk: RiskLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToolResult {
    Success {
        message: String,
        signature: Option<String>,
        data: Option<serde_json::Value>,
    },
    Failure {
        message: String,
        recoverable: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub message: String,
    pub trade_entry: Option<TradeEntry>,
    pub tokens_used: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentOutput {
    NeedsConfirmation(PendingToolCall),
    Reply(ChatResponse),
}

pub fn classify_risk(call: &ToolCall) -> RiskLevel {
    match call {
        ToolCall::SwapToken { .. } => RiskLevel::High,
        ToolCall::X402Fetch { .. } => RiskLevel::High,
        ToolCall::SetBotMode { .. } => RiskLevel::Moderate,
        _ => RiskLevel::Safe,
    }
}
