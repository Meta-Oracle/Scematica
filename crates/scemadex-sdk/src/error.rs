use thiserror::Error;

/// Errors surfaced across the ScemaDEX SDK.
#[derive(Debug, Error)]
pub enum ScemaDexError {
    #[error("no route found for intent: {0}")]
    NoRoute(String),
    #[error("intent violated constraint: {0}")]
    ConstraintViolation(String),
    #[error("bond settlement failed: {0}")]
    Bond(String),
    #[error("venue execution failed: {0}")]
    Venue(String),
    #[error("signal oracle error: {0}")]
    Oracle(String),
    #[error("peer market error: {0}")]
    Mesh(String),
    #[error("invalid address: {0}")]
    InvalidAddress(String),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, ScemaDexError>;
