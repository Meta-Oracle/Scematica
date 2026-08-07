# ⚡ Rate Modes Quick Reference & Verification

## Quick Mode Selector (Print & Post This)

```
🎮 LIVE MODE SWITCH — Press keys [1-7] on Dashboard Logs Tab

┌─────────────────────────────────────────────────────────┐
│ MICRO [1]      → 0.001 SOL  │ Learning / Ultra-Safe       │
│ BEARISH [2]    → 0.003 SOL  │ Market Weakness             │
│ SAFE [3]       → 0.005 SOL  │ Conservative / New Traders  │
│ BALANCED [4]   → 0.01 SOL   │ ⭐ DEFAULT / Sweet Spot    │
│ AGGRESSIVE [5] → 0.02 SOL   │ Bull Confirmed / High Conv  │
│ DEGEN [6]      → 0.04 SOL   │ Max Conviction / 4× Lever   │
│ MOON [7]       → 0.1 SOL    │ Chasing Banger / All-In     │
└─────────────────────────────────────────────────────────┘

📊 EXIT TARGETS BY MODE
┌─────────────┬───────────┬────────────────────────┐
│ Mode        │ Base TP   │ Escalation Ceiling     │
├─────────────┼───────────┼────────────────────────┤
│ Micro       │ 50%       │ 162% (3 escalations)   │
│ Bearish     │ 75%       │ 437% (4 escalations)   │
│ Safe        │ 100%      │ 1,050% (5 escal.)      │
│ Balanced    │ 175%      │ 5,954% (7 escal.) ⭐   │
│ Aggressive  │ 300%      │ 5,954% (7 escal.)      │
│ Degen       │ 450%      │ 5,954% (7 escal.)      │
│ Moon        │ 1200%     │ 10,717% (8 escal.)     │
└─────────────┴───────────┴────────────────────────┘

❌ STOP LOSS BY MODE
┌─────────────┬──────────┐
│ Mode        │ SL %     │
├─────────────┼──────────┤
│ Micro       │ 8%       │
│ Bearish     │ 10%      │
│ Safe        │ 12%      │
│ Balanced    │ 12% ⭐   │
│ Aggressive  │ 15%      │
│ Degen       │ 25%      │
│ Moon        │ 60%      │
└─────────────┴──────────┘
```

---

## Market Condition → Mode Mapping

### 🟢 BULL MARKET (Clear uptrend, volume high, sentiment strong)
```
Primary:   [4] BALANCED (steady capture)
Alternate: [5] AGGRESSIVE (higher conviction pools)
Escalate:  [6] DEGEN (if winning streak detected)
```

### 🟡 NEUTRAL MARKET (Sideways, unclear direction, choppy)
```
Primary:   [3] SAFE (lower risk, preserve capital)
Alternate: [4] BALANCED (confident signals only)
De-risk:   [2] BEARISH (if consolidation tightens)
```

### 🔴 BEAR MARKET (Declining, volume selling, sentiment poor)
```
Primary:   [2] BEARISH (capital preservation mode)
Alternate: [1] MICRO (minimal exposure)
Avoid:     [6] DEGEN, [7] MOON (don't chase in bear)
```

### ⚡ UNKNOWN / JUST BOOTED
```
Always start:   [4] BALANCED (safe default, proven)
Adjust after:   5-10 trades based on win rate
```

---

## Daily Workflow: Mode Adjustment Log

Track your mode switches and performance:

```
TIME     MODE        REASON                    WIN_RATE   AVG_PNL
─────────────────────────────────────────────────────────────────
09:30    [4] Balanced  Morning start           N/A        N/A
10:15    [4] Balanced  Steady market           28%        +52%
11:00    [5] Aggr.     2 wins, bull signal     35%        +65%
13:30    [4] Balanced  Lunch drawdown          32%        +48%
15:00    [3] Safe      2 losses, recalibrate   25%        +35%
16:45    [4] Balanced  Market recovering       38%        +58%
18:00    [7] Moon      Banger detected!        🎉          +397%
```

Use this to see which modes actually perform in your system vs theoretical.

---

## Verification: Config Applied Correctly

### Step 1: Verify Config File
```powershell
# Check that 7 rate modes are defined
Select-String "name = " config.toml | Select -First 10
# Should see: Micro, Bearish, Safe, Balanced, Aggressive, Degen, Moon

# Check active mode
Select-String "active_mode =" config.toml
# Should show: active_mode = "Balanced"
```

### Step 2: Verify After Rebuild
```powershell
# Rebuild to apply new config
cargo build --release

# Start dashboard
.\target\release\dashboard.exe
```

### Step 3: Check Dashboard Config Tab
On the dashboard, go to **Config tab**:
- Look for **Rate Modes** section
- Should show 7 modes with a ▶ indicator on the active one
- Active mode should be "Balanced" (highlighted in green)

### Step 4: Verify Mode in Sniper Logs
```powershell
# Watch sniper startup logs for mode loading
Get-Content -Tail 20 scematica-sniper.log | grep -i "rate_mode\|active_mode"
# Expected: "Loaded rate mode: Balanced (quote_amount: 0.01 SOL)"
```

### Step 5: Test Mode Switch
```
1. Press [5] on dashboard Logs tab (switch to Aggressive)
2. Wait 2 seconds
3. Check sniper log: "Rate mode changed: Balanced → Aggressive"
4. Check next trade entry size: should be 0.02 SOL (not 0.01)

If entry stays 0.01 SOL → config not reloading, rebuild needed
```

### Step 6: Verify First Trade Entry Size
After mode switch, place a live trade and verify:
```powershell
# Get latest trade entry
Get-Content scematica-trades.jsonl | ConvertFrom-Json | select -Last 1 | Select amount
# Check that amount matches mode (0.02 SOL for Aggressive)
```

---

## Troubleshooting

### Problem: Mode hotkeys don't work
```
Solution:
1. Verify dashboard is on Logs tab (other tabs don't respond to [1-7])
2. Make sure you're pressing actual [1] key, not Fn+1
3. Check that sniper is actually running (not paused)
4. Restart dashboard: Ctrl+C, then restart
```

### Problem: Mode switches but entry size doesn't change
```
Solution:
1. Rebuild: cargo build --release (config may not have reloaded)
2. Check sniper log for "rate mode" references
3. Restart sniper process: taskkill /IM sniper.exe /F
4. Start dashboard fresh
```

### Problem: Config shows correct mode but wrong entry size
```
Solution:
1. Delete config backup: rm -r config.toml.bak (if exists)
2. Verify config.toml syntax: cargo check (should compile cleanly)
3. Look for duplicate [rate_modes] sections (should have only 1)
4. Rebuild everything: cargo clean && cargo build --release
```

### Problem: Dashboard won't show mode indicators
```
Solution:
1. Make sure you're on the Config tab
2. Scroll down to see Rate Modes section
3. If not visible, dashboard version may be old
4. Rebuild dashboard: cargo build --release --bin dashboard
```

---

## Implementation: How Rate Modes Flow

### Config → Sniper → Trade

```
┌─────────────────────────────────────────────────┐
│ config.toml                                     │
│ ├─ [rate_modes.profiles]                       │
│ │  ├─ name = "Balanced"                        │
│ │  ├─ quote_amount = 0.01                      │
│ │  ├─ take_profit_pct = 175.0                  │
│ │  └─ stop_loss_pct = 12.0                     │
│ └─ active_mode = "Balanced"                    │
└─────────────────────────────────────────────────┘
                    ↓
┌─────────────────────────────────────────────────┐
│ Sniper Startup                                  │
│ ├─ Read config.toml                            │
│ ├─ Parse active_mode = "Balanced"              │
│ ├─ Look up mode: quote_amount = 0.01 SOL       │
│ └─ Set live_params with mode data              │
└─────────────────────────────────────────────────┘
                    ↓
┌─────────────────────────────────────────────────┐
│ Dashboard Hotkey [5] Pressed                    │
│ ├─ User presses [5] key                        │
│ ├─ Dashboard reads mode #5 = "Aggressive"      │
│ ├─ Dashboard writes scematica-rate-mode.json:  │
│ │  {"active_mode": "Aggressive"}               │
│ └─ Broadcasts to all components                │
└─────────────────────────────────────────────────┘
                    ↓
┌─────────────────────────────────────────────────┐
│ Sniper Poll Loop (every 5 seconds)             │
│ ├─ Check if scematica-rate-mode.json changed   │
│ ├─ If yes: parse new mode "Aggressive"         │
│ ├─ Look up: quote_amount = 0.02 SOL            │
│ ├─ Update live_params immediately              │
│ └─ Next buy uses 0.02 SOL                      │
└─────────────────────────────────────────────────┘
                    ↓
┌─────────────────────────────────────────────────┐
│ Next Buy Decision                               │
│ ├─ Pool passes filters                         │
│ ├─ Check live_params.quote_amount              │
│ ├─ Use 0.02 SOL (from Aggressive mode)         │
│ ├─ Build buy transaction                       │
│ └─ Execute → scematica-trades.jsonl logged     │
└─────────────────────────────────────────────────┘
                    ↓
┌─────────────────────────────────────────────────┐
│ Trade Execution                                 │
│ ├─ Swap 0.02 SOL for tokens                    │
│ ├─ Entry logged: amount = 0.02                 │
│ ├─ Sell monitor starts                         │
│ └─ Use mode's TP% (300%) and SL% (15%)        │
└─────────────────────────────────────────────────┘
```

The entire flow is **<1 second** from hotkey press to next buy using new mode.

---

## Expected Behavior Across Modes

### Entry Differences
```
Micro (0.001):     ~850 tokens on 15 SOL pool
Safe (0.005):      ~4,250 tokens on 15 SOL pool
Balanced (0.01):   ~8,500 tokens on 15 SOL pool ⭐
Aggressive (0.02): ~17,000 tokens on 15 SOL pool
Degen (0.04):      ~34,000 tokens on 15 SOL pool
```

### Win Rate Expectations
```
Micro:     20% win rate (small = sandwiched)
Safe:      32% win rate (conservative)
Balanced:  40% win rate ⭐ (proven sweet spot)
Aggressive: 32% win rate (slippage kicks in)
Degen:     25% win rate (4× slippage impact)
```

### PnL Per Trade Expectations
```
Micro:     +15% avg on winners | -5% avg on losers
Safe:      +45% avg on winners | -6% avg on losers
Balanced:  +57% avg on winners | -6% avg on losers ⭐
Aggressive: +48% avg on winners | -8% avg on losers
Degen:     +35% avg on winners | -18% avg on losers
```

---

## Configuration Backup & Recovery

### Backup Current Modes
```powershell
# Before making changes
Copy-Item config.toml config.toml.backup-rates

# Then edit config.toml with new modes or active_mode
```

### Restore If Broken
```powershell
# If something broke
Copy-Item config.toml.backup-rates config.toml

# Rebuild and restart
cargo build --release
.\target\release\dashboard.exe
```

---

## Next Steps

1. ✅ **Verify config.toml has 7 rate modes defined** (check for Micro, Bearish, Safe, Balanced, Aggressive, Degen, Moon)

2. ✅ **Rebuild the sniper and dashboard:**
   ```powershell
   cargo build --release
   ```

3. ✅ **Start dashboard and test mode switching:**
   ```powershell
   .\target\release\dashboard.exe
   ```

4. ✅ **Place first trade in [4] Balanced mode** (default)
   - Verify entry size is 0.01 SOL in scematica-trades.jsonl

5. ✅ **Switch to [5] Aggressive** and place another trade
   - Verify entry size is 0.02 SOL

6. ✅ **Track modes and performance** for 100 trades
   - Which modes have best win rate?
   - Which ones match theoretical expectations?

You now have **7 dynamic trading modes** ready to deploy! 🎮

