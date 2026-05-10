use crate::opportunity::ArbPath;
use anyhow::Result;
use scematica_core::metrics::BotMetrics;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction,
    instruction::Instruction,
    message::Message,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::Transaction,
};
use std::sync::Arc;
use tracing::{error, info, warn};

/// Executes an arbitrage path atomically.
/// The on-chain program (scematica-swap) enforces profit-or-revert:
/// if the final output < initial input, the transaction reverts.
pub struct ArbExecutor {
    rpc: Arc<RpcClient>,
    wallet: Arc<Keypair>,
    metrics: Arc<BotMetrics>,
    compute_unit_limit: u32,
    compute_unit_price: u64,
    skip_preflight: bool,
    min_profit_lamports: u64,
    /// Program ID of the on-chain scematica-swap program
    swap_program_id: Pubkey,
}

impl ArbExecutor {
    pub fn new(
        rpc: Arc<RpcClient>,
        wallet: Arc<Keypair>,
        metrics: Arc<BotMetrics>,
        swap_program_id: Pubkey,
        min_profit_lamports: u64,
    ) -> Self {
        Self {
            rpc,
            wallet,
            metrics,
            compute_unit_limit: 400_000,
            compute_unit_price: 100_000,
            skip_preflight: true,
            min_profit_lamports,
            swap_program_id,
        }
    }

    /// Execute an arbitrage path. Returns the transaction signature if successful.
    pub async fn execute(&self, path: &ArbPath) -> Result<Option<String>> {
        // Sanity check: minimum profit threshold
        if path.profit < self.min_profit_lamports as i128 {
            warn!(
                "Skipping arb: profit {} < min {}",
                path.profit, self.min_profit_lamports
            );
            return Ok(None);
        }

        info!(
            "Executing arb: {} hops, profit={} ({:.3}%)",
            path.hops(),
            path.profit,
            path.profit_pct
        );

        self.metrics.record_arb_found();
        self.metrics.record_trade_attempt();

        let ixs = self.build_arb_instructions(path)?;

        let blockhash = self.rpc.get_latest_blockhash().await?;
        let msg = Message::new_with_blockhash(&ixs, Some(&self.wallet.pubkey()), &blockhash);
        let mut tx = Transaction::new_unsigned(msg);
        tx.sign(&[&*self.wallet], blockhash);

        match self.rpc.send_and_confirm_transaction_with_spinner(&tx).await {
            Ok(sig) => {
                info!("Arb confirmed: {}", sig);
                self.metrics.record_arb_executed();
                self.metrics.record_trade_confirmed(path.profit as i64);
                Ok(Some(sig.to_string()))
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
    fn build_arb_instructions(&self, path: &ArbPath) -> Result<Vec<Instruction>> {
        let mut ixs = vec![
            ComputeBudgetInstruction::set_compute_unit_limit(self.compute_unit_limit),
            ComputeBudgetInstruction::set_compute_unit_price(self.compute_unit_price),
        ];

        let start_mint = path.mint_path.first()
            .copied()
            .unwrap_or(scematica_core::types::known_tokens::USDC_MINT);

        // StartSwap instruction
        ixs.push(self.build_start_swap_ix(path.input_amount as u64, &start_mint)?);

        // Per-hop swap instructions
        for (i, edge) in path.pool_path.iter().enumerate() {
            let in_mint = path.mint_path[i];
            let out_mint = path.mint_path[i + 1];
            ixs.push(self.build_swap_ix(edge, &in_mint, &out_mint)?);
        }

        // ProfitOrRevert instruction
        ixs.push(self.build_profit_or_revert_ix(path.input_amount as u64, &start_mint)?);

        Ok(ixs)
    }

    fn build_start_swap_ix(&self, input_amount: u64, start_mint: &Pubkey) -> Result<Instruction> {
        // Calls scematica-swap::start_swap(input_amount)
        // Stores input_amount in swap_state PDA for later comparison
        let (swap_state_pda, _) = Pubkey::find_program_address(
            &[b"swap_state", self.wallet.pubkey().as_ref()],
            &self.swap_program_id,
        );

        let src_ata = spl_associated_token_account::get_associated_token_address(
            &self.wallet.pubkey(),
            start_mint,
        );

        // Instruction data: discriminator (8 bytes) + input_amount (8 bytes)
        let mut data = vec![0u8; 16];
        data[0..8].copy_from_slice(&anchor_discriminator("start_swap"));
        data[8..16].copy_from_slice(&input_amount.to_le_bytes());

        Ok(Instruction {
            program_id: self.swap_program_id,
            accounts: vec![
                solana_sdk::instruction::AccountMeta::new(src_ata, false),
                solana_sdk::instruction::AccountMeta::new(swap_state_pda, false),
                solana_sdk::instruction::AccountMeta::new_readonly(self.wallet.pubkey(), true),
                solana_sdk::instruction::AccountMeta::new_readonly(
                    solana_sdk::system_program::id(),
                    false,
                ),
            ],
            data,
        })
    }

    fn build_swap_ix(
        &self,
        edge: &crate::graph::PoolEdge,
        in_mint: &Pubkey,
        out_mint: &Pubkey,
    ) -> Result<Instruction> {
        // Each DEX has its own swap instruction format.
        // This dispatches to the correct builder based on edge.dex.
        // Full implementations are in scematica-executor.
        use scematica_core::types::DexKind;
        match edge.dex {
            DexKind::Raydium => self.build_raydium_swap_ix(edge, in_mint, out_mint),
            DexKind::Orca => self.build_orca_swap_ix(edge, in_mint, out_mint),
            DexKind::Meteora => self.build_meteora_swap_ix(edge, in_mint, out_mint),
            _ => Err(anyhow::anyhow!("Unsupported DEX: {:?}", edge.dex)),
        }
    }

    fn build_raydium_swap_ix(
        &self,
        edge: &crate::graph::PoolEdge,
        in_mint: &Pubkey,
        out_mint: &Pubkey,
    ) -> Result<Instruction> {
        // Raydium V4 swap instruction
        // Full account list required: amm, authority, open_orders, target_orders,
        // coin_vault, pc_vault, serum_program, serum_market, etc.
        // Placeholder — full impl in scematica-executor
        Ok(Instruction {
            program_id: scematica_core::dex::program_ids::RAYDIUM_AMM_V4,
            accounts: vec![],
            data: vec![9u8], // Raydium swap discriminator
        })
    }

    fn build_orca_swap_ix(
        &self,
        edge: &crate::graph::PoolEdge,
        in_mint: &Pubkey,
        out_mint: &Pubkey,
    ) -> Result<Instruction> {
        Ok(Instruction {
            program_id: scematica_core::dex::program_ids::ORCA_WHIRLPOOL,
            accounts: vec![],
            data: vec![0xf8, 0xc6, 0x9e, 0x91], // Whirlpool swap discriminator
        })
    }

    fn build_meteora_swap_ix(
        &self,
        edge: &crate::graph::PoolEdge,
        in_mint: &Pubkey,
        out_mint: &Pubkey,
    ) -> Result<Instruction> {
        Ok(Instruction {
            program_id: scematica_core::dex::program_ids::METEORA_DLMM,
            accounts: vec![],
            data: vec![0xf8, 0xc6, 0x9e, 0x91],
        })
    }

    fn build_profit_or_revert_ix(&self, min_output: u64, start_mint: &Pubkey) -> Result<Instruction> {
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
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    // Simplified: in production use sha256("global:<name>")[0..8]
    let mut hasher = DefaultHasher::new();
    format!("global:{}", name).hash(&mut hasher);
    let h = hasher.finish();
    h.to_le_bytes()
}
