use crate::SwapInstructionBuilder;
use anyhow::Result;
use async_trait::async_trait;
use scematica_core::{dex::program_ids, types::DexKind};
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};

/// Orca Whirlpool swap instruction builder
pub struct OrcaBuilder;

impl OrcaBuilder {
    pub fn new() -> Self {
        Self
    }
}

/// Orca Whirlpool swap discriminator: sha256("global:swap")[0..8]
const WHIRLPOOL_SWAP_DISCRIMINATOR: [u8; 8] = [0xf8, 0xc6, 0x9e, 0x91, 0xe1, 0x75, 0x27, 0x43];

fn orca_swap_data(amount: u64, other_amount_threshold: u64, sqrt_price_limit: u128, amount_specified_is_input: bool, a_to_b: bool) -> Vec<u8> {
    let mut data = WHIRLPOOL_SWAP_DISCRIMINATOR.to_vec();
    data.extend_from_slice(&amount.to_le_bytes());
    data.extend_from_slice(&other_amount_threshold.to_le_bytes());
    data.extend_from_slice(&sqrt_price_limit.to_le_bytes());
    data.push(amount_specified_is_input as u8);
    data.push(a_to_b as u8);
    data
}

#[async_trait]
impl SwapInstructionBuilder for OrcaBuilder {
    fn dex(&self) -> DexKind {
        DexKind::Orca
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
        // Orca Whirlpool swap accounts:
        // 0: token_program
        // 1: token_authority (owner)
        // 2: whirlpool (pool)
        // 3: token_owner_account_a (ata_in or ata_out depending on direction)
        // 4: token_vault_a
        // 5: token_owner_account_b
        // 6: token_vault_b
        // 7: tick_array_0
        // 8: tick_array_1
        // 9: tick_array_2
        // 10: oracle

        // Determine direction: a_to_b means token_in is token_a
        // In production, fetch whirlpool state to determine token_a/token_b
        let a_to_b = true; // placeholder

        // sqrt_price_limit: 0 means no limit (use max/min)
        let sqrt_price_limit: u128 = if a_to_b {
            4295048016u128 // MIN_SQRT_PRICE
        } else {
            79226673515401279992447579055u128 // MAX_SQRT_PRICE
        };

        let accounts = vec![
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new_readonly(*owner, true),
            AccountMeta::new(*pool, false),
            AccountMeta::new(*ata_in, false),
            AccountMeta::new(Pubkey::default(), false), // vault_a — from pool state
            AccountMeta::new(*ata_out, false),
            AccountMeta::new(Pubkey::default(), false), // vault_b
            AccountMeta::new(Pubkey::default(), false), // tick_array_0
            AccountMeta::new(Pubkey::default(), false), // tick_array_1
            AccountMeta::new(Pubkey::default(), false), // tick_array_2
            AccountMeta::new_readonly(Pubkey::default(), false), // oracle
        ];

        Ok(vec![Instruction {
            program_id: program_ids::ORCA_WHIRLPOOL,
            accounts,
            data: orca_swap_data(amount_in, min_amount_out, sqrt_price_limit, true, a_to_b),
        }])
    }
}
