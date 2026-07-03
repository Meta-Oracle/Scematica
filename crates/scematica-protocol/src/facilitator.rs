use anyhow::Result;
use base64::Engine;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig, signature::Keypair, transaction::Transaction,
};
use std::sync::Arc;
use tracing::{info, warn};

use crate::{
    scheme::svm_exact,
    types::{PaymentPayload, PaymentRequirements, SettlementResponse, VerifyResponse},
};

/// Core facilitator: verifies payment payloads and settles them on-chain.
///
/// The fee payer keypair funds the transaction fees so the client doesn't need SOL —
/// they only need the SPL token being transferred.
pub struct Facilitator {
    fee_payer: Arc<Keypair>,
    rpc: Arc<RpcClient>,
    network: String,
}

impl Facilitator {
    pub fn new(fee_payer: Arc<Keypair>, rpc: Arc<RpcClient>, network: impl Into<String>) -> Self {
        Self {
            fee_payer,
            rpc,
            network: network.into(),
        }
    }

    /// Verify without touching the chain — pure local validation.
    pub fn verify(
        &self,
        payload: &PaymentPayload,
        requirements: &PaymentRequirements,
    ) -> VerifyResponse {
        if payload.scheme != "exact" {
            return VerifyResponse {
                is_valid: false,
                invalid_reason: Some(format!("Unsupported scheme: {}", payload.scheme)),
                payer: None,
            };
        }
        if payload.network != requirements.network {
            return VerifyResponse {
                is_valid: false,
                invalid_reason: Some(format!(
                    "Network mismatch: payload={} requirements={}",
                    payload.network, requirements.network
                )),
                payer: None,
            };
        }
        svm_exact::verify(&payload.payload, requirements)
    }

    /// Settle the payment: re-verify, then sign + submit the transaction on-chain.
    pub async fn settle(
        &self,
        payload: &PaymentPayload,
        requirements: &PaymentRequirements,
    ) -> SettlementResponse {
        let verify = self.verify(payload, requirements);
        if !verify.is_valid {
            return SettlementResponse {
                success: false,
                transaction: None,
                error: verify.invalid_reason,
                payer: verify.payer,
                network: self.network.clone(),
            };
        }

        match self.submit(&payload.payload.transaction).await {
            Ok(sig) => {
                info!(sig = %sig, payer = ?verify.payer, "Payment settled");
                SettlementResponse {
                    success: true,
                    transaction: Some(sig),
                    error: None,
                    payer: verify.payer,
                    network: self.network.clone(),
                }
            }
            Err(e) => {
                warn!("Payment settlement failed: {}", e);
                SettlementResponse {
                    success: false,
                    transaction: None,
                    error: Some(e.to_string()),
                    payer: verify.payer,
                    network: self.network.clone(),
                }
            }
        }
    }

    async fn submit(&self, tx_b64: &str) -> Result<String> {
        let tx_bytes = base64::engine::general_purpose::STANDARD.decode(tx_b64)?;
        let mut tx: Transaction = bincode::deserialize(&tx_bytes)?;

        // Refresh blockhash so the transaction lands
        let (recent_blockhash, _) = self
            .rpc
            .get_latest_blockhash_with_commitment(CommitmentConfig::confirmed())
            .await?;
        tx.message.recent_blockhash = recent_blockhash;

        // Add fee payer signature (client's transfer signature is already present)
        tx.partial_sign(&[self.fee_payer.as_ref()], recent_blockhash);

        let sig = self.rpc.send_and_confirm_transaction(&tx).await?;
        Ok(sig.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::build_payment_payload;
    use crate::types::PaymentRequirements;
    use solana_sdk::signer::Signer;

    const USDC: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";

    /// A facilitator whose RPC is never contacted — `verify` is pure local
    /// validation, so `RpcClient::new` (which does not connect) is enough.
    fn offline_facilitator() -> Facilitator {
        Facilitator::new(
            Arc::new(Keypair::new()),
            Arc::new(RpcClient::new("http://127.0.0.1:1".to_string())),
            "solana-mainnet",
        )
    }

    fn reqs(pay_to: &str) -> PaymentRequirements {
        PaymentRequirements {
            scheme: "exact".into(),
            network: "solana-mainnet".into(),
            asset: USDC.into(),
            amount: 1_000,
            pay_to: pay_to.into(),
            max_timeout_seconds: 120,
            extra: serde_json::Value::Null,
        }
    }

    #[test]
    fn rejects_unsupported_scheme_before_touching_chain() {
        let f = offline_facilitator();
        let payer = Keypair::new();
        let pay_to = Keypair::new().pubkey().to_string();
        let mut payload = build_payment_payload(&payer, &reqs(&pay_to), 6).unwrap();
        payload.scheme = "erc3009".into(); // a non-SVM scheme

        let resp = f.verify(&payload, &reqs(&pay_to));
        assert!(!resp.is_valid);
        assert!(resp.invalid_reason.unwrap().contains("scheme"));
    }

    #[test]
    fn rejects_network_mismatch() {
        let f = offline_facilitator();
        let payer = Keypair::new();
        let pay_to = Keypair::new().pubkey().to_string();
        let mut payload = build_payment_payload(&payer, &reqs(&pay_to), 6).unwrap();
        payload.network = "solana-devnet".into(); // requirements demand mainnet

        let resp = f.verify(&payload, &reqs(&pay_to));
        assert!(!resp.is_valid);
        assert!(resp
            .invalid_reason
            .unwrap()
            .to_lowercase()
            .contains("network"));
    }

    #[test]
    fn accepts_a_well_formed_matching_payment() {
        let f = offline_facilitator();
        let payer = Keypair::new();
        let pay_to = Keypair::new().pubkey().to_string();
        let payload = build_payment_payload(&payer, &reqs(&pay_to), 6).unwrap();

        let resp = f.verify(&payload, &reqs(&pay_to));
        assert!(resp.is_valid, "reason: {:?}", resp.invalid_reason);
        assert_eq!(
            resp.payer.as_deref(),
            Some(payer.pubkey().to_string().as_str())
        );
    }
}
