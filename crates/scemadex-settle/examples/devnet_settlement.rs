//! Live devnet settling node — **one command produces a real on-chain bond slash**.
//!
//! The moment you have a funded devnet keypair (SOL + an SPL "USDC" mint) and a
//! beneficiary token account, this drives the full Conviction-Routing loop and
//! settles a slashed bond on-chain, printing the explorer link.
//!
//!   cargo run -p scemadex-settle --example devnet_settlement -- \
//!     --keypair agent.json \
//!     --usdc-mint <DEVNET_MINT> \
//!     --beneficiary <CALLER_USDC_TOKEN_ACCOUNT> \
//!     [--mode slash|honor]
//!
//! Devnet setup (once):
//!   solana config set --url devnet
//!   solana airdrop 2
//!   spl-token create-token                 # -> <DEVNET_MINT>
//!   spl-token create-account <MINT>        # agent's ATA
//!   spl-token mint <MINT> 100              # fund the agent so a bond can be paid
//!   # create a second token account for the beneficiary; pass its address below

use std::str::FromStr;
use std::sync::Arc;

use clap::Parser;
use scemadex_sdk::{
    demo_intent, Amount, BondEngine, Fill, ReferenceRoutePolicy, RoutePolicy,
};
use scemadex_settle::DevnetUsdcSettler;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{read_keypair_file, Signer};
use spl_associated_token_account::get_associated_token_address;

#[derive(Parser)]
#[command(about = "ScemaDEX devnet settling node — settles a Conviction-Routing bond on-chain")]
struct Args {
    /// Agent keypair JSON (funded with devnet SOL + USDC).
    #[arg(long)]
    keypair: String,
    /// Devnet SPL mint used as USDC.
    #[arg(long)]
    usdc_mint: String,
    /// Caller's USDC token account — receives a slashed bond.
    #[arg(long)]
    beneficiary: String,
    /// RPC endpoint.
    #[arg(long, default_value = "https://api.devnet.solana.com")]
    rpc_url: String,
    /// `slash` under-delivers the fill → real on-chain USDC transfer.
    /// `honor` meets the guarantee → nothing moves.
    #[arg(long, default_value = "slash")]
    mode: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let slash = args.mode != "honor";

    let agent = Arc::new(
        read_keypair_file(&args.keypair)
            .map_err(|e| anyhow::anyhow!("read keypair {}: {e}", args.keypair))?,
    );
    let usdc_mint = Pubkey::from_str(&args.usdc_mint)?;
    let beneficiary = Pubkey::from_str(&args.beneficiary)?;
    let agent_pk = agent.pubkey();
    let agent_ata = get_associated_token_address(&agent_pk, &usdc_mint);

    // ── Preflight ──────────────────────────────────────────────────────────
    println!("── ScemaDEX devnet settling node ──");
    println!("rpc          : {}", args.rpc_url);
    println!("agent        : {agent_pk}");
    println!("agent USDC   : {agent_ata}");
    println!("beneficiary  : {beneficiary}");
    println!("mode         : {}", if slash { "SLASH (will move USDC)" } else { "HONOR (no transfer)" });

    let rpc = RpcClient::new(args.rpc_url.clone());
    match rpc.get_balance(&agent_pk).await {
        Ok(lamports) => {
            let sol = lamports as f64 / 1e9;
            println!("SOL balance  : {sol:.4}");
            if sol < 0.01 {
                eprintln!("  ⚠ low SOL — run `solana airdrop 2 --url devnet`");
            }
        }
        Err(e) => eprintln!("  ⚠ could not reach RPC: {e}"),
    }
    match rpc.get_token_account_balance(&agent_ata).await {
        Ok(bal) => println!("USDC balance : {} ({} base units)", bal.ui_amount_string, bal.amount),
        Err(_) => eprintln!("  ⚠ agent USDC account not found/empty — create + mint devnet USDC first"),
    }

    // ── Solve → escrow → settle ────────────────────────────────────────────
    let settler = DevnetUsdcSettler::devnet(agent, usdc_mint, beneficiary);
    let solution = ReferenceRoutePolicy.solve(&demo_intent()).await?;
    let bond = settler.escrow(&solution).await?;
    println!(
        "\nescrowed bond: {} µUSDC, guaranteed ≥ {} out (conviction {:.2})",
        bond.amount.0, bond.min_out_raw, solution.conviction.0
    );

    // A slash needs a fill below the guaranteed minimum; honor meets it exactly.
    let fill = if slash {
        Fill { amount_out: Amount::new(bond.min_out_raw / 2, 6), executed_unix: 0 }
    } else {
        Fill { amount_out: Amount::new(bond.min_out_raw, 6), executed_unix: 0 }
    };
    println!(
        "submitting {} fill → settling on devnet...",
        if slash { "an under-delivering" } else { "a guarantee-meeting" }
    );

    let (outcome, sig) = settler.settle_onchain(&bond, &fill).await?;
    println!("outcome      : {outcome:?}");
    match sig {
        Some(sig) => {
            println!("✅ on-chain slash transfer: {sig}");
            println!("   explorer: https://explorer.solana.com/tx/{sig}?cluster=devnet");
        }
        None => println!("(honored — nothing moved; run with `--mode slash` to transfer USDC)"),
    }

    let l = settler.ledger();
    println!("ledger       : {} honored / {} slashed", l.honored, l.slashed);
    Ok(())
}
