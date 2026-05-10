use anyhow::Result;
use solana_client::{
    nonblocking::rpc_client::RpcClient,
    rpc_config::RpcSendTransactionConfig,
};
use solana_sdk::{
    commitment_config::CommitmentConfig,
    pubkey::Pubkey,
    signature::Signature,
    transaction::VersionedTransaction,
};
use std::sync::Arc;
use tracing::{debug, warn};

/// Thin wrapper around RpcClient with retry logic and helpers
#[derive(Clone)]
pub struct RpcConnection {
    pub client: Arc<RpcClient>,
    pub commitment: CommitmentConfig,
}

impl RpcConnection {
    pub fn new(endpoint: &str, commitment: CommitmentConfig) -> Self {
        let client = Arc::new(RpcClient::new_with_commitment(
            endpoint.to_string(),
            commitment,
        ));
        Self { client, commitment }
    }

    /// Send a versioned transaction with skip_preflight
    pub async fn send_transaction(
        &self,
        tx: &VersionedTransaction,
        skip_preflight: bool,
    ) -> Result<Signature> {
        let config = RpcSendTransactionConfig {
            skip_preflight,
            preflight_commitment: Some(self.commitment.commitment),
            ..Default::default()
        };
        let sig = self.client.send_transaction_with_config(tx, config).await?;
        Ok(sig)
    }

    /// Confirm a transaction with retries
    pub async fn confirm_transaction(
        &self,
        sig: &Signature,
        max_retries: u32,
    ) -> Result<bool> {
        for attempt in 0..max_retries {
            match self.client.confirm_transaction(sig).await {
                Ok(confirmed) => {
                    if confirmed {
                        debug!("Transaction {} confirmed on attempt {}", sig, attempt + 1);
                        return Ok(true);
                    }
                }
                Err(e) => {
                    warn!("Confirm attempt {} failed: {}", attempt + 1, e);
                }
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }
        Ok(false)
    }

    /// Get SOL balance for a pubkey
    pub async fn get_sol_balance(&self, pubkey: &Pubkey) -> Result<u64> {
        Ok(self.client.get_balance(pubkey).await?)
    }

    /// Get token account balance (raw u64)
    pub async fn get_token_balance(&self, token_account: &Pubkey) -> Result<u64> {
        let balance = self.client.get_token_account_balance(token_account).await?;
        Ok(balance.amount.parse::<u64>().unwrap_or(0))
    }
}

impl std::fmt::Debug for RpcConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RpcConnection(commitment={:?})", self.commitment)
    }
}
