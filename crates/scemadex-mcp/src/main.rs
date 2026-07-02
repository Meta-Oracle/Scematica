//! `scemadex-mcp` — a Model Context Protocol server that exposes the ScemaDEX
//! agentic-liquidity rail (reputation / pool-score / advice signals, bonded
//! inference quotes, and experience purchases) to any MCP-capable LLM agent.
//!
//! It bridges MCP (stdio JSON-RPC) → a running `scemadex-relay` (HTTP). Because
//! the relay's signal endpoints can be x402-gated, this turns "buy trading
//! intelligence" into a discoverable, priced tool call for LLM agents.
//!
//! ```text
//! LLM agent ⇄ (MCP/stdio) ⇄ scemadex-mcp ⇄ (HTTP) ⇄ scemadex-relay ⇄ bot artifacts
//! ```
//!
//! Run: `scemadex-mcp --relay-url http://localhost:8080`
//! (or set `SCEMADEX_RELAY_URL`). Configure it as an MCP server in your client.

mod mcp;
mod relay;

use anyhow::Result;
use clap::Parser;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use mcp::McpServer;
use relay::RelayClient;

const DEFAULT_RELAY_URL: &str = "http://localhost:8080";

#[derive(Parser, Debug)]
#[command(name = "scemadex-mcp", about = "MCP server for the ScemaDEX agentic-liquidity rail")]
struct Cli {
    /// Base URL of the scemadex-relay to proxy to. Falls back to the
    /// `SCEMADEX_RELAY_URL` env var, then `http://localhost:8080`.
    #[arg(long)]
    relay_url: Option<String>,
}

/// Resolve the relay URL: CLI flag > `SCEMADEX_RELAY_URL` env > default.
fn resolve_relay_url(flag: Option<String>) -> String {
    flag.or_else(|| std::env::var("SCEMADEX_RELAY_URL").ok())
        .unwrap_or_else(|| DEFAULT_RELAY_URL.to_string())
}

#[tokio::main]
async fn main() -> Result<()> {
    // stdout is the MCP transport — send ALL logs to stderr.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_target(false)
        .init();

    let cli = Cli::parse();
    let relay_url = resolve_relay_url(cli.relay_url);
    let relay = RelayClient::new(relay_url);

    // Best-effort startup probe (non-fatal): the relay may come up after us.
    match relay.health().await {
        Ok(r) if r.status == 200 => tracing::info!("relay healthy at {}", relay.base_url()),
        Ok(r) => tracing::warn!("relay at {} returned {} on /health", relay.base_url(), r.status),
        Err(e) => tracing::warn!("relay at {} not reachable yet: {e}", relay.base_url()),
    }

    let server = McpServer::new(relay);
    tracing::info!("scemadex-mcp ready");

    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin).lines();
    let mut stdout = tokio::io::stdout();

    // Newline-delimited JSON-RPC: one message per line.
    while let Some(line) = reader.next_line().await? {
        if let Some(response) = server.handle_line(&line).await {
            stdout.write_all(response.as_bytes()).await?;
            stdout.write_all(b"\n").await?;
            stdout.flush().await?;
        }
    }
    Ok(())
}
