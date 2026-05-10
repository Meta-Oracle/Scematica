use anyhow::Result;
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, error, info, warn};

use crate::cache::{CachedMarket, CachedPool};
use scematica_core::dex::program_ids;

/// Events emitted by the listener
#[derive(Debug, Clone)]
pub enum ListenerEvent {
    NewPool(CachedPool),
    NewMarket(CachedMarket),
    WalletUpdate { account: Pubkey, mint: Pubkey, amount: u64 },
}

/// Subscribes to Solana WebSocket for new Raydium pools and wallet changes
pub struct PoolListener {
    ws_url: String,
    wallet: Pubkey,
    quote_mint: Pubkey,
    event_tx: mpsc::Sender<ListenerEvent>,
}

impl PoolListener {
    pub fn new(
        ws_url: impl Into<String>,
        wallet: Pubkey,
        quote_mint: Pubkey,
        event_tx: mpsc::Sender<ListenerEvent>,
    ) -> Self {
        Self {
            ws_url: ws_url.into(),
            wallet,
            quote_mint,
            event_tx,
        }
    }

    /// Start listening. Runs until the channel is closed or connection drops.
    pub async fn run(&self) -> Result<()> {
        info!("Connecting to WebSocket: {}", self.ws_url);
        let (ws_stream, _) = connect_async(&self.ws_url).await?;
        let (mut write, mut read) = ws_stream.split();

        // Subscribe to Raydium AMM V4 program account changes (new pools)
        let pool_sub = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "programSubscribe",
            "params": [
                program_ids::RAYDIUM_AMM_V4.to_string(),
                {
                    "encoding": "base64",
                    "commitment": "confirmed",
                    "filters": [
                        { "dataSize": 752 }  // Raydium V4 pool state size
                    ]
                }
            ]
        });
        write.send(Message::Text(pool_sub.to_string())).await?;

        // Subscribe to wallet account changes (for sell triggers)
        let wallet_sub = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "accountSubscribe",
            "params": [
                self.wallet.to_string(),
                { "encoding": "jsonParsed", "commitment": "confirmed" }
            ]
        });
        write.send(Message::Text(wallet_sub.to_string())).await?;

        info!("WebSocket subscriptions active");

        while let Some(msg) = read.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    if let Err(e) = self.handle_message(&text).await {
                        warn!("Error handling WS message: {}", e);
                    }
                }
                Ok(Message::Ping(data)) => {
                    let _ = write.send(Message::Pong(data)).await;
                }
                Ok(Message::Close(_)) => {
                    warn!("WebSocket connection closed");
                    break;
                }
                Err(e) => {
                    error!("WebSocket error: {}", e);
                    break;
                }
                _ => {}
            }
        }

        Ok(())
    }

    async fn handle_message(&self, text: &str) -> Result<()> {
        let value: Value = serde_json::from_str(text)?;

        // Subscription confirmation
        if value.get("result").is_some() && value.get("id").is_some() {
            debug!("Subscription confirmed: id={}", value["id"]);
            return Ok(());
        }

        let method = value["method"].as_str().unwrap_or("");

        match method {
            "programNotification" => {
                self.handle_pool_notification(&value).await?;
            }
            "accountNotification" => {
                self.handle_wallet_notification(&value).await?;
            }
            _ => {}
        }

        Ok(())
    }

    async fn handle_pool_notification(&self, value: &Value) -> Result<()> {
        let params = &value["params"]["result"];
        let account_id = params["context"]["slot"].as_u64().unwrap_or(0);
        let pubkey_str = params["value"]["pubkey"].as_str().unwrap_or("");
        let data = &params["value"]["account"]["data"];

        if let Some(b64) = data[0].as_str() {
            use base64::Engine;
            let bytes = base64::engine::general_purpose::STANDARD.decode(b64)?;
            if bytes.len() >= 752 {
                if let Some(pool) = decode_raydium_v4_pool(pubkey_str, &bytes) {
                    debug!(pool = %pool.id, base = %pool.base_mint, "New pool detected");
                    let _ = self.event_tx.send(ListenerEvent::NewPool(pool)).await;
                }
            }
        }

        Ok(())
    }

    async fn handle_wallet_notification(&self, value: &Value) -> Result<()> {
        let parsed = &value["params"]["result"]["value"]["data"]["parsed"]["info"];
        if let (Some(mint_str), Some(amount_str)) = (
            parsed["mint"].as_str(),
            parsed["tokenAmount"]["amount"].as_str(),
        ) {
            if let (Ok(mint), Ok(amount)) = (
                Pubkey::from_str(mint_str),
                amount_str.parse::<u64>(),
            ) {
                // Skip quote token updates
                if mint != self.quote_mint {
                    let account_str = value["params"]["result"]["context"]["slot"]
                        .as_str()
                        .unwrap_or("");
                    let account = Pubkey::from_str(account_str).unwrap_or(self.wallet);
                    let _ = self
                        .event_tx
                        .send(ListenerEvent::WalletUpdate { account, mint, amount })
                        .await;
                }
            }
        }
        Ok(())
    }
}

/// Decode a Raydium V4 pool state from raw account bytes
fn decode_raydium_v4_pool(pubkey_str: &str, data: &[u8]) -> Option<CachedPool> {
    use scematica_core::dex::raydium_v4::*;

    if data.len() < POOL_STATE_SIZE {
        return None;
    }

    let pool_id = Pubkey::from_str(pubkey_str).ok()?;

    let base_mint = Pubkey::try_from(&data[BASE_MINT_OFFSET..BASE_MINT_OFFSET + 32]).ok()?;
    let quote_mint = Pubkey::try_from(&data[QUOTE_MINT_OFFSET..QUOTE_MINT_OFFSET + 32]).ok()?;
    let base_vault = Pubkey::try_from(&data[BASE_VAULT_OFFSET..BASE_VAULT_OFFSET + 32]).ok()?;
    let quote_vault = Pubkey::try_from(&data[QUOTE_VAULT_OFFSET..QUOTE_VAULT_OFFSET + 32]).ok()?;
    let market_id = Pubkey::try_from(&data[MARKET_ID_OFFSET..MARKET_ID_OFFSET + 32]).ok()?;

    let open_time = u64::from_le_bytes(
        data[OPEN_TIME_OFFSET..OPEN_TIME_OFFSET + 8].try_into().ok()?
    );

    Some(CachedPool {
        id: pool_id,
        base_mint,
        quote_mint,
        base_vault,
        quote_vault,
        market_id,
        open_time,
        base_decimals: 9,  // will be fetched on-demand
        quote_decimals: 9,
    })
}
