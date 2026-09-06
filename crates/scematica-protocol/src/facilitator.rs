use anyhow::Result;
use base64::Engine;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig, hash::Hash, pubkey::Pubkey, signature::Keypair,
    signer::Signer, transaction::Transaction,
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

    /// The pubkey clients must name as fee payer when building a payment.
    pub fn fee_payer(&self) -> Pubkey {
        self.fee_payer.pubkey()
    }

    /// A blockhash for a client to build against, with the fee payer to name.
    ///
    /// Served in the `extra` block of a 402 so the payer can sign the message that will
    /// actually be submitted — see `submit` for why the facilitator can no longer supply
    /// one after the fact.
    pub async fn payment_context(&self) -> Result<(Pubkey, Hash)> {
        let (blockhash, _) = self
            .rpc
            .get_latest_blockhash_with_commitment(CommitmentConfig::confirmed())
            .await?;
        Ok((self.fee_payer.pubkey(), blockhash))
    }

    /// Countersign and submit the transaction **exactly as the payer signed it**.
    ///
    /// ## This used to refresh the blockhash, which destroyed the payer's signature
    ///
    /// A Solana signature commits to the serialized message, and the blockhash is part of
    /// that message. Overwriting `recent_blockhash` after the payer had signed left a
    /// signature over a message that no longer existed, so the network rejected every
    /// such transaction: the bundled happy path could not collect a payment at all.
    /// Nothing noticed, because `verify` was not checking the signature either (X-01) and
    /// the middleware had already served the resource and discarded the settlement error
    /// (X-02) — three defects that each hid the next.
    ///
    /// A second defect sat in the same three lines. `Message::new(&ixs, None)` makes the
    /// *payer* the fee payer, so `self.fee_payer` was not among the message's signer
    /// accounts, and `Transaction::partial_sign` panics on that rather than returning an
    /// error — inside the middleware's detached `tokio::spawn` that panic was swallowed,
    /// which is why the symptom was "payments silently never arrive".
    ///
    /// So the blockhash now comes from the server, in the 402, and travels one way. The
    /// cost is that a payload expires with its blockhash, which is the correct behaviour
    /// for a payment authorization and is what `max_timeout_seconds` was always
    /// describing.
    async fn submit(&self, tx_b64: &str) -> Result<String> {
        let tx_bytes = base64::engine::general_purpose::STANDARD.decode(tx_b64)?;
        let mut tx: Transaction = bincode::deserialize(&tx_bytes)?;

        if tx.message.recent_blockhash == Hash::default() {
            anyhow::bail!(
                "Payment carries no recent blockhash; the client must build against the one served in the 402 `extra.recentBlockhash`"
            );
        }

        let fee_payer = self.fee_payer.pubkey();
        let slot = tx
            .message
            .account_keys
            .iter()
            .position(|k| *k == fee_payer)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Payment does not name {} as an account; the client must build against the fee payer served in the 402 `extra.feePayer`",
                    fee_payer
                )
            })?;
        if slot >= tx.message.header.num_required_signatures as usize {
            anyhow::bail!("Fee payer {fee_payer} is present but not in a signing slot");
        }

        // Sign over the message as it stands. `partial_sign` would panic if the key had
        // no slot, which the check above has already ruled out with an error instead.
        tx.partial_sign(&[self.fee_payer.as_ref()], tx.message.recent_blockhash);

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
            extra: serde_json::json!({
                "feePayer": "11111111111111111111111111111112",
                "recentBlockhash": "11111111111111111111111111111112",
            }),
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
