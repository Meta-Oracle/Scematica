# Scematica v1.6.0 Setup - Changes Summary

## Files Updated

### 1. `.env` - Environment Configuration ✓
**Added:**
- `RPC_ENDPOINT` - Helius mainnet RPC URL
- `RPC_WS_ENDPOINT` - Helius WebSocket URL
- Emergency gate bypass comment

**Kept:**
- All existing AI API keys (Groq, OpenRouter, Anthropic, Cerebras)

### 2. `config.toml` - Main Configuration ✓
**Updated to v1.6.0 standards:**

#### Exit Gate System (v1.6.0)
- `take_profit_pct = 500.0` - Exit gate threshold
- `stop_loss_pct = 18.0` - Raised from 8% to match README
- `trailing_stop_loss_pct = 12.0` - Tightens to 2% on swell signal
- `momentum_min_peak_pct = 500.0` - Pullback gate
- `velocity_decay_min_pnl_pct = 500.0` - Decay gate
- `volume_exhaustion_pct = 0.0` - Disabled (swell handles this)
- `whale_exit_vault_drop_pct = 0.0` - Disabled in profit zone
- `flash_crash_pct = 0.0` - Disabled in profit zone
- `profit_lock_checks = 0` - Disabled (code handles floor)

#### Momentum Escalation (v1.4.0)
- `momentum_hold = true`
- `momentum_max_escalations = 7` - 7-round ladder
- `momentum_escalation_factor = 1.8` - 1.8× per round
- `momentum_escalation_threshold_pct = 3.0` - Lower velocity bar
- `momentum_pullback_exit_pct = 8.0` - Adaptive formula base

#### Dead-Zone Exit (v1.5.2)
- `no_pump_timeout_secs = 30` - Reduced from 45s
- `no_pump_min_gain_pct = 3.0` - Peak gain threshold

#### Fresh Position Protection (v1.5.2)
- `min_dump_hold_secs = 90` - Protect from dump-mode force-sell

#### Pool Quality (v1.5.2)
- `min_pool_score = 65` - Bayesian scorer threshold
- `min_pool_size = 10.0` - Sweet spot 10-25 SOL
- `max_pool_size = 50.0` - Reject mega-pools

#### Kelly Sizing (v1.1.0)
- `kelly_sizing = false`
- `kelly_fraction = 0.25`
- `kelly_lookback = 20`
- `kelly_min_trades = 10`

#### Session Heat Cooldown (v1.1.0)
- `session_heat_losses = 0` - Disabled by default
- `session_heat_window_secs = 3600`
- `session_heat_cooldown_mins = 15`

#### Buy Improvements (v1.1.0)
- `min_sol_reserve = 0.02` - Keep gas money
- `confirmation_window_ms = 0` - Disabled by default
- `pool_quality_sizing = false`
- `check_interval_acceleration = true`

#### Builder Modes (v1.4.0)
- `profit_first_mode = true`
- `profit_first_floor_pct = 25.0` - Exit rugs faster
- `wallet_target_sol = 0.15`

#### Risk Breakers (v1.4.0)
- `ath_drawdown_pct = 0.0`
- `grief_loss_window_secs = 300`
- `grief_loss_limit_sol = 0.0`

#### Multi-RPC Failover
- `extra_rpc_endpoints = []` - Add backups here

#### Filter Configuration
**Enabled:**
- `check_mint_renounced = true` - Re-enabled with social enrichment
- `check_freezable = true`
- `check_burned = true`
- `check_mutable = true`
- `check_name = true` - Scam-word filter
- `check_volume = true` - Transaction activity
- `check_liquidity_depth = true`
- `check_holder_concentration = true` - With 67% threshold
- `check_liquidity_momentum = true`
- `check_cross_pool_correlation = true`

**Social Enrichment (v1.6.0):**
- `check_socials = false` - Set true to require social links
- Scorer applies −4 to +10 boost based on social count

**Deployer Quality:**
- `check_deployer_wallet_age = false` - Disabled (pump.fun uses fresh wallets)
- `deployer_min_age_hours = 24` - Only used if enabled
- `max_deployer_rugs_24h = 2` - Reputation-based blocking

**Thresholds:**
- `max_price_impact_pct = 3.5` - Tightened from 5.0%
- `max_top10_holder_pct = 67.0` - Calibrated threshold
- `filter_cache_ttl_secs = 30` - Cache results

#### Tiered Partial TP
- `tiered_partial_tp = false` - Disabled for 500% full-exit strategy
- Configuration structure added (commented out)

## New Files Created

### 3. `verify-setup.bat` ✓
Checks all prerequisites:
- Rust installation
- Solana CLI (optional)
- Wallet keypair
- .env file and RPC configuration
- config.toml
- Compiled binaries

### 4. `build.bat` ✓
Compiles all binaries in release mode:
- dashboard.exe
- sniper.exe
- arb.exe
- scematica-protocol.exe

### 5. `start-dashboard.bat` ✓
Launches dashboard in full mode (requires SCEMA tokens).

### 6. `start-dashboard-demo.bat` ✓
Launches dashboard in demo mode (no tokens/RPC needed).

### 7. `QUICKSTART.md` ✓
Comprehensive quick start guide with:
- Setup status checklist
- Launch script documentation
- v1.6.0 feature overview
- Dashboard navigation reference
- Rate mode and builder mode tables
- Troubleshooting guide
- Next steps

## Configuration Philosophy

### v1.6.0 Exit Strategy
**Goal**: Guarantee ≥0.05 SOL profit per winning trade

**Math**:
- Buy: 0.01 SOL
- Target: 500% gain (6× multiplier)
- Exit value: 0.06 SOL
- Net profit: 0.05 SOL

**Implementation**:
1. All momentum exits (trailing stop, pullback, velocity decay, volume exhaustion, whale exit, flash crash) are **gated** behind 500% TP
2. Position holds through market noise below 500%
3. Once TP hit, profit floor locks at that level
4. Subsequent exits guaranteed ≥0.05 SOL
5. Hard SL (18%) and no-pump timeout (30s) exempt from gate

### Swell-Based Exit
- Monitors 6-check sliding window of quote vault deltas
- When vault draining AND position at/above TP:
  - Trailing stop tightens from 12% → 2%
  - Locks gains before reversal completes

### Social Link Enrichment
- Reads Metaplex on-chain metadata (real name/symbol)
- Fetches off-chain URI JSON (1.5s timeout)
- Checks for Twitter, Telegram, website, Discord
- Pool scorer boost: −4 (zero socials) → +10 (all four)
- AI receives real token info for context-aware analysis
- Optional hard-reject: `check_socials = true`

### Pool Quality Scoring
**Bayesian model calibrated on 834 trades:**
- Score 65 = P(win)≈0.09 (requires 6+ SOL + fresh + velocity)
- Score 75 = P(win)≈0.18 (sweet spot + ultra-fresh + buy pressure)
- Score 85 = P(win)≈0.35 (8-20 SOL + <10s + strong velocity)

**Components:**
- Pool size sweet spot: 10-25 SOL
- Age freshness: <60s detection time
- Velocity bonus: SOL/second inflow rate
- Buy-pressure ratio: quote_vault / base_vault

### Filter Pipeline
**Cost-ordered execution** (cheapest first):
1. In-memory blacklist
2. Freeze authority check
3. Mint renounce check
4. LP burn check
5. Pool size check
6. Liquidity depth check
7. Name scam-word filter
8. Volume check
9. Cross-pool correlation
10. Deployer age check (disabled by default)
11. Holder concentration
12. Liquidity momentum
13. Jupiter integration

**Cache**: 30s TTL per pool to avoid redundant RPC calls

## Recommended Workflow

### First Time Setup
1. Run `verify-setup.bat` - Check prerequisites
2. Run `build.bat` - Compile binaries (~5-10 min)
3. Run `start-dashboard-demo.bat` - Test in demo mode
4. Acquire 250,000+ SCEMA tokens
5. Run `start-dashboard.bat` - Launch full mode

### Daily Operation
1. Launch dashboard: `start-dashboard.bat`
2. Start with Safe mode (`3` key) - 0.5× size, 50% TP, 10% SL
3. Monitor trades in Trades tab
4. Check logs in Logs tab
5. Adjust rate mode as market conditions change
6. Use Sell Mode (`e` key) to pause and exit all positions
7. Use Dump Mode (`d` key) for emergency exits

### Configuration Tuning
1. Start with default settings
2. Monitor win rate and average PnL in Overview tab
3. Adjust `min_pool_score` if too many/few buys
4. Adjust `min_pool_size` based on liquidity observations
5. Enable `check_socials` if anonymous tokens underperform
6. Tune `no_pump_timeout_secs` based on dead position frequency
7. Enable Kelly sizing after 10+ trades for dynamic position sizing

## Key Differences from Previous Versions

### v1.6.0 vs v1.5.x
- **Exit gate**: All momentum exits gated behind 500% TP
- **Swell signal**: Trailing stop tightens to 2% on vault drain
- **Social enrichment**: Metaplex + off-chain URI parsing
- **Pool scorer boost**: −4 to +10 based on social count
- **Profit floor**: Locks at TP level permanently

### v1.5.x vs v1.4.x
- **Token-2022 support**: Startup scan includes Token-2022 program
- **Drain guard**: Raised 10k → 500k lamports
- **Retry acceleration**: 12s → 3s total retry rounds
- **Dead-zone exit**: 45s → 30s timeout
- **Deployer age filter**: Disabled by default

### v1.4.x vs v1.3.x
- **Base TP raised**: 80% → 175%
- **Momentum escalation**: 5 → 7 rounds, 1.6× → 1.8× factor
- **Adaptive pullback**: 18% → 8% base (tighter formula)
- **Builder modes**: Growth / Builder / SuperBuilder
- **Tiered partial TP**: Shifted up (100/300/600%)

## Files to Monitor

### During Operation
- `scematica-sniper.log` - Real-time log stream
- `scematica-trades.jsonl` - Trade history
- `scematica-positions.json` - Open positions
- `scematica-metrics.json` - Live metrics

### For Analysis
- `scematica-deployer-reputation.json` - Deployer rug history
- `scematica-filter-stats.json` - Per-filter rejection counts
- `scematica-nn-stats.json` - Neural network performance
- `pool-cache.json` - Pool → mint lookup cache

### For Control
- `scematica-rate-mode.json` - Active rate mode
- `scematica-sell-mode.json` - Sell mode state
- `scematica-dump-mode.json` - Dump mode state
- `scematica-builder-mode.json` - Builder mode state
- `scematica-moon-chase.json` - Moon chase mode state

## Security Notes

### API Keys
- Never commit `.env` to version control
- Rotate RPC keys periodically
- Use separate keys for testing vs production

### Wallet Safety
- Keep keypair file secure (never share)
- Use a dedicated trading wallet (not your main wallet)
- Start with small amounts to test
- Monitor wallet balance regularly

### RPC Limits
- Helius free tier: 100 req/s
- Add backup RPCs in `extra_rpc_endpoints`
- Monitor rate limit errors in logs
- Upgrade RPC plan if hitting limits

## Next Steps

1. **Verify setup**: Run `verify-setup.bat`
2. **Build binaries**: Run `build.bat` (first time only)
3. **Test demo mode**: Run `start-dashboard-demo.bat`
4. **Acquire SCEMA**: Get 250,000+ tokens
5. **Launch full mode**: Run `start-dashboard.bat`
6. **Start conservative**: Use Safe mode (`3` key)
7. **Monitor performance**: Check Overview and Trades tabs
8. **Tune configuration**: Adjust config.toml based on results
9. **Scale up**: Move to Balanced/Aggressive modes as comfortable

## Support

- **Documentation**: README.md, BEGINNER_GUIDE.md
- **Quick Reference**: QUICKSTART.md (this file)
- **Equations**: EQUATIONS_AND_STRATEGIES.md
- **Architecture**: WHITEPAPER.md

---

**Setup completed**: All files updated to v1.6.0 standards ✓

**Ready to launch**: Run `verify-setup.bat` to confirm ✓
