use serde::{Deserialize, Serialize};

pub const X402_VERSION: u32 = 2;

/// Returned by the resource server as the body of a 402 Payment Required response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentRequired {
    pub x402_version: u32,
    pub resource: ResourceInfo,
    pub accepts: Vec<PaymentRequirements>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceInfo {
    pub url: String,
    pub method: String,
    pub description: String,
}

/// One accepted payment option served in the 402 response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentRequirements {
    /// "exact" (only supported scheme for now)
    pub scheme: String,
    /// "solana-mainnet" | "solana-devnet"
    pub network: String,
    /// SPL token mint address (use WSOL mint for native SOL payments)
    pub asset: String,
    /// Required transfer amount in raw atomic units (lamports for SOL, micro-USDC for USDC)
    pub amount: u64,
    /// Recipient wallet address
    pub pay_to: String,
    /// Seconds after which the payment authorization expires
    pub max_timeout_seconds: u64,
    /// Optional scheme-specific extras (decimals, memo requirements, etc.)
    #[serde(default)]
    pub extra: serde_json::Value,
}

/// Sent by the client in the `X-Payment` header (base64-encoded JSON).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentPayload {
    pub x402_version: u32,
    /// Must match `PaymentRequirements.scheme`
    pub scheme: String,
    /// Must match `PaymentRequirements.network`
    pub network: String,
    /// Scheme-specific payload
    pub payload: SvmExactPayload,
}

/// Payload for the Solana exact scheme.
/// The client creates a partially-signed SPL Token TransferChecked transaction,
/// serialises it with bincode, and base64-encodes it here.
/// The fee payer slot is left empty — the facilitator fills it at settlement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SvmExactPayload {
    /// bincode + base64 encoded `solana_sdk::transaction::Transaction`
    pub transaction: String,
}

/// Returned by the facilitator `POST /verify` endpoint and the inline verifier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyResponse {
    pub is_valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invalid_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payer: Option<String>,
}

/// Returned by the facilitator `POST /settle` endpoint
/// and attached to the `X-Payment-Response` header as base64-encoded JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettlementResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transaction: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payer: Option<String>,
    pub network: String,
}
