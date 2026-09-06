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
/// Read the fee payer and blockhash the resource server served in its 402.
///
/// Carried in `PaymentRequirements::extra` — which the x402 spec reserves for exactly
/// this, scheme-specific material a payer needs in order to construct a payment. Both are
/// required: without the fee payer the payer would put itself in that slot and need SOL,
/// which is the whole thing the facilitator exists to avoid, and without the blockhash
/// the transaction cannot land. A missing or malformed value is an error rather than a
/// default, because every available default here produces a payment that verifies locally
/// and can never be collected.
fn payment_context(requirements: &PaymentRequirements) -> Result<(Pubkey, solana_sdk::hash::Hash)> {
    let field = |name: &str| -> Result<&str> {
        requirements
            .extra
            .get(name)
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Payment requirements carry no `extra.{name}`; the resource server must serve one in its 402 response"
                )
            })
    };
    let fee_payer = Pubkey::from_str(field("feePayer")?)
        .map_err(|_| anyhow::anyhow!("Invalid extra.feePayer address"))?;
    let blockhash = solana_sdk::hash::Hash::from_str(field("recentBlockhash")?)
        .map_err(|_| anyhow::anyhow!("Invalid extra.recentBlockhash"))?;
    Ok((fee_payer, blockhash))
}

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

    // ## Sign the message that will actually be submitted
    //
    // This used to build with `Message::new(&ixs, None)` and sign over `Hash::default()`,
    // on the understanding that the facilitator would refresh the blockhash before
    // submitting. It does not work and cannot: a signature commits to the serialized
    // message, blockhash included, so rewriting it afterwards leaves a signature over a
    // message that no longer exists and the network rejects the transaction. `None` also
    // made the *payer* the fee payer, so the facilitator's key had no slot to sign into
    // and `partial_sign` panicked rather than erroring.
    //
    // Both are fixed by the server saying, in the 402, which fee payer to name and which
    // blockhash to build against. The payer then signs the exact bytes that get
    // submitted, which is what makes a verified payment a collectible one.
    let (fee_payer, recent_blockhash) = payment_context(requirements)?;
    let mut message = Message::new(&[cu_limit_ix, cu_price_ix, transfer_ix], Some(&fee_payer));
    message.recent_blockhash = recent_blockhash;
    let mut tx = Transaction::new_unsigned(message);
    tx.partial_sign(&[payer], recent_blockhash);

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
