#!/usr/bin/env python3
"""Deep, true-to-fact mining of scematica-trades.jsonl.

Pairs BUYs with SELLs, then slices realized PnL by every dimension we have:
entry size, hold time, hour, weekday, exit reason, pool size, pool score.
Also computes capital requirements: peak concurrent SOL exposure, drawdown,
fee overhead, and a recommended bankroll. Everything from the real log —
nothing modeled.
"""
import json
import sys
from collections import defaultdict, deque
from datetime import datetime

try:
    sys.stdout.reconfigure(encoding="utf-8")
except Exception:
    pass

SRC = sys.argv[1] if len(sys.argv) > 1 else "scematica-trades.jsonl"


def parse_ts(s):
    if not s:
        return None
    s = s.replace("Z", "+00:00")
    if "." in s and "+" in s:
        head, rest = s.split(".", 1)
        frac, tz = rest.split("+", 1)
        s = f"{head}.{frac[:6]}+{tz}"
    try:
        return datetime.fromisoformat(s)
    except ValueError:
        return None


rows = []
with open(SRC, encoding="utf-8") as f:
    for line in f:
        line = line.strip()
        if not line:
            continue
        try:
            r = json.loads(line)
        except json.JSONDecodeError:
            continue
        r["_ts"] = parse_ts(r.get("timestamp", ""))
        if r["_ts"] is not None:
            rows.append(r)
rows.sort(key=lambda r: r["_ts"])

# ── pair BUYs with SELLs by mint (FIFO) ────────────────────────────────────────
open_by_mint = defaultdict(deque)
trades = []  # completed round trips
for r in rows:
    if r.get("kind") == "BUY":
        open_by_mint[r.get("mint", "?")].append(r)
    elif r.get("kind") == "SELL":
        mint = r.get("mint", "?")
        buy = open_by_mint[mint].popleft() if open_by_mint[mint] else None
        trades.append({
            "mint": mint,
            "buy_ts": buy["_ts"] if buy else None,
            "sell_ts": r["_ts"],
            "entry_sol": float(buy.get("amount", 0) or 0) if buy else None,
            "pnl": float(r.get("pnl", 0) or 0),
            "pnl_pct": r.get("pnl_pct"),
            "hold_s": r.get("position_age_secs"),
            "exit": r.get("exit_reason", "") or "(none)",
            "pool_size": r.get("pool_size_sol"),
            "pool_score": r.get("pool_score"),
        })

n = len(trades)
tot = sum(t["pnl"] for t in trades)
wins = [t for t in trades if t["pnl"] > 0]
losses = [t for t in trades if t["pnl"] < 0]
gross_w = sum(t["pnl"] for t in wins)
gross_l = -sum(t["pnl"] for t in losses)
pf = gross_w / gross_l if gross_l > 0 else float("inf")

print("=" * 72)
print("SCEMATICA DEEP ANALYSIS — every number from the real trade log")
print("=" * 72)
print(f"round trips paired    : {n}")
print(f"net realized PnL      : {tot:+.6f} SOL")
print(f"wins / losses / flat  : {len(wins)} / {len(losses)} / {n-len(wins)-len(losses)}")
print(f"win rate              : {100*len(wins)/n:.1f}%")
print(f"gross wins / losses   : +{gross_w:.4f} / -{gross_l:.4f} SOL")
print(f"PROFIT FACTOR         : {pf:.2f}")
print(f"avg win / avg loss    : +{gross_w/len(wins):.6f} / -{gross_l/len(losses):.6f} SOL")
print(f"expectancy per trade  : {tot/n:+.6f} SOL")
w_sorted = sorted(trades, key=lambda t: -t["pnl"])
top10 = sum(t["pnl"] for t in w_sorted[:max(1, n//10)])
top33 = sum(t["pnl"] for t in w_sorted[:33])
print(f"top 10% of trades     : {top10:+.4f} SOL ({100*top10/tot:.0f}% of net)")
print(f"top 33 trades         : {top33:+.4f} SOL ({100*top33/tot:.0f}% of net)")


def bucket_table(title, key_fn, buckets):
    agg = {b: [0, 0.0, 0] for b in buckets}  # count, pnl, wins
    for t in trades:
        b = key_fn(t)
        if b is None:
            continue
        agg[b][0] += 1
        agg[b][1] += t["pnl"]
        agg[b][2] += 1 if t["pnl"] > 0 else 0
    print(f"\n── {title} " + "─" * max(0, 58 - len(title)))
    print(f"{'bucket':<22}{'n':>6}{'net SOL':>12}{'avg SOL':>12}{'WR%':>7}")
    for b in buckets:
        c, p, w = agg[b]
        if c == 0:
            continue
        print(f"{b:<22}{c:>6}{p:>12.4f}{p/c:>12.6f}{100*w/c:>7.1f}")


# entry size
def sz(t):
    e = t["entry_sol"]
    if e is None:
        return None
    for lo, hi, lbl in [(0, .004, "<0.004"), (.004, .0075, "0.004-0.0075"),
                        (.0075, .015, "0.0075-0.015"), (.015, .03, "0.015-0.03"),
                        (.03, 1e9, ">0.03")]:
        if lo <= e < hi:
            return lbl
bucket_table("PnL by ENTRY SIZE (SOL)", sz,
             ["<0.004", "0.004-0.0075", "0.0075-0.015", "0.015-0.03", ">0.03"])

# hold time
def hold(t):
    h = t["hold_s"]
    if h is None:
        return None
    for lo, hi, lbl in [(0, 5, "0-5s"), (5, 15, "5-15s"), (15, 45, "15-45s"),
                        (45, 120, "45-120s"), (120, 400, "120-400s"), (400, 1e9, ">400s")]:
        if lo <= h < hi:
            return lbl
bucket_table("PnL by HOLD TIME", hold, ["0-5s", "5-15s", "15-45s", "45-120s", "120-400s", ">400s"])

# exit reason
reasons = sorted({t["exit"] for t in trades})
bucket_table("PnL by EXIT REASON", lambda t: t["exit"], reasons)

# hour of day (UTC)
bucket_table("PnL by HOUR (UTC)", lambda t: f"{t['sell_ts'].hour:02d}h",
             [f"{h:02d}h" for h in range(24)])

# weekday
days = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"]
bucket_table("PnL by WEEKDAY", lambda t: days[t["sell_ts"].weekday()], days)

# pool size (only where recorded > 0)
def psize(t):
    p = t["pool_size"]
    if not p:
        return None
    for lo, hi, lbl in [(0, 10, "<10"), (10, 20, "10-20"), (20, 40, "20-40"),
                        (40, 80, "40-80"), (80, 1e9, ">80")]:
        if lo <= p < hi:
            return lbl
bucket_table("PnL by POOL SIZE (SOL, where recorded)", psize, ["<10", "10-20", "20-40", "40-80", ">80"])

# pool score (only where recorded > 0)
def pscore(t):
    p = t["pool_score"]
    if not p:
        return None
    for lo, hi, lbl in [(0, 50, "<50"), (50, 65, "50-65"), (65, 80, "65-80"),
                        (80, 92, "80-92"), (92, 101, "92+")]:
        if lo <= p < hi:
            return lbl
bucket_table("PnL by POOL SCORE (where recorded)", pscore, ["<50", "50-65", "65-80", "80-92", "92+"])

# ── capital requirements ───────────────────────────────────────────────────────
print("\n" + "=" * 72)
print("CAPITAL / BANKROLL ANALYSIS")
print("=" * 72)

# peak concurrent exposure: replay buys/sells
events = []
for t in trades:
    if t["buy_ts"] and t["entry_sol"]:
        events.append((t["buy_ts"], +t["entry_sol"]))
        events.append((t["sell_ts"], -t["entry_sol"]))
events.sort(key=lambda e: e[0])
cur = peak = 0.0
peak_at = None
for ts, d in events:
    cur += d
    if cur > peak:
        peak, peak_at = cur, ts
print(f"peak concurrent SOL deployed : {peak:.4f} SOL (at {peak_at})")

# equity curve drawdown
run = 0.0
run_peak = -1e9
maxdd = 0.0
for t in trades:
    run += t["pnl"]
    run_peak = max(run_peak, run)
    maxdd = max(maxdd, run_peak - run)
print(f"max equity drawdown          : {maxdd:.4f} SOL")

# fee overhead estimate: ~5000 lamports base + priority per tx, 2 tx per round trip
n_tx = 2 * n
fee_lo, fee_hi = n_tx * 5_000 / 1e9, n_tx * 100_000 / 1e9
print(f"tx count (buy+sell)          : {n_tx}")
print(f"est. network fees            : {fee_lo:.4f}-{fee_hi:.4f} SOL total "
      f"({fee_lo/n*1e3:.4f}-{fee_hi/n*1e3:.4f} mSOL per round trip)")

entry_sizes = [t["entry_sol"] for t in trades if t["entry_sol"]]
avg_entry = sum(entry_sizes) / len(entry_sizes)
print(f"avg / max entry size         : {avg_entry:.4f} / {max(entry_sizes):.4f} SOL")

# recommended bankroll: peak exposure + 3x observed drawdown + fee/WSOL-rent buffer
rec = peak + 3 * maxdd + 0.1
print(f"\nRECOMMENDED MINIMUM BANKROLL : {rec:.2f} SOL")
print(f"  = peak exposure ({peak:.3f}) + 3x max drawdown ({3*maxdd:.3f}) + fees/rent buffer (0.10)")
per_day = tot / max(1, (trades[-1]["sell_ts"] - trades[0]["sell_ts"]).days or 1)
print(f"observed daily expectancy    : {per_day:+.4f} SOL/day at these sizes")
print(f"time span                    : {trades[0]['sell_ts']:%Y-%m-%d} -> {trades[-1]['sell_ts']:%Y-%m-%d}")
