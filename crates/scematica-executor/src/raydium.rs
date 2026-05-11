use crate::{SwapInstructionBuilder, StateDecoder};
use anyhow::Result;
use async_trait::async_trait;
use borsh::BorshDeserialize;
use crate::raydium_state::RaydiumAmmV4;
use scematica_core::{dex::program_ids, types::DexKind};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};
use std::sync::Arc;

/// Raydium AMM V4 swap instruction builder
pub struct RaydiumBuilder {
    rpc: Arc<RpcClient>,
}

impl RaydiumBuilder {
    pub fn new(rpc: Arc<RpcClient>) -> Self {
        Self { rpc }
    }
}

impl StateDecoder for RaydiumBuilder {
    fn decode_pool_state(&self, data: &[u8]) -> Result<(u64, u64)> {
        let state = RaydiumAmmV4::try_from_slice(&data[..RaydiumAmmV4::LEN])?;
        Ok((state.lp_reserve, 0)) // Raydium reserves are in vaults
    }
}

/// Raydium V4 swap instruction data layout
/// Discriminator: 9 (swap base in)
/// amount_in: u64
/// min_amount_out: u64
fn raydium_swap_data(amount_in: u64, min_amount_out: u64) -> Vec<u8> {
    let mut data = vec![9u8]; // swap instruction discriminator
    data.extend_from_slice(&amount_in.to_le_bytes());
    data.extend_from_slice(&min_amount_out.to_le_bytes());
    data
}

#[async_trait]
impl SwapInstructionBuilder for RaydiumBuilder {
    fn dex(&self) -> DexKind {
        DexKind::Raydium
    }

    async fn build_swap(
        &self,
        pool: &Pubkey,
        owner: &Pubkey,
        token_in: &Pubkey,
        token_out: &Pubkey,
        ata_in: &Pubkey,
        ata_out: &Pubkey,
        amount_in: u64,
        min_amount_out: u64,
    ) -> Result<Vec<Instruction>> {
        // ── Step 1: Fetch and decode pool state (Req 2.1, 2.2, 2.3, 2.4, 2.5) ──

        // Req 2.4: fetch pool account data; propagate RPC error with pool pubkey context
        let data = self
            .rpc
            .get_account_data(pool)
            .await
            .map_err(|e| anyhow::anyhow!("RPC error fetching pool {}: {}", pool, e))?;

        // Req 2.3: guard against truncated data
        if data.len() < RaydiumAmmV4::LEN {
            anyhow::bail!(
                "pool data too short: {} < {} bytes for pool {}",
                data.len(),
                RaydiumAmmV4::LEN,
                pool
            );
        }

        // Req 2.2: Borsh-deserialize the pool state
        let state = RaydiumAmmV4::try_from_slice(&data[..RaydiumAmmV4::LEN])?;

        // Req 2.5: reject uninitialized pools (status == 0)
        if state.status == 0 {
            anyhow::bail!("pool {} is uninitialized (status == 0)", pool);
        }

        // Raydium V4 requires many accounts. In production these are fetched
        // from the pool state account and the associated Serum/OpenBook market.
        // The full account list:
        // 0: token_program
        // 1: amm_id (pool)
        // 2: amm_authority (PDA)
        // 3: amm_open_orders
        // 4: amm_target_orders
        // 5: pool_coin_token_account (base vault)
        // 6: pool_pc_token_account (quote vault)
        // 7: serum_program_id
        // 8: serum_market
        // 9: serum_bids
        // 10: serum_asks
        // 11: serum_event_queue
        // 12: serum_coin_vault_account
        // 13: serum_pc_vault_account
        // 14: serum_vault_signer
        // 15: user_source_token_account (ata_in)
        // 16: user_destination_token_account (ata_out)
        // 17: user_source_owner (owner)

        // Derive AMM authority PDA
        let (amm_authority, _) = Pubkey::find_program_address(
            &[b"amm authority"],
            &program_ids::RAYDIUM_AMM_V4,
        );

        // NOTE: serum bids/asks/event_queue/vaults/vault_signer must be
        // fetched from the Serum market state (task 2.3). Placeholders used here.
        let accounts = vec![
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new(*pool, false),
            AccountMeta::new_readonly(amm_authority, false),
            AccountMeta::new(state.open_orders, false),
            AccountMeta::new(state.target_orders, false),
            AccountMeta::new(state.base_vault, false),
            AccountMeta::new(state.quote_vault, false),
            AccountMeta::new_readonly(state.market_program_id, false),
            AccountMeta::new(state.market_id, false),
            AccountMeta::new(Pubkey::default(), false), // serum_bids — fetch from market state (task 2.3)
            AccountMeta::new(Pubkey::default(), false), // serum_asks
            AccountMeta::new(Pubkey::default(), false), // serum_event_queue
            AccountMeta::new(Pubkey::default(), false), // serum_coin_vault
            AccountMeta::new(Pubkey::default(), false), // serum_pc_vault
            AccountMeta::new_readonly(Pubkey::default(), false), // serum_vault_signer
            AccountMeta::new(*ata_in, false),
            AccountMeta::new(*ata_out, false),
            AccountMeta::new_readonly(*owner, true),
        ];

        Ok(vec![Instruction {
            program_id: program_ids::RAYDIUM_AMM_V4,
            accounts,
            data: raydium_swap_data(amount_in, min_amount_out),
        }])
    }
}
