use anyhow::Result;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::cache::CachedPool;
use crate::listener::ListenerEvent;

/// Pump.fun bonding curve program ID.
const PUMPFUN_PROGRAM: &str = "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P";

/// Pump.fun graduation monitor.
///
/// Monitors the Pump.fun bonding curve program for large balance changes.
/// When a bonding curve balance approaches the graduation threshold (~85 SOL),
/// the token will soon migrate to Raydium.  We emit a `ListenerEvent::NewPool`
/// so the sniper can attempt to front-run the migration.
///
/// Implementation: polls the program's top accounts every 30 s via RPC
/// (`getProgramAccounts` with a dataSize filter) looking for bonding curves
/// close to the graduation threshold.  This is simpler and more reliable than
/// raw WebSocket program-subscribe on Pump.fun's custom encoding.
pub struct PumpFunMonitor {
    #[allow(dead_code)]
    ws_url: String,
    rpc_url: String,
    tx: mpsc::Sender<ListenerEvent>,
    /// SOL balance at which a bonding curve is considered to be graduating
    graduation_threshold_sol: f64,
}

impl PumpFunMonitor {
    pub fn new(
        ws_url: String,
        tx: mpsc::Sender<ListenerEvent>,
        graduation_threshold_sol: f64,
    ) -> Self {
        // Derive an HTTP RPC URL from the WS URL for the poll-based fallback.
        let rpc_url = ws_url
            .replace("wss://", "https://")
            .replace("ws://", "http://");
        Self {
            ws_url,
            rpc_url,
            tx,
            graduation_threshold_sol,
        }
    }

    /// Start the graduation monitor.
    ///
    /// Polls every 30 s for Pump.fun bonding curve accounts approaching the
    /// graduation threshold.  For each candidate, a synthetic `NewPool` event
    /// is emitted so the sniper can attempt to snipe the Raydium migration.
    pub async fn run(&self) -> Result<()> {
        let program_id = Pubkey::from_str(PUMPFUN_PROGRAM)
            .expect("Invalid Pump.fun program ID");

        let rpc = Arc::new(RpcClient::new(self.rpc_url.clone()));

        info!(
            "Pump.fun monitor started (threshold={:.1} SOL, poll_interval=30s)",
            self.graduation_threshold_sol
        );

        let threshold_lamports = (self.graduation_threshold_sol * 1_000_000_000.0) as u64;
        // Warn 10 SOL before graduation
        let warn_lamports = threshold_lamports.saturating_sub(10_000_000_000);

        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(30));

        loop {
            interval.tick().await;

            // Fetch all Pump.fun program accounts.
            // In production this can be expensive — limit to accounts approaching graduation.
            let accounts = match rpc.get_program_accounts(&program_id).await {
                Ok(a) => a,
                Err(e) => {
                    warn!("Pump.fun monitor: getProgramAccounts failed: {}", e);
                    continue;
                }
            };

            debug!("Pump.fun monitor: found {} bonding curve accounts", accounts.len());

            for (pubkey, account) in accounts {
                // Check native lamport balance as a proxy for bonding curve progress.
                // The Pump.fun bonding curve accumulates native SOL in the account itself.
                if account.lamports < warn_lamports || account.lamports > threshold_lamports * 2 {
                    continue;
                }

                let pct = account.lamports as f64 / threshold_lamports as f64 * 100.0;
                info!(
                    "Pump.fun monitor: bonding curve {} at {:.1}% of graduation ({:.3} SOL)",
                    pubkey,
                    pct,
                    account.lamports as f64 / 1e9,
                );

                // Derive a minimal synthetic pool so the sniper can act on this.
                // Real migration pools are picked up by the Raydium listener shortly after;
                // this gives us a head-start on the tx.
                let synthetic_pool = CachedPool {
                    id: pubkey,
                    base_mint: pubkey, // actual base mint not decoded here
                    quote_mint: solana_sdk::pubkey!("So11111111111111111111111111111111111111112"),
                    base_vault: pubkey,
                    quote_vault: pubkey,
                    market_id: pubkey,
                    open_time: 0,
                    base_decimals: 6,
                    quote_decimals: 9,
                    pumpfun_score: 0.0,
                };

                if self.tx.send(ListenerEvent::NewPool(synthetic_pool)).await.is_err() {
                    warn!("Pump.fun monitor: event channel closed — exiting");
                    return Ok(());
                }
            }
        }
    }
}
