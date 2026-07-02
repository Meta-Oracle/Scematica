//! Bot-side ScemaDEX integrations that cannot ship to crates.io because they
//! depend on the proprietary `scematica-protocol` stack. This crate is
//! `publish = false`; it wires the published [`scemadex_sdk`] traits to the real
//! x402 facilitator.

use std::sync::Arc;

use async_trait::async_trait;
use scematica_protocol::{
    Facilitator, PaymentPayload, PaymentRequirements, SettlementResponse, X402_VERSION,
};

use scemadex_sdk::{
    Bond, BondEngine, BondLedger, BondOutcome, EscrowBondEngine, Fill, Result, Solution, Usdc,
};

pub mod jupiter;
pub mod scar_profile;
pub mod signal;

/// USDC mint on Solana mainnet — the unit of account for bonds and fees.
const USDC_MINT: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";

/// A Conviction-Routing bond engine that expresses bonds as **x402 payment
/// requirements** over the Scematica protocol.
///
/// The honored/slashed state machine is delegated to an inner
/// [`EscrowBondEngine`]; this type adds the x402 wire representation
/// ([`PaymentRequirements`]) so a `scematica-protocol` facilitator can settle the
/// bond on-chain at the application boundary.
pub struct X402BondEngine {
    inner: EscrowBondEngine,
    pay_to: String,
    asset_mint: String,
    network: String,
    timeout_secs: u64,
    facilitator: Option<Arc<Facilitator>>,
}

impl X402BondEngine {
    /// Create an engine that settles bonds to `pay_to` on `solana-mainnet` in USDC.
    pub fn new(inner: EscrowBondEngine, pay_to: impl Into<String>) -> Self {
        Self {
            inner,
            pay_to: pay_to.into(),
            asset_mint: USDC_MINT.to_string(),
            network: "solana-mainnet".to_string(),
            timeout_secs: 120,
            facilitator: None,
        }
    }

    pub fn with_network(mut self, network: impl Into<String>) -> Self {
        self.network = network.into();
        self
    }

    /// Attach a live x402 [`Facilitator`] so bonds/fees settle on-chain.
    pub fn with_facilitator(mut self, facilitator: Arc<Facilitator>) -> Self {
        self.facilitator = Some(facilitator);
        self
    }

    /// Settle a client-signed x402 payment for `amount` micro-USDC on-chain.
    ///
    /// Verifies and submits the payment transaction through the configured
    /// [`Facilitator`] (the fee payer funds the network fee). Use this to move
    /// the inference fee or a slashed bond between agent and caller.
    pub async fn settle_payment(
        &self,
        payload: &PaymentPayload,
        amount: Usdc,
    ) -> anyhow::Result<SettlementResponse> {
        let facilitator = self
            .facilitator
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no facilitator configured"))?;
        let requirements = self.payment_requirements(amount);
        Ok(facilitator.settle(payload, &requirements).await)
    }

    /// Build the x402 [`PaymentRequirements`] that settle a bond/fee of `amount`
    /// micro-USDC. Hand this to a facilitator's `/settle` flow.
    pub fn payment_requirements(&self, amount: Usdc) -> PaymentRequirements {
        PaymentRequirements {
            scheme: "exact".to_string(),
            network: self.network.clone(),
            asset: self.asset_mint.clone(),
            amount: amount.0,
            pay_to: self.pay_to.clone(),
            max_timeout_seconds: self.timeout_secs,
            extra: serde_json::json!({
                "x402_version": X402_VERSION,
                "purpose": "scemadex-conviction-bond",
            }),
        }
    }

    pub fn ledger(&self) -> BondLedger {
        self.inner.ledger()
    }

    pub fn open_bonds(&self) -> usize {
        self.inner.open_bonds()
    }
}

#[async_trait]
impl BondEngine for X402BondEngine {
    async fn escrow(&self, solution: &Solution) -> Result<Bond> {
        self.inner.escrow(solution).await
    }

    async fn settle(&self, bond: &Bond, fill: &Fill) -> Result<BondOutcome> {
        self.inner.settle(bond, fill).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scemadex_sdk::BondConfig;

    #[test]
    fn builds_x402_payment_requirements() {
        let engine = X402BondEngine::new(
            EscrowBondEngine::new(BondConfig::default()),
            "PayToWa11et1111111111111111111111111111111",
        );
        let req = engine.payment_requirements(Usdc::from_usdc(1.0));
        assert_eq!(req.scheme, "exact");
        assert_eq!(req.amount, 1_000_000);
        assert_eq!(req.asset, USDC_MINT);
        assert_eq!(req.network, "solana-mainnet");
    }
}
