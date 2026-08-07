use thiserror::Error;

#[derive(Debug, Error)]
pub enum ScematicaError {
    #[error("RPC error: {0}")]
    Rpc(#[from] solana_client::client_error::ClientError),

    #[error("Transaction failed: {0}")]
    Transaction(String),

    #[error("Pool not found: {0}")]
    PoolNotFound(String),

    #[error("Insufficient balance: have {have}, need {need}")]
    InsufficientBalance { have: u64, need: u64 },

    #[error("Slippage exceeded: expected {expected}, got {got}")]
    SlippageExceeded { expected: u64, got: u64 },

    #[error("No arbitrage opportunity found")]
    NoArbitrageOpportunity,

    #[error("Filter rejected pool: {reason}")]
    FilterRejected { reason: String },

    #[error("Config error: {0}")]
    Config(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("WebSocket error: {0}")]
    WebSocket(String),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Unknown error: {0}")]
    Unknown(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, ScematicaError>;
