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
        token_out: &Pubkey,
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
        let _tick_spacing = u16::from_le_bytes([data[8 + 9], data[8 + 10]]);
        let _tick_current_index = i32::from_le_bytes([
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

        // ── Orca Whirlpool swap accounts (Req 4.12) ──────────────────────────
        // 0:  token_program       (readonly)
        // 1:  token_authority     (signer)
        // 2:  whirlpool           (writable)
        // 3:  token_owner_acct_a  (writable)
        // 4:  token_vault_a       (writable)
        // 5:  token_owner_acct_b  (writable)
        // 6:  token_vault_b       (writable)
        // 7:  tick_array_0        (writable) — placeholder until task 3.3
        // 8:  tick_array_1        (writable) — placeholder until task 3.3
        // 9:  tick_array_2        (writable) — placeholder until task 3.3
        // 10: oracle              (readonly) — placeholder until task 3.3
        let accounts = vec![
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new_readonly(*owner, true),
            AccountMeta::new(*pool, false),
            AccountMeta::new(*user_token_a, false),
            AccountMeta::new(token_vault_a, false),
            AccountMeta::new(*user_token_b, false),
            AccountMeta::new(token_vault_b, false),
            AccountMeta::new(Pubkey::default(), false), // tick_array_0 — task 3.3
            AccountMeta::new(Pubkey::default(), false), // tick_array_1 — task 3.3
            AccountMeta::new(Pubkey::default(), false), // tick_array_2 — task 3.3
            AccountMeta::new_readonly(Pubkey::default(), false), // oracle — task 3.3
        ];

        Ok(vec![Instruction {
            program_id: program_ids::ORCA_WHIRLPOOL,
            accounts,
            data: orca_swap_data(amount_in, min_amount_out, sqrt_price_limit, true, a_to_b),
        }])
    }
}
