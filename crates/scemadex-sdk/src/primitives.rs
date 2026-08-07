use serde::{Deserialize, Serialize};

use crate::error::{Result, ScemaDexError};

/// A base58-encoded Solana account address.
///
/// The lean core stores it as a validated string so the published crate carries
/// no `solana-sdk` dependency. The `scematica` feature bridges to
/// `solana_sdk::pubkey::Pubkey` at the integration boundary.
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Address(String);

impl Address {
    /// Validate and wrap a base58 32-byte address.
    pub fn new(s: impl Into<String>) -> Result<Self> {
        let s = s.into();
        match bs58::decode(&s).into_vec() {
            Ok(bytes) if bytes.len() == 32 => Ok(Self(s)),
            _ => Err(ScemaDexError::InvalidAddress(s)),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::fmt::Debug for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Address({})", self.0)
    }
}

/// A token amount in base units plus its mint decimals.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Amount {
    pub raw: u64,
    pub decimals: u8,
}

impl Amount {
    pub fn new(raw: u64, decimals: u8) -> Self {
        Self { raw, decimals }
    }

    /// Human-readable value (e.g. `1.5` SOL).
    pub fn ui(&self) -> f64 {
        self.raw as f64 / 10f64.powi(self.decimals as i32)
    }
}

/// A micro-USDC amount (6 decimals) — the unit of account for x402 metering,
/// inference fees, and Conviction Routing bonds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Usdc(pub u64);

impl Usdc {
    pub fn from_usdc(v: f64) -> Self {
        Self((v * 1_000_000.0) as u64)
    }

    pub fn as_usdc(&self) -> f64 {
        self.0 as f64 / 1_000_000.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_base58_address() {
        assert!(Address::new("not-an-address!!").is_err());
    }

    #[test]
    fn accepts_valid_mint() {
        assert!(Address::new("So11111111111111111111111111111111111111112").is_ok());
    }

    #[test]
    fn amount_ui_scales_by_decimals() {
        assert_eq!(Amount::new(1_500_000_000, 9).ui(), 1.5);
    }

    #[test]
    fn usdc_round_trips() {
        assert_eq!(Usdc::from_usdc(2.5).as_usdc(), 2.5);
    }
}
