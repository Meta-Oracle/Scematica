use anyhow::Result;
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, info, warn};

use crate::cache::CachedPool;
use crate::listener::ListenerEvent;
use scematica_core::dex::program_ids;

/// Subscribes to a list of whale wallet addresses over the Solana WebSocket.
/// When a watched wallet executes a Raydium buy, extracts the pool/token and
/// emits a `ListenerEvent::NewPool` so the sniper treats it like a fresh pool.
pub struct WhaleCopyListener {
    ws_url: String,
    wallets: Vec<String>,
    event_tx: mpsc::Sender<ListenerEvent>,
}

impl WhaleCopyListener {
    pub fn new(
        ws_url: impl Into<String>,
        wallets: Vec<String>,
        event_tx: mpsc::Sender<ListenerEvent>,
    ) -> Self {
        Self { ws_url: ws_url.into(), wallets, event_tx }
    }

    pub async fn run(&self) -> Result<()> {
        if self.wallets.is_empty() {
            return Ok(());
        }

        info!("🐋 Whale copy listener connecting to {}", self.ws_url);
        let (ws_stream, _) = connect_async(&self.ws_url).await?;
        let (mut write, mut read) = ws_stream.split();

        // Subscribe to each wallet's account changes
        for (i, wallet) in self.wallets.iter().enumerate() {
            let sub = json!({
                "jsonrpc": "2.0",
                "id": i + 100,
                "method": "logsSubscribe",
                "params": [
                    { "mentions": [wallet] },
                    { "commitment": "confirmed" }
                ]
            });
            write.send(Message::Text(sub.to_string())).await?;
            info!("🐋 Subscribed to wallet logs: {}…", &wallet[..8.min(wallet.len())]);
        }

        while let Some(msg) = read.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    if let Ok(v) = serde_json::from_str::<Value>(&text) {
                        self.handle_message(&v).await;
                    }
                }
                Ok(Message::Ping(p)) => {
                    let _ = write.send(Message::Pong(p)).await;
                }
                Ok(Message::Close(_)) => {
                    warn!("🐋 Whale copy WS connection closed");
                    break;
                }
                Err(e) => {
                    warn!("🐋 Whale copy WS error: {}", e);
                    break;
                }
                _ => {}
            }
        }
        Ok(())
    }

    async fn handle_message(&self, v: &Value) {
        // logsSubscribe notification format:
        // { "method": "logsNotification", "params": { "result": { "value": { "logs": [...], "signature": "..." } } } }
        let Some(result) = v.pointer("/params/result/value") else { return };
        let logs = result["logs"].as_array();
        let Some(logs) = logs else { return };
        let signature = result["signature"].as_str().unwrap_or("unknown");

        // Check if this transaction invoked the Raydium AMM V4 program
        let raydium_id = program_ids::RAYDIUM_AMM_V4.to_string();
        let is_raydium = logs.iter().any(|l| {
            l.as_str().map(|s| s.contains(&raydium_id) || s.contains("Program log: ray_log")).unwrap_or(false)
        });
        if !is_raydium {
            return;
        }

        // Try to extract the pool id from the log messages
        // Raydium logs include "Program log: AMMPool: <pool_pubkey>" style entries
        let mut pool_id_str: Option<String> = None;
        for log in logs {
            let s = log.as_str().unwrap_or("");
            if s.contains("initialize2") || s.contains("Instruction: Initialize2") {
                // New pool creation — we may be too late, but attempt anyway
                debug!("🐋 Whale triggered possible new pool via tx {}", &signature[..8.min(signature.len())]);
            }
            if s.starts_with("Program log: AMMPool:") {
                let parts: Vec<&str> = s.split_whitespace().collect();
                if parts.len() >= 3 {
                    pool_id_str = Some(parts[2].to_string());
                }
            }
        }

        // Build a minimal CachedPool from what we know and emit it
        // The sniper's filter pipeline will validate and enrich it via RPC
        if let Some(pool_str) = pool_id_str {
            if let Ok(pool_id) = Pubkey::from_str(&pool_str) {
                let pool = CachedPool {
                    id: pool_id,
                    base_mint: Pubkey::default(),
                    quote_mint: Pubkey::default(),
                    base_vault: Pubkey::default(),
                    quote_vault: Pubkey::default(),
                    market_id: Pubkey::default(),
                    open_time: 0,
                    base_decimals: 9,
                    quote_decimals: 9,
                };
                info!(
                    "🐋 Copy-trade: whale bought pool {} (tx {}…) — emitting for sniper",
                    &pool_str[..8.min(pool_str.len())],
                    &signature[..8.min(signature.len())]
                );
                let _ = self.event_tx.send(ListenerEvent::NewPool(pool)).await;
            }
        } else {
            // Even without the pool ID, log that we saw the whale act on Raydium
            info!(
                "🐋 Copy-trade: whale Raydium activity detected (tx {}…) — pool ID not parseable from logs",
                &signature[..8.min(signature.len())]
            );
        }
    }
}
