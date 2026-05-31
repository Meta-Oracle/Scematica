# Scematica Profit Analysis

## Thesis

Scematica does not, and cannot, guarantee profit in the literal financial sense. No trading system operating on public Solana liquidity can guarantee profit because realized PnL depends on adversarial market behavior, liquidity at execution time, RPC availability, transaction ordering, fees, slippage, token contract behavior, and the future actions of other market participants.

What Scematica can guarantee is narrower and more useful:

- It can enforce deterministic software rules before entering a trade.
- It can reject pools that fail configured risk gates.
- It can size entries according to configured wallet, rate-mode, Kelly, and builder constraints.
- It can attempt exits according to a defined exit policy.
- It can record the data needed to audit whether the strategy has positive expectancy.
- It can make the live Intelligence tab reflect the actual state of decisions, Deep Q* advice, and transaction execution quality.

The practical profit claim is therefore conditional: Scematica is designed to produce profit only when its measured edge, execution quality, and market regime remain favorable enough that expected value stays positive after fees, slippage, failed transactions, and losses.

## Profit Equation

For each completed trade:

```text
realized_profit =
  exit_quote_received
  - entry_quote_spent
  - transaction_fees
  - priority_fees
  - slippage_cost
  - failed_attempt_cost
```

For the strategy over many trades:

```text
expected_value =
  win_rate * average_win
  - loss_rate * average_loss
  - execution_cost_per_trade
```

Profit requires expected value to remain above zero over a large enough sample. A single winning trade does not prove an edge, and a single losing trade does not disprove it. The system must continuously measure whether live results still support the assumptions used by the filters, scorer, DQ* agent, and exit logic.

## Where Scematica Seeks Edge

Scematica's edge is not one mechanism. It is a layered pipeline that tries to reduce bad entries, size good entries appropriately, and exit before reversals erase gains.

### 1. Early Pool Detection

The listener stack observes candidate pools and pump.fun graduation/trending signals. Earlier detection can matter because new Solana pools often move within seconds. The benefit is time priority: the bot can evaluate a pool before slower participants notice it.

This is not a profit guarantee. Early entry is only useful when the detected pool has real buy pressure, adequate liquidity, and survivable execution conditions.

### 2. Pool Decision Ledger

`scematica-pool-decisions.jsonl` records accepted, rejected, and ignored pools. This file is the audit trail for the Intelligence tab. It matters because a profitable system needs to know not only what it bought, but what it refused to buy.

The ledger supports these questions:

- Are most rejections caused by healthy risk gates or by stale/misconfigured filters?
- Are accepted pools scoring higher than rejected pools?
- Are live inflow, velocity, and pump.fun scores predictive of later realized PnL?
- Are decisions being made at the intended stage of the pipeline?

Without this file, the dashboard can show trades but cannot explain the selection process.

### 3. Scoring and Risk Gates

The pool scorer evaluates liquidity, age, velocity, buy pressure, pump.fun score, social/deployer signals, and live inflow. Risk gates reject pools that do not satisfy configured constraints.

The intended profit mechanism is selection pressure: avoid the long tail of pools whose structure resembles prior losers, and concentrate capital on pools whose live signals resemble prior winners.

The guarantee is operational, not financial. If a gate is configured correctly and the code path is reached, the bot can guarantee it will not intentionally buy a pool that fails that gate. It cannot guarantee that a pool passing the gates will remain profitable.

### 4. Deep Q* Advice

`scematica-nn-advice.json` contains the current DQ* action, confidence, Q-values, and explanation. The DQ* agent learns from the trade stream and advises entry sizing or rejection when it has enough training signal.

The DQ* agent helps by learning nonlinear relationships that fixed rules may miss, for example:

- high pool score but weak live inflow,
- strong velocity but unfavorable time-of-day regime,
- profitable entry profile but historically poor exit reliability,
- repeated losses under a changed market regime.

This agent is an adaptive advisor, not an oracle. Its advice is only as good as the live data distribution and the reward signal. Model drift, sparse data, corrupted training rows, and regime changes can all degrade it.

### 5. Position Sizing

Scematica uses mode-based sizing, wallet percentage controls, builder modes, and optional Kelly-style sizing. The purpose is to avoid overbetting when confidence is weak and scale only when recent evidence supports it.

Sizing contributes to survival. It cannot turn negative expectancy into guaranteed profit. If the selection edge disappears, larger sizing only accelerates drawdown.

### 6. Exit Logic

The exit stack attempts to protect gains and cut dead capital through take-profit targets, stop loss, trailing stop, momentum decay, peak stagnation, no-pump timeout, Fibonacci exits, dump mode, and sell mode.

The intended edge is asymmetric payoff:

- Let fast winners extend.
- Exit failed launches quickly.
- Avoid holding dead positions long after momentum is gone.
- Prevent a profitable move from bleeding all the way back to flat or negative PnL.

Exit rules can be deterministic in code, but execution is not deterministic on-chain. A sell can fail, slip, get delayed, or land after liquidity changes. Therefore stop-loss and take-profit rules are intentions enforced by transaction attempts, not absolute guarantees of fill price.

### 7. Execution Telemetry

`scematica-tx-telemetry.jsonl` records transaction latency, attempts, confirmation status, errors, rate-limit counts, timeout counts, slippage errors, and compute-budget settings.

This file is critical because a strategy with theoretical edge can still lose money if execution quality is poor. A high rejection rate, slow confirmation, or frequent slippage failure can turn expected profit negative.

The Intelligence tab uses this data to answer:

- Are buys and sells landing reliably?
- Is average latency rising?
- Are rate limits or blockhash errors increasing?
- Are slippage failures concentrated around exits?
- Is high-speed mode increasing failure costs?

## What Is Actually Guaranteed

Scematica can make guarantees about its own software behavior, assuming the process is running, the configuration is valid, and the Solana/RPC dependencies respond within expected bounds.

### Data Guarantees

As of v1.11.0:

- The sniper creates `scematica-pool-decisions.jsonl` at startup.
- The sniper creates `scematica-tx-telemetry.jsonl` at startup.
- The sniper writes an initial `scematica-nn-advice.json` from the loaded DQ* agent.
- The sniper, API, terminal dashboard, and web dashboard resolve artifacts through the same workspace data directory.
- `SCEMATICA_DATA_DIR` can override the data directory for deployments.
- The API exposes `/api/nn-advice`, `/api/decisions`, `/api/tx-telemetry`, and `/api/intelligence`.

These guarantees make the Intelligence tab auditable during live runs.

### Risk-Control Guarantees

Within software limits, Scematica can guarantee:

- no intentional buy when sell mode is active,
- no duplicate sniper instance when the lock-file check detects an active process,
- no buy above configured sizing constraints,
- no buy when configured filters reject the pool,
- no DQ*-forced buy while the agent is still in warm-up mode,
- no silent absence of Intelligence artifacts after startup.

These are control guarantees. They reduce avoidable losses but do not guarantee positive PnL.

## Why Literal Profit Cannot Be Guaranteed

Profit cannot be guaranteed because every trade depends on unknown future state.

Major sources of irreducible uncertainty include:

- Liquidity can vanish between signal and execution.
- A deployer can rug or mint/sell supply after entry.
- Other bots can front-run or back-run the transaction.
- Solana RPC can delay, drop, or rate-limit requests.
- Priority fees can rise faster than expected.
- Price can gap through the intended stop-loss level.
- Slippage protection can prevent an exit from landing.
- Token programs can behave unexpectedly.
- Market regimes can invalidate historical calibration.
- The DQ* agent can overfit or learn from biased data.

Any document claiming guaranteed profit from a live memecoin sniper would be making a false statement unless it defined "guarantee" as something other than realized financial return.

## Conditional Profit Framework

The strongest defensible statement is:

Scematica is profitable only if the live strategy maintains positive expectancy after execution costs.

That condition can be monitored with:

- realized PnL from `scematica-trades.jsonl`,
- selection quality from `scematica-pool-decisions.jsonl`,
- agent behavior from `scematica-nn-advice.json` and `scematica-nn-stats.json`,
- execution reliability from `scematica-tx-telemetry.jsonl`,
- open exposure from `scematica-positions.json`,
- logs from `scematica-sniper.log`.

If any of these show deterioration, the rational response is to reduce size, tighten gates, switch rate mode, pause buying, or stop the bot.

## Practical Profit Invariants

A live Scematica run should be considered healthy only when these invariants hold:

1. Decision volume is present.

   The pool decision ledger should grow during active listener periods. If it does not, the listener, RPC, or data directory is miswired.

2. Accepted pools are rare relative to rejected pools.

   A sniper should be selective. Too many accepted pools usually means the gates are loose or high-speed mode is bypassing too much.

3. Execution telemetry is clean.

   Confirmed transactions should dominate failed transactions. Rate limits, timeouts, and slippage errors should remain low enough that execution costs do not consume edge.

4. DQ* advice is bounded.

   Q-values should remain finite and stable. Exploding values indicate bad rewards, corrupted state inputs, or a checkpoint that should be reset.

5. Losses are bounded by sizing.

   Position sizing should make a run of losses survivable. The system should not require every trade to win.

6. Winners are large enough to pay for losers.

   The average realized win must exceed the average realized loss by enough to overcome failed transactions, priority fees, and low-liquidity exits.

## Conclusion

Scematica's profit design is a disciplined positive-expectancy pipeline: detect early, reject aggressively, size conditionally, exit mechanically, learn from outcomes, and audit execution quality. That design can improve the probability of profitability and can enforce many operational constraints.

It does not guarantee profit. The honest guarantee is that Scematica now records and exposes the live evidence needed to prove or disprove profitability during a run. Profit must be measured continuously, not assumed.
