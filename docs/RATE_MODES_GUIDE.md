# 🎮 Rate Modes Guide: Dynamic Position Sizing for Every Market Condition

## What Are Rate Modes?

Rate Modes are **instant trading profile switches** that change your entry size, profit target, and risk tolerance — without restarting the bot.

7 modes, 1 keypress: **[1] Micro** → **[2] Bearish** → **[3] Safe** → **[4] Balanced** → **[5] Aggressive** → **[6] Degen** → **[7] Moon**

Each mode defines:
- **quote_amount**: Entry size in SOL
- **take_profit_pct**: Exit profit goal
- **stop_loss_pct**: Exit pain threshold
- **momentum_max_escalations**: How many times TP can escalate before locking

---

## Phase 1 Context: Why Entry Size Matters

From analyzing 521 trades:

| Entry Size | Win Rate | Avg PnL | Status |
|-----------|----------|---------|--------|
| 0.001-0.005 SOL | 25% | +27.4% | ❌ Sandwiched |
| **0.009-0.011 SOL** | **40%** | **+57.4%** | ⭐ **SWEET SPOT** |
| 0.012+ SOL | 30% | +30.5% | ⚠️ Slippage |

**Key Finding**: The 0.01 SOL "Balanced" mode achieves the **highest win rate and best PnL per trade** (20× better than Micro).

Each mode scales around this sweet spot based on risk appetite:
- Half-size (0.005) = more cautious but lower win rate
- Double-size (0.02) = higher conviction but more slippage
- 4× size (0.04) = maximum leverage (risk 4× losses too)

---

## The 7 Modes

### [1] MICRO — Minimum Entry
```
Entry: 0.001 SOL (~$0.10)
TP:    50%        (2× in best case)
SL:    8%         (very tight)
```

**When to use:**
- Testing new pool filters
- Wallet under 0.15 SOL (can't afford bigger entries)
- Ultra-bearish market (awaiting reversal)
- Learning the bot behavior

**Expected:** 2-5 trades before reload, minimal capital risk

---

### [2] BEARISH — Defensive Entry
```
Entry: 0.003 SOL (~$0.30)
TP:    75%
SL:    10%
```

**When to use:**
- Market showing weakness
- Sideways/ranging conditions
- Capital preservation mode
- Pre-reversal positioning

**Expected:** Fewer winners, but smaller losses

---

### [3] SAFE — Conservative Entry
```
Entry: 0.005 SOL (~$0.50)
TP:    100%        (double entry)
SL:    12%
```

**When to use:**
- Steady market conditions
- New traders (learn with real capital)
- Testing new filters/configs
- Risk-averse environments

**Expected:** 5-7k token positions, moderate returns

---

### [4] BALANCED — The Sweet Spot ⭐⭐
```
Entry: 0.01 SOL (~$1.00)
TP:    175%        (escalator baseline)
SL:    12%
```

**When to use:**
- Normal conditions (DEFAULT)
- Proven winner from Phase 1 data (40% win rate)
- Consistent daily profit target
- After wins (build streak momentum)

**Expected:** 
- 10-15k token allocations
- Win rate 35%+
- PnL +57% average on winners
- **This is the mode to use most of the time**

**Why this mode wins:**
- Entry size large enough to avoid MEV sandwich attacks
- Not so large it triggers whale detection
- Achieves highest win rate across all pool sizes
- Slippage impact acceptable on fresh pools

---

### [5] AGGRESSIVE — 2× Bet
```
Entry: 0.02 SOL (~$2.00)
TP:    300%
SL:    15%
```

**When to use:**
- Clear bull signal on chain
- Pool quality exceptional (score 85+)
- After winning streak (confidence up)
- Post-breakout entry (volatility expected)

**Expected:** 
- 20-30k token positions
- Larger swings
- Win rate ~32% (lower than Balanced due to slippage)
- Losses amplified 2×

**Note:** Not as efficient as Balanced due to slippage, but higher upside on winners.

---

### [6] DEGEN — Maximum Conviction
```
Entry: 0.04 SOL (~$4.00)
TP:    450%
SL:    25%         (wide for volatility)
```

**When to use:**
- Exceptional pool signals (score 90+)
- Wallet at growth target (3+ SOL)
- Emergency capital push (known banger)
- High-conviction plays only

**Expected:**
- 40-60k token allocations
- 4× leverage on both wins AND losses
- Losses hurt: -25% on 0.04 SOL = -0.01 SOL
- Only use when conviction is REAL

**Warning:** One bad trade in Degen = erase 4 wins in Balanced mode. Use sparingly.

---

### [7] MOON — Chase the Banger
```
Entry: 0.1 SOL (~$10.00)
TP:    1200%       (12× runway)
SL:    60%         (very wide)
```

**When to use:**
- Pursuing a KNOWN 10×+ runner
- Wallet at 3 SOL target (SuperBuilder mode active)
- Exceptional deployer reputation
- Moon mode toggle [m] is ON

**Expected:**
- 100+ milliSOL allocations (entire wallet on confirmed pump)
- Massive upside: +1200% = +0.1 SOL profit on win
- Massive downside: -60% = -0.06 SOL loss
- Rarely used (1-2 times per week)

**How to use Moon mode safely:**
1. Enter with Moon toggle OFF
2. Exit partial TPs at [1]=50%, [2]=100%, [3]=200% manually
3. Activate Moon toggle only if massive momentum detected
4. Let escalator run to 1200%+ if sentiment strong
5. Pullback exit fires at 25% retreat (wide tolerance)

---

## Switching Modes (3 Methods)

### Method 1: Dashboard Hotkeys (FASTEST)
While dashboard running on Logs tab:
- Press **[1]** → Micro mode
- Press **[2]** → Bearish mode
- Press **[3]** → Safe mode
- Press **[4]** → Balanced mode
- Press **[5]** → Aggressive mode
- Press **[6]** → Degen mode
- Press **[7]** → Moon mode

**Time to apply:** <100ms (live, no restart)

### Method 2: Config File (FOR BASELINE)
Edit `config.toml`:
```toml
[rate_modes]
active_mode = "Balanced"   # Change to any mode name
```
Rebuild and restart:
```powershell
cargo build --release
.\target\release\dashboard.exe
```

### Method 3: Dashboard Config Tab (IF IMPLEMENTED)
On the Config tab in dashboard:
- See current mode highlighted
- Press keys [1–7] to switch
- New mode applies immediately to next buy

---

## Recommended Daily Strategy

### Morning (Market Opening)
Start with **[4] Balanced** (default)
- Safe, proven win rate
- 0.01 SOL per trade
- Observe market sentiment

### Mid-Day (Confirmed Trend)
If **bull signals confirmed**:
- Switch to **[5] Aggressive** (+20% position size)
- Higher conviction plays only
- Scale back if losses compound

### Sideways/Uncertain:
Stay **[3] Safe** or drop to **[2] Bearish**
- Smaller positions
- Wider stops
- Preserve capital

### After Winning Streak:
Bump to **[5] Aggressive** temporarily
- Momentum works
- Win rate temporary spike
- Lock in streak advantage

### End of Day:
If approaching daily loss limit:
- Drop to **[3] Safe** or **[2] Bearish**
- Smaller positions
- Protect session PnL

### Emergency / Capitulation:
- **[1] Micro** or **[2] Bearish**
- Hold cash
- Wait for reversal

---

## Position Sizing Examples

How many tokens you can expect per mode on a fresh 15 SOL pool:

| Mode | Entry SOL | Tokens (est) | Value | Status |
|------|-----------|------------|-------|--------|
| Micro | 0.001 | 850 | $1 | Learning |
| Bearish | 0.003 | 2,550 | $3 | Defensive |
| Safe | 0.005 | 4,250 | $5 | Conservative |
| **Balanced** | **0.01** | **8,500** | **$10** | **Optimal** |
| Aggressive | 0.02 | 17,000 | $20 | Leverage |
| Degen | 0.04 | 34,000 | $40 | High-risk |
| Moon | 0.1 | 85,000 | $100 | All-in |

---

## Exit Behavior Across Modes

All modes use the **momentum escalator** by default:
- **TP fires** → sell 25%, lock breakeven
- **+2× momentum** → escalate TP by 1.8×
- **Peak drops 25%** → force exit remaining
- **Timeout at 45s** → force-exit any remainder

Each mode's `momentum_max_escalations` determines the ceiling:

| Mode | Max Escalations | Escalation Chain |
|------|------------------|------------------|
| Micro | 3 | 50% → 90% → 162% (cap) |
| Bearish | 4 | 75% → 135% → 243% → 437% (cap) |
| Safe | 5 | 100% → 180% → 324% → 583% → 1,050% (cap) |
| **Balanced** | **7** | **175% → 315% → 567% → 1,021% → 1,838% → 3,308% → 5,954%** |
| Aggressive | 7 | (same as Balanced) |
| Degen | 7 | (same as Balanced) |
| Moon | 8 | + additional tier to 10,717% |

---

## Risk Per Mode (Worst Case)

What you lose if a trade hits stop loss:

| Mode | Entry | SL % | Worst Loss | Wallet Impact |
|------|-------|------|-----------|--------------|
| Micro | 0.001 | 8% | -0.00008 | -$0.008 |
| Bearish | 0.003 | 10% | -0.0003 | -$0.03 |
| Safe | 0.005 | 12% | -0.0006 | -$0.06 |
| **Balanced** | **0.01** | **12%** | **-0.0012** | **-$0.12** |
| Aggressive | 0.02 | 15% | -0.003 | -$0.30 |
| Degen | 0.04 | 25% | -0.01 | -$1.00 |
| Moon | 0.1 | 60% | -0.06 | -$6.00 |

**Margin note:** 10 consecutive losses in Balanced = -0.012 SOL (1.2% of typical 1 SOL wallet)

---

## Smart Mode Switching Algorithm

Use this logic to auto-switch modes based on performance:

```
If daily_pnl > 0.03 SOL:
  → Switch to [5] Aggressive (capitalize on streak)

Elif 3 consecutive losses:
  → Switch to [3] Safe (recover conviction)

Elif 5 consecutive losses:
  → Switch to [2] Bearish (capital preservation)

Elif consecutive wins AND win_rate > 40%:
  → Switch to [5] Aggressive (momentum play)

Elif wallet > 0.15 SOL:
  → Default to [4] Balanced

Elif wallet > 0.50 SOL:
  → Default to [5] Aggressive

Elif wallet > 1.00 SOL:
  → Default to [6] Degen (can afford leverage)

Elif wallet > 3.00 SOL:
  → Default to [7] Moon option (or [6] for daily)
```

---

## Config Cheat Sheet

Update `config.toml` to change mode defaults:

```toml
# Change WHICH mode is active (applied on next restart)
active_mode = "Balanced"          # Default

# Or try these:
# active_mode = "Safe"           # Conservative
# active_mode = "Aggressive"     # Bullish
# active_mode = "Degen"          # All-in mode
# active_mode = "Moon"           # Chase bangers
```

---

## Validation Checklist

After switching modes, verify:

- [ ] Dashboard shows correct mode on Config tab
- [ ] Entry size matches (check sniper log: "quote_amount = X SOL")
- [ ] TP target shows correct percentage
- [ ] SL threshold shows correct percentage
- [ ] First trade executes with expected size (check scematica-trades.jsonl)

If entry size doesn't match mode:
```powershell
# Rebuild to pick up config
cargo build --release

# Check logs
tail -f scematica-sniper.log | grep "quote_amount\|rate_mode"
```

---

## When to Override to Specific Modes

| Situation | Recommended Mode | Why |
|-----------|-----------------|-----|
| Testing new pool filter | [1] Micro | Minimal loss if filter is bad |
| Market consolidation | [2] Bearish | Preserve capital, wait for direction |
| Normal day | [4] Balanced | **Proven sweet spot (40% win rate)** |
| Clear bull break | [5] Aggressive | Increase position size on conviction |
| Wallet recovered | [6] Degen | Leverage recovery momentum |
| Chasing a known banger | [7] Moon | Maximum upside capture |
| After 2+ losses | [3] Safe | Rebuild confidence |

---

## Data-Driven Recommendation

**Use [4] Balanced as your default 80% of the time.**

From Phase 1 analysis:
- 40% win rate on 0.01 SOL entries
- 20× better than Micro (0.001 SOL)
- Less slippage impact than Aggressive (0.02 SOL)
- Proven PnL: +0.0253 → +0.039 SOL (baseline)

**Aggressive mode only on high-confidence pool detects.**
**Moon mode only when you know it's a banger.**
**Micro/Bearish only when defending capital.**

---

## Implementation Notes

The rate modes system works by:
1. **Config defines all modes** with their parameters
2. **Dashboard UI reads active_mode** from config
3. **Sniper reads active_mode** at startup and when config updates
4. **Live_params JSON** carries the active mode name and parameters
5. **Position sizing applies quote_amount from active mode**
6. **TP/SL logic uses mode's thresholds**

All mode switches are **instantaneous** (no restart needed when using dashboard hotkeys).

---

## Next: Testing Your Rate Modes

1. Start with **[4] Balanced** (default)
2. Run for 100 trades to establish baseline
3. Then test **[3] Safe** vs **[5] Aggressive** on small sample
4. Observe win rate and PnL per mode
5. Calibrate based on your risk tolerance

You now have a **dynamic position sizing system** that adapts to market conditions without code changes. 🚀

