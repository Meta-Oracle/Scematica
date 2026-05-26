use crate::app::{AppState, BotMode, TradeEntry};
use chrono::Utc;
use rand::{rngs::SmallRng, Rng, SeedableRng};
use scematica_core::metrics::METRICS_FILE;
use std::sync::Arc;
use tokio::time::{sleep, Duration};

const DEMO_WALLET: &str = "7ycLhn5WsodcbYwV9ecQDd3qWQhKgGzgMK5pc4CYXkEc";
const DEMO_SOL: f64 = 0.025;
const DEMO_SCEMA: f64 = 250_000.0;
const QUOTE_SOL: f64 = 0.005;

static TOKENS: &[(&str, &str)] = &[
    ("4k3Dyjzvzp8eMZWUXbBCjEvwSkkk59S5iCNLY3QrkX6R", "BONK"),
    ("7GCihgDB8fe6KNjn2MYtkzZcRjQy3t9GHdC8uHYmW2hr", "WIF"),
    ("9n4nbM75f5Ui33ZbPYXn59EwSgE8CGsHtAeTH5YFeJ9E", "POPCAT"),
    ("DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263", "MEW"),
    ("HZ1JovNiVvGrGNiiYvEozEVgZ58xaU3RKwX8eACQBCt3", "BOME"),
    ("6DNSN2BJsaPFdFFc1zP37kkeNe4Usc1Sqkzr9C9vPWcD", "SAMO"),
    ("2b1kV6DkPAnxd5ixfnxCpjxmKwqjjaYmCZfHsFu24GXo", "COPE"),
    ("mSoLzYCxHdYgdzU16g5QSh3i5K3z3KZK7ytfqcJm7So", "MSOL"),
    ("AGFEad2et2ZJif9jaGpdMixQqvW5i81aBdvKe7im23wR", "FIDA"),
    ("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v", "ORCA"),
    ("3NZ9JMVBmGAqocybic2c7LQCJScmgsAZ6vQqTDzcqmJh", "WBTC"),
    ("CpMah17kLowc3oC98sEtBmEFWez6vzsKE1fy5ST6Lm6P", "NOOT"),
    ("BZLbGTNCSFfoth2GYDtwr7e4imWzpR5jqcUuGEwr646K", "PENG"),
    ("7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU", "SAMO2"),
    ("kinXdEcpDQeHPEuQnqmUgtYykqKCSVgjzGr9e9HKCNo", "KIN"),
];

static SNIPER_DEXES: &[&str] = &["Raydium", "Orca", "Meteora", "PumpFun"];

static ARB_ROUTES: &[(&str, u8)] = &[
    ("Raydium → Orca", 2),
    ("Orca → Meteora", 2),
    ("Raydium → Meteora → Orca", 3),
    ("PumpFun → Raydium", 2),
    ("Orca → Raydium → Meteora", 3),
    ("Meteora → Orca → Raydium", 3),
    ("PumpFun → Orca", 2),
];

fn fake_sig() -> String {
    let mut rng = SmallRng::from_entropy();
    let bytes: Vec<u8> = (0..32).map(|_| rng.gen::<u8>()).collect();
    bs58::encode(bytes).into_string()
}

pub async fn run_demo(state: Arc<AppState>) {
    *state.wallet_address.write() = DEMO_WALLET.to_string();
    *state.sol_balance.write() = DEMO_SOL;
    *state.scematica_balance.write() = DEMO_SCEMA;
    *state.active_mode.write() = BotMode::Both;

    state.push_log("[DEMO] ════════════════════════════════════════════");
    state.push_log("[DEMO] Scematica Trading Demo  —  Simulation Mode");
    state.push_log(format!(
        "[DEMO] Wallet : {}...{}",
        &DEMO_WALLET[..8],
        &DEMO_WALLET[DEMO_WALLET.len() - 8..]
    ));
    state.push_log(format!(
        "[DEMO] SOL    : {:.4}  |  SCEMA : {:.0}",
        DEMO_SOL, DEMO_SCEMA
    ));
    state.push_log("[DEMO] Bots   : SNIPER + ARB running (simulated)");
    state.push_log("[DEMO] ════════════════════════════════════════════");

    let s1 = state.clone();
    let s2 = state.clone();
    let s3 = state.clone();

    tokio::spawn(async move { sniper_loop(s1).await });
    tokio::spawn(async move { arb_loop(s2).await });
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(30));
        loop {
            tick.tick().await;
            s3.metrics.flush_to_file(METRICS_FILE);
        }
    });
}

async fn sniper_loop(state: Arc<AppState>) {
    sleep(Duration::from_secs(4)).await;

    let mut rng = SmallRng::from_entropy();
    let mut pools_seen: u64 = 0;

    loop {
        sleep(Duration::from_secs(rng.gen_range(18..50))).await;

        let (mint, symbol) = TOKENS[rng.gen_range(0..TOKENS.len())];
        let dex = SNIPER_DEXES[rng.gen_range(0..SNIPER_DEXES.len())];
        let pool_sol: f64 = rng.gen_range(5.0..150.0);

        pools_seen += 1;
        state.metrics.set_pools_tracked(pools_seen);

        state.push_log(format!(
            "[SNIPER] 🔍 Pool #{}: {} ({}) on {} | size: {:.1} SOL",
            pools_seen,
            symbol,
            &mint[..8],
            dex,
            pool_sol
        ));

        sleep(Duration::from_millis(rng.gen_range(600..1400))).await;

        // Filter stage — 18% rejection rate
        if rng.gen_bool(0.18) {
            let reason = match rng.gen_range(0..4) {
                0 => "mint not renounced",
                1 => "freezable authority active",
                2 => "pool too small",
                _ => "mutable metadata",
            };
            state.push_log(format!(
                "[SNIPER] ❌ {} rejected: {} — skipping",
                symbol, reason
            ));
            continue;
        }

        state.push_log(format!(
            "[SNIPER] ✅ {} passed all filters — sending buy...",
            symbol
        ));
        state.metrics.record_trade_attempt();
        sleep(Duration::from_millis(rng.gen_range(350..1100))).await;

        // Buy tx — 13% failure rate (congestion / slippage)
        if rng.gen_bool(0.13) {
            state.metrics.record_trade_failed();
            let reason = if rng.gen_bool(0.5) {
                "slippage exceeded"
            } else {
                "simulation failed"
            };
            state.push_log(format!(
                "[SNIPER] ✗ Buy tx failed for {} ({})",
                symbol, reason
            ));
            state.push_trade(TradeEntry {
                timestamp: Utc::now(),
                kind: "BUY".to_string(),
                mint: mint.to_string(),
                amount: QUOTE_SOL,
                pnl: 0.0,
                status: "✗".to_string(),
                signature: fake_sig(),
                exit_reason: String::new(),
                pnl_pct: 0.0,
                position_age_secs: 0.0,
            });
            continue;
        }

        let buy_sig = fake_sig();
        state.push_log(format!(
            "[SNIPER] ✓ BUY confirmed  {} | {:.4} SOL → {} | sig: {}…{}",
            symbol,
            QUOTE_SOL,
            symbol,
            &buy_sig[..6],
            &buy_sig[buy_sig.len() - 4..]
        ));

        {
            let mut bal = state.sol_balance.write();
            *bal = (*bal - QUOTE_SOL).max(0.0);
        }
        state.push_trade(TradeEntry {
            timestamp: Utc::now(),
            kind: "BUY".to_string(),
            mint: mint.to_string(),
            amount: QUOTE_SOL,
            pnl: 0.0,
            status: "✓".to_string(),
            signature: buy_sig,
            exit_reason: String::new(),
            pnl_pct: 0.0,
            position_age_secs: 0.0,
        });

        // Holding period with live price-check logs
        let hold_secs: u64 = rng.gen_range(8..80);
        let checks: u64 = rng.gen_range(2..5);
        let check_interval = hold_secs / (checks + 1);

        // Simulate price drift during hold
        let profitable = rng.gen_bool(0.42);
        let final_pct: f64 = if profitable {
            rng.gen_range(0.20..1.85)
        } else {
            rng.gen_range(-0.22..-0.14)
        };

        for i in 1..=checks {
            sleep(Duration::from_secs(check_interval)).await;
            // Interpolate toward final price, add some noise
            let progress = i as f64 / (checks + 1) as f64;
            let noise: f64 = rng.gen_range(-0.05..0.05);
            let current_pct = final_pct * progress + noise;
            state.push_log(format!(
                "[SNIPER] 📊 {} check {}/{}: {}{:.1}%  (TP {:.0}% / SL -{:.0}%)",
                symbol,
                i,
                checks,
                if current_pct >= 0.0 { "+" } else { "" },
                current_pct * 100.0,
                50.0,
                20.0
            ));
        }

        sleep(Duration::from_secs(check_interval)).await;

        // Sell
        let sell_amount = QUOTE_SOL * (1.0 + final_pct);
        let pnl = sell_amount - QUOTE_SOL;
        let pnl_lamps = (pnl * 1_000_000_000.0) as i64;

        state.metrics.record_trade_attempt();
        sleep(Duration::from_millis(rng.gen_range(300..900))).await;

        if rng.gen_bool(0.07) {
            state.push_log(format!(
                "[SNIPER] ⚠ Sell retry 1/3 for {} (rpc timeout)...",
                symbol
            ));
            sleep(Duration::from_secs(2)).await;
        }

        state.metrics.record_trade_confirmed(pnl_lamps);
        {
            let mut bal = state.sol_balance.write();
            *bal += sell_amount;
        }

        let sell_sig = fake_sig();
        state.push_log(format!(
            "[SNIPER] {} SELL {} | {:.4} SOL received | PnL: {}{:.5} SOL ({}{:.1}%) | sig: {}…{}",
            if pnl >= 0.0 { "💰" } else { "🔻" },
            symbol,
            sell_amount,
            if pnl >= 0.0 { "+" } else { "" },
            pnl,
            if final_pct >= 0.0 { "+" } else { "" },
            final_pct * 100.0,
            &sell_sig[..6],
            &sell_sig[sell_sig.len() - 4..]
        ));

        state.push_trade(TradeEntry {
            timestamp: Utc::now(),
            kind: "SELL".to_string(),
            mint: mint.to_string(),
            amount: sell_amount,
            pnl,
            status: "✓".to_string(),
            signature: sell_sig,
            exit_reason: if pnl >= 0.0 {
                "take_profit"
            } else {
                "stop_loss"
            }
            .to_string(),
            pnl_pct: final_pct * 100.0,
            position_age_secs: hold_secs as f64,
        });
    }
}

async fn arb_loop(state: Arc<AppState>) {
    sleep(Duration::from_secs(7)).await;

    let mut rng = SmallRng::from_entropy();
    let mut scan_n: u64 = 0;

    loop {
        sleep(Duration::from_millis(rng.gen_range(7_000..18_000))).await;

        scan_n += 1;
        state.metrics.record_arb_found();

        let (route, hops) = ARB_ROUTES[rng.gen_range(0..ARB_ROUTES.len())];
        let (mint, symbol) = TOKENS[rng.gen_range(0..TOKENS.len())];
        let profit_lamps: i64 = rng.gen_range(900..9_500);
        let profit_sol = profit_lamps as f64 / 1_000_000_000.0;

        // ~35% of scanned arbs are not executable (spread too thin, already closed)
        if rng.gen_bool(0.35) {
            state.push_log(format!(
                "[ARB] 🔍 Scan #{}: {} via {}  ({} hops) | +{:.6} SOL — spread closed before landing",
                scan_n, symbol, route, hops, profit_sol
            ));
            continue;
        }

        state.push_log(format!(
            "[ARB] ✅ Executing #{}: {} via {}  ({} hops) | expected +{:.6} SOL",
            scan_n, symbol, route, hops, profit_sol
        ));

        sleep(Duration::from_millis(rng.gen_range(150..550))).await;

        // ~14% revert (profit window closed between quote and landing)
        if rng.gen_bool(0.14) {
            state.push_log(
                "[ARB] ✗ Tx reverted — profit window closed (profit_or_revert)".to_string(),
            );
            continue;
        }

        state.metrics.record_arb_executed();
        state.metrics.record_trade_confirmed(profit_lamps);
        {
            let mut bal = state.sol_balance.write();
            *bal += profit_sol;
        }

        let sig = fake_sig();
        state.push_log(format!(
            "[ARB] 💰 Confirmed! +{:.6} SOL | {} | {} | sig: {}…{}",
            profit_sol,
            symbol,
            route,
            &sig[..6],
            &sig[sig.len() - 4..]
        ));

        state.push_trade(TradeEntry {
            timestamp: Utc::now(),
            kind: "ARB".to_string(),
            mint: mint.to_string(),
            amount: QUOTE_SOL,
            pnl: profit_sol,
            status: "✓".to_string(),
            signature: sig,
            exit_reason: String::new(),
            pnl_pct: 0.0,
            position_age_secs: 0.0,
        });
    }
}
