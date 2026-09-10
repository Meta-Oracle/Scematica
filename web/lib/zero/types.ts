// Scematica Zero — core types.
//
// See docs/SCEMATICA-ZERO.md. This module is part of the PURE CORE: no DOM, no React,
// no `chrome.*`, no `window`, no I/O. Everything the loop needs from the outside world
// arrives as a `ZeroEvent`, and everything it wants done leaves as an `Effect`.
//
// That is not architectural taste. It is the only way `check:zero` can pin behaviour
// that would otherwise need real money on mainnet to exercise — a stop-loss firing, a
// spend cap refusing, a fill nobody could observe.

import type { LadderState } from './sniper/exit-ladder.ts'
export type { LadderState } from './sniper/exit-ladder.ts'

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
  | {
      kind: 'vault.changed'
      mint: string
      priceSol: number
      slot: number
      atUnix: number
      /**
       * Quote-vault balance in lamports, when the read resolved.
       *
       * `null` — never 0 — when it did not. The whale-exit and volume-exhaustion rules
       * both read a vault DROP, so a failed read presented as zero is a 100% drain and
       * sells the position. The measured/unmeasured rule at its most expensive.
       */
      quoteVaultLamports?: number | null
    }
  /**
   * An RPC-bound read resolved, or did not. Feeds the coherence breaker.
   *
   * `atUnix` is load-bearing: the breaker rolls a 120-second window, so a sample has to
   * say when it happened or a read arriving after a long silence lands in a stale window.
   */
  | { kind: 'read.resolved'; label: string; atUnix: number }
  | { kind: 'read.failed'; label: string; reason: string; atUnix: number }
  /**
   * A submitted swap was seen to land.
   *
   * `outAmount` is `null` when the transaction landed but its effect could not be read —
   * a distinct case from a fill of zero. It becomes an unmeasured `tokensOut`, and from
   * there a position `exits.ts` refuses to price rather than selling on a percentage
   * nobody computed. Base units for a buy, lamports for a sell.
   */
  | { kind: 'fill.observed'; mint: string; signature: string; outAmount: number | null; side: 'buy' | 'sell'; atUnix: number }
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
  /**
   * The sell monitor's loop-locals, carried between arrivals.
   *
   * Absent while `opening` — there is no entry amount to anchor a stop to until a fill is
   * observed — and absent on an `unknown` position, which is the point: a ladder needs an
   * entry value, and a position whose fill nobody could read has none. Building one from a
   * guessed entry would let every percentage rule in `exit-ladder.ts` fire on a number
   * nobody measured.
   */
  ladder?: LadderState
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
//
// There is no Zero configuration any more, and that is the point of this section.
//
// `ZeroConfig` used to be a hand-written struct of round numbers — a 100% take-profit, a
// 15% stop, three concurrent positions — none of which the sniper has ever used. It read
// like a sensible default set and it made Zero a different bot wearing the same name.
//
// The type is now an alias for the sniper's own config, loaded from `config.toml` and
// pinned against it by `check:zero`. Anything that wants to change a threshold changes
// `config.toml`, regenerates the fixture, and both bots move together. See
// `sniper/config.ts`.

export type { SniperConfig, RateMode } from './sniper/config.ts'
export {
  SNIPER_CONFIG,
  RATE_MODES,
  ACTIVE_MODE_NAME,
  withRateMode,
  configProblem,
} from './sniper/config.ts'

import type { SniperConfig } from './sniper/config.ts'
import { SNIPER_CONFIG } from './sniper/config.ts'

/**
 * The name Zero's own modules use. An alias, not a second type.
 *
 * Kept as a name rather than replaced everywhere because the alias is a seam: one line
 * says "Zero's configuration IS the sniper's configuration", where thirty import
 * rewrites would say nothing at all.
 */
export type ZeroConfig = SniperConfig

export const DEFAULT_CONFIG: ZeroConfig = SNIPER_CONFIG

// ── the host's own limits ────────────────────────────────────────────────────
//
// These have no counterpart in the sniper and must not be smuggled into the config above,
// where they would look like bot settings somebody had changed. They are facts about
// running in a browser against a capped session key.

/**
 * A trade too small to matter is not worth its fee.
 *
 * The only sizing number Zero owns. The base size, the Kelly multiplier and the lookback
 * all come from `config.toml` now; this one has no counterpart there because the sniper
 * never sizes below its own `quote_amount` and so never needed a floor.
 */
export const DUST_LAMPORTS = 2_000_000 // 0.002 SOL
