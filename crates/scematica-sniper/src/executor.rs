use anyhow::Result;
use async_trait::async_trait;
use base64::{engine::general_purpose, Engine as _};
use scematica_core::metrics::{TxTelemetryEvent, TX_TELEMETRY_FILE};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    compute_budget::ComputeBudgetInstruction,
    instruction::Instruction,
    message::Message,
    signature::{Keypair, Signer},
    transaction::Transaction,
};
use std::{sync::Arc, time::Instant};
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

#[allow(clippy::too_many_arguments)]
fn append_tx_telemetry(
    executor: &str,
    tx_kind: &str,
    signature: Option<&str>,
    confirmed: bool,
    error: impl Into<String>,
    attempts: u32,
    instruction_count: usize,
    compute_unit_limit: u32,
    compute_unit_price: u64,
    compute_unit_price_hard_cap: u64,
    loaded_accounts_data_size_limit: u32,
    skip_preflight: bool,
    high_speed: bool,
    started: &Instant,
    blockhash_fetch_ms_total: u64,
    send_confirm_ms_total: u64,
    retry_delay_ms_total: u64,
    timeout_count: u32,
    rate_limit_count: u32,
    slippage_error_count: u32,
    blockhash_error_count: u32,
) {
    TxTelemetryEvent {
        timestamp: chrono::Utc::now(),
        executor: executor.to_string(),
        tx_kind: tx_kind.to_string(),
        signature: signature.unwrap_or_default().to_string(),
        confirmed,
        error: error.into(),
        attempts,
        instruction_count,
        compute_unit_limit,
        compute_unit_price,
        compute_unit_price_hard_cap,
        loaded_accounts_data_size_limit,
        skip_preflight,
        high_speed,
        elapsed_ms: started.elapsed().as_millis() as u64,
        blockhash_fetch_ms_total,
        send_confirm_ms_total,
        retry_delay_ms_total,
        timeout_count,
        rate_limit_count,
        slippage_error_count,
        blockhash_error_count,
    }
    .append_to_file(TX_TELEMETRY_FILE);
}

/// Default executor: sends via standard RPC with compute budget
pub struct DefaultExecutor {
    pub compute_unit_limit: u32,
    pub compute_unit_price: u64,
    pub compute_unit_price_hard_cap: u64,
    pub loaded_accounts_data_size_limit: u32,
    pub skip_preflight: bool,
    pub max_retries: u32,
    /// When true, fetches recent priority fees and uses the p75 percentile
    pub dynamic_fees: bool,
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
            compute_unit_price_hard_cap: 0,
            loaded_accounts_data_size_limit: 0,
            skip_preflight,
            max_retries,
            dynamic_fees: false,
        }
    }

    pub fn with_priority_fee_hard_cap(mut self, hard_cap: u64) -> Self {
        self.compute_unit_price_hard_cap = hard_cap;
        self
    }

    pub fn with_loaded_accounts_data_size_limit(mut self, bytes_limit: u32) -> Self {
        self.loaded_accounts_data_size_limit = bytes_limit;
        self
    }

    /// Enable dynamic priority fee: fetches getRecentPrioritizationFees and uses p75.
    /// Falls back to compute_unit_price if the RPC call fails.
    pub fn with_dynamic_fees(mut self) -> Self {
        self.dynamic_fees = true;
        self
    }

    async fn get_dynamic_fee(&self, rpc: &Arc<RpcClient>) -> u64 {
        match rpc.get_recent_prioritization_fees(&[]).await {
            Ok(fees) if !fees.is_empty() => {
                let mut vals: Vec<u64> = fees.iter().map(|f| f.prioritization_fee).collect();
                vals.sort_unstable();
                let p75 = vals[(vals.len() * 3) / 4];
                let result = p75.max(self.compute_unit_price);
                debug!(
                    "Dynamic priority fee: {} (p75={}, floor={})",
                    result, p75, self.compute_unit_price
                );
                result
            }
            Ok(_) => {
                warn!("get_recent_prioritization_fees returned empty — using configured fee");
                self.compute_unit_price
            }
            Err(e) => {
                warn!(
                    "get_recent_prioritization_fees failed: {} — using configured fee",
                    e
                );
                self.compute_unit_price
            }
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
        let started = Instant::now();
        let mut attempts_made = 0;
        let mut blockhash_fetch_ms_total = 0;
        let mut send_confirm_ms_total = 0;
        let mut retry_delay_ms_total = 0;
        let mut timeout_count = 0;
        let mut rate_limit_count = 0;
        let mut slippage_error_count = 0;
        let mut blockhash_error_count = 0;

        // Read high-speed sentinel file once — shared for the whole execute() call.
        let high_speed = std::path::Path::new("scematica-highspeed-mode.json").exists();

        let cpu_price = {
            let base = if self.dynamic_fees {
                self.get_dynamic_fee(rpc).await
            } else {
                self.compute_unit_price
            };
            let escalated = if high_speed {
                base.saturating_mul(3)
            } else {
                base
            };
            if self.compute_unit_price_hard_cap > 0 {
                escalated.min(self.compute_unit_price_hard_cap)
            } else {
                escalated
            }
        };

        // Capture caller-supplied instruction count BEFORE moving into all_ixs —
        // used below for the buy-vs-sell heuristic (buy=5+, sell=1–3).
        let caller_ix_count = instructions.len();
        let tx_kind = if caller_ix_count >= 4 {
            "buy"
        } else if caller_ix_count > 0 {
            "sell"
        } else {
            "unknown"
        };
        let mut all_ixs = vec![
            ComputeBudgetInstruction::set_compute_unit_limit(self.compute_unit_limit),
            ComputeBudgetInstruction::set_compute_unit_price(cpu_price),
        ];
        if self.loaded_accounts_data_size_limit > 0 {
            all_ixs.insert(
                0,
                ComputeBudgetInstruction::set_loaded_accounts_data_size_limit(
                    self.loaded_accounts_data_size_limit,
                ),
            );
        }
        all_ixs.extend(instructions);

        let send_config = solana_client::rpc_config::RpcSendTransactionConfig {
            skip_preflight: self.skip_preflight,
            ..Default::default()
        };

        // Heuristic: buys carry many caller-supplied instructions (create-WSOL-ATA +
        // transfer + sync_native + create-base-ATA + swap, typically 5+). Sells are
        // just the swap (and maybe a close-account), typically 1–2. We tighten the
        // per-attempt deadline only on buys in high-speed mode — sells must always
        // get enough time to LAND, otherwise the position can't exit and we get the
        // "sell exhausted 5 retries" loop.
        let is_buy_shape = caller_ix_count >= 4;

        for attempt in 0..self.max_retries {
            attempts_made = attempt + 1;
            // Fetch a FRESH blockhash before every attempt.
            //
            // The previous design signed once and resubmitted the same tx on all retries.
            // After a 429 backoff (8 s + 16 s + 32 s = 56 s total), Solana's ~150-slot
            // (~60 s) blockhash TTL expired and every retry returned "BlockhashNotFound"
            // even though the RPC was healthy. Fetching fresh here fixes that and adds
            // <20 ms of latency per attempt (one roundtrip vs re-signing a stale tx).
            let blockhash_started = Instant::now();
            let blockhash = match rpc.get_latest_blockhash().await {
                Ok(bh) => bh,
                Err(e) => {
                    blockhash_fetch_ms_total += blockhash_started.elapsed().as_millis() as u64;
                    blockhash_error_count += 1;
                    warn!(
                        "Blockhash fetch failed on attempt {}: {} — retrying",
                        attempt + 1,
                        e
                    );
                    if attempt + 1 < self.max_retries {
                        let delay_ms = 200;
                        retry_delay_ms_total += delay_ms;
                        tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
                    }
                    continue;
                }
            };
            blockhash_fetch_ms_total += blockhash_started.elapsed().as_millis() as u64;
            let msg = Message::new_with_blockhash(&all_ixs, Some(&wallet.pubkey()), &blockhash);
            let mut tx = Transaction::new_unsigned(msg);
            tx.sign(&[wallet], blockhash);

            debug!(
                "Sending transaction attempt {}/{} (high_speed={}, buy_shape={})",
                attempt + 1,
                self.max_retries,
                high_speed,
                is_buy_shape,
            );
            let per_attempt_deadline = if high_speed && is_buy_shape {
                tokio::time::Duration::from_millis(2500)
            } else {
                tokio::time::Duration::from_secs(6)
            };
            const POLL_INTERVAL: tokio::time::Duration = tokio::time::Duration::from_millis(400);

            let send_started = Instant::now();
            let outcome = tokio::time::timeout(per_attempt_deadline, async {
                // 1. Fire the tx (non-blocking — returns once the RPC accepts it).
                let sig = rpc.send_transaction_with_config(&tx, send_config).await?;

                // 2. Poll for `processed` commitment. `processed` is the fastest meaningful
                //    state on Solana — typically lands within 400–800 ms. Waiting for
                //    `confirmed` here costs another 800–1500 ms per snipe with no value
                //    because the sell-monitor verifies the position via on-chain balance
                //    read anyway.
                loop {
                    match rpc
                        .confirm_transaction_with_commitment(&sig, CommitmentConfig::processed())
                        .await
                    {
                        Ok(resp) if resp.value => return Ok::<_, anyhow::Error>(sig),
                        Ok(_) => tokio::time::sleep(POLL_INTERVAL).await,
                        Err(e) => return Err(e.into()),
                    }
                }
            })
            .await;
            send_confirm_ms_total += send_started.elapsed().as_millis() as u64;

            match outcome {
                Ok(Ok(sig)) => {
                    info!("Transaction landed: {}", sig);
                    let sig_str = sig.to_string();
                    append_tx_telemetry(
                        "default",
                        tx_kind,
                        Some(sig_str.as_str()),
                        true,
                        "",
                        attempts_made,
                        caller_ix_count,
                        self.compute_unit_limit,
                        cpu_price,
                        self.compute_unit_price_hard_cap,
                        self.loaded_accounts_data_size_limit,
                        self.skip_preflight,
                        high_speed,
                        &started,
                        blockhash_fetch_ms_total,
                        send_confirm_ms_total,
                        retry_delay_ms_total,
                        timeout_count,
                        rate_limit_count,
                        slippage_error_count,
                        blockhash_error_count,
                    );
                    return Ok(ExecResult {
                        signature: Some(sig_str),
                        confirmed: true,
                        error: None,
                    });
                }
                Ok(Err(e)) => {
                    let err_str = e.to_string();
                    warn!("Transaction attempt {} failed: {}", attempt + 1, err_str);
                    // Error-specific retry delay:
                    //   Slippage (0x26): immediate — rebuild with zero min_out at the do_sell layer
                    //   BlockhashNotFound: immediate — fresh hash already fetched at loop top
                    //   Rate-limit (429): exponential backoff (Helius free-plan window ~10 s)
                    //   High-speed: always immediate regardless of error type
                    //   Other transient: short 200 ms gap
                    let is_slippage = err_str.contains("0x26") || err_str.contains("slippage");
                    let is_blockhash = err_str.contains("BlockhashNotFound")
                        || err_str.contains("Blockhash not found");
                    let is_rate_limit = err_str.contains("429");
                    if is_slippage {
                        slippage_error_count += 1;
                    }
                    if is_blockhash {
                        blockhash_error_count += 1;
                    }
                    if is_rate_limit {
                        rate_limit_count += 1;
                    }
                    if attempt + 1 >= self.max_retries {
                        append_tx_telemetry(
                            "default",
                            tx_kind,
                            None,
                            false,
                            err_str.clone(),
                            attempts_made,
                            caller_ix_count,
                            self.compute_unit_limit,
                            cpu_price,
                            self.compute_unit_price_hard_cap,
                            self.loaded_accounts_data_size_limit,
                            self.skip_preflight,
                            high_speed,
                            &started,
                            blockhash_fetch_ms_total,
                            send_confirm_ms_total,
                            retry_delay_ms_total,
                            timeout_count,
                            rate_limit_count,
                            slippage_error_count,
                            blockhash_error_count,
                        );
                        return Ok(ExecResult {
                            signature: None,
                            confirmed: false,
                            error: Some(err_str),
                        });
                    }
                    let delay_ms: u64 = if high_speed || is_slippage || is_blockhash {
                        0 // retry immediately; slippage rebuilt by caller, hash refreshed at loop top
                    } else if is_rate_limit {
                        8000u64 << attempt.min(2)
                    } else {
                        200
                    };
                    if delay_ms > 0 {
                        retry_delay_ms_total += delay_ms;
                        tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
                    }
                }
                Err(_) => {
                    timeout_count += 1;
                    warn!(
                        "Transaction attempt {}/{} timed out after {:?}",
                        attempt + 1,
                        self.max_retries,
                        per_attempt_deadline,
                    );
                    if attempt + 1 >= self.max_retries {
                        let err = format!("timeout after {:?}", per_attempt_deadline);
                        append_tx_telemetry(
                            "default",
                            tx_kind,
                            None,
                            false,
                            err.clone(),
                            attempts_made,
                            caller_ix_count,
                            self.compute_unit_limit,
                            cpu_price,
                            self.compute_unit_price_hard_cap,
                            self.loaded_accounts_data_size_limit,
                            self.skip_preflight,
                            high_speed,
                            &started,
                            blockhash_fetch_ms_total,
                            send_confirm_ms_total,
                            retry_delay_ms_total,
                            timeout_count,
                            rate_limit_count,
                            slippage_error_count,
                            blockhash_error_count,
                        );
                        return Ok(ExecResult {
                            signature: None,
                            confirmed: false,
                            error: Some(err),
                        });
                    }
                    // Timeout: fresh blockhash on next attempt is sufficient, no extra delay
                }
            }
        }

        append_tx_telemetry(
            "default",
            tx_kind,
            None,
            false,
            "Max retries exceeded",
            attempts_made,
            caller_ix_count,
            self.compute_unit_limit,
            cpu_price,
            self.compute_unit_price_hard_cap,
            self.loaded_accounts_data_size_limit,
            self.skip_preflight,
            high_speed,
            &started,
            blockhash_fetch_ms_total,
            send_confirm_ms_total,
            retry_delay_ms_total,
            timeout_count,
            rate_limit_count,
            slippage_error_count,
            blockhash_error_count,
        );
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

        let started = Instant::now();
        let instruction_count = instructions.len();
        let tx_kind = if instruction_count >= 4 {
            "buy"
        } else if instruction_count > 0 {
            "sell"
        } else {
            "unknown"
        };

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

        let blockhash_started = Instant::now();
        let blockhash = match rpc.get_latest_blockhash().await {
            Ok(bh) => bh,
            Err(e) => {
                let blockhash_ms = blockhash_started.elapsed().as_millis() as u64;
                append_tx_telemetry(
                    "jito",
                    tx_kind,
                    None,
                    false,
                    e.to_string(),
                    1,
                    instruction_count,
                    0,
                    0,
                    0,
                    0,
                    false,
                    false,
                    &started,
                    blockhash_ms,
                    0,
                    0,
                    0,
                    0,
                    0,
                    1,
                );
                return Err(e.into());
            }
        };
        let blockhash_fetch_ms_total = blockhash_started.elapsed().as_millis() as u64;
        let msg = Message::new_with_blockhash(&all_ixs, Some(&wallet.pubkey()), &blockhash);
        let mut tx = Transaction::new_unsigned(msg);
        tx.sign(&[wallet], blockhash);

        // Serialize and base64-encode
        let tx_bytes = match bincode::serialize(&tx) {
            Ok(bytes) => bytes,
            Err(e) => {
                append_tx_telemetry(
                    "jito",
                    tx_kind,
                    None,
                    false,
                    e.to_string(),
                    1,
                    instruction_count,
                    0,
                    0,
                    0,
                    0,
                    false,
                    false,
                    &started,
                    blockhash_fetch_ms_total,
                    0,
                    0,
                    0,
                    0,
                    0,
                    0,
                );
                return Err(e.into());
            }
        };
        let tx_b64 = general_purpose::STANDARD.encode(&tx_bytes);

        // Send to Jito block engine
        let payload = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "sendBundle",
            "params": [[tx_b64]]
        });

        let send_started = Instant::now();
        let response = match self
            .http_client
            .post(format!("{}/api/v1/bundles", self.jito_url))
            .json(&payload)
            .send()
            .await
        {
            Ok(response) => response,
            Err(e) => {
                let send_confirm_ms_total = send_started.elapsed().as_millis() as u64;
                append_tx_telemetry(
                    "jito",
                    tx_kind,
                    None,
                    false,
                    e.to_string(),
                    1,
                    instruction_count,
                    0,
                    0,
                    0,
                    0,
                    false,
                    false,
                    &started,
                    blockhash_fetch_ms_total,
                    send_confirm_ms_total,
                    0,
                    0,
                    0,
                    0,
                    0,
                );
                return Err(e.into());
            }
        };

        let result: serde_json::Value = match response.json().await {
            Ok(result) => result,
            Err(e) => {
                let send_confirm_ms_total = send_started.elapsed().as_millis() as u64;
                append_tx_telemetry(
                    "jito",
                    tx_kind,
                    None,
                    false,
                    e.to_string(),
                    1,
                    instruction_count,
                    0,
                    0,
                    0,
                    0,
                    false,
                    false,
                    &started,
                    blockhash_fetch_ms_total,
                    send_confirm_ms_total,
                    0,
                    0,
                    0,
                    0,
                    0,
                );
                return Err(e.into());
            }
        };
        let send_confirm_ms_total = send_started.elapsed().as_millis() as u64;
        if let Some(bundle_id) = result["result"].as_str() {
            info!("Jito bundle submitted: {}", bundle_id);
            append_tx_telemetry(
                "jito",
                tx_kind,
                Some(bundle_id),
                true,
                "",
                1,
                instruction_count,
                0,
                0,
                0,
                0,
                false,
                false,
                &started,
                blockhash_fetch_ms_total,
                send_confirm_ms_total,
                0,
                0,
                0,
                0,
                0,
            );
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
            append_tx_telemetry(
                "jito",
                tx_kind,
                None,
                false,
                err.clone(),
                1,
                instruction_count,
                0,
                0,
                0,
                0,
                false,
                false,
                &started,
                blockhash_fetch_ms_total,
                send_confirm_ms_total,
                0,
                0,
                0,
                0,
                0,
            );
            Ok(ExecResult {
                signature: None,
                confirmed: false,
                error: Some(err),
            })
        }
    }
}
