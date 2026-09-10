// `KellySizer` — a port of `crates/scematica-sniper/src/kelly.rs`.
//
// ⚠️  PORT. Rust is authoritative. `check:zero` runs every case in
// `fixtures/sniper-parity.json`'s `kelly.cases` through `computeMultiplier` and requires
// bit-identical answers.
//
// ── What changed when this became a real port ────────────────────────────────
//
// The previous `size.ts` was a different sizer wearing this one's name. Every number in
// it was invented, and each difference moves real money:
//
//   • **Quarter-Kelly, not half.** `kelly_fraction` is 0.25 in both `config.rs` and
//     `config.toml`. The old file's `KELLY_FRACTION = 0.5` doubled the Kelly component
//     of every size.
//   • **Ten settled trades, not eight.** Off-by-two on the warm-up cliff, which is the
//     single most consequential threshold here: below it the answer is a flat 0.5×
//     regardless of what the trades say.
//   • **SOL, not percentages.** Rust averages |pnl_sol|; the old file averaged pnl_pct.
//     The payoff ratio `b` is a ratio, so this agrees only when every position is the
//     same size — which is exactly what Kelly sizing stops being true.
//   • **A multiplier, not a fraction of base.** Rust returns `1 + f*·fraction` clamped to
//     **[0.25, 3.0]**, so a strong edge can size *up* to three times base. The old file
//     could only ever scale base down, and separately invented a `UNKNOWN_EDGE_FRACTION`
//     for a case Rust answers with a plain 1.0.
//
// ── The degenerate arms are the port, not an afterthought ────────────────────
//
// Three of the six branches return a bare 1.0 for inputs a formula would happily produce
// a number from: an empty history, all wins, all losses. Rust's comment for the middle
// one — "Not enough data to compute ratio" — is the whole argument. A window with no
// losses has no payoff ratio, and inventing one (an infinite `b`, or a floor on
// `avg_loss`) manufactures an edge estimate out of a missing denominator. Staying at base
// is the honest answer, and it is the same rule as `Term::absent` in a system that has to
// return a number.

/** A settled trade. `profitable` is the bot's own verdict, not `pnlSol > 0`. */
export interface KellyTrade {
  profitable: boolean
  /** Realised PnL in SOL. Sign is carried by `profitable`; magnitude is what is used. */
  pnlSol: number
}

/** `config.rs` default `kelly_fraction`. Quarter-Kelly. */
export const DEFAULT_FRACTION = 0.25

/** `KellySizer::new`'s implicit `min_trades`. */
export const DEFAULT_MIN_TRADES = 10

/** The clamp rails, `multiplier.max(0.25).min(3.0)`. */
export const CLAMP_MIN = 0.25
export const CLAMP_MAX = 3.0

/** `KellySizer::with_min_trades` — the fraction is itself clamped into [0.01, 1.0]. */
export interface KellySizer {
  fraction: number
  minTrades: number
}

export function kellySizer(
  fraction = DEFAULT_FRACTION,
  minTrades = DEFAULT_MIN_TRADES,
): KellySizer {
  return { fraction: Math.min(Math.max(fraction, 0.01), 1.0), minTrades }
}

export interface KellyResult {
  multiplier: number
  /** Why the multiplier is what it is. Not in Rust — Rust logs; Zero has to render. */
  reason: string
  /** Present only when the formula actually ran. */
  winRate?: number
  payoffRatio?: number
  rawKelly?: number
}

/**
 * `KellySizer::compute_multiplier`.
 *
 * Returns a multiplier on the base quote amount: 1.0 is base, above is more aggressive,
 * below is smaller. Never a lamport count — sizing in absolute terms is the caller's job,
 * and in Zero the session caps clamp it afterwards. Two gates, different questions.
 */
export function computeMultiplier(sizer: KellySizer, history: KellyTrade[]): KellyResult {
  if (history.length === 0) {
    return { multiplier: 1.0, reason: 'no settled trades — base size' }
  }

  // Warm-up guard. A flat 0.5×, not a computed number: the first few trades of a new
  // session are the ones most likely to be luck, and a Kelly fit to them sizes up hardest
  // exactly when the estimate is worst.
  if (history.length < sizer.minTrades) {
    return {
      multiplier: 0.5,
      reason: `warm-up: ${history.length} of ${sizer.minTrades} settled trades — half base`,
    }
  }

  const wins = history.filter(t => t.profitable).map(t => Math.abs(t.pnlSol))
  // A loss of exactly zero is excluded from the loss set but stays in the denominator of
  // `p`. That asymmetry is Rust's and it is deliberate: a break-even trade happened, so it
  // is part of the record, but it carries no information about how much a loss costs.
  const losses = history
    .filter(t => !t.profitable && Math.abs(t.pnlSol) > 1e-9)
    .map(t => Math.abs(t.pnlSol))

  const p = wins.length / history.length
  const q = 1.0 - p

  if (losses.length === 0 || wins.length === 0) {
    return {
      multiplier: 1.0,
      reason:
        wins.length === 0
          ? 'no wins in the window — no payoff ratio, holding at base'
          : 'no losses in the window — no payoff ratio, holding at base',
      winRate: p,
    }
  }

  const avgWin = wins.reduce((s, v) => s + v, 0) / wins.length
  const avgLoss = losses.reduce((s, v) => s + v, 0) / losses.length

  if (avgLoss < 1e-9) {
    return { multiplier: 1.0, reason: 'average loss is zero — no ratio, holding at base', winRate: p }
  }

  const b = avgWin / avgLoss
  const rawKelly = (p * b - q) / b
  const adjusted = rawKelly * sizer.fraction
  const multiplier = Math.min(Math.max(1.0 + adjusted, CLAMP_MIN), CLAMP_MAX)

  const clamped =
    1.0 + adjusted < CLAMP_MIN ? ' (clamped at the floor)'
      : 1.0 + adjusted > CLAMP_MAX ? ' (clamped at the ceiling)'
        : ''

  return {
    multiplier,
    reason:
      `win rate ${(p * 100).toFixed(0)}%, payoff ${b.toFixed(2)}, ` +
      `f* ${rawKelly.toFixed(3)} × ${sizer.fraction} → ×${multiplier.toFixed(4)}${clamped}`,
    winRate: p,
    payoffRatio: b,
    rawKelly,
  }
}
