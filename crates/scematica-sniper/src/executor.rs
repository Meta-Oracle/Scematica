use anyhow::Result;
use async_trait::async_trait;
use base64::{engine::general_purpose, Engine as _};
use scematica_core::metrics::{
    artifact_path, TxTelemetryEvent, HIGH_SPEED_FILE, TX_TELEMETRY_FILE,
};
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

/// Render a `TransactionError` so the retry classifier sees the program's custom code in
/// the same `0x..` form the RPC uses in its own error strings.
///
/// Necessary because the two sources disagree on spelling for the identical failure. A
/// preflight rejection arrives as text already containing `custom program error: 0x1e`,
/// but a status read arrives as a typed `TransactionError` whose `Debug` is
/// `InstructionError(7, Custom(30))` — decimal, no `0x`. Matching on the debug form alone
/// would classify an on-chain slippage revert as an unknown transient purely because it
/// was discovered by polling rather than by preflight.
fn format_tx_error(e: &solana_sdk::transaction::TransactionError) -> String {
    use solana_sdk::instruction::InstructionError;
    use solana_sdk::transaction::TransactionError;
    if let TransactionError::InstructionError(ix, InstructionError::Custom(code)) = e {
        return format!("InstructionError({ix}, Custom({code})) custom program error: {code:#x}");
    }
    format!("{e:?}")
}

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
        let high_speed = artifact_path(HIGH_SPEED_FILE).exists();

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

            // THE SIGNATURE MUST OUTLIVE THE CONFIRMATION DEADLINE.
            //
            // Send and confirm used to sit inside one `timeout`, so when the deadline
            // expired the whole future was dropped — signature included — and the caller
            // was told "timeout". But the transaction had already been accepted by the
            // cluster at that point. Two bad outcomes followed, and the second is much
            // worse than the one that prompted this:
            //
            //   • A tx that landed and REVERTED was reported as a timeout, so the
            //     error-specific retry logic below never saw the real reason. Measured
            //     2026-08-16: a buy reverted with Raydium's "Exceeded desired slippage
            //     limit" and the bot logged `timed out after 6s`, then burned two more
            //     attempts on the same min_out.
            //   • A tx that landed and SUCCEEDED would be reported as failed. That is an
            //     untracked position: tokens bought, no sell-monitor spawned, no stop
            //     loss, no exit. With real capital that is the worst failure in the file,
            //     and it was one slow RPC response away at any time.
            //
            // So: send under its own deadline, keep the signature, then poll under the
            // remaining budget. On expiry, ask the cluster once what actually happened.
            // ── 1. Send, under its own deadline, keeping the signature. ──────────
            let send_deadline = per_attempt_deadline / 2;
            let sent = tokio::time::timeout(
                send_deadline,
                rpc.send_transaction_with_config(&tx, send_config),
            )
            .await;

            // Set only when the send itself failed, so the shared "undecided" path below
            // can report why instead of a bare deadline.
            let mut send_error: Option<String> = None;
            let sig = match sent {
                Ok(Ok(sig)) => Some(sig),
                // The RPC rejected it outright (preflight, malformed, 429). Nothing
                // reached the cluster, so there is no signature to reconcile.
                Ok(Err(e)) => {
                    send_error = Some(e.to_string());
                    None
                }
                // No signature came back, so we cannot know whether the cluster saw it.
                Err(_) => {
                    send_error = Some(format!("send timed out after {send_deadline:?}"));
                    None
                }
            };

            // ── 2. Poll the signature's real status. ─────────────────────────────
            //
            // `confirm_transaction_with_commitment` cannot be used here: it collapses
            // `status.is_ok()` into the same `false` it returns for "not seen yet", so a
            // REVERTED transaction is indistinguishable from a pending one and the loop
            // simply spins until the deadline. That is the mechanism behind the
            // 2026-08-16 report of `timed out after 6s` for a buy that had in fact
            // already reverted on-chain with Raydium's slippage error two seconds in.
            //
            // `get_signature_statuses` keeps the three cases apart: absent (keep
            // polling), present-with-err (a decided failure — return the real reason so
            // the classifier below can act on it), present-without-err (landed).
            let outcome: Option<Result<solana_sdk::signature::Signature, String>> = match sig {
                None => None,
                Some(sig) => {
                    let confirm_budget =
                        per_attempt_deadline.saturating_sub(send_started.elapsed());
                    let polled = tokio::time::timeout(confirm_budget, async {
                        loop {
                            match rpc.get_signature_statuses(&[sig]).await {
                                Ok(resp) => match resp.value.first().and_then(|s| s.as_ref()) {
                                    Some(status)
                                        if status.satisfies_commitment(
                                            CommitmentConfig::processed(),
                                        ) =>
                                    {
                                        return match &status.err {
                                            Some(e) => Err(format_tx_error(e)),
                                            None => Ok(sig),
                                        };
                                    }
                                    // Not seen yet, or seen below our commitment.
                                    _ => tokio::time::sleep(POLL_INTERVAL).await,
                                },
                                // A failed status query says nothing about the tx.
                                Err(_) => tokio::time::sleep(POLL_INTERVAL).await,
                            }
                        }
                    })
                    .await;

                    match polled {
                        Ok(decided) => Some(decided),
                        // The polling budget ran out. Ask once more, directly: the
                        // cluster is the authority, and on a throttled RPC the answer
                        // frequently arrives just after the budget expires. Reporting a
                        // landed buy as failed would leave an untracked position with no
                        // sell-monitor and no stop loss — the worst outcome in this file.
                        Err(_) => match rpc.get_signature_statuses(&[sig]).await {
                            Ok(resp) => resp
                                .value
                                .first()
                                .and_then(|s| s.as_ref())
                                .map(|status| match &status.err {
                                    Some(e) => Err(format_tx_error(e)),
                                    None => Ok(sig),
                                }),
                            Err(_) => None,
                        },
                    }
                }
            };
            send_confirm_ms_total += send_started.elapsed().as_millis() as u64;

            match outcome {
                Some(Ok(sig)) => {
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
                Some(Err(err_str)) => {
                    warn!("Transaction attempt {} failed: {}", attempt + 1, err_str);
                    // Error-specific retry delay:
                    //   Slippage: immediate — rebuild with zero min_out at the do_sell layer
                    //   BlockhashNotFound: immediate — fresh hash already fetched at loop top
                    //   Rate-limit (429): exponential backoff (Helius free-plan window ~10 s)
                    //   High-speed: always immediate regardless of error type
                    //   Other transient: short 200 ms gap
                    //
                    // `0x1e` is the one that matters and it was missing until 2026-08-16.
                    // Raydium AMM V4 (675kPX9…) returns **30 / 0x1e** for
                    // `ExceededSlippage` — confirmed on-chain from the program's own log,
                    // `AMM error: Exceeded desired slippage limit.` followed by
                    // `custom program error: 0x1e`. Matching only `0x26` (38) meant a real
                    // Raydium slippage revert was classified as an unknown transient: no
                    // zero-slippage rebuild, no immediate retry, just a 200 ms nap and two
                    // more attempts at the same doomed min_out. Both codes stay listed —
                    // other DEXes in the executor use their own numbering — and the
                    // human-readable form is matched too, since it survives any
                    // renumbering the programs do later.
                    let is_slippage = err_str.contains("0x1e")
                        || err_str.contains("0x26")
                        || err_str.to_ascii_lowercase().contains("slippage");
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
                // Undecided: either the send never produced a signature, or the cluster
                // still had no status for it after the final direct check. This is the
                // only branch entitled to say "timeout" — an on-chain revert now lands
                // in the arm above with its real reason.
                None => {
                    timeout_count += 1;
                    let reason = send_error
                        .clone()
                        .unwrap_or_else(|| format!("timeout after {per_attempt_deadline:?}"));
                    warn!(
                        "Transaction attempt {}/{} undecided: {}",
                        attempt + 1,
                        self.max_retries,
                        reason,
                    );
                    if attempt + 1 >= self.max_retries {
                        let err = reason.clone();
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

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::instruction::InstructionError;
    use solana_sdk::transaction::TransactionError;

    /// The exact failure measured on-chain 2026-08-16 (sig 5X7CTRDb…svvfJWs): a buy that
    /// landed and reverted with Raydium AMM V4's "Exceeded desired slippage limit".
    ///
    /// It must render with the `0x1e` spelling, because the retry classifier reads a
    /// string and the typed error's own `Debug` says `Custom(30)`. Getting this wrong is
    /// silent: the buy still fails, it is just filed as an unknown transient and the
    /// zero-slippage rebuild never runs.
    #[test]
    fn a_raydium_slippage_revert_renders_with_its_hex_code() {
        let e = TransactionError::InstructionError(7, InstructionError::Custom(30));
        let s = format_tx_error(&e);
        assert!(s.contains("0x1e"), "got: {s}");
        assert!(s.contains("Custom(30)"), "the raw form stays readable too: {s}");
    }

    /// Whatever discovers the failure — preflight text or a polled status — the classifier
    /// must reach the same verdict. These are the two spellings of one event.
    #[test]
    fn both_spellings_of_a_slippage_failure_classify_alike() {
        let is_slippage = |s: &str| {
            s.contains("0x1e") || s.contains("0x26") || s.to_ascii_lowercase().contains("slippage")
        };
        let from_status =
            format_tx_error(&TransactionError::InstructionError(7, InstructionError::Custom(30)));
        let from_preflight =
            "Error processing Instruction 7: custom program error: 0x1e".to_string();
        assert!(is_slippage(&from_status), "status form: {from_status}");
        assert!(is_slippage(&from_preflight));
        // And the human-readable form the program logs, in case codes are renumbered.
        assert!(is_slippage("AMM error: Exceeded desired slippage limit."));
    }

    /// A revert that is NOT slippage must not be swept into the slippage path — that path
    /// rebuilds the swap with `min_out = 0`, which on a genuinely broken pool is how you
    /// buy the top with no floor at all.
    #[test]
    fn unrelated_reverts_are_not_treated_as_slippage() {
        let is_slippage = |s: &str| {
            s.contains("0x1e") || s.contains("0x26") || s.to_ascii_lowercase().contains("slippage")
        };
        let insufficient = format_tx_error(&TransactionError::InsufficientFundsForFee);
        assert!(!is_slippage(&insufficient), "got: {insufficient}");
        // Custom(1) — seen on this wallet 2026-08-14 — is not slippage.
        let other =
            format_tx_error(&TransactionError::InstructionError(2, InstructionError::Custom(1)));
        assert!(!is_slippage(&other), "got: {other}");
    }

    /// Non-instruction errors still have to say something useful.
    #[test]
    fn other_transaction_errors_keep_their_debug_form() {
        assert_eq!(
            format_tx_error(&TransactionError::BlockhashNotFound),
            "BlockhashNotFound"
        );
    }
}
