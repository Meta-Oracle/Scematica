use crate::chat_types::{ToolCall, ToolResult};
use crate::symbol_resolver::resolve_symbol;
use anyhow::Result;
use parking_lot::RwLock;
use scematica_protocol::{
    MarketplaceSearchRequest, OpenDexterClient, PriceCheck, WalletStatus,
    DEFAULT_MIN_QUALITY_SCORE, DEFAULT_SEARCH_LIMIT,
};
use serde_json::json;
use std::sync::Arc;

/// Live bot data that tools can read to give real answers
#[derive(Debug, Default, Clone)]
pub struct LiveData {
    pub sol_balance: f64,
    pub scema_balance: f64,
    pub wallet_address: String,
    pub trades_attempted: u64,
    pub trades_confirmed: u64,
    pub arbs_found: u64,
    pub arbs_executed: u64,
    pub total_pnl_lamports: i64,
    pub uptime_secs: u64,
}

impl LiveData {
    pub fn total_pnl_sol(&self) -> f64 {
        self.total_pnl_lamports as f64 / 1_000_000_000.0
    }

    pub fn win_rate(&self) -> f64 {
        if self.trades_attempted == 0 {
            return 0.0;
        }
        self.trades_confirmed as f64 / self.trades_attempted as f64 * 100.0
    }
}

pub struct ToolDispatcher {
    pub live_data: Option<Arc<RwLock<LiveData>>>,
}

impl ToolDispatcher {
    pub fn new() -> Self {
        Self { live_data: None }
    }

    pub fn with_live_data(live_data: Arc<RwLock<LiveData>>) -> Self {
        Self {
            live_data: Some(live_data),
        }
    }

    fn live(&self) -> Option<LiveData> {
        self.live_data.as_ref().map(|d| d.read().clone())
    }

    pub async fn dispatch(&self, call: ToolCall) -> Result<ToolResult> {
        match call {
            ToolCall::SwapToken { from, to, amount_sol, slippage_bps } => {
                let from_mint = resolve_symbol(&from).await?;
                let to_mint = resolve_symbol(&to).await?;
                let slippage = slippage_bps.unwrap_or(100);
                Ok(ToolResult::Success {
                    message: format!(
                        "Swap queued: {:.4} {} → {}\nSlippage: {}bps\n\
                         ⚠️ Live execution requires the bot processes to be running.",
                        amount_sol, from_mint, to_mint, slippage
                    ),
                    signature: None,
                    data: None,
                })
            }

            ToolCall::GetQuote { from, to, amount_sol } => {
                let from_mint = resolve_symbol(&from).await?;
                let to_mint = resolve_symbol(&to).await?;
                Ok(ToolResult::Success {
                    message: format!(
                        "Quote: {:.4} {} → {}\nLive quotes require the arb engine (RPC connection).",
                        amount_sol, from_mint, to_mint
                    ),
                    signature: None,
                    data: None,
                })
            }

            ToolCall::GetBalance => {
                let msg = if let Some(d) = self.live() {
                    let wallet = if d.wallet_address.is_empty() {
                        "not connected".to_string()
                    } else {
                        format!("{}…{}", &d.wallet_address[..4], &d.wallet_address[d.wallet_address.len().saturating_sub(4)..])
                    };
                    format!(
                        "Wallet: {}\n  SOL:   {:.4} SOL\n  SCEMA: {:.2} SCEMA",
                        wallet, d.sol_balance, d.scema_balance
                    )
                } else {
                    "Balance data unavailable — dashboard is not connected to a wallet.".to_string()
                };
                Ok(ToolResult::Success { message: msg, signature: None, data: None })
            }

            ToolCall::SetBotMode { mode } => {
                Ok(ToolResult::Success {
                    message: format!(
                        "Mode '{}' noted. To start or stop bot processes, use the CLI:\n  cargo run --bin sniper\n  cargo run --bin arb",
                        mode
                    ),
                    signature: None,
                    data: None,
                })
            }

            ToolCall::ScanArb => {
                let msg = if let Some(d) = self.live() {
                    format!(
                        "Arb engine stats:\n  Opportunities found: {}\n  Executed: {}\n  PnL: {:.4} SOL\n  Uptime: {}s",
                        d.arbs_found, d.arbs_executed, d.total_pnl_sol(), d.uptime_secs
                    )
                } else {
                    "Arb engine is not running. Start with: cargo run --bin arb".to_string()
                };
                Ok(ToolResult::Success { message: msg, signature: None, data: None })
            }

            ToolCall::GetTradeHistory { n } => {
                let count = n.unwrap_or(10);
                let msg = if let Some(d) = self.live() {
                    format!(
                        "Last {} trades:\n  Attempted: {}  |  Confirmed: {}  |  Failed: {}\n  Win rate:  {:.1}%\n  PnL:       {:.4} SOL",
                        count,
                        d.trades_attempted,
                        d.trades_confirmed,
                        d.trades_attempted.saturating_sub(d.trades_confirmed),
                        d.win_rate(),
                        d.total_pnl_sol()
                    )
                } else {
                    "No trade history available — bot has not run yet.".to_string()
                };
                Ok(ToolResult::Success { message: msg, signature: None, data: None })
            }

            ToolCall::GetBotStatus => {
                let msg = if let Some(d) = self.live() {
                    format!(
                        "Bot status ✅\n  Uptime:          {}s\n  Trades:          {} attempted / {} confirmed\n  Arbs found:      {}\n  PnL:             {:.4} SOL\n  SOL balance:     {:.4}\n  SCEMA balance:   {:.2}",
                        d.uptime_secs,
                        d.trades_attempted,
                        d.trades_confirmed,
                        d.arbs_found,
                        d.total_pnl_sol(),
                        d.sol_balance,
                        d.scema_balance
                    )
                } else {
                    "Bot data unavailable. Start dashboard with a running bot process.".to_string()
                };
                Ok(ToolResult::Success { message: msg, signature: None, data: None })
            }

            ToolCall::X402Search {
                query,
                verified_only,
                min_quality_score,
                limit,
            } => {
                if query.trim().is_empty() {
                    return Ok(ToolResult::Failure {
                        message: "x402_search requires a non-empty query.".to_string(),
                        recoverable: true,
                    });
                }

                let client = OpenDexterClient::from_env()?;
                let request = MarketplaceSearchRequest {
                    query,
                    verified_only: verified_only.unwrap_or(false),
                    min_quality_score: min_quality_score.unwrap_or(DEFAULT_MIN_QUALITY_SCORE),
                    limit: limit
                        .map(|n| n as usize)
                        .unwrap_or(DEFAULT_SEARCH_LIMIT)
                        .clamp(1, 25),
                };

                match client.search(request).await {
                    Ok(results) => Ok(ToolResult::Success {
                        message: format_marketplace_results(&results),
                        signature: None,
                        data: Some(json!({ "results": results })),
                    }),
                    Err(e) => Ok(ToolResult::Failure {
                        message: e.to_string(),
                        recoverable: true,
                    }),
                }
            }

            ToolCall::X402Check { url, method } => {
                if url.trim().is_empty() {
                    return Ok(ToolResult::Failure {
                        message: "x402_check requires a URL.".to_string(),
                        recoverable: true,
                    });
                }

                let client = OpenDexterClient::from_env()?;
                match client.check(&url, method.as_deref()).await {
                    Ok(check) => Ok(ToolResult::Success {
                        message: format_price_check(&check),
                        signature: None,
                        data: Some(json!(check)),
                    }),
                    Err(e) => Ok(ToolResult::Failure {
                        message: e.to_string(),
                        recoverable: true,
                    }),
                }
            }

            ToolCall::X402Fetch { url, method } => {
                if url.trim().is_empty() {
                    return Ok(ToolResult::Failure {
                        message: "x402_fetch requires a URL.".to_string(),
                        recoverable: true,
                    });
                }

                let client = OpenDexterClient::from_env()?;
                match client.fetch(&url, method.as_deref()).await {
                    Ok(response) => {
                        let paid = response
                            .paid_usdc
                            .map(|v| format!("{v:.6} USDC"))
                            .unwrap_or_else(|| "0 USDC".to_string());
                        let network = response.network.clone().unwrap_or_else(|| "free".to_string());
                        Ok(ToolResult::Success {
                            message: format!(
                                "x402 fetch succeeded with HTTP {} on {}. Paid: {}.",
                                response.status, network, paid
                            ),
                            signature: response.payment_response.clone(),
                            data: Some(json!(response)),
                        })
                    }
                    Err(e) => Ok(ToolResult::Failure {
                        message: e.to_string(),
                        recoverable: true,
                    }),
                }
            }

            ToolCall::X402Wallet => {
                let client = OpenDexterClient::from_env()?;
                let status = client.wallet_status();
                Ok(ToolResult::Success {
                    message: format_wallet_status(&status),
                    signature: None,
                    data: Some(json!(status)),
                })
            }
        }
    }
}

fn format_marketplace_results(results: &[scematica_protocol::MarketplaceApi]) -> String {
    if results.is_empty() {
        return "No matching x402 marketplace APIs met the current filters.".to_string();
    }

    let mut lines = vec![format!("Found {} x402 API result(s):", results.len())];
    for (idx, api) in results.iter().enumerate() {
        let price = api
            .price_usdc
            .map(|v| format!("{v:.6} USDC"))
            .unwrap_or_else(|| "price unknown".to_string());
        let quality = api
            .quality_score
            .map(|v| format!("{v:.0}"))
            .unwrap_or_else(|| "unknown".to_string());
        let verified = if api.verified {
            "verified"
        } else {
            "unverified"
        };
        let endpoint = api.endpoint.as_deref().unwrap_or("endpoint not listed");
        lines.push(format!(
            "{}. {} - {} - quality {} - {} - {}",
            idx + 1,
            api.name,
            price,
            quality,
            verified,
            endpoint
        ));
    }
    lines.join("\n")
}

fn format_price_check(check: &PriceCheck) -> String {
    if !check.requires_payment {
        return format!(
            "{} {} returned HTTP {} and does not require x402 payment.",
            check.method, check.url, check.status
        );
    }

    if check.payment_options.is_empty() {
        return format!(
            "{} {} requires payment, but no usable payment options were returned.",
            check.method, check.url
        );
    }

    let mut lines = vec![format!(
        "{} {} requires x402 payment. Per-call limit: {:.6} USDC.",
        check.method, check.url, check.max_payment_usdc
    )];
    for option in &check.payment_options {
        let price = option
            .amount_usdc
            .map(|v| format!("{v:.6} USDC"))
            .unwrap_or_else(|| format!("{} raw units", option.amount_raw));
        lines.push(format!(
            "- {} on {}: {} to {}",
            option.scheme, option.network, price, option.pay_to
        ));
    }
    lines.join("\n")
}

fn format_wallet_status(status: &WalletStatus) -> String {
    let networks = if status.active_networks.is_empty() {
        "none".to_string()
    } else {
        status.active_networks.join(", ")
    };
    let mut lines = vec![
        format!(
            "x402 wallets: SVM={}, EVM={}",
            status.svm_configured, status.evm_configured
        ),
        format!("Active networks: {networks}"),
        format!("Per-call spend limit: {:.6} USDC", status.max_payment_usdc),
        format!(
            "Marketplace search configured: {}",
            status.marketplace_url_configured
        ),
    ];
    lines.extend(
        status
            .warnings
            .iter()
            .map(|warning| format!("Warning: {warning}")),
    );
    lines.join("\n")
}
