# Scematica v1.6.0 - Quick Start Guide

## Your Setup Status ✓

- **Wallet**: `C:\Users\deads\.config\solana\id.json` ✓
- **RPC**: Helius (configured in .env) ✓
- **Config**: Updated to v1.6.0 standards ✓
- **AI Keys**: Groq, OpenRouter, Anthropic, Cerebras ✓

## Quick Launch Scripts

### 1. Verify Setup
```bash
verify-setup.bat
```
Checks all prerequisites before running.

### 2. Build Binaries (First Time Only)
```bash
build.bat
```
Compiles all binaries (~5-10 min first run).

### 3. Launch Dashboard
```bash
# Demo mode (no tokens/RPC needed)
start-dashboard-demo.bat

# Full mode (requires 250,000+ SCEMA tokens)
start-dashboard.bat
```

## Manual Commands

```bash
# Build everything
cargo build --release

# Run dashboard (demo mode)
cargo run --release --bin dashboard -- --demo

# Run dashboard (full mode)
cargo run --release --bin dashboard

# Run sniper standalone
cargo run --release --bin sniper

# Run arbitrage bot
cargo run --release --bin arb
```

## v1.6.0 Key Features Configured

### Exit Gate System
- **Guaranteed ≥0.05 SOL exits**: All momentum exits gated behind 500% TP
- **Swell-based exit**: Trailing stop tightens to 2% when vault draining
- **Profit floor**: Once TP hit, stop-loss locks at that level permanently

### Social Link Enrichment
- Reads Metaplex on-chain metadata
- Fetches off-chain URI JSON for social links
- Pool scorer applies −4 to +10 boost based on social count
- Set `check_socials = true` to hard-reject tokens with zero socials

### Momentum Escalation
- 7-round ladder: 175→315→567→1020→1836→3305→5949%
- Escalation factor: 1.8× per round
- Adaptive pullback: 8 × √(1 + peak/100)

### Pool Quality
- `min_pool_score = 65`: Bayesian scorer calibrated on 834 trades
- `min_pool_size = 10.0`: Sweet spot 10-25 SOL
- `no_pump_timeout_secs = 30`: Recycle dead capital faster

## Dashboard Navigation

| Tab | Key | Description |
|-----|-----|-------------|
| Overview | `Tab` | SOL balance, SCEMA, positions, PnL, NN status |
| Trades | `x` | Export trades to CSV |
| Logs | `e` | Toggle Sell Mode (pause buys, sell all) |
| Logs | `d` | Toggle Dump Mode (force-sell at zero slippage) |
| Control | `s` | Start Sniper |
| Control | `a` | Start Arb |
| Control | `b` | Start Both |
| Control | `x` | Stop all bots |

### Rate Modes (Control Tab)
| Key | Mode | Multiplier | TP | SL |
|-----|------|------------|----|----|
| `1` | Bearish | 0.3× | 30% | 8% |
| `2` | Micro | 0.1× | 40% | 10% |
| `3` | Safe | 0.5× | 50% | 10% |
| `4` | Balanced | 1.0× | 100% | 15% |
| `5` | Aggressive | 2.0× | 200% | 25% |
| `6` | Degen | 4.0× | 300% | 40% |
| `7` | Bullish | 6.0× | 500% | 50% |

### Builder Modes (Control Tab)
| Key | Mode | Target | Scaling |
|-----|------|--------|---------|
| `g` | Growth | 0.2 SOL | 1.0–2.0× geometric |
| `j` | Builder | 1.0 SOL | 1.5–3.5× geometric + TP scaling |
| `k` | SuperBuilder | 3.0 SOL | 2.0–8.0× parabolic + auto moon-chase |

## Important Notes

### Token Gate
- Requires **≥250,000 SCEMA** tokens in your wallet
- CA: `AbKiP2Jc6nM7937jTDfqoJC1bsg5FQ24Buk2iqRFpump`
- Emergency bypass: Set `SCEMATICA_SKIP_GATE=1` in .env (RPC outages only)

### File-Based IPC
All bot communication happens through JSON files:
- `scematica-sniper.log` - Log stream
- `scematica-trades.jsonl` - Trade history (rotates at 10k lines)
- `scematica-metrics.json` - Live metrics (every 5s)
- `scematica-positions.json` - Open positions
- `scematica-nn-stats.json` - Neural network stats
- `scematica-rate-mode.json` - Active rate mode
- `scematica-sell-mode.json` - Sell mode state
- `scematica-dump-mode.json` - Dump mode state

### Recommended Settings for Beginners

Start with **Safe mode** (`3` key):
- 0.5× position size (0.005 SOL per trade)
- 50% take profit
- 10% stop loss
- Lower risk while learning

Once comfortable, move to **Balanced mode** (`4` key):
- 1.0× position size (0.01 SOL per trade)
- 100% take profit
- 15% stop loss
- Default configuration

## Troubleshooting

### "SCEMA token gate failed"
- Check wallet balance: `solana balance`
- Verify you have ≥250,000 SCEMA tokens
- Check RPC connection in .env

### "RPC connection failed"
- Verify Helius API key in .env
- Check internet connection
- Try backup RPC in `extra_rpc_endpoints`

### "Build failed"
- Update Rust: `rustup update`
- Clean build: `cargo clean && cargo build --release`
- Check disk space (needs 5-10 GB)

### Dashboard not showing positions
- Check `scematica-positions.json` exists
- Verify sniper is running
- Restart dashboard

## Support & Resources

- **README**: Full documentation in README.md
- **Beginner Guide**: BEGINNER_GUIDE.md for step-by-step setup
- **Equations**: EQUATIONS_AND_STRATEGIES.md for math details
- **Whitepaper**: WHITEPAPER.md for architecture overview

## Next Steps

1. Run `verify-setup.bat` to confirm everything is ready
2. Run `build.bat` to compile binaries (first time only)
3. Test with `start-dashboard-demo.bat` (no tokens needed)
4. Acquire 250,000+ SCEMA tokens
5. Launch full mode with `start-dashboard.bat`
6. Start with Safe mode (`3` key) and small positions
7. Monitor trades in the Trades tab
8. Adjust settings in config.toml as needed

Good luck! 🚀
