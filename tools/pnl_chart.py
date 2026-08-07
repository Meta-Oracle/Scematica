#!/usr/bin/env python3
"""Build a true-to-fact cumulative SOL PnL chart from the bot's real trade log.

Reads `scematica-trades.jsonl` (append-only trade events written by the sniper) and
plots the running sum of *realized* SOL PnL — i.e. the `pnl` field on SELL records,
which is the SOL gained/lost when a position is closed. BUYs carry pnl=0. Nothing is
inferred or fabricated: every point is the cumulative sum of actual closed-trade PnL.
"""
import json
import sys
from datetime import datetime, timezone
from pathlib import Path

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
import matplotlib.dates as mdates

# Windows consoles default to cp1252; force UTF-8 so unicode in output never crashes.
try:
    sys.stdout.reconfigure(encoding="utf-8")
except Exception:
    pass

SRC = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("scematica-trades.jsonl")
OUT = Path(sys.argv[2]) if len(sys.argv) > 2 else Path("scematica-pnl.png")


def parse_ts(s: str):
    if not s:
        return None
    s = s.replace("Z", "+00:00")
    # Trim fractional seconds to 6 digits (datetime rejects nanoseconds).
    if "." in s and "+" in s:
        head, rest = s.split(".", 1)
        frac, tz = rest.split("+", 1)
        frac = frac[:6]
        s = f"{head}.{frac}+{tz}"
    try:
        return datetime.fromisoformat(s)
    except ValueError:
        return None


rows = []
kinds = {}
bad = 0
with SRC.open("r", encoding="utf-8") as f:
    for line in f:
        line = line.strip()
        if not line:
            continue
        try:
            r = json.loads(line)
        except json.JSONDecodeError:
            bad += 1
            continue
        rows.append(r)
        kinds[r.get("kind", "?")] = kinds.get(r.get("kind", "?"), 0) + 1

# Realized PnL comes from SELLs (closed positions). Keep those with a numeric pnl + ts.
sells = []
for r in rows:
    if r.get("kind") != "SELL":
        continue
    pnl = r.get("pnl")
    ts = parse_ts(r.get("timestamp", ""))
    if pnl is None or ts is None:
        continue
    sells.append((ts, float(pnl), r))
sells.sort(key=lambda x: x[0])

buys = [r for r in rows if r.get("kind") == "BUY"]
sol_deployed = sum(float(r.get("amount", 0) or 0) for r in buys)  # BUY amount is SOL in

# Cumulative realized PnL series.
times, cum = [], []
running = 0.0
wins = losses = flat = 0
best = worst = None
for ts, pnl, r in sells:
    running += pnl
    times.append(ts)
    cum.append(running)
    if pnl > 0:
        wins += 1
    elif pnl < 0:
        losses += 1
    else:
        flat += 1
    if best is None or pnl > best[1]:
        best = (ts, pnl)
    if worst is None or pnl < worst[1]:
        worst = (ts, pnl)

total = running
n = len(sells)
peak = max(cum) if cum else 0.0
trough = min(cum) if cum else 0.0
# Max drawdown of the equity curve (peak-to-later-trough).
maxdd = 0.0
run_peak = float("-inf")
for v in cum:
    run_peak = max(run_peak, v)
    maxdd = max(maxdd, run_peak - v)

span = (
    f"{sells[0][0].astimezone(timezone.utc):%Y-%m-%d %H:%M} to "
    f"{sells[-1][0].astimezone(timezone.utc):%Y-%m-%d %H:%M} UTC"
    if sells
    else "n/a"
)

print("=== TRUE-TO-FACT PnL SUMMARY (from scematica-trades.jsonl) ===")
print(f"records parsed        : {len(rows)}  (malformed skipped: {bad})")
print(f"event kinds           : {kinds}")
print(f"BUYs                  : {len(buys)}   SOL deployed (sum of buy amounts): {sol_deployed:.4f}")
print(f"closed trades (SELLs) : {n}")
print(f"  wins/losses/flat    : {wins}/{losses}/{flat}   win-rate: {100*wins/n if n else 0:.1f}%")
print(f"time span             : {span}")
print(f"TOTAL realized PnL    : {total:+.6f} SOL")
print(f"peak / trough equity  : {peak:+.6f} / {trough:+.6f} SOL")
print(f"max drawdown          : {maxdd:.6f} SOL")
if best:
    print(f"best trade            : {best[1]:+.6f} SOL @ {best[0]:%Y-%m-%d %H:%M}")
if worst:
    print(f"worst trade           : {worst[1]:+.6f} SOL @ {worst[0]:%Y-%m-%d %H:%M}")

if not sells:
    print("no closed trades with realized PnL — nothing to plot")
    sys.exit(0)

# ── chart ──────────────────────────────────────────────────────────────────────
plt.style.use("dark_background")
fig, ax = plt.subplots(figsize=(12, 6.5), dpi=140)
fig.patch.set_facecolor("#0a0a0a")
ax.set_facecolor("#0a0a0a")

up = "#00cc44"
down = "#ff2020"
line_color = up if total >= 0 else down
ax.plot(times, cum, color=line_color, lw=1.6, zorder=3)
ax.fill_between(times, cum, 0, where=[c >= 0 for c in cum], color=up, alpha=0.12, zorder=1)
ax.fill_between(times, cum, 0, where=[c < 0 for c in cum], color=down, alpha=0.12, zorder=1)
ax.axhline(0, color="#555", lw=0.8, ls="--", zorder=2)

# Mark final point.
ax.scatter([times[-1]], [cum[-1]], color=line_color, s=28, zorder=4)
ax.annotate(
    f"{total:+.4f} SOL",
    xy=(times[-1], cum[-1]),
    xytext=(-10, 12 if total >= 0 else -18),
    textcoords="offset points",
    color=line_color,
    fontsize=11,
    fontweight="bold",
    ha="right",
)

ax.set_title(
    f"Scematica — Cumulative Realized PnL   ·   {n} closed trades   ·   {total:+.4f} SOL",
    color="#e0e0e0",
    fontsize=13,
    fontweight="bold",
    pad=14,
)
ax.set_xlabel(f"{span}   (win-rate {100*wins/n:.0f}%, max DD {maxdd:.4f} SOL)", color="#888", fontsize=9)
ax.set_ylabel("Cumulative realized PnL (SOL)", color="#888", fontsize=10)
ax.xaxis.set_major_formatter(mdates.DateFormatter("%m-%d"))
ax.grid(True, color="#1c1c1c", lw=0.6)
for spine in ax.spines.values():
    spine.set_color("#333")
ax.tick_params(colors="#888", labelsize=8)
fig.text(0.5, 0.01, "Source: scematica-trades.jsonl (realized SOL PnL, sum of SELL events). No modeled or simulated data.",
         ha="center", color="#555", fontsize=7)
fig.tight_layout(rect=(0, 0.02, 1, 1))
fig.savefig(OUT, facecolor=fig.get_facecolor())
print(f"\nchart written → {OUT}")
