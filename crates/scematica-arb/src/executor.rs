use crate::opportunity::ArbPath;
use anyhow::Result;
use scematica_ai::agents::AiCoordinator;
use scematica_core::metrics::{BotMetrics, TradeEvent, TRADES_FILE};
use scematica_core::rpc::RpcConnection;
use scematica_executor::{get_builder, SwapInstructionBuilder};
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    instruction::Instruction,
    message::Message,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
};
use std::sync::Arc;
use tracing::{info, warn};

/// Executes an arbitrage path atomically.
/// The on-chain program (scematica-swap) enforces profit-or-revert:
/// if the final output < initial input, the transaction reverts.
pub struct ArbExecutor {
    rpc: Arc<RpcConnection>,
    wallet: Arc<Keypair>,
    metrics: Arc<BotMetrics>,
    ai: Option<Arc<AiCoordinator>>,
    compute_unit_limit: u32,
    compute_unit_price: u64,
    #[allow(dead_code)]
    skip_preflight: bool,
    min_profit_lamports: u64,
    /// Program ID of the on-chain scematica-swap program
    swap_program_id: Pubkey,
    /// DEX builders for generating swap instructions
    builders: std::collections::HashMap<scematica_core::types::DexKind, Box<dyn SwapInstructionBuilder>>,
}

impl ArbExecutor {
    pub fn new(
        rpc: Arc<RpcConnection>,
        wallet: Arc<Keypair>,
        metrics: Arc<BotMetrics>,
        ai: Option<Arc<AiCoordinator>>,
        swap_program_id: Pubkey,
        min_profit_lamports: u64,
    ) -> Self {
        let mut builders = std::collections::HashMap::new();
        for dex in [
            scematica_core::types::DexKind::Raydium,
            scematica_core::types::DexKind::Orca,
            scematica_core::types::DexKind::Meteora,
            scematica_core::types::DexKind::Jupiter,
        ] {
            if let Some(builder) = get_builder(dex, rpc.client.clone()) {
                builders.insert(dex, builder);
            }
        }

        Self {
            rpc,
            wallet,
            metrics,
            ai,
            compute_unit_limit: 400_000,
            compute_unit_price: 100_000,
            skip_preflight: true,
            min_profit_lamports,
            swap_program_id,
            builders,
        }
    }

    /// Execute an arbitrage path. Returns the transaction signature if successful.
    pub async fn execute(&self, path: &ArbPath) -> Result<Option<String>> {
        // Gas-adjusted minimum profit: tx_fee × 3 (covers worst-case CU cost and leaves net profit).
        // On Solana a 400k CU tx at 100k microlamports/CU costs ~40M microlamports = 40_000 lamports.
        // We require 3× that so even failed retries don't erode capital.
        let cu_fee_lamports = (self.compute_unit_limit as u64 * self.compute_unit_price) / 1_000_000;
        let gas_adjusted_min = (cu_fee_lamports * 3).max(self.min_profit_lamports);
        if path.profit < gas_adjusted_min as i128 {
            warn!(
                "Skipping arb: profit {} < gas-adjusted min {} (cu_fee={} × 3)",
                path.profit, gas_adjusted_min, cu_fee_lamports
            );
            return Ok(None);
        }

        // Stale quote check: >2 Solana slots (~800ms) means on-chain state may have moved.
        // Executing on stale quotes risks negative-profit transactions that revert on-chain
        // but still cost ~5000 lamports in priority fees.
        if path.fetched_at_ms > 0 {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let quote_age_ms = now_ms.saturating_sub(path.fetched_at_ms);
            if quote_age_ms > 800 {
                warn!(
                    quote_age_ms,
                    "Skipping arb: quote is stale (>800ms = 2 slots) — reserves likely changed"
                );
                return Ok(None);
            }
        }

        // AI evaluation
        if let Some(ai) = &self.ai {
            let dex_names: Vec<String> = path.pool_path.iter().map(|e| format!("{:?}", e.dex)).collect();
            let pool_reserves: Vec<(u64, u64)> = path.pool_path.iter().map(|e| (e.reserve_a, e.reserve_b)).collect();

            let score = ai.arb.score_arb(
                path.hops(),
                &dex_names,
                path.input_amount as u64,
                path.profit as i64,
                path.profit_pct,
                &pool_reserves,
            ).await;

            info!(
                confidence = score.confidence,
                recommendation = %score.recommendation,
                reasoning = %score.reasoning,
                "AI arb evaluation"
            );

            if !score.should_execute() {
                info!("AI rejected arb — skipping execution");
                return Ok(None);
            }
        }

        info!(
            "Executing arb: {} hops, profit={} ({:.3}%)",
            path.hops(),
            path.profit,
            path.profit_pct
        );

        self.metrics.record_arb_found();
        self.metrics.record_trade_attempt();

        let ixs = self.build_arb_instructions(path).await?;

        let blockhash = self.rpc.client.get_latest_blockhash().await?;
        let msg = Message::new_with_blockhash(&ixs, Some(&self.wallet.pubkey()), &blockhash);
        let tx = solana_sdk::transaction::VersionedTransaction::from(
            solana_sdk::transaction::Transaction::new(&[&*self.wallet], msg, blockhash)
        );

        match self.rpc.send_transaction(&tx, self.skip_preflight).await {
            Ok(sig) => {
                info!("Arb submitted: {}", sig);
                if self.rpc.confirm_transaction(&sig, 10).await? {
                    info!("Arb confirmed: {}", sig);
                    self.metrics.record_arb_executed();
                    self.metrics.record_trade_confirmed(path.profit as i64);

                    // Emit trade event so the dashboard picks it up in real time
                    let dex_label = path.pool_path
                        .iter()
                        .map(|e| format!("{}", e.dex))
                        .collect::<Vec<_>>()
                        .join("→");
                    TradeEvent {
                        timestamp: chrono::Utc::now(),
                        kind: "ARB".into(),
                        mint: path.mint_path
                            .first()
                            .map(|m| m.to_string())
                            .unwrap_or_default(),
                        symbol: String::new(),
                        amount: scematica_core::token::raw_to_ui(
                            path.input_amount as u64,
                            6, // USDC default; fine for display
                        ),
                        pnl: path.profit as f64 / 1_000_000_000.0,
                        status: "✓".into(),
                        signature: sig.to_string(),
                        dex: dex_label,
                        hops: path.hops() as u8,
                        pnl_pct: 0.0,
                        position_age_secs: 0.0,
                        exit_reason: String::new(),
                        pool_size_sol: 0.0,
                        pool_age_secs: 0.0,
                        velocity_sol_per_sec: 0.0,
                        buy_pressure_ratio: 0.0,
                        pool_score: 0.0,
                        pumpfun_score: 0.0,
                        inflow_rate_sol_per_sec: 0.0,
                    }
                    .append_to_file(TRADES_FILE);

                    Ok(Some(sig.to_string()))
                } else {
                    warn!("Arb confirmation timeout: {}", sig);
                    self.metrics.record_trade_failed();

                    TradeEvent {
                        timestamp: chrono::Utc::now(),
                        kind: "ARB".into(),
                        mint: path.mint_path.first().map(|m| m.to_string()).unwrap_or_default(),
                        symbol: String::new(),
                        amount: scematica_core::token::raw_to_ui(path.input_amount as u64, 6),
                        pnl: 0.0,
                        status: "✗".into(),
                        signature: sig.to_string(),
                        dex: path.pool_path.iter().map(|e| format!("{}", e.dex)).collect::<Vec<_>>().join("→"),
                        hops: path.hops() as u8,
                        pnl_pct: 0.0,
                        position_age_secs: 0.0,
                        exit_reason: String::new(),
                        pool_size_sol: 0.0,
                        pool_age_secs: 0.0,
                        velocity_sol_per_sec: 0.0,
                        buy_pressure_ratio: 0.0,
                        pool_score: 0.0,
                        pumpfun_score: 0.0,
                        inflow_rate_sol_per_sec: 0.0,
                    }
                    .append_to_file(TRADES_FILE);

                    Ok(None)
                }
            }
            Err(e) => {
                // Expected: profit-or-revert will cause many txs to fail
                warn!("Arb tx failed (likely reverted): {}", e);
                self.metrics.record_trade_failed();
                Ok(None)
            }
        }
    }

    /// Build the instruction sequence for an arb path:
    /// 1. ComputeBudget instructions
    /// 2. StartSwap (initialize swap state PDA with input amount)
    /// 3. N swap instructions (one per hop)
    /// 4. ProfitOrRevert (assert output >= input, else revert)
    async fn build_arb_instructions(&self, path: &ArbPath) -> Result<Vec<Instruction>> {
        let mut ixs = vec![
            ComputeBudgetInstruction::set_compute_unit_limit(self.compute_unit_limit),
            ComputeBudgetInstruction::set_compute_unit_price(self.compute_unit_price),
        ];

        let start_mint = path.mint_path.first()
            .copied()
            .unwrap_or(scematica_core::types::known_tokens::USDC_MINT);

        // StartSwap instruction.
        // Require enough raw start-token profit to cover priority fees and the
        // configured profit floor, not merely input_amount + 1.
        let cu_fee_lamports = (self.compute_unit_limit as u64 * self.compute_unit_price) / 1_000_000;
        let gas_adjusted_min = (cu_fee_lamports * 3).max(self.min_profit_lamports);
        let min_output = (path.input_amount as u64).saturating_add(gas_adjusted_min);
        ixs.push(self.build_start_swap_ix(path.input_amount as u64, min_output, &start_mint)?);

        // Per-hop swap instructions
        for (i, edge) in path.pool_path.iter().enumerate() {
            let in_mint = path.mint_path[i];
            let out_mint = path.mint_path[i + 1];
            
            let builder = self.builders.get(&edge.dex)
                .ok_or_else(|| anyhow::anyhow!("No builder for DEX {:?}", edge.dex))?;

            let ata_in = spl_associated_token_account::get_associated_token_address(
                &self.wallet.pubkey(),
                &in_mint,
            );
            let ata_out = spl_associated_token_account::get_associated_token_address(
                &self.wallet.pubkey(),
                &out_mint,
            );

            let hop_amount_in = path.hop_amounts.get(i).copied().unwrap_or(0);
            // min_amount_out for intermediate hops: use next hop's input as floor, or 1 for last hop
            // (final safety is enforced by ProfitOrRevert on-chain)
            let hop_min_out = path.hop_amounts.get(i + 1).copied().unwrap_or(1);

            let hop_ixs = builder.build_swap(
                &edge.pool_address,
                &self.wallet.pubkey(),
                &in_mint,
                &out_mint,
                &ata_in,
                &ata_out,
                hop_amount_in,
                hop_min_out,
            ).await?;

            ixs.extend(hop_ixs);
        }

        // ProfitOrRevert instruction
        ixs.push(self.build_profit_or_revert_ix(path.input_amount as u64, &start_mint)?);

        Ok(ixs)
    }

    fn build_start_swap_ix(&self, input_amount: u64, min_output: u64, start_mint: &Pubkey) -> Result<Instruction> {
        // Calls scematica-swap::start_swap(input_amount, min_output)
        // Stores input_amount and min_output in swap_state PDA for later comparison
        let (swap_state_pda, _) = Pubkey::find_program_address(
            &[b"swap_state", self.wallet.pubkey().as_ref()],
            &self.swap_program_id,
        );

        let src_ata = spl_associated_token_account::get_associated_token_address(
            &self.wallet.pubkey(),
            start_mint,
        );

        // Instruction data: discriminator (8 bytes) + input_amount (8 bytes) + min_output (8 bytes)
        let mut data = vec![0u8; 24];
        data[0..8].copy_from_slice(&anchor_discriminator("start_swap"));
        data[8..16].copy_from_slice(&input_amount.to_le_bytes());
        data[16..24].copy_from_slice(&min_output.to_le_bytes());

        Ok(Instruction {
            program_id: self.swap_program_id,
            accounts: vec![
                solana_sdk::instruction::AccountMeta::new(src_ata, false),
                solana_sdk::instruction::AccountMeta::new(swap_state_pda, false),
                solana_sdk::instruction::AccountMeta::new(self.wallet.pubkey(), true),
                solana_sdk::instruction::AccountMeta::new_readonly(
                    solana_sdk::system_program::id(),
                    false,
                ),
                solana_sdk::instruction::AccountMeta::new_readonly(
                    spl_token::id(),
                    false,
                ),
            ],
            data,
        })
    }

    fn build_profit_or_revert_ix(&self, _min_output: u64, start_mint: &Pubkey) -> Result<Instruction> {
        let (swap_state_pda, _) = Pubkey::find_program_address(
            &[b"swap_state", self.wallet.pubkey().as_ref()],
            &self.swap_program_id,
        );

        let src_ata = spl_associated_token_account::get_associated_token_address(
            &self.wallet.pubkey(),
            start_mint,
        );

        let mut data = vec![0u8; 8];
        data[0..8].copy_from_slice(&anchor_discriminator("profit_or_revert"));

        Ok(Instruction {
            program_id: self.swap_program_id,
            accounts: vec![
                solana_sdk::instruction::AccountMeta::new(src_ata, false),
                solana_sdk::instruction::AccountMeta::new(swap_state_pda, false),
                solana_sdk::instruction::AccountMeta::new_readonly(self.wallet.pubkey(), true),
            ],
            data,
        })
    }
}

/// Compute Anchor instruction discriminator: sha256("global:<name>")[0..8]
fn anchor_discriminator(name: &str) -> [u8; 8] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(format!("global:{}", name).as_bytes());
    let result = hasher.finalize();
    let mut discriminator = [0u8; 8];
    discriminator.copy_from_slice(&result[..8]);
    discriminator
}
