/// Scematica Backtesting CLI
///
/// Replays historical pool data through the filter pipeline (synchronous,
/// no RPC) and prints a summary of simulated trading performance.
///
/// Usage:
///   cargo run --bin backtest -- --data historical-pools.jsonl --tp 100 --sl 15
///   cargo run --bin backtest -- --data data.jsonl --tp 200 --sl 25 --output results.json
use anyhow::Result;
use clap::Parser;
use scematica_core::config::FilterConfig;
use scematica_sniper::backtester::Backtester;

#[derive(Parser, Debug)]
#[command(
    name = "backtest",
    about = "Replay historical pool data through the Scematica filter pipeline"
)]
struct Args {
    /// Path to a JSONL file of BacktestPool records (one JSON object per line)
    #[arg(long, default_value = "historical-pools.jsonl")]
    data: String,

    /// Take-profit threshold (% above entry)
    #[arg(long, default_value_t = 100.0)]
    tp: f64,

    /// Stop-loss threshold (% below entry)
    #[arg(long, default_value_t = 15.0)]
    sl: f64,

    /// Optional path to a FilterConfig JSON file (uses permissive defaults if omitted)
    #[arg(long)]
    config: Option<String>,

    /// Optional path to write the JSON results file
    #[arg(long)]
    output: Option<String>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Load FilterConfig from file or use permissive defaults so all static
    // checks pass and the backtest exercises the TP/SL logic.
    let filter_config: FilterConfig = if let Some(path) = &args.config {
        let data = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Cannot read config {}: {}", path, e))?;
        serde_json::from_str(&data)
            .map_err(|e| anyhow::anyhow!("Cannot parse config {}: {}", path, e))?
    } else {
        // Permissive defaults — all RPC-bound checks off so they don't block
        // the static filter pass-through.
        FilterConfig::default()
    };

    println!("Loading pool data from '{}'…", args.data);
    let pools = Backtester::load_from_file(&args.data)
        .map_err(|e| anyhow::anyhow!("Failed to load pool data: {}", e))?;
    println!("Loaded {} pool records.", pools.len());

    let bt = Backtester::new(filter_config, args.tp, args.sl);
    let result = bt.run(&pools);

    println!();
    println!("=== Backtest Results ===");
    println!("  Total pools evaluated : {}", result.total_pools);
    println!("  Passed filters        : {}", result.passed_filters);
    println!("  Simulated buys        : {}", result.simulated_buys);
    println!("  Wins                  : {}", result.wins);
    println!("  Losses                : {}", result.losses);
    println!("  Win rate              : {:.1}%", result.win_rate * 100.0);
    println!("  Avg win               : {:.1}%", result.avg_win_pct);
    println!("  Avg loss              : {:.1}%", result.avg_loss_pct);
    println!("  Expected value        : {:.2}%", result.expected_value);
    println!();

    if let Some(out_path) = &args.output {
        Backtester::save_results(&result, out_path)?;
        println!("Results written to '{}'.", out_path);
    }

    Ok(())
}
