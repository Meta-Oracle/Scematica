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
        Ok((state.lp_reserve, 0))
    }
}

/// Raydium V4 swap instruction data: discriminator 9 + amount_in + min_amount_out
fn raydium_swap_data(amount_in: u64, min_amount_out: u64) -> Vec<u8> {
    let mut data = vec![9u8];
    data.extend_from_slice(&amount_in.to_le_bytes());
    data.extend_from_slice(&min_amount_out.to_le_bytes());
    data
}

/// Serum/OpenBook market state layout offsets (little-endian, after 5-byte header).
/// Reference: https://github.com/openbook-dex/openbook-v1/blob/master/dex/src/state.rs
mod serum_offsets {
    pub const HEADER: usize = 5;          // "serum" magic prefix
    pub const BIDS: usize = 5 + 5 * 8 + 32 * 5;      // offset 197
    pub const ASKS: usize = BIDS + 32;                 // offset 229
    pub const EVENT_QUEUE: usize = ASKS + 32;          // offset 261
    pub const BASE_VAULT: usize = EVENT_QUEUE + 32;    // offset 293
    pub const PC_VAULT: usize = BASE_VAULT + 32;       // offset 325
    pub const VAULT_SIGNER_NONCE: usize = HEADER + 8; // offset 13 (u64)
}

/// Parse a Pubkey from a byte slice at the given offset.
fn read_pubkey(data: &[u8], offset: usize) -> Result<Pubkey> {
    Pubkey::try_from(&data[offset..offset + 32])
        .map_err(|_| anyhow::anyhow!("failed to read Pubkey at serum offset {}", offset))
}

/// Derive the Serum vault signer PDA from the market address and nonce.
fn derive_vault_signer(market: &Pubkey, nonce: u64, market_program: &Pubkey) -> Result<Pubkey> {
    let nonce_bytes = nonce.to_le_bytes();
    // Serum vault signer uses create_program_address (not find), nonce is the bump
    Pubkey::create_program_address(
        &[market.as_ref(), &nonce_bytes],
        market_program,
    ).map_err(|e| anyhow::anyhow!("vault signer derivation failed: {}", e))
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
        _token_in: &Pubkey,
        _token_out: &Pubkey,
        ata_in: &Pubkey,
        ata_out: &Pubkey,
        amount_in: u64,
        min_amount_out: u64,
    ) -> Result<Vec<Instruction>> {
        // Fetch and decode pool state
        let pool_data = self.rpc.get_account_data(pool).await
            .map_err(|e| anyhow::anyhow!("RPC error fetching pool {}: {}", pool, e))?;

        if pool_data.len() < RaydiumAmmV4::LEN {
            anyhow::bail!("pool data too short: {} < {} for pool {}", pool_data.len(), RaydiumAmmV4::LEN, pool);
        }

        let state = RaydiumAmmV4::try_from_slice(&pool_data[..RaydiumAmmV4::LEN])?;

        if state.status == 0 {
            anyhow::bail!("pool {} is uninitialized (status == 0)", pool);
        }

        // Fetch and decode Serum market state to get bids/asks/event_queue/vaults/vault_signer
        let market_data = self.rpc.get_account_data(&state.market_id).await
            .map_err(|e| anyhow::anyhow!("RPC error fetching market {}: {}", state.market_id, e))?;

        const MIN_MARKET_LEN: usize = serum_offsets::PC_VAULT + 32;
        if market_data.len() < MIN_MARKET_LEN {
            anyhow::bail!("market data too short: {} < {}", market_data.len(), MIN_MARKET_LEN);
        }

        let bids          = read_pubkey(&market_data, serum_offsets::BIDS)?;
        let asks          = read_pubkey(&market_data, serum_offsets::ASKS)?;
        let event_queue   = read_pubkey(&market_data, serum_offsets::EVENT_QUEUE)?;
        let base_vault    = read_pubkey(&market_data, serum_offsets::BASE_VAULT)?;
        let pc_vault      = read_pubkey(&market_data, serum_offsets::PC_VAULT)?;

        // Vault signer nonce is a u64 at offset 13 (after 5-byte header + 8-byte account flags)
        let nonce_bytes: [u8; 8] = market_data[serum_offsets::VAULT_SIGNER_NONCE
            ..serum_offsets::VAULT_SIGNER_NONCE + 8]
            .try_into()
            .map_err(|_| anyhow::anyhow!("failed to read vault signer nonce"))?;
        let vault_signer_nonce = u64::from_le_bytes(nonce_bytes);
        let vault_signer = derive_vault_signer(&state.market_id, vault_signer_nonce, &state.market_program_id)?;

        let (amm_authority, _) = Pubkey::find_program_address(
            &[b"amm authority"],
            &program_ids::RAYDIUM_AMM_V4,
        );

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
            AccountMeta::new(bids, false),
            AccountMeta::new(asks, false),
            AccountMeta::new(event_queue, false),
            AccountMeta::new(base_vault, false),
            AccountMeta::new(pc_vault, false),
            AccountMeta::new_readonly(vault_signer, false),
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
