use anyhow::Result;
use crate::chat_types::{ToolCall, ToolResult};
use crate::symbol_resolver::resolve_symbol;
use parking_lot::RwLock;
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
        if self.trades_attempted == 0 { return 0.0; }
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
        Self { live_data: Some(live_data) }
    }

    fn live(&self) -> Option<LiveData> {
        self.live_data.as_ref().map(|d| d.read().clone())
    }

    pub async fn dispatch(&self, call: ToolCall) -> Result<ToolResult> {
        match call {
            ToolCall::SwapToken { from, to, amount_sol, slippage_bps } => {
                let from_mint = resolve_symbol(&from)?;
                let to_mint = resolve_symbol(&to)?;
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
                let from_mint = resolve_symbol(&from)?;
                let to_mint = resolve_symbol(&to)?;
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
        }
    }
}
