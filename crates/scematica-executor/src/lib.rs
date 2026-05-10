pub mod raydium;
pub mod orca;
pub mod meteora;
pub mod jupiter;

use anyhow::Result;
use async_trait::async_trait;
use scematica_core::types::DexKind;
use solana_sdk::{instruction::Instruction, pubkey::Pubkey};

/// Trait for building DEX-specific swap instructions
#[async_trait]
pub trait SwapInstructionBuilder: Send + Sync {
    fn dex(&self) -> DexKind;

    /// Build swap instructions for this DEX
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
    ) -> Result<Vec<Instruction>>;
}

/// Factory: get the right builder for a DEX
pub fn get_builder(dex: DexKind) -> Option<Box<dyn SwapInstructionBuilder>> {
    match dex {
        DexKind::Raydium => Some(Box::new(raydium::RaydiumBuilder::new())),
        DexKind::Orca => Some(Box::new(orca::OrcaBuilder::new())),
        DexKind::Meteora => Some(Box::new(meteora::MeteoraBuilder::new())),
        DexKind::Jupiter => Some(Box::new(jupiter::JupiterBuilder::new())),
        _ => None,
    }
}
