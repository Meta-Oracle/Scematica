use anyhow::Result;
use async_trait::async_trait;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    compute_budget::ComputeBudgetInstruction,
    instruction::Instruction,
    signature::{Keypair, Signer},
    transaction::Transaction,
    message::Message,
};
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Result of a transaction execution attempt
#[derive(Debug, Clone)]
pub struct ExecResult {
    pub signature: Option<String>,
    pub confirmed: bool,
    pub error: Option<String>,
}

/// Trait for transaction executors
#[async_trait]
pub trait TxExecutor: Send + Sync {
    async fn execute(
        &self,
        instructions: Vec<Instruction>,
        wallet: &Keypair,
        rpc: &Arc<RpcClient>,
    ) -> Result<ExecResult>;
}

/// Default executor: sends via standard RPC with compute budget
pub struct DefaultExecutor {
    pub compute_unit_limit: u32,
    pub compute_unit_price: u64,
    pub skip_preflight: bool,
    pub max_retries: u32,
}

impl DefaultExecutor {
    pub fn new(
        compute_unit_limit: u32,
        compute_unit_price: u64,
        skip_preflight: bool,
        max_retries: u32,
    ) -> Self {
        Self {
            compute_unit_limit,
            compute_unit_price,
            skip_preflight,
            max_retries,
        }
    }
}

#[async_trait]
impl TxExecutor for DefaultExecutor {
    async fn execute(
        &self,
        instructions: Vec<Instruction>,
        wallet: &Keypair,
        rpc: &Arc<RpcClient>,
    ) -> Result<ExecResult> {
        let mut all_ixs = vec![
            ComputeBudgetInstruction::set_compute_unit_limit(self.compute_unit_limit),
            ComputeBudgetInstruction::set_compute_unit_price(self.compute_unit_price),
        ];
        all_ixs.extend(instructions);

        let blockhash = rpc.get_latest_blockhash().await?;
        // Build a legacy transaction (compatible with all RPC nodes)
        let msg = Message::new_with_blockhash(&all_ixs, Some(&wallet.pubkey()), &blockhash);
        let mut tx = Transaction::new_unsigned(msg);
        tx.sign(&[wallet], blockhash);

        for attempt in 0..self.max_retries {
            debug!("Sending transaction attempt {}/{}", attempt + 1, self.max_retries);
            match rpc
                .send_and_confirm_transaction_with_spinner_and_config(
                    &tx,
                    CommitmentConfig::confirmed(),
                    solana_client::rpc_config::RpcSendTransactionConfig {
                        skip_preflight: self.skip_preflight,
                        ..Default::default()
                    },
                )
                .await
            {
                Ok(sig) => {
                    info!("Transaction confirmed: {}", sig);
                    return Ok(ExecResult {
                        signature: Some(sig.to_string()),
                        confirmed: true,
                        error: None,
                    });
                }
                Err(e) => {
                    let msg = e.to_string();
                    warn!("Transaction attempt {} failed: {}", attempt + 1, msg);
                    if attempt + 1 < self.max_retries {
                        // Exponential back-off: 429 → 8 s, 16 s, 32 s; others → 500 ms
                        // Helius free plan rate window is ~10 s — start above it
                        let delay_ms: u64 = if msg.contains("429") {
                            8000u64 << attempt.min(2)
                        } else {
                            500
                        };
                        tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
                    } else {
                        return Ok(ExecResult {
                            signature: None,
                            confirmed: false,
                            error: Some(msg),
                        });
                    }
                }
            }
        }

        Ok(ExecResult {
            signature: None,
            confirmed: false,
            error: Some("Max retries exceeded".into()),
        })
    }
}

/// Jito executor: bundles transactions through Jito block engine for MEV protection
pub struct JitoExecutor {
    pub jito_url: String,
    pub tip_lamports: u64,
    pub http_client: reqwest::Client,
}

impl JitoExecutor {
    pub fn new(jito_url: impl Into<String>, tip_sol: f64) -> Self {
        Self {
            jito_url: jito_url.into(),
            tip_lamports: (tip_sol * 1e9) as u64,
            http_client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl TxExecutor for JitoExecutor {
    async fn execute(
        &self,
        instructions: Vec<Instruction>,
        wallet: &Keypair,
        rpc: &Arc<RpcClient>,
    ) -> Result<ExecResult> {
        use solana_sdk::system_instruction;

        // Jito tip account (one of the 8 official tip accounts)
        let jito_tip_account: solana_sdk::pubkey::Pubkey =
            "96gYZGLnJYVFmbjzopPSU6QiEV5fGqZNyN9nmNhvrZU5"
                .parse()
                .unwrap();

        let mut all_ixs = instructions;
        // Add tip instruction
        all_ixs.push(system_instruction::transfer(
            &wallet.pubkey(),
            &jito_tip_account,
            self.tip_lamports,
        ));

        let blockhash = rpc.get_latest_blockhash().await?;
        let msg = Message::new_with_blockhash(&all_ixs, Some(&wallet.pubkey()), &blockhash);
        let mut tx = Transaction::new_unsigned(msg);
        tx.sign(&[wallet], blockhash);

        // Serialize and base64-encode
        let tx_bytes = bincode::serialize(&tx)?;
        let tx_b64 = base64::encode(&tx_bytes);

        // Send to Jito block engine
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "sendBundle",
            "params": [[tx_b64]]
        });

        let response = self
            .http_client
            .post(format!("{}/api/v1/bundles", self.jito_url))
            .json(&payload)
            .send()
            .await?;

        let result: serde_json::Value = response.json().await?;
        if let Some(bundle_id) = result["result"].as_str() {
            info!("Jito bundle submitted: {}", bundle_id);
            Ok(ExecResult {
                signature: Some(bundle_id.to_string()),
                confirmed: true, // Jito doesn't give immediate confirmation
                error: None,
            })
        } else {
            let err = result["error"]["message"]
                .as_str()
                .unwrap_or("Unknown Jito error")
                .to_string();
            Ok(ExecResult {
                signature: None,
                confirmed: false,
                error: Some(err),
            })
        }
    }
}
