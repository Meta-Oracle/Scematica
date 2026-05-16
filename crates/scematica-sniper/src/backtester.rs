use crate::cache::CachedPool;
use chrono::{DateTime, Utc};
use scematica_core::config::FilterConfig;
use serde::{Deserialize, Serialize};

/// A historical pool record used for backtesting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestPool {
    pub pool: CachedPool,
    pub opened_at: DateTime<Utc>,
    /// Peak price percentage above entry (simulated PnL)
    pub max_price_pct: f64,
    /// True if the token dumped to effectively zero
    pub rugged: bool,
}

/// Aggregated backtest results.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct BacktestResult {
    pub total_pools: usize,
    pub passed_filters: usize,
    pub simulated_buys: usize,
    pub wins: usize,
    pub losses: usize,
    pub win_rate: f64,
    pub avg_win_pct: f64,
    pub avg_loss_pct: f64,
    /// Expected value: win_rate * avg_win - (1 - win_rate) * avg_loss
    pub expected_value: f64,
}

/// Synchronous backtester that replays historical pool data through the filter pipeline.
///
/// Because no live RPC is available during backtesting the filter check is
/// limited to the static, configuration-driven predicates (min/max size, name
/// patterns, snipe-list membership, enabled-flag checks). RPC-bound filters
/// (mint-renounce, freeze, LP-burn, social links) are skipped when the
/// corresponding `FilterConfig` flags are off; when they *are* enabled we
/// conservatively count the pool as failed because we cannot verify on-chain
/// state offline.
pub struct Backtester {
    pub config: FilterConfig,
    /// Take-profit threshold (% above entry)
    pub tp_pct: f64,
    /// Stop-loss threshold (% below entry)
    pub sl_pct: f64,
}

impl Backtester {
    pub fn new(config: FilterConfig, tp_pct: f64, sl_pct: f64) -> Self {
        Self { config, tp_pct, sl_pct }
    }

    /// Check whether a historical pool would have passed the static filter predicates.
    ///
    /// Rules that require live RPC data (mint renounce, freeze, LP burn, socials)
    /// are treated as **failed** when their config flag is enabled (because we
    /// cannot confirm them), and **skipped** (assumed passed) when their flag
    /// is disabled.
    fn static_filter_check(&self, bp: &BacktestPool) -> bool {
        let cfg = &self.config;

        // Min pool size: if min_pool_size > 0 we cannot verify without an RPC
        // call to fetch the quote vault balance — conservatively fail.
        if cfg.min_pool_size > 0.0 {
            return false;
        }

        // RPC-bound filters: if any are enabled we cannot verify offline → fail.
        if cfg.check_mint_renounced || cfg.check_freezable || cfg.check_burned || cfg.check_socials {
            return false;
        }

        // Mint string-based name filter (if a blocklist is not checked here —
        // we can't access the file at backtest time, so we skip it).

        // open_time sanity: pool must have been opened in a plausible window.
        let open_ts = bp.opened_at.timestamp() as u64;
        if open_ts == 0 {
            // Unknown open time — allow through.
            return true;
        }

        true
    }

    /// Run a backtest over a slice of historical pools.
    pub fn run(&self, pools: &[BacktestPool]) -> BacktestResult {
        let mut result = BacktestResult {
            total_pools: pools.len(),
            ..Default::default()
        };

        let mut win_pcts: Vec<f64> = Vec::new();
        let mut loss_pcts: Vec<f64> = Vec::new();

        for bp in pools {
            if !self.static_filter_check(bp) {
                continue;
            }
            result.passed_filters += 1;
            result.simulated_buys += 1;

            let is_win = if bp.rugged {
                // Rug → always a loss regardless of max_price_pct
                false
            } else {
                bp.max_price_pct >= self.tp_pct
            };

            if is_win {
                result.wins += 1;
                win_pcts.push(self.tp_pct);
            } else {
                result.losses += 1;
                // If rugged, loss is 100%; otherwise it's limited to sl_pct.
                let loss = if bp.rugged { 100.0 } else { self.sl_pct };
                loss_pcts.push(loss);
            }
        }

        let total = result.simulated_buys;
        if total > 0 {
            result.win_rate = result.wins as f64 / total as f64;
        }
        result.avg_win_pct = if win_pcts.is_empty() {
            0.0
        } else {
            win_pcts.iter().sum::<f64>() / win_pcts.len() as f64
        };
        result.avg_loss_pct = if loss_pcts.is_empty() {
            0.0
        } else {
            loss_pcts.iter().sum::<f64>() / loss_pcts.len() as f64
        };
        result.expected_value =
            result.win_rate * result.avg_win_pct
            - (1.0 - result.win_rate) * result.avg_loss_pct;

        result
    }

    /// Load historical pool data from a JSONL file (one `BacktestPool` JSON object per line).
    pub fn load_from_file(path: &str) -> anyhow::Result<Vec<BacktestPool>> {
        use std::io::{BufRead, BufReader};
        let file = std::fs::File::open(path)
            .map_err(|e| anyhow::anyhow!("Cannot open {}: {}", path, e))?;
        let reader = BufReader::new(file);
        let mut pools = Vec::new();
        for (line_no, line) in reader.lines().enumerate() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with("//") {
                continue;
            }
            let pool: BacktestPool = serde_json::from_str(trimmed)
                .map_err(|e| anyhow::anyhow!("{}:{}: parse error: {}", path, line_no + 1, e))?;
            pools.push(pool);
        }
        Ok(pools)
    }

    /// Save `BacktestResult` to a JSON file.
    pub fn save_results(results: &BacktestResult, path: &str) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(results)?;
        std::fs::write(path, json)
            .map_err(|e| anyhow::anyhow!("Cannot write {}: {}", path, e))?;
        Ok(())
    }
}
