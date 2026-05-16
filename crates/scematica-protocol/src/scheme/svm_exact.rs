use anyhow::{bail, Result};
use base64::Engine;
use solana_sdk::{pubkey::Pubkey, transaction::Transaction};
use spl_associated_token_account::get_associated_token_address;
use std::str::FromStr;
use tracing::debug;

use crate::types::{PaymentRequirements, SvmExactPayload, VerifyResponse};

pub fn verify(payload: &SvmExactPayload, requirements: &PaymentRequirements) -> VerifyResponse {
    match verify_inner(payload, requirements) {
        Ok(payer) => VerifyResponse { is_valid: true, invalid_reason: None, payer: Some(payer) },
        Err(e) => VerifyResponse { is_valid: false, invalid_reason: Some(e.to_string()), payer: None },
    }
}

fn verify_inner(payload: &SvmExactPayload, requirements: &PaymentRequirements) -> Result<String> {
    // 1. Decode and deserialize the partially-signed transaction
    let tx_bytes = base64::engine::general_purpose::STANDARD
        .decode(&payload.transaction)
        .map_err(|_| anyhow::anyhow!("Invalid base64 encoding on transaction"))?;
    let tx: Transaction = bincode::deserialize(&tx_bytes)
        .map_err(|_| anyhow::anyhow!("Failed to deserialize Solana transaction"))?;

    let pay_to = Pubkey::from_str(&requirements.pay_to)
        .map_err(|_| anyhow::anyhow!("Invalid pay_to address: {}", requirements.pay_to))?;
    let asset_mint = Pubkey::from_str(&requirements.asset)
        .map_err(|_| anyhow::anyhow!("Invalid asset mint: {}", requirements.asset))?;

    // 2. Instruction count: SVM spec requires 3-6
    let n = tx.message.instructions.len();
    if !(3..=6).contains(&n) {
        bail!("Expected 3-6 instructions, got {}", n);
    }

    // 3. Locate and validate the SPL TransferChecked instruction
    let token_prog = spl_token::id();
    let mut transfer_validated = false;
    let mut payer_key: Option<Pubkey> = None;

    for ix in &tx.message.instructions {
        let prog_idx = ix.program_id_index as usize;
        if prog_idx >= tx.message.account_keys.len() { continue; }
        if tx.message.account_keys[prog_idx] != token_prog { continue; }

        let token_ix = match spl_token::instruction::TokenInstruction::unpack(&ix.data) {
            Ok(i) => i,
            Err(_) => continue,
        };

        if let spl_token::instruction::TokenInstruction::TransferChecked { amount, .. } = token_ix {
            // accounts: [source, mint, destination, authority, ...]
            if ix.accounts.len() < 4 {
                bail!("TransferChecked instruction has fewer than 4 accounts");
            }
            let mint_idx  = ix.accounts[1] as usize;
            let dest_idx  = ix.accounts[2] as usize;
            let auth_idx  = ix.accounts[3] as usize;

            let keys = &tx.message.account_keys;
            if mint_idx >= keys.len() || dest_idx >= keys.len() || auth_idx >= keys.len() {
                bail!("TransferChecked account index out of range");
            }

            // Mint must match required asset
            if keys[mint_idx] != asset_mint {
                bail!("Transfer mint {} != required asset {}", keys[mint_idx], asset_mint);
            }

            // Destination must be the ATA of (pay_to, asset)
            let expected_dest = get_associated_token_address(&pay_to, &asset_mint);
            if keys[dest_idx] != expected_dest {
                bail!(
                    "Transfer destination {} != expected ATA {} for payTo={}",
                    keys[dest_idx], expected_dest, pay_to
                );
            }

            // Amount must match exactly
            if amount != requirements.amount {
                bail!("Transfer amount {} != required amount {}", amount, requirements.amount);
            }

            payer_key = Some(keys[auth_idx]);
            transfer_validated = true;
            debug!(
                dest = %keys[dest_idx],
                amount,
                "TransferChecked validated"
            );
            break;
        }
    }

    if !transfer_validated {
        bail!("No valid SPL TransferChecked instruction found in transaction");
    }

    // 4. Verify the client actually signed (authority signature must not be default/zero)
    let payer = payer_key.ok_or_else(|| anyhow::anyhow!("Could not identify payer"))?;
    if let Some(idx) = tx.message.account_keys.iter().position(|k| k == &payer) {
        if idx < tx.signatures.len() {
            if tx.signatures[idx] == solana_sdk::signature::Signature::default() {
                bail!("Transaction is not signed by the payer authority");
            }
        } else {
            bail!("Payer account index {} has no corresponding signature slot", idx);
        }
    }

    Ok(payer.to_string())
}
