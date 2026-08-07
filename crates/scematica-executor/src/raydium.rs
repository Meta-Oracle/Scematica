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
        let mut slice = data;
        let state = RaydiumAmmV4::deserialize(&mut slice)?;
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

/// Serum/OpenBook V1 market state layout (verified against openbook-dex/openbook-v1 state.rs).
///
/// [0..5]    padding ("serum" magic)
/// [5..13]   accountFlags        u64
/// [13..45]  ownAddress          Pubkey
/// [45..53]  vaultSignerNonce    u64
/// [53..85]  baseMint            Pubkey
/// [85..117] quoteMint           Pubkey
/// [117..149] baseVault          Pubkey
/// [149..157] baseDepositsTotal  u64
/// [157..165] baseFeesAccrued    u64
/// [165..197] quoteVault         Pubkey
/// [197..205] quoteDepositsTotal u64
/// [205..213] quoteFeesAccrued   u64
/// [213..221] quoteDustThreshold u64
/// [221..253] requestQueue       Pubkey
/// [253..285] eventQueue         Pubkey
/// [285..317] bids               Pubkey
/// [317..349] asks               Pubkey
/// [349..357] baseLotSize        u64
/// [357..365] quoteLotSize       u64
/// [365..373] feeRateBps         u64
/// [373..381] referrerRebatesAccrued u64
/// [381..388] trailing padding (7 bytes)  — total 388 bytes
mod serum_offsets {
    pub const VAULT_SIGNER_NONCE: usize = 45;
    pub const BASE_VAULT: usize = 117;
    pub const PC_VAULT: usize = 165;
    pub const EVENT_QUEUE: usize = 253;
    pub const BIDS: usize = 285;
    pub const ASKS: usize = 317;
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
        use scematica_core::dex::raydium_v4 as offsets;

        // Fetch pool state — 752 bytes on-chain
        let pool_data = self.rpc.get_account_data(pool).await
            .map_err(|e| anyhow::anyhow!("RPC error fetching pool {}: {}", pool, e))?;

        if pool_data.len() < offsets::POOL_STATE_SIZE {
            anyhow::bail!("pool data too short: {} bytes for {}", pool_data.len(), pool);
        }

        // Read status (u64 LE at byte 0)
        let status = u64::from_le_bytes(pool_data[0..8].try_into()?);
        if status == 0 {
            anyhow::bail!("pool {} is uninitialized", pool);
        }

        // Read pool accounts directly from verified byte offsets (avoids Borsh struct misalignment)
        let pool_base_vault   = read_pubkey(&pool_data, offsets::BASE_VAULT_OFFSET)?;
        let pool_quote_vault  = read_pubkey(&pool_data, offsets::QUOTE_VAULT_OFFSET)?;
        let open_orders       = read_pubkey(&pool_data, offsets::OPEN_ORDERS_OFFSET)?;
        let target_orders     = read_pubkey(&pool_data, offsets::TARGET_ORDERS_OFFSET)?;
        let market_id         = read_pubkey(&pool_data, offsets::MARKET_ID_OFFSET)?;
        let market_program_id = read_pubkey(&pool_data, offsets::MARKET_PROGRAM_OFFSET)?;

        // Fetch OpenBook/Serum market state
        let market_data = self.rpc.get_account_data(&market_id).await
            .map_err(|e| anyhow::anyhow!("RPC error fetching market {}: {}", market_id, e))?;

        const MIN_MARKET_LEN: usize = serum_offsets::PC_VAULT + 32;
        if market_data.len() < MIN_MARKET_LEN {
            anyhow::bail!("market data too short: {} < {} for market {}", market_data.len(), MIN_MARKET_LEN, market_id);
        }

        let bids        = read_pubkey(&market_data, serum_offsets::BIDS)?;
        let asks        = read_pubkey(&market_data, serum_offsets::ASKS)?;
        let event_queue = read_pubkey(&market_data, serum_offsets::EVENT_QUEUE)?;
        let mkt_base_vault = read_pubkey(&market_data, serum_offsets::BASE_VAULT)?;
        let mkt_pc_vault   = read_pubkey(&market_data, serum_offsets::PC_VAULT)?;

        let nonce_bytes: [u8; 8] = market_data[serum_offsets::VAULT_SIGNER_NONCE
            ..serum_offsets::VAULT_SIGNER_NONCE + 8]
            .try_into()
            .map_err(|_| anyhow::anyhow!("failed to read vault signer nonce"))?;
        let vault_signer_nonce = u64::from_le_bytes(nonce_bytes);
        let vault_signer = derive_vault_signer(&market_id, vault_signer_nonce, &market_program_id)?;

        let (amm_authority, _) = Pubkey::find_program_address(
            &[b"amm authority"],
            &program_ids::RAYDIUM_AMM_V4,
        );

        let accounts = vec![
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new(*pool, false),
            AccountMeta::new_readonly(amm_authority, false),
            AccountMeta::new(open_orders, false),
            AccountMeta::new(target_orders, false),
            AccountMeta::new(pool_base_vault, false),
            AccountMeta::new(pool_quote_vault, false),
            AccountMeta::new_readonly(market_program_id, false),
            AccountMeta::new(market_id, false),
            AccountMeta::new(bids, false),
            AccountMeta::new(asks, false),
            AccountMeta::new(event_queue, false),
            AccountMeta::new(mkt_base_vault, false),
            AccountMeta::new(mkt_pc_vault, false),
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
