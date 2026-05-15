use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChatLine {
    User(String),
    Bot(String),
    ToolResult(String),
    Error(String),
    Pending { summary: String, risk: String },
}

/// Commands sent from the event loop to the AI worker task
#[derive(Debug, Clone)]
pub enum ChatUpdate {
    Send(String),
    Confirm,
    Reject,
}
