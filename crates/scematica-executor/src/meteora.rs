use crate::SwapInstructionBuilder;
use anyhow::Result;
use async_trait::async_trait;
use scematica_core::{dex::program_ids, types::DexKind};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};

pub struct MeteoraBuilder;

impl MeteoraBuilder {
    pub fn new() -> Self {
        Self
    }
}

/// Meteora DLMM swap discriminator
const METEORA_SWAP_DISCRIMINATOR: [u8; 8] = [0xf8, 0xc6, 0x9e, 0x91, 0xe1, 0x75, 0x27, 0x44];

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
        token_out: &Pubkey,
        ata_in: &Pubkey,
        ata_out: &Pubkey,
        amount_in: u64,
        min_amount_out: u64,
    ) -> Result<Vec<Instruction>> {
        let mut data = METEORA_SWAP_DISCRIMINATOR.to_vec();
        data.extend_from_slice(&amount_in.to_le_bytes());
        data.extend_from_slice(&min_amount_out.to_le_bytes());
        data.push(1u8); // swap_for_y: true

        let accounts = vec![
            AccountMeta::new(*pool, false),
            AccountMeta::new(Pubkey::default(), false), // bin_array_bitmap_extension
            AccountMeta::new(Pubkey::default(), false), // reserve_x
            AccountMeta::new(Pubkey::default(), false), // reserve_y
            AccountMeta::new(*ata_in, false),
            AccountMeta::new(*ata_out, false),
            AccountMeta::new_readonly(*token_in, false),
            AccountMeta::new_readonly(*token_out, false),
            AccountMeta::new_readonly(Pubkey::default(), false), // oracle
            AccountMeta::new(Pubkey::default(), false), // bin_array_lower
            AccountMeta::new(Pubkey::default(), false), // bin_array_upper
            AccountMeta::new_readonly(*owner, true),
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
        ];

        Ok(vec![Instruction {
            program_id: program_ids::METEORA_DLMM,
            accounts,
            data,
        }])
    }
}
