use anyhow::{bail, Result};
use base64::Engine;
use solana_sdk::{pubkey::Pubkey, transaction::Transaction};
use spl_associated_token_account::get_associated_token_address;
use std::str::FromStr;
use tracing::debug;

use crate::types::{PaymentRequirements, SvmExactPayload, VerifyResponse};

pub fn verify(payload: &SvmExactPayload, requirements: &PaymentRequirements) -> VerifyResponse {
    match verify_inner(payload, requirements) {
        Ok(payer) => VerifyResponse {
            is_valid: true,
            invalid_reason: None,
            payer: Some(payer),
        },
        Err(e) => VerifyResponse {
            is_valid: false,
            invalid_reason: Some(e.to_string()),
            payer: None,
        },
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
        if prog_idx >= tx.message.account_keys.len() {
            continue;
        }
        if tx.message.account_keys[prog_idx] != token_prog {
            continue;
        }

        let token_ix = match spl_token::instruction::TokenInstruction::unpack(&ix.data) {
            Ok(i) => i,
            Err(_) => continue,
        };

        if let spl_token::instruction::TokenInstruction::TransferChecked { amount, .. } = token_ix {
            // accounts: [source, mint, destination, authority, ...]
            if ix.accounts.len() < 4 {
                bail!("TransferChecked instruction has fewer than 4 accounts");
            }
            let mint_idx = ix.accounts[1] as usize;
            let dest_idx = ix.accounts[2] as usize;
            let auth_idx = ix.accounts[3] as usize;

            let keys = &tx.message.account_keys;
            if mint_idx >= keys.len() || dest_idx >= keys.len() || auth_idx >= keys.len() {
                bail!("TransferChecked account index out of range");
            }

            // Mint must match required asset
            if keys[mint_idx] != asset_mint {
                bail!(
                    "Transfer mint {} != required asset {}",
                    keys[mint_idx],
                    asset_mint
                );
            }

            // Destination must be the ATA of (pay_to, asset)
            let expected_dest = get_associated_token_address(&pay_to, &asset_mint);
            if keys[dest_idx] != expected_dest {
                bail!(
                    "Transfer destination {} != expected ATA {} for payTo={}",
                    keys[dest_idx],
                    expected_dest,
                    pay_to
                );
            }

            // Amount must match exactly
            if amount != requirements.amount {
                bail!(
                    "Transfer amount {} != required amount {}",
                    amount,
                    requirements.amount
                );
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
            bail!(
                "Payer account index {} has no corresponding signature slot",
                idx
            );
        }
    }

    Ok(payer.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::build_payment_payload;
    use solana_sdk::{
        compute_budget::ComputeBudgetInstruction, message::Message, signature::Keypair,
        signer::Signer,
    };
    use spl_associated_token_account::get_associated_token_address;
    use spl_token::instruction::transfer_checked;

    // A real, well-known SPL mint (USDC) — verify never touches the chain, it only
    // needs valid base58 pubkeys.
    const USDC: &str = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
    const DECIMALS: u8 = 6;

    fn reqs(asset: &str, pay_to: &str, amount: u64) -> PaymentRequirements {
        PaymentRequirements {
            scheme: "exact".into(),
            network: "solana-mainnet".into(),
            asset: asset.into(),
            amount,
            pay_to: pay_to.into(),
            max_timeout_seconds: 120,
            extra: serde_json::Value::Null,
        }
    }

    /// The honest path: a payload built by our own client for the exact
    /// requirements must verify, and must report the real payer.
    #[test]
    fn valid_payload_verifies_and_returns_payer() {
        let payer = Keypair::new();
        let pay_to = Keypair::new().pubkey().to_string();
        let r = reqs(USDC, &pay_to, 1_000);

        let payload = build_payment_payload(&payer, &r, DECIMALS).unwrap();
        let resp = verify(&payload.payload, &r);

        assert!(resp.is_valid, "reason: {:?}", resp.invalid_reason);
        assert_eq!(
            resp.payer.as_deref(),
            Some(payer.pubkey().to_string().as_str())
        );
    }

    /// A payer who signed for 1_000 must not be able to satisfy a 2_000 charge —
    /// the amount is validated exactly, not as a floor.
    #[test]
    fn tampered_amount_is_rejected() {
        let payer = Keypair::new();
        let pay_to = Keypair::new().pubkey().to_string();
        // Client signs for the real amount…
        let payload = build_payment_payload(&payer, &reqs(USDC, &pay_to, 1_000), DECIMALS).unwrap();
        // …but the server demands more.
        let resp = verify(&payload.payload, &reqs(USDC, &pay_to, 2_000));

        assert!(!resp.is_valid);
        assert!(resp.invalid_reason.unwrap().contains("amount"));
    }

    /// Paying the wrong recipient (destination ATA belongs to someone else) must
    /// be caught — this is the "pay yourself" forgery.
    #[test]
    fn wrong_destination_is_rejected() {
        let payer = Keypair::new();
        let real_dest = Keypair::new().pubkey().to_string();
        let attacker_dest = Keypair::new().pubkey().to_string();

        // Payload directs funds to `real_dest`…
        let payload =
            build_payment_payload(&payer, &reqs(USDC, &real_dest, 1_000), DECIMALS).unwrap();
        // …but the resource server requires payment to `attacker_dest`.
        let resp = verify(&payload.payload, &reqs(USDC, &attacker_dest, 1_000));

        assert!(!resp.is_valid);
        assert!(resp.invalid_reason.unwrap().contains("destination"));
    }

    /// Paying in a different token than the one required must be rejected.
    #[test]
    fn wrong_asset_mint_is_rejected() {
        let payer = Keypair::new();
        let pay_to = Keypair::new().pubkey().to_string();
        let other_mint = Keypair::new().pubkey().to_string();

        let payload =
            build_payment_payload(&payer, &reqs(&other_mint, &pay_to, 1_000), DECIMALS).unwrap();
        let resp = verify(&payload.payload, &reqs(USDC, &pay_to, 1_000));

        assert!(!resp.is_valid);
        // Either the mint check or the derived-ATA check fires first; both are correct rejections.
        let reason = resp.invalid_reason.unwrap();
        assert!(reason.contains("mint") || reason.contains("destination"));
    }

    /// A transaction whose payer never signed must not settle — otherwise anyone
    /// could forge a transfer from a stranger's account.
    #[test]
    fn unsigned_transaction_is_rejected() {
        let payer = Keypair::new();
        let pay_to = Pubkey::new_unique();
        let asset = Pubkey::from_str(USDC).unwrap();

        let source = get_associated_token_address(&payer.pubkey(), &asset);
        let dest = get_associated_token_address(&pay_to, &asset);
        let ix = transfer_checked(
            &spl_token::id(),
            &source,
            &asset,
            &dest,
            &payer.pubkey(),
            &[],
            1_000,
            DECIMALS,
        )
        .unwrap();
        // 3 instructions so the count check passes, but we never sign.
        let msg = Message::new(
            &[
                ComputeBudgetInstruction::set_compute_unit_limit(50_000),
                ComputeBudgetInstruction::set_compute_unit_price(1),
                ix,
            ],
            None,
        );
        let tx = Transaction::new_unsigned(msg);
        let b64 =
            base64::engine::general_purpose::STANDARD.encode(bincode::serialize(&tx).unwrap());

        let resp = verify(
            &SvmExactPayload { transaction: b64 },
            &reqs(USDC, &pay_to.to_string(), 1_000),
        );

        assert!(!resp.is_valid);
        assert!(resp.invalid_reason.unwrap().contains("not signed"));
    }

    /// A transaction with no TransferChecked (e.g. only compute-budget ixs) or an
    /// out-of-range instruction count must be rejected, not silently accepted.
    #[test]
    fn too_few_instructions_is_rejected() {
        let payer = Keypair::new();
        let pay_to = Pubkey::new_unique();
        let asset = Pubkey::from_str(USDC).unwrap();
        let source = get_associated_token_address(&payer.pubkey(), &asset);
        let dest = get_associated_token_address(&pay_to, &asset);
        let ix = transfer_checked(
            &spl_token::id(),
            &source,
            &asset,
            &dest,
            &payer.pubkey(),
            &[],
            1_000,
            DECIMALS,
        )
        .unwrap();
        // Single instruction: below the SVM-spec minimum of 3.
        let msg = Message::new(&[ix], Some(&payer.pubkey()));
        let mut tx = Transaction::new_unsigned(msg);
        tx.partial_sign(&[&payer], solana_sdk::hash::Hash::default());
        let b64 =
            base64::engine::general_purpose::STANDARD.encode(bincode::serialize(&tx).unwrap());

        let resp = verify(
            &SvmExactPayload { transaction: b64 },
            &reqs(USDC, &pay_to.to_string(), 1_000),
        );

        assert!(!resp.is_valid);
        assert!(resp.invalid_reason.unwrap().contains("instruction"));
    }

    /// Garbage that isn't a base64 transaction must fail cleanly, not panic.
    #[test]
    fn malformed_transaction_is_rejected_not_panicked() {
        let resp = verify(
            &SvmExactPayload {
                transaction: "not-base64-!!!".into(),
            },
            &reqs(USDC, &Pubkey::new_unique().to_string(), 1_000),
        );
        assert!(!resp.is_valid);
    }
}
