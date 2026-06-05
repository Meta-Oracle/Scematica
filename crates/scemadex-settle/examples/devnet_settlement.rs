//! End-to-end Conviction Routing with a **real devnet USDC slash transfer**.
//!
//!   cargo run -p scemadex-settle --example devnet_settlement
//!
//! Requires three env vars (the example prints setup steps and exits cleanly if
//! any are missing, so it is safe to run in CI):
//!
//!   SCEMADEX_AGENT_KEYPAIR    path to a funded devnet keypair JSON (SOL + USDC)
//!   SCEMADEX_USDC_MINT        your devnet SPL mint (e.g. `spl-token create-token`)
//!   SCEMADEX_BENEFICIARY_ATA  the caller's USDC token account (receives the slash)
//!
//! Setup on devnet:
//!   solana config set --url devnet
//!   solana airdrop 2
//!   spl-token create-token                 # -> USDC_MINT
//!   spl-token create-account <USDC_MINT>   # agent's ATA
//!   spl-token mint <USDC_MINT> 100         # fund the agent so it can be slashed
//!   # create a second account for the beneficiary and use its address as ATA

use std::str::FromStr;
use std::sync::Arc;

use scemadex_sdk::{demo_intent, Amount, BondEngine, Fill, ReferenceRoutePolicy, RoutePolicy};
use scemadex_settle::DevnetUsdcSettler;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::read_keypair_file;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (Ok(kp_path), Ok(mint), Ok(ata)) = (
        std::env::var("SCEMADEX_AGENT_KEYPAIR"),
        std::env::var("SCEMADEX_USDC_MINT"),
        std::env::var("SCEMADEX_BENEFICIARY_ATA"),
    ) else {
        eprintln!("Set SCEMADEX_AGENT_KEYPAIR, SCEMADEX_USDC_MINT, SCEMADEX_BENEFICIARY_ATA");
        eprintln!("(see the example header for devnet setup). Skipping live run.");
        return Ok(());
    };

    let agent = Arc::new(
        read_keypair_file(&kp_path)
            .map_err(|e| anyhow::anyhow!("read keypair {kp_path}: {e}"))?,
    );
    let usdc_mint = Pubkey::from_str(&mint)?;
    let beneficiary = Pubkey::from_str(&ata)?;

    let settler = DevnetUsdcSettler::devnet(agent, usdc_mint, beneficiary);
    println!("agent USDC account: {}", settler.agent_usdc_account());

    // Solve an intent and escrow a conviction-weighted bond.
    let solution = ReferenceRoutePolicy.solve(&demo_intent()).await?;
    let bond = settler.escrow(&solution).await?;
    println!(
        "escrowed bond: {} micro-USDC, guaranteed >= {} out (conviction {:.2})",
        bond.amount.0, bond.min_out_raw, solution.conviction.0
    );

    // Force a SLASH: deliver less than the guaranteed minimum output.
    let bad_fill = Fill {
        amount_out: Amount::new(bond.min_out_raw / 2, 6),
        executed_unix: 0,
    };
    println!("submitting an under-delivering fill -> expecting a slash + transfer...");
    let (outcome, sig) = settler.settle_onchain(&bond, &bad_fill).await?;
    println!("outcome: {outcome:?}");
    if let Some(sig) = sig {
        println!("devnet slash transfer: {sig}");
        println!("explorer: https://explorer.solana.com/tx/{sig}?cluster=devnet");
    }
    let l = settler.ledger();
    println!("ledger: {} honored / {} slashed", l.honored, l.slashed);
    Ok(())
}
