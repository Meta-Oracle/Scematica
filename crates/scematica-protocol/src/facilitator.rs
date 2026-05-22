use anyhow::Result;
use base64::Engine;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    signature::Keypair,
    transaction::Transaction,
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
    pub fn new(
        fee_payer: Arc<Keypair>,
        rpc: Arc<RpcClient>,
        network: impl Into<String>,
    ) -> Self {
        Self { fee_payer, rpc, network: network.into() }
    }

    /// Verify without touching the chain — pure local validation.
    pub fn verify(&self, payload: &PaymentPayload, requirements: &PaymentRequirements) -> VerifyResponse {
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
        let (recent_blockhash, _) = self.rpc
            .get_latest_blockhash_with_commitment(CommitmentConfig::confirmed())
            .await?;
        tx.message.recent_blockhash = recent_blockhash;

        // Add fee payer signature (client's transfer signature is already present)
        tx.partial_sign(&[self.fee_payer.as_ref()], recent_blockhash);

        let sig = self.rpc.send_and_confirm_transaction(&tx).await?;
        Ok(sig.to_string())
    }
}
