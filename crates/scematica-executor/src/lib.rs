pub mod raydium;
pub mod orca;
pub mod meteora;
pub mod jupiter;
pub mod raydium_state;

use anyhow::Result;
use async_trait::async_trait;
use scematica_core::types::DexKind;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{instruction::Instruction, pubkey::Pubkey};
use std::sync::Arc;

/// Trait for decoding on-chain pool state
pub trait StateDecoder: Send + Sync {
    fn decode_pool_state(&self, data: &[u8]) -> Result<(u64, u64)>;
}

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

/// Factory: get the right builder for a DEX.
/// `rpc` will be forwarded to each builder once they accept `Arc<RpcClient>` (tasks 2–4).
pub fn get_builder(dex: DexKind, rpc: Arc<RpcClient>) -> Option<Box<dyn SwapInstructionBuilder>> {
    match dex {
        DexKind::Raydium => Some(Box::new(raydium::RaydiumBuilder::new(rpc.clone()))),
        DexKind::Orca => Some(Box::new(orca::OrcaBuilder::new(rpc.clone()))),
        DexKind::Meteora => Some(Box::new(meteora::MeteoraBuilder::new(rpc.clone()))),
        DexKind::Jupiter => Some(Box::new(jupiter::JupiterBuilder::new())),
        _ => None,
    }
}
