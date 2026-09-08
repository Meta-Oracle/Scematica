// Scematica Zero — core types.
//
// See docs/SCEMATICA-ZERO.md. This module is part of the PURE CORE: no DOM, no React,
// no `chrome.*`, no `window`, no I/O. Everything the loop needs from the outside world
// arrives as a `ZeroEvent`, and everything it wants done leaves as an `Effect`.
//
// That is not architectural taste. It is the only way `check:zero` can pin behaviour
// that would otherwise need real money on mainnet to exercise — a stop-loss firing, a
// spend cap refusing, a fill nobody could observe.

// ── the measured / unmeasured distinction ────────────────────────────────────

/**
 * A quantity that knows whether anybody measured it.
 *
 * Same mechanism as `scematica_mesh::cognition::Term` and `scema-policy`'s `Term`, and
 * it matters MORE here than in either: a browser can resolve fewer facts about a pool
 * than the Rust bot can, and it fails to resolve them far more often (a rate limit is
 * an ordinary event, not an outage). A field nobody could read must not arrive at the
 * policy as a number, because the net cannot ask a follow-up question.
 */
export interface Term {
  value: number
  measured: boolean
  /** Why it is unmeasured, when it is. Renders instead of the number. */
  note?: string
}

export const measured = (value: number): Term => ({ value, measured: true })
export const absent = (note: string): Term => ({ value: 0, measured: false, note })

/**
 * How much of an aggregate was real.
 *
 * A count, never a ratio. 2/5 and 4/10 are different claims and a percentage renders
 * them identically — the same reason `scema-tui`'s coverage meter is one cell per term
 * rather than a proportional bar.
 */
export interface Coverage {
  measuredCount: number
  total: number
}

export const coverage = (terms: Term[]): Coverage => ({
  measuredCount: terms.filter(t => t.measured).length,
  total: terms.length,
})

/**
 * The one place a `Term` becomes a string.
 *
 * An unmeasured term prints an em dash, never `0.00`. A MEASURED zero prints `0.00`,
 * because that is a real observation. `scema_policy::render::cell` is the Rust original;
 * `lib/mesh/view.ts` and `lib/omni/view.ts` are the other two ports. The rule is shared,
 * not the code, and a copy that drifts is worse than no copy.
 */
export function cell(t: Term, digits = 2): string {
  return t.measured ? t.value.toFixed(digits) : '—'
}

// ── what the world tells Zero ────────────────────────────────────────────────

/** A pool as Zero perceived it. Mirrors `FeedPool` plus what only a chain read gives. */
export interface ObservedPool {
  mint: string
  symbol: string
  /** Quote-side depth in SOL. */
  sizeSol: Term
  ageSecs: Term
  createdAtUnix: number
  holderCount: Term
  mintRenounced: Term
  freezeDisabled: Term
  devHoldingPct: Term
  /** Buy/sell pressure from the vaults. Absent unless both vaults were read. */
  buyPressure: Term
  lpBurned: Term
  dev: string
}

/**
 * Everything that can move the loop.
 *
 * `tick` is deliberately in this union and deliberately powerless — see `Effect` and
 * `check:zero`'s Z-1 assertion. It exists so the UI can re-render and so an operator
 * can see that Zero is alive; it must never be the reason money moves.
 */
export type ZeroEvent =
  /** A pool surfaced by the discovery subscription. */
  | { kind: 'pool.observed'; pool: ObservedPool; atUnix: number }
  /**
   * A vault balance changed. THE load-bearing event: every exit predicate is evaluated
   * here, because a WebSocket notification is delivered to a hidden tab where a timer
   * is throttled to roughly once a minute.
   */
  | { kind: 'vault.changed'; mint: string; priceSol: number; slot: number; atUnix: number }
  /** An RPC-bound read resolved, or did not. Feeds the coherence gate. */
  | { kind: 'read.resolved'; label: string }
  | { kind: 'read.failed'; label: string; reason: string }
  /** A submitted swap was seen to land. */
  | { kind: 'fill.observed'; mint: string; signature: string; outAmount: number; atUnix: number }
  /** A submitted swap could not be observed either way. Not a failure — see Z-8. */
  | { kind: 'fill.unknown'; mint: string; signature: string; atUnix: number }
  /** A submitted swap demonstrably never landed; nothing moved. */
  | { kind: 'fill.failed'; mint: string; signature: string; reason: string }
  | { kind: 'socket.opened'; atUnix: number }
  | { kind: 'socket.closed'; atUnix: number; reason: string }
  /** Cosmetic heartbeat. Powerless by construction. */
  | { kind: 'tick'; atUnix: number }
  /** Operator commands. */
  | { kind: 'arm'; atUnix: number }
  | { kind: 'disarm'; atUnix: number; reason: string }
  | { kind: 'kill'; atUnix: number }
  /** This tab won or lost the single-writer election. See `lease.ts`. */
  | { kind: 'lease.acquired'; atUnix: number }
  | { kind: 'lease.lost'; atUnix: number }

// ── what Zero asks the host to do ────────────────────────────────────────────

export type Effect =
  /** Subscribe to a pool's vaults so exits become arrival-driven. */
  | { kind: 'subscribe'; mint: string }
  | { kind: 'unsubscribe'; mint: string }
  /** Move money. The only effect that can. */
  | {
      kind: 'swap'
      mint: string
      side: 'buy' | 'sell'
      /** Lamports in for a buy; token base units in for a sell. */
      amount: number
      /** Which signer, decided by the core rather than by the shell. */
      signer: 'wallet' | 'session'
      reason: string
    }
  /** Seal a decision record — including the declines. */
  | { kind: 'seal'; record: DecisionRecord }
  /** Persist the ledger. Versioned; see `ledger.ts`. */
  | { kind: 'persist' }
  /** Tell the operator something they must read. */
  | { kind: 'notify'; level: 'info' | 'warn' | 'alarm'; text: string }

// ── decisions ────────────────────────────────────────────────────────────────

/**
 * Why Zero did not act.
 *
 * Abstention is a first-class outcome with distinguishable reasons, exactly as in
 * `scema-policy`: each one sends an operator somewhere different, and collapsing them
 * into "no trade" throws away the only actionable part.
 */
export type DeclineReason =
  | 'not-armed'
  | 'no-lease'
  | 'killed'
  | 'filters-rejected'
  | 'score-below-floor'
  | 'policy-veto'
  | 'coherence-degraded'
  | 'budget-exhausted'
  | 'session-expired'
  | 'position-open'
  | 'max-positions'
  | 'size-below-dust'

export interface Verdict {
  act: boolean
  /** Present exactly when `act` is false. */
  decline?: DeclineReason
  /** Lamports to spend. Zero when declining. */
  sizeLamports: number
  /** Human-readable, one line, names the cause rather than restating the outcome. */
  reason: string
  /** Rides with every score. A qualified argmax over invented inputs looks identical. */
  coverage: Coverage
  /** The policy's Q-vector, when it ran. Absent when it was not consulted. */
  q?: number[]
  score: Term
  psi: Term
}

// ── positions ────────────────────────────────────────────────────────────────

/**
 * `unknown` is a real arm, not an error state.
 *
 * A swap Zero submitted but could not observe is neither open nor absent. Recording it
 * as open invents a position; recording it as absent loses one that may hold real
 * tokens. The treasury path already paid for this: a successful payout presenting as a
 * dead faucet is the worst pair of facts available, and the obvious retry pays twice.
 */
export type PositionState = 'opening' | 'open' | 'closing' | 'closed' | 'unknown'

export interface Position {
  mint: string
  symbol: string
  state: PositionState
  /** Lamports actually spent, as observed. */
  spentLamports: number
  /** Token base units received, as observed. Absent while `opening`/`unknown`. */
  tokensOut: Term
  entryPriceSol: Term
  /** Best price seen since entry — drives the pullback exit. */
  peakPriceSol: Term
  lastPriceSol: Term
  openedAtUnix: number
  /** Last time an arrival let Zero evaluate this position's exits. */
  lastEvaluatedUnix: number
  /** Set once a close is submitted, so a second one cannot be. */
  closeSignature?: string
  openSignature?: string
}

// ── records ──────────────────────────────────────────────────────────────────

/**
 * A sealed decision. Written for the declines too — a branch nobody took has no
 * outcome and never will, and that asymmetry is the whole reason the record is worth
 * keeping (`calibration.rs`).
 */
export interface DecisionRecord {
  schema: 'scema.zero.decision/1'
  id: string
  atUnix: number
  mint: string
  act: boolean
  decline?: DeclineReason
  reason: string
  score: Term
  psi: Term
  coverage: Coverage
  sizeLamports: number
  q?: number[]
  /** The world Zero believed it was acting on, committed whole. */
  world: ObservedPool
  /** Weights are a stated preference, never a fitted parameter, and are hashed in. */
  policyId: string
}

// ── configuration ────────────────────────────────────────────────────────────

export interface ZeroConfig {
  /** Below this, Zero declines. Mirrors config.toml `min_pool_score`. */
  minPoolScore: number
  /** Fraction of the session budget a single entry may use, before Kelly. */
  maxEntryFraction: number
  /** Hard floor; a trade too small to matter is not worth its fee. */
  dustLamports: number
  maxOpenPositions: number
  takeProfitPct: number
  stopLossPct: number
  /** Give back this much of the peak and Zero exits. */
  pullbackExitPct: number
  /** A peak must exceed this before the pullback rule arms. See the invariant below. */
  momentumMinPeakPct: number
  /** No movement for this long (evaluated on arrival) and Zero exits. */
  noPumpTimeoutSecs: number
  /** Ψ below this halts entries. Never exits. */
  minPsi: number
  /** Reads that must have resolved before the gate has an opinion at all. */
  minCoherenceSamples: number
}

/**
 * `momentumMinPeakPct` must exceed `takeProfitPct + pullbackExitPct`, or the pullback
 * exit is unsatisfiable: the peak arms only above the momentum floor, but any position
 * that high has already taken profit and closed. This exact relationship has been
 * broken in the Rust config before — it is an arithmetic property, not a preference,
 * so it is asserted rather than documented.
 */
export function configProblem(c: ZeroConfig): string | null {
  if (c.momentumMinPeakPct <= c.takeProfitPct + c.pullbackExitPct) {
    return `momentumMinPeakPct (${c.momentumMinPeakPct}) must exceed takeProfitPct + pullbackExitPct (${c.takeProfitPct + c.pullbackExitPct}), or the pullback exit can never fire`
  }
  if (c.stopLossPct <= 0 || c.stopLossPct >= 100) return 'stopLossPct must be within (0, 100)'
  if (c.maxOpenPositions < 1) return 'maxOpenPositions must be at least 1'
  if (c.dustLamports <= 0) return 'dustLamports must be positive'
  return null
}

export const DEFAULT_CONFIG: ZeroConfig = {
  minPoolScore: 65,
  maxEntryFraction: 0.25,
  dustLamports: 2_000_000, // 0.002 SOL — below this the fee dominates
  maxOpenPositions: 3,
  takeProfitPct: 100,
  stopLossPct: 15,
  pullbackExitPct: 25,
  momentumMinPeakPct: 140, // > 100 + 25, per configProblem
  noPumpTimeoutSecs: 30,
  minPsi: 0.55,
  minCoherenceSamples: 12,
}
