use crate::SwapInstructionBuilder;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use scematica_core::{dex::program_ids, types::DexKind};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};
use std::sync::Arc;

pub struct MeteoraBuilder {
    pub rpc: Arc<RpcClient>,
}

impl MeteoraBuilder {
    pub fn new(rpc: Arc<RpcClient>) -> Self {
        Self { rpc }
    }
}

/// Meteora DLMM swap discriminator: sha256("global:swap")[0..8]
/// Correct value is [0xf8, 0xc6, 0x9e, 0x91, 0xe1, 0x75, 0x27, 0x43]
const METEORA_SWAP_DISCRIMINATOR: [u8; 8] = [0xf8, 0xc6, 0x9e, 0x91, 0xe1, 0x75, 0x27, 0x43];

/// Minimum byte length required to decode all fields from the LbPair account.
/// bin_array_bitmap_extension is at offset 802 and is 32 bytes, so we need at least 834 bytes.
const DLMM_MIN_DATA_LEN: usize = 834;

/// Byte offsets within the LbPair account data (after the 8-byte Anchor discriminator).
/// All offsets are absolute (from the start of the account data, including the discriminator).
mod offsets {
    pub const ACTIVE_ID: usize = 76;       // i32
    pub const BIN_STEP: usize = 80;        // u16
    pub const TOKEN_X_MINT: usize = 88;    // Pubkey (32 bytes)
    pub const TOKEN_Y_MINT: usize = 120;   // Pubkey (32 bytes)
    pub const RESERVE_X: usize = 152;      // Pubkey (32 bytes)
    pub const RESERVE_Y: usize = 184;      // Pubkey (32 bytes)
    pub const ORACLE: usize = 488;         // Pubkey (32 bytes)
    pub const BIN_ARRAY_BITMAP_EXTENSION: usize = 802; // Pubkey (32 bytes)
}

/// Derive a Meteora DLMM bin array PDA for a given pool and bin array index.
fn derive_bin_array_pda(pool: &Pubkey, index: i32, program_id: &Pubkey) -> Result<Pubkey> {
    let index_bytes = index.to_le_bytes();
    let (pda, _) = Pubkey::find_program_address(
        &[b"bin_array", pool.as_ref(), &index_bytes],
        program_id,
    );
    Ok(pda)
}
fn read_pubkey(data: &[u8], offset: usize) -> Result<Pubkey> {
    Pubkey::try_from(&data[offset..offset + 32])
        .map_err(|_| anyhow!("failed to read Pubkey at offset {}", offset))
}

/// Decode an `i32` from a 4-byte little-endian slice at the given offset.
fn read_i32(data: &[u8], offset: usize) -> Result<i32> {
    let bytes: [u8; 4] = data[offset..offset + 4]
        .try_into()
        .map_err(|_| anyhow!("failed to read i32 at offset {}", offset))?;
    Ok(i32::from_le_bytes(bytes))
}

/// Decode a `u16` from a 2-byte little-endian slice at the given offset.
fn read_u16(data: &[u8], offset: usize) -> Result<u16> {
    let bytes: [u8; 2] = data[offset..offset + 2]
        .try_into()
        .map_err(|_| anyhow!("failed to read u16 at offset {}", offset))?;
    Ok(u16::from_le_bytes(bytes))
}

#[async_trait]
impl SwapInstructionBuilder for MeteoraBuilder {
    fn dex(&self) -> DexKind {
        DexKind::Meteora
    }

    async fn build_swap(
        &self,
        pool: &Pubkey,
        owner: &Pubkey,
        token_in: &Pubkey,
        _token_out: &Pubkey,
        ata_in: &Pubkey,
        ata_out: &Pubkey,
        amount_in: u64,
        min_amount_out: u64,
    ) -> Result<Vec<Instruction>> {
        // Req 6.2: fetch DLMM LbPair account data
        let data = self
            .rpc
            .get_account_data(pool)
            .await
            .map_err(|e| anyhow!("RPC error fetching DLMM pool {}: {}", pool, e))?;

        // Req 6.3: validate minimum data length
        if data.len() < DLMM_MIN_DATA_LEN {
            return Err(anyhow!(
                "DLMM pool data too short: {} < {} bytes",
                data.len(),
                DLMM_MIN_DATA_LEN
            ));
        }

        // Req 6.4: extract fields from the Anchor-serialized layout (8-byte discriminator already
        // accounted for in the offset constants above)
        let active_id = read_i32(&data, offsets::ACTIVE_ID)?;
        let _bin_step = read_u16(&data, offsets::BIN_STEP)?;
        let token_x_mint = read_pubkey(&data, offsets::TOKEN_X_MINT)?;
        let token_y_mint = read_pubkey(&data, offsets::TOKEN_Y_MINT)?;
        let reserve_x = read_pubkey(&data, offsets::RESERVE_X)?;
        let reserve_y = read_pubkey(&data, offsets::RESERVE_Y)?;
        let oracle = read_pubkey(&data, offsets::ORACLE)?;
        let bin_array_bitmap_extension = read_pubkey(&data, offsets::BIN_ARRAY_BITMAP_EXTENSION)?;

        // Req 6.5 / 6.6: determine swap direction; error if token_in matches neither mint
        let swap_for_y = if token_in == &token_x_mint {
            true
        } else if token_in == &token_y_mint {
            false
        } else {
            return Err(anyhow!(
                "token_in {} matches neither token_x_mint {} nor token_y_mint {} for DLMM pool {}",
                token_in,
                token_x_mint,
                token_y_mint,
                pool
            ));
        };

        // Derive bin array PDAs from active_id and bin_step.
        // Each bin array covers BIN_ARRAY_SIZE (70) bins.
        // lower covers the active bin, upper covers the next array in swap direction.
        const BIN_ARRAY_SIZE: i32 = 70;
        let lower_idx = active_id.div_euclid(BIN_ARRAY_SIZE);
        let upper_idx = if swap_for_y { lower_idx - 1 } else { lower_idx + 1 };

        let bin_array_lower = derive_bin_array_pda(pool, lower_idx, &program_ids::METEORA_DLMM)?;
        let bin_array_upper = derive_bin_array_pda(pool, upper_idx, &program_ids::METEORA_DLMM)?;

        // Encode instruction data (25 bytes total per Req 7.4 / 11.3):
        //   [0..8]   METEORA_SWAP_DISCRIMINATOR
        //   [8..16]  amount_in (u64 LE)
        //   [16..24] min_amount_out (u64 LE)
        //   [24]     swap_for_y (1u8 = true, 0u8 = false)
        let mut ix_data = METEORA_SWAP_DISCRIMINATOR.to_vec();
        ix_data.extend_from_slice(&amount_in.to_le_bytes());
        ix_data.extend_from_slice(&min_amount_out.to_le_bytes());
        ix_data.push(swap_for_y as u8);

        // Account order (14 accounts per Req 7.2 / design):
        //  0: pool                        writable
        //  1: bin_array_bitmap_extension  writable
        //  2: reserve_x                   writable
        //  3: reserve_y                   writable
        //  4: ata_in                      writable
        //  5: ata_out                     writable
        //  6: token_x_mint                readonly
        //  7: token_y_mint                readonly
        //  8: oracle                      readonly
        //  9: bin_array_lower             writable
        // 10: bin_array_upper             writable
        // 11: owner                       signer
        // 12: spl_token                   readonly
        // 13: spl_token                   readonly
        let accounts = vec![
            AccountMeta::new(*pool, false),
            AccountMeta::new(bin_array_bitmap_extension, false),
            AccountMeta::new(reserve_x, false),
            AccountMeta::new(reserve_y, false),
            AccountMeta::new(*ata_in, false),
            AccountMeta::new(*ata_out, false),
            AccountMeta::new_readonly(token_x_mint, false),
            AccountMeta::new_readonly(token_y_mint, false),
            AccountMeta::new_readonly(oracle, false),
            AccountMeta::new(bin_array_lower, false),
            AccountMeta::new(bin_array_upper, false),
            AccountMeta::new_readonly(*owner, true),
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
        ];

        Ok(vec![Instruction {
            program_id: program_ids::METEORA_DLMM,
            accounts,
            data: ix_data,
        }])
    }
}
