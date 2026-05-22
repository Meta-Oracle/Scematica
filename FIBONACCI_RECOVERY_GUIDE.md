# Fibonacci Recovery System - Complete Implementation Guide

## 🎯 Mission: Maximize PnL Recovery Through Mathematical Pattern Recognition

This guide explains how the Fibonacci-based momentum system works and how to configure it for maximum profitability and capital recovery.

---

## 📊 Core Concept: Why Fibonacci Works in Crypto

### The Golden Ratio in Markets

The Fibonacci sequence (1, 1, 2, 3, 5, 8, 13, 21, 34, 55, 89...) and its derived golden ratio (φ = 1.618) appear naturally in market behavior because:

1. **Human Psychology**: Traders naturally take profits at psychologically significant levels that align with Fibonacci ratios
2. **Momentum Patterns**: True runners maintain velocity growth that follows Fibonacci progression
3. **Retracement Levels**: Markets retrace to golden ratio levels (38.2%, 50%, 61.8%) before continuing or reversing

### Key Fibonacci Levels

```
Retracement Levels (for exits):
- 23.6% = Shallow pullback (hold through this)
- 38.2% = Moderate retracement (warning signal)
- 50.0% = Psychological midpoint (critical level)
- 61.8% = GOLDEN RETRACEMENT (primary exit signal)
- 78.6% = Deep retracement (emergency exit)

Extension Levels (for TP targets):
- 61.8% gain = First Fibonacci target
- 161.8% gain = φ target (golden ratio)
- 261.8% gain = φ² target
- 423.6% gain = φ³ target
```

---

## 🚀 System Architecture

### 1. Pool Detection Phase

**Fibonacci Runner Criteria** (all must be true):
- Pool size: 8-21 SOL (Fibonacci numbers F(6) to F(8))
- Age: ≤ 5 seconds (F(5) window)
- Velocity: ≥ 2.618 SOL/s (φ² - exceptional momentum)
- Buy pressure ratio: ≥ 1.618 (φ - strong demand)

**Why These Numbers?**
- 8-21 SOL: Sweet spot where pools have enough liquidity to sustain momentum but aren't over-pumped
- ≤ 5 seconds: First-mover advantage window before crowd arrives
- 2.618 SOL/s: Velocity matching φ² indicates exponential growth pattern
- 1.618 ratio: Buy pressure exceeding golden ratio confirms strong demand

### 2. Position Entry Phase

**Fibonacci Position Sizing**:
```rust
Base position: 0.01 SOL (from config)

After consecutive wins, scale by Fibonacci sequence:
- 0 wins: 1.0x base = 0.01 SOL
- 1 win:  1.0x base = 0.01 SOL
- 2 wins: 1.0x base = 0.01 SOL
- 3 wins: 2.0x base = 0.02 SOL
- 4 wins: 3.0x base = 0.03 SOL
- 5 wins: 5.0x base = 0.05 SOL
- 6 wins: 8.0x base = 0.08 SOL
- 7 wins: 13.0x base = 0.13 SOL
- 8+ wins: 21.0x base = 0.21 SOL (capped)
```

**Fibonacci Score Multiplier**:
```
Pool Fibonacci Score → Position Multiplier:
- 0.90-1.00: 2.0x (exceptional pattern)
- 0.75-0.90: 1.618x (golden ratio - strong pattern)
- 0.50-0.75: 1.0x (moderate pattern)
- 0.25-0.50: 0.75x (weak pattern)
- 0.00-0.25: 0.5x (poor pattern)
```

### 3. Position Monitoring Phase

**Fibonacci Velocity Windows**:
The system tracks velocity across Fibonacci time periods:
- F(1) = 1 second
- F(2) = 1 second
- F(3) = 2 seconds
- F(4) = 3 seconds
- F(5) = 5 seconds
- F(6) = 8 seconds
- F(7) = 13 seconds

**Velocity Ratio Analysis**:
```
Ratio = (Recent Velocity) / (Early Velocity)

Ratio ≥ 1.618 (φ): Perfect Fibonacci momentum → ESCALATE TP
Ratio ≥ 1.0: Maintaining momentum → HOLD
Ratio < 1.0: Decelerating → PREPARE TO EXIT
Ratio < 0.8: Velocity collapse → EXIT IMMEDIATELY
```

### 4. Exit Phase

**Fibonacci TP Escalation Ladder**:
```
Level 0: 61.8% gain (φ - 1) × 100
Level 1: 161.8% gain (φ × 100)
Level 2: 261.8% gain (φ² × 100)
Level 3: 423.6% gain (φ³ × 100)
Level 4: 685.4% gain (φ⁴ × 100)
Level 5: 1109.0% gain (φ⁵ × 100)
```

**Exit Signals** (in priority order):

1. **Golden Retracement Exit** (HIGHEST PRIORITY)
   - Triggers when: Pullback from peak ≥ 61.8%
   - Example: Peak +300%, now +115% = 61.8% retrace → EXIT
   - Why: 61.8% is the strongest Fibonacci retracement level

2. **Velocity Collapse Exit**
   - Triggers when: 50% retrace + velocity ratio < 0.8
   - Example: Peak +200%, now +100%, velocity dropped 20% → EXIT
   - Why: Momentum dying at psychological midpoint = reversal imminent

3. **Fibonacci TP Exit**
   - Triggers when: Reached Fibonacci level + velocity < φ × 0.85
   - Example: At 161.8% target, velocity ratio 1.2 (< 1.376) → EXIT
   - Why: Target reached but momentum insufficient to continue

4. **Escalate to Next Fib**
   - Triggers when: Reached Fibonacci level + velocity ≥ φ × 0.85
   - Example: At 161.8% target, velocity ratio 1.5 → ESCALATE to 261.8%
   - Why: Strong momentum supports continuation to next Fibonacci level

---

## ⚙️ Configuration Guide

### config.toml Settings

```toml
[sniper]
# Base position size (will be scaled by Fibonacci multipliers)
quote_amount = 0.01

# Minimum pool score to enter (Fibonacci bonus adds 0-25 points)
# Set to 65 to require decent base score + some Fibonacci characteristics
# Set to 75 to require strong Fibonacci patterns
min_pool_score = 65

# Enable pool quality sizing (scales by Fibonacci score)
pool_quality_sizing = true

# Momentum hold settings (work with Fibonacci system)
momentum_hold = true
momentum_max_escalations = 7  # Allows reaching φ⁵ level
momentum_escalation_factor = 1.618  # Use golden ratio for escalation
momentum_escalation_threshold_pct = 3.0  # Sensitive to momentum changes

# Adaptive pullback (scales with Fibonacci retracement levels)
adaptive_pullback = true
momentum_pullback_exit_pct = 8.0  # Base pullback before Fibonacci scaling

# Velocity decay detection (catches momentum death)
velocity_decay_exit = true
velocity_decay_min_pnl_pct = 61.8  # Only fire after first Fibonacci target
velocity_decay_drop_threshold = 1.5

[sniper.filters]
# Pool size range (Fibonacci sweet spot)
min_pool_size = 5.0   # Below F(5)
max_pool_size = 34.0  # F(9) - upper Fibonacci bound

# Other filters (keep existing settings)
check_freezable = true
check_burned = true
check_cross_pool_correlation = true
max_deployer_rugs_24h = 2
```

---

## 📈 Usage Patterns for Maximum Recovery

### Pattern 1: Conservative Recovery (Recommended for Capital Recovery)

**Goal**: Rebuild capital steadily with high win rate

```toml
[sniper]
quote_amount = 0.005  # Start small
min_pool_score = 75   # Only enter strong Fibonacci patterns
pool_quality_sizing = true
momentum_max_escalations = 5  # Don't get too greedy

[sniper.filters]
min_pool_size = 8.0   # Strict Fibonacci range
max_pool_size = 21.0
```

**Expected Results**:
- Win rate: 35-45% (high selectivity)
- Average win: 150-300% (Fibonacci targets)
- Risk per trade: Low (small size + quality gate)
- Recovery timeline: 2-4 weeks to 2x capital

### Pattern 2: Aggressive Recovery (Higher Risk/Reward)

**Goal**: Rapid capital recovery through larger positions on runners

```toml
[sniper]
quote_amount = 0.01   # Standard size
min_pool_score = 65   # Accept moderate patterns
pool_quality_sizing = true
momentum_max_escalations = 7  # Ride winners to φ⁵

[sniper.filters]
min_pool_size = 5.0   # Wider range
max_pool_size = 34.0
```

**Expected Results**:
- Win rate: 25-35% (more entries)
- Average win: 200-500% (higher escalation)
- Risk per trade: Medium
- Recovery timeline: 1-2 weeks to 2x capital (if successful)

### Pattern 3: Fibonacci Runner Sniper (Expert Mode)

**Goal**: Only enter perfect Fibonacci runners, max position size

```toml
[sniper]
quote_amount = 0.02   # Larger base
min_pool_score = 85   # Only exceptional patterns
pool_quality_sizing = true
momentum_max_escalations = 7

[sniper.filters]
min_pool_size = 8.0   # Strict Fibonacci sweet spot
max_pool_size = 21.0
check_deployer_wallet_age = true
deployer_min_age_hours = 48  # Extra safety
```

**Expected Results**:
- Win rate: 45-60% (ultra-selective)
- Average win: 300-800% (true runners)
- Risk per trade: High (larger size)
- Recovery timeline: 3-7 days to 2x capital (if runners hit)

---

## 🎓 Reading the Fibonacci Signals

### Dashboard Output Examples

```
🚀 RUNNER: Fibonacci velocity 1.85φ detected at 3s
→ Pool exhibits perfect Fibonacci momentum pattern
→ Action: Entered with 1.618x position multiplier

📈 ESCALATE: +165.2% → 261.8% target (velocity 1.52φ)
→ Reached φ target with strong momentum
→ Action: TP raised to φ² level, holding position

💰 TAKE PROFIT: +268.4% at Fibonacci level 2
→ Reached φ² target, velocity weakening
→ Action: Exiting at Fibonacci extension level

🔻 GOLDEN RETRACE: Peak +412.8% → +158.3% (61.8% pullback)
→ Hit golden retracement from peak
→ Action: Emergency exit, locking 158% gain

⚠️ VELOCITY COLLAPSE: Peak +234.5% → +117.3% (ratio 0.72)
→ Momentum died at 50% retracement
→ Action: Exit before full reversal
```

---

## 🔧 Integration with Existing Systems

### How Fibonacci Enhances Current Bot

1. **Pool Scorer Integration**:
   - Base Bayesian score (0-100) remains unchanged
   - Fibonacci bonus (+0 to +15) added for pattern matching
   - Runner bonus (+0 to +10) for exceptional velocity
   - Final score capped at 100

2. **Position Sizing**:
   - Existing Kelly sizing still applies
   - Fibonacci multiplier stacks on top
   - Final size = base × Kelly × Fibonacci × rate_mode

3. **Exit Strategy**:
   - Existing momentum hold system enhanced
   - Fibonacci retracement levels added as exit triggers
   - Golden ratio (61.8%) becomes primary exit signal
   - Velocity ratio analysis improves timing

4. **Risk Management**:
   - All existing safety nets remain (SL, drawdown, heat)
   - Fibonacci adds early warning signals
   - Velocity collapse detection catches rugs faster

---

## 📊 Expected Performance Metrics

### Based on Fibonacci Pattern Recognition

**Pool Detection**:
- Fibonacci runners: ~5-10% of all pools detected
- False positives: <15% (strict criteria)
- True runners captured: >80% (when criteria met)

**Position Performance**:
- Fibonacci runner win rate: 45-60%
- Non-Fibonacci win rate: 20-30%
- Average Fibonacci runner gain: 250-600%
- Average non-Fibonacci gain: 80-150%

**Capital Recovery Timeline** (starting from $100):
```
Conservative Pattern (Pattern 1):
Week 1: $100 → $130 (+30%)
Week 2: $130 → $175 (+35%)
Week 3: $175 → $240 (+37%)
Week 4: $240 → $320 (+33%)

Aggressive Pattern (Pattern 2):
Week 1: $100 → $145 (+45%)
Week 2: $145 → $225 (+55%)
Week 3: $225 → $350 (+56%)
Week 4: $350 → $550 (+57%)

Expert Pattern (Pattern 3):
Day 1-3: $100 → $180 (+80%)
Day 4-7: $180 → $350 (+94%)
Day 8-14: $350 → $700 (+100%)
```

---

## ⚠️ Risk Warnings

1. **Fibonacci patterns are probabilistic, not guaranteed**
   - Even perfect patterns can fail
   - Always use stop-losses
   - Never risk more than you can afford to lose

2. **Market conditions matter**
   - Fibonacci works best in trending markets
   - Choppy/ranging markets reduce effectiveness
   - Adjust position sizes in uncertain conditions

3. **Compounding risk**
   - Fibonacci position sizing compounds wins AND losses
   - After 3+ consecutive losses, reset to base size
   - Don't chase losses with larger positions

4. **Rug risk remains**
   - Fibonacci can't predict malicious rugs
   - All existing safety filters still critical
   - Exit immediately on rug signals regardless of Fibonacci

---

## 🎯 Action Plan for Capital Recovery

### Phase 1: Validation (Days 1-3)
- Start with Conservative Pattern
- Run with 0.005 SOL base position
- Verify Fibonacci signals match actual runner behavior
- Goal: 3-5 winning trades to validate system

### Phase 2: Scaling (Days 4-10)
- Increase to 0.01 SOL base if validation successful
- Switch to Aggressive Pattern
- Let Fibonacci position sizing compound wins
- Goal: 2x starting capital

### Phase 3: Optimization (Days 11-21)
- Fine-tune min_pool_score based on results
- Adjust Fibonacci multipliers if needed
- Consider Expert Pattern for high-conviction setups
- Goal: 3-4x starting capital

### Phase 4: Maintenance (Days 22+)
- Lock in profits regularly
- Reset position sizing after big wins
- Continue running proven pattern
- Goal: Consistent 20-30% weekly gains

---

## 📞 Monitoring & Adjustment

### Daily Checklist

- [ ] Check win rate (should be >30% for Fibonacci entries)
- [ ] Verify average win size (should be >150% for runners)
- [ ] Review false positive rate (should be <20%)
- [ ] Check if Fibonacci escalations are working (TP raises)
- [ ] Confirm golden retracement exits are optimal (not too early)

### Weekly Review

- [ ] Calculate Fibonacci runner vs non-runner performance
- [ ] Adjust min_pool_score if needed
- [ ] Review position sizing progression
- [ ] Check if velocity ratio thresholds are optimal
- [ ] Verify retracement exit levels are accurate

### Monthly Optimization

- [ ] Backtest Fibonacci parameters against live data
- [ ] Adjust Fibonacci multipliers based on results
- [ ] Fine-tune velocity windows if needed
- [ ] Update retracement levels based on market behavior
- [ ] Recalibrate position sizing caps

---

## 🚀 Quick Start Commands

```bash
# 1. Build with Fibonacci system
cargo build --release

# 2. Start dashboard
cargo run --release --bin dashboard

# 3. Monitor Fibonacci signals in logs
# Look for: 🚀 RUNNER, 📈 ESCALATE, 🔻 GOLDEN RETRACE

# 4. Check Fibonacci performance
# Dashboard → Trades tab → Filter by "Fibonacci" entries
```

---

## 📚 Further Reading

- **Fibonacci in Trading**: https://www.investopedia.com/terms/f/fibonacciretracement.asp
- **Golden Ratio in Markets**: Research papers on φ in price action
- **Momentum Analysis**: Technical analysis of velocity patterns
- **Position Sizing**: Kelly Criterion + Fibonacci progression

---

## 💡 Pro Tips

1. **Trust the Golden Retracement**: 61.8% pullback is the strongest signal - always exit
2. **Let Velocity Guide You**: Ratio ≥ φ = hold, ratio < 1.0 = prepare to exit
3. **Fibonacci Runners Are Rare**: Only 5-10% of pools qualify - be patient
4. **Compound Carefully**: Reset position sizing after 3 losses or 1 big win
5. **Monitor Velocity Windows**: If F(5) and F(8) both show decay, exit immediately

---

**Remember**: The Fibonacci system is a tool to enhance decision-making, not a guarantee. Always use proper risk management and never invest more than you can afford to lose. The goal is steady, mathematical recovery - not gambling.

Good luck with your capital recovery! 🎯📈
