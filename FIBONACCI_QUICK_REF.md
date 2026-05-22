# Fibonacci Recovery - Quick Reference Card

## 🎯 Your Goal
**Recover 0.09 SOL loss and reach 1 SOL in 50-100 trades**

---

## 📊 Success Metrics (Check Every 10 Trades)

### Entry Quality
- ✅ **Entry Rejection Rate**: >75% (should be entering ~2 out of 10 pools)
- ✅ **Avg Fibonacci Score**: >0.80 (only high-quality pools)
- ✅ **Pool Size Range**: 8-21 SOL (Fibonacci sweet spot)

### Exit Performance  
- ✅ **Win Rate**: >35% (target: 40%+)
- ✅ **Avg Win**: >+150% (Fibonacci TP levels: 61.8%, 161.8%, 261.8%)
- ✅ **Avg Loss**: <-0.6% (dead pool fast exit at 3s)
- ✅ **Dead Pool Exits**: <5 seconds hold time
- ✅ **Winning Exits**: <10 seconds hold time

### PnL Tracking
- ✅ **Net PnL**: Positive after 10 trades
- ✅ **ROI per 100 trades**: >+100% (target: +138%)
- ✅ **Break-Even Point**: 7-13 trades

---

## 🚨 Red Flags (Stop and Debug If You See These)

### Entry Issues
- ❌ **Entering >50% of pools** → Fibonacci gate not working
  - **Fix**: Check `min_entry_score` is set to 0.75
  - **Debug**: Log Fibonacci scores of rejected pools

- ❌ **Avg Fibonacci score <0.75** → Entering low-quality pools
  - **Fix**: Raise `min_entry_score` to 0.80
  - **Debug**: Check pool size filter (should be 8-21 SOL)

### Exit Issues
- ❌ **Losses averaging >-0.8%** → Dead pool exit not firing
  - **Fix**: Check `no_pump_timeout_secs` is set to 3
  - **Debug**: Log hold time for losing trades

- ❌ **Wins averaging <+100%** → Exiting too early
  - **Fix**: Check Fibonacci TP levels (61.8%, 161.8%, 261.8%)
  - **Debug**: Log exit reason for winning trades

- ❌ **Hold time >30 seconds** → Exits not firing
  - **Fix**: Check Fibonacci momentum system is active
  - **Debug**: Log Fibonacci signals in sell monitor

### PnL Issues
- ❌ **Net PnL negative after 20 trades** → System not working
  - **Fix**: Revert to default config and debug integration
  - **Debug**: Check all 3 components (entry gate, dead pool exit, TP levels)

---

## 📈 Recovery Milestones

### Milestone 1: Break Even (7-13 trades)
- **Target**: Recover 0.09 SOL loss
- **Time**: 1-4 hours
- **Metrics**: Win rate >30%, avg loss <-0.6%

### Milestone 2: Double Capital (50 trades)
- **Target**: 0.1 SOL → 0.2 SOL
- **Time**: 5-10 hours  
- **Metrics**: Win rate >35%, ROI >+100%

### Milestone 3: 10× Capital (100 trades)
- **Target**: 0.1 SOL → 1.0 SOL
- **Time**: 10-20 hours
- **Metrics**: Win rate >40%, ROI >+138%, Fibonacci compounding active

---

## 🔧 Quick Adjustments

### If Too Few Entries (<10% of pools)
```toml
min_entry_score = 0.70  # Lower from 0.75
min_pool_size = 6.5     # Lower from 8.0
```

### If Too Many Losses (Win rate <25%)
```toml
min_entry_score = 0.80  # Raise from 0.75
min_pool_size = 10.0    # Raise from 8.0
no_pump_timeout_secs = 2  # Faster exit from 3
```

### If Wins Too Small (Avg <+100%)
```toml
# Check Fibonacci TP levels in code
# Should be: 61.8%, 161.8%, 261.8%
# NOT: 500% (too high, never hit)
```

### If Losses Too Large (Avg >-0.8%)
```toml
no_pump_timeout_secs = 2  # Faster exit from 3
no_pump_min_gain_pct = 3.0  # Lower from 5.0
```

---

## 📝 Logging Checklist

Add these debug logs to monitor Fibonacci system:

```rust
// On pool detection
info!("🔍 Pool: {:.1} SOL, {}s old, {:.2} SOL/s velocity, {:.2} pressure",
    size_sol, age_secs, velocity, buy_pressure);

// On entry decision
info!("📊 Fibonacci Score: {:.2} (threshold: {:.2})",
    fib_score, min_entry_score);

// On entry
info!("✅ ENTER: Score {:.2}, Multiplier {:.2}×, Expected TP {:.0}%",
    fib_score, position_multiplier, expected_tp_pct);

// On exit
info!("🚪 EXIT: {} | Hold: {}s | PnL: {:.1}%",
    exit_reason, hold_time_secs, pnl_pct);

// Every 10 trades
info!("📊 Stats: Entries={}, Wins={}, Losses={}, Win%={:.1}, Avg PnL={:.4} SOL",
    total_entries, wins, losses, win_rate * 100.0, avg_pnl_sol);
```

---

## 🎓 Understanding Fibonacci Patterns

### Golden Ratio (φ = 1.618)
- **Pool Velocity**: >1.618 SOL/s = strong momentum
- **Buy Pressure**: >1.618 ratio = heavy buying
- **Position Sizing**: 1.618× for high-score pools

### Fibonacci Sequence (1, 1, 2, 3, 5, 8, 13, 21...)
- **Pool Size**: 8-21 SOL (F(6) to F(8)) = sweet spot
- **Age**: <5 seconds (F(5)) = ultra-fresh
- **Timeout**: 3 seconds (F(4)) = dead pool exit

### Fibonacci Retracements
- **61.8%** (φ⁻¹): First TP level, golden retracement
- **38.2%** (1 - φ⁻¹): Pullback tolerance before exit
- **23.6%**: Shallow pullback (hold)

### Fibonacci Extensions
- **161.8%** (φ × 100): Second TP level
- **261.8%** (φ² × 100): Third TP level  
- **423.6%** (φ³ × 100): Moon shot target

---

## 🚀 Next Actions

1. **Backup current config**: `copy config.toml config-backup.toml`
2. **Switch to Fibonacci config**: `copy config-fibonacci-recovery.toml config.toml`
3. **Integrate Fibonacci system**: Follow `FIBONACCI_INTEGRATION_GUIDE.md`
4. **Start bot with small size**: `quote_amount = 0.005`
5. **Monitor first 10 trades**: Check metrics above
6. **Adjust if needed**: Use quick adjustments section
7. **Scale up after 20 trades**: If win rate >30%, increase `quote_amount`

---

## 💡 Pro Tips

1. **Don't panic on first 5 trades** - variance is high, need 10+ for signal
2. **Dead pool exits are GOOD** - losing 0.5% fast is better than -0.9% slow
3. **Fibonacci compounding kicks in after 3 wins** - be patient
4. **High rejection rate is GOOD** - 80% filtered = 80% fewer losses
5. **Fast exits are GOOD** - winners exit in <10s, losers in <5s

---

## 📞 Debug Checklist

If not working after 20 trades:

- [ ] Fibonacci recovery system integrated in sniper code?
- [ ] Config using `config-fibonacci-recovery.toml`?
- [ ] Entry rejection rate >75%?
- [ ] Dead pool exits happening <5 seconds?
- [ ] Fibonacci TP levels set correctly (not 500%)?
- [ ] Position sizing scaling with Fibonacci score?
- [ ] Logs showing Fibonacci scores and exit reasons?

If all checked and still not working, review `FIBONACCI_INTEGRATION_GUIDE.md` Step 1-4.

---

**Remember**: The Fibonacci system is designed specifically for YOUR live data patterns. Trust the math, monitor the metrics, and give it 20 trades to prove itself. Good luck! 🍀
