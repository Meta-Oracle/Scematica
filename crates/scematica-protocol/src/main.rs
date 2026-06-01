use anyhow::Result;
use clap::Parser;
use scematica_core::wallet::Wallet;
use scematica_protocol::{types::PaymentRequirements, Facilitator, ProtocolServer};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use std::{net::SocketAddr, sync::Arc};
use tracing::info;

#[derive(Parser, Debug)]
#[command(
    name = "scematica-protocol",
    about = "Scematica Protocol — x402 signal API server"
)]
struct Args {
    /// RPC endpoint
    #[arg(long, default_value = "https://api.mainnet-beta.solana.com")]
    rpc: String,

    /// Keypair path for the fee payer (funds transaction fees for payment settlement)
    #[arg(long, default_value = "~/.config/solana/id.json")]
    keypair: String,

    /// Listen address
    #[arg(long, default_value = "0.0.0.0:4020")]
    bind: String,

    /// Solana network label
    #[arg(long, default_value = "solana-mainnet")]
    network: String,

    /// Payment recipient address (where micro-payments are sent)
    #[arg(long)]
    pay_to: String,

    /// Price per API call in lamports (default: 10_000 = 0.00001 SOL)
    #[arg(long, default_value_t = 10_000)]
    price_lamports: u64,

    /// Log level
    #[arg(long, default_value = "info")]
    log_level: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&args.log_level)),
        )
        .init();

    dotenv::dotenv().ok();

    info!("╔══════════════════════════════════════╗");
    info!(
        "║   SCEMATICA PROTOCOL  v{}         ║",
        env!("CARGO_PKG_VERSION")
    );
    info!("║   Rust-native x402 for Solana        ║");
    info!("╚══════════════════════════════════════╝");

    let wallet = Wallet::from_source(&args.keypair)?;
    let fee_payer = Arc::new(wallet.keypair);

    let rpc = Arc::new(RpcClient::new_with_commitment(
        args.rpc.clone(),
        CommitmentConfig::confirmed(),
    ));

    let facilitator = Arc::new(Facilitator::new(fee_payer, rpc, args.network.clone()));

    // WSOL (native SOL) payment requirement — callers pay per API call
    let wsol_mint = scematica_core::types::known_tokens::WSOL_MINT.to_string();
    let requirements = vec![PaymentRequirements {
        scheme: "exact".into(),
        network: args.network.clone(),
        asset: wsol_mint,
        amount: args.price_lamports,
        pay_to: args.pay_to.clone(),
        max_timeout_seconds: 300,
        extra: serde_json::json!({
            "description": "Scematica pool signal + trade intelligence API",
            "price_sol": args.price_lamports as f64 / 1e9,
        }),
    }];

    info!(
        "💳 Payment: {} lamports ({:.6} SOL) → {}",
        args.price_lamports,
        args.price_lamports as f64 / 1e9,
        &args.pay_to[..8.min(args.pay_to.len())]
    );

    let addr: SocketAddr = args.bind.parse()?;
    let server = ProtocolServer::new(facilitator, requirements, addr);
    server.run().await?;
    Ok(())
}
