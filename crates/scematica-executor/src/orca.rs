use crate::SwapInstructionBuilder;
use anyhow::Result;
use async_trait::async_trait;
use scematica_core::{dex::program_ids, types::DexKind};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    instruction::{AccountMeta, Instruction},
    pubkey::Pubkey,
};
use std::sync::Arc;

/// Number of ticks per tick array in Orca Whirlpool
const TICK_ARRAY_SIZE: i32 = 88;

/// Derive a Whirlpool tick array PDA for a given start tick index.
fn derive_tick_array_pda(whirlpool: &Pubkey, start_tick_index: i32) -> Pubkey {
    let start_bytes = start_tick_index.to_string();
    let (pda, _) = Pubkey::find_program_address(
        &[b"tick_array", whirlpool.as_ref(), start_bytes.as_bytes()],
        &program_ids::ORCA_WHIRLPOOL,
    );
    pda
}

/// Round a tick index down to the nearest tick array start index.
fn tick_array_start_index(tick_index: i32, tick_spacing: u16) -> i32 {
    let ticks_in_array = TICK_ARRAY_SIZE * tick_spacing as i32;
    tick_index.div_euclid(ticks_in_array) * ticks_in_array
}

/// Derive the oracle PDA for a Whirlpool.
fn derive_oracle_pda(whirlpool: &Pubkey) -> Pubkey {
    let (pda, _) = Pubkey::find_program_address(
        &[b"oracle", whirlpool.as_ref()],
        &program_ids::ORCA_WHIRLPOOL,
    );
    pda
}

/// Orca Whirlpool swap instruction builder
pub struct OrcaBuilder {
    rpc: Arc<RpcClient>,
}

impl OrcaBuilder {
    pub fn new(rpc: Arc<RpcClient>) -> Self {
        Self { rpc }
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
        _token_out: &Pubkey,
        ata_in: &Pubkey,
        ata_out: &Pubkey,
        amount_in: u64,
        min_amount_out: u64,
    ) -> Result<Vec<Instruction>> {
        // ── Fetch Whirlpool state (Req 4.2, 4.3) ────────────────────────────
        let data = self
            .rpc
            .get_account_data(pool)
            .await
            .map_err(|e| anyhow::anyhow!("OrcaBuilder: RPC error fetching pool {}: {}", pool, e))?;

        if data.len() < 272 {
            anyhow::bail!(
                "OrcaBuilder: Whirlpool data too short: {} < 272 bytes",
                data.len()
            );
        }

        // ── Extract fields (Req 4.4) ─────────────────────────────────────────
        // Layout (Anchor-serialized, 8-byte discriminator prefix):
        //   8+0  : whirlpools_config (Pubkey, 32)
        //   8+32 : whirlpool_bump ([u8; 1])
        //   8+33 : padding / whirlpool_bump_seed
        //   8+9  : tick_spacing (u16)  — NOTE: offset within the struct body
        //   8+37 : tick_current_index (i32)
        //   8+101: token_mint_a (Pubkey, 32)
        //   8+133: token_vault_a (Pubkey, 32)
        //   8+181: token_mint_b (Pubkey, 32)
        //   8+213: token_vault_b (Pubkey, 32)
        // tick_spacing and tick_current_index are used in task 3.3 for tick array PDA derivation
        let tick_spacing = u16::from_le_bytes([data[8 + 9], data[8 + 10]]);
        let tick_current_index = i32::from_le_bytes([
            data[8 + 37],
            data[8 + 38],
            data[8 + 39],
            data[8 + 40],
        ]);

        let token_mint_a = Pubkey::try_from(&data[8 + 101..8 + 133])
            .map_err(|_| anyhow::anyhow!("OrcaBuilder: failed to parse token_mint_a"))?;
        let token_vault_a = Pubkey::try_from(&data[8 + 133..8 + 165])
            .map_err(|_| anyhow::anyhow!("OrcaBuilder: failed to parse token_vault_a"))?;
        let token_mint_b = Pubkey::try_from(&data[8 + 181..8 + 213])
            .map_err(|_| anyhow::anyhow!("OrcaBuilder: failed to parse token_mint_b"))?;
        let token_vault_b = Pubkey::try_from(&data[8 + 213..8 + 245])
            .map_err(|_| anyhow::anyhow!("OrcaBuilder: failed to parse token_vault_b"))?;

        // ── Determine swap direction (Req 4.5, 4.6) ─────────────────────────
        let a_to_b = if token_in == &token_mint_a {
            true
        } else if token_in == &token_mint_b {
            false
        } else {
            anyhow::bail!(
                "OrcaBuilder: token_in {} matches neither token_mint_a {} nor token_mint_b {}",
                token_in,
                token_mint_a,
                token_mint_b
            );
        };

        // ── sqrt_price_limit (Req 4.7, 4.8) ─────────────────────────────────
        let sqrt_price_limit: u128 = if a_to_b {
            4295048016u128 // MIN_SQRT_PRICE
        } else {
            79226673515401279992447579055u128 // MAX_SQRT_PRICE
        };

        // ── User token accounts depend on direction (Req 4.11) ───────────────
        let (user_token_a, user_token_b) = if a_to_b {
            (ata_in, ata_out)
        } else {
            (ata_out, ata_in)
        };

        // Derive tick arrays: 3 consecutive arrays starting from current tick, in swap direction.
        let start_0 = tick_array_start_index(tick_current_index, tick_spacing);
        let ticks_in_array = TICK_ARRAY_SIZE * tick_spacing as i32;
        let (start_1, start_2) = if a_to_b {
            (start_0 - ticks_in_array, start_0 - 2 * ticks_in_array)
        } else {
            (start_0 + ticks_in_array, start_0 + 2 * ticks_in_array)
        };
        let tick_array_0 = derive_tick_array_pda(pool, start_0);
        let tick_array_1 = derive_tick_array_pda(pool, start_1);
        let tick_array_2 = derive_tick_array_pda(pool, start_2);
        let oracle = derive_oracle_pda(pool);

        // Orca Whirlpool swap accounts
        let accounts = vec![
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new_readonly(*owner, true),
            AccountMeta::new(*pool, false),
            AccountMeta::new(*user_token_a, false),
            AccountMeta::new(token_vault_a, false),
            AccountMeta::new(*user_token_b, false),
            AccountMeta::new(token_vault_b, false),
            AccountMeta::new(tick_array_0, false),
            AccountMeta::new(tick_array_1, false),
            AccountMeta::new(tick_array_2, false),
            AccountMeta::new_readonly(oracle, false),
        ];

        Ok(vec![Instruction {
            program_id: program_ids::ORCA_WHIRLPOOL,
            accounts,
            data: orca_swap_data(amount_in, min_amount_out, sqrt_price_limit, true, a_to_b),
        }])
    }
}
