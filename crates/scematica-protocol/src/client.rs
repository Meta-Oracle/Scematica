/// Client-side helpers for building Scematica Protocol payment payloads.
///
/// Usage:
///   1. Make a request; get back a 402 with `PaymentRequired` body.
///   2. Pick an entry from `accepts` that your wallet supports.
///   3. Call `build_payment_payload` to create a signed partial transaction.
///   4. Base64-encode the `PaymentPayload` JSON and put it in the `X-Payment` header.
///   5. Retry the original request.
use anyhow::{bail, Result};
use base64::Engine;
use solana_sdk::{
    compute_budget::ComputeBudgetInstruction, message::Message, pubkey::Pubkey, signature::Keypair,
    signer::Signer, transaction::Transaction,
};
use spl_associated_token_account::get_associated_token_address;
use spl_token::instruction::transfer_checked;
use std::str::FromStr;

use crate::types::{PaymentPayload, PaymentRequirements, SvmExactPayload, X402_VERSION};

/// Build a partially-signed payment transaction for the SVM exact scheme.
///
/// The transaction includes:
///   1. ComputeBudget: SetComputeUnitLimit (50_000)
///   2. ComputeBudget: SetComputeUnitPrice (1 microlamport)
///   3. SPL Token: TransferChecked (payer → payTo ATA, exact amount)
///
/// The fee payer slot is left empty (zero pubkey); the facilitator fills it at settlement.
pub fn build_payment_payload(
    payer: &Keypair,
    requirements: &PaymentRequirements,
    token_decimals: u8,
) -> Result<PaymentPayload> {
    let payer_pubkey = payer.pubkey();
    let asset_mint = Pubkey::from_str(&requirements.asset)
        .map_err(|_| anyhow::anyhow!("Invalid asset mint: {}", requirements.asset))?;
    let pay_to = Pubkey::from_str(&requirements.pay_to)
        .map_err(|_| anyhow::anyhow!("Invalid pay_to address: {}", requirements.pay_to))?;

    let source_ata = get_associated_token_address(&payer_pubkey, &asset_mint);
    let dest_ata = get_associated_token_address(&pay_to, &asset_mint);

    // Compute budget to keep fees within spec bounds
    let cu_limit_ix = ComputeBudgetInstruction::set_compute_unit_limit(50_000);
    let cu_price_ix = ComputeBudgetInstruction::set_compute_unit_price(1);

    // SPL TransferChecked
    let transfer_ix = transfer_checked(
        &spl_token::id(),
        &source_ata,
        &asset_mint,
        &dest_ata,
        &payer_pubkey,
        &[],
        requirements.amount,
        token_decimals,
    )?;

    // Build with a dummy recent blockhash — facilitator refreshes it before submission
    let message = Message::new(
        &[cu_limit_ix, cu_price_ix, transfer_ix],
        None, // fee payer left empty for facilitator to fill
    );
    let mut tx = Transaction::new_unsigned(message);
    tx.partial_sign(&[payer], solana_sdk::hash::Hash::default());

    let tx_bytes = bincode::serialize(&tx)?;
    let tx_b64 = base64::engine::general_purpose::STANDARD.encode(&tx_bytes);

    if requirements.scheme != "exact" {
        bail!(
            "Only 'exact' scheme is supported; got '{}'",
            requirements.scheme
        );
    }

    Ok(PaymentPayload {
        x402_version: X402_VERSION,
        scheme: "exact".into(),
        network: requirements.network.clone(),
        payload: SvmExactPayload {
            transaction: tx_b64,
        },
    })
}

/// Encode a `PaymentPayload` as the value for the `X-Payment` HTTP header.
pub fn encode_payment_header(payload: &PaymentPayload) -> Result<String> {
    let json = serde_json::to_vec(payload)?;
    Ok(base64::engine::general_purpose::STANDARD.encode(&json))
}
