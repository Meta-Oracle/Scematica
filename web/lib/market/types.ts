// Shared vocabulary for the Escrow Market.
//
// THE ONE IDEA THIS FILE ENCODES. A market observation and a custody claim are
// different kinds of statement, and the whole product depends on never letting the
// second inherit the credibility of the first.
//
//   • A price is an observation. It comes from a third party, it moves every second, it
//     can be manipulated by anyone with enough capital, and being wrong about it costs
//     the reader an opinion.
//   • A reserve is a claim about custody. It is read from the chain at a named slot, it
//     is checkable by a stranger with no trust in us, and being wrong about it costs the
//     reader money.
//
// So `MarketToken` and `BackingClaim` are separate types that are never merged into one
// flat row. A component renders both, side by side, and the union type below forces the
// caller to acknowledge that a token usually has NO backing — `null` is not an error
// state here, it is the overwhelmingly common truth and the reason the product exists.
//
// See app/api/escrow/vault/route.ts for the same rules stated at the network boundary.

/** Venues the market boards are organised by. `unknown` is honest, not a bucket. */
export type Dex = 'raydium' | 'pumpfun' | 'orca' | 'meteora' | 'jupiter' | 'unknown'

export const DEX_LABEL: Record<Dex, string> = {
  raydium: 'Raydium',
  pumpfun: 'pump.fun',
  orca: 'Orca',
  meteora: 'Meteora',
  jupiter: 'Jupiter',
  unknown: 'Unknown',
}

/**
 * A token as the market sees it. Every field here is an OBSERVATION sourced from a
 * third-party API — never from us, and never from a simulation.
 *
 * Numeric fields are `number | null`, and the distinction is load-bearing: `null` means
 * "this source did not tell us", which must render as an em dash rather than a zero.
 * Coercing an absent volume to 0 turns "we don't know" into "there was no trading",
 * which is a different and false claim — the same failure the sniper's audit fields
 * guard against in lib/feed/jupiter.ts.
 */
export interface MarketToken {
  mint: string
  symbol: string
  name: string
  decimals: number
  /** Which venue this row was observed on. Drives the per-DEX boards. */
  dex: Dex
  /** Which API answered. Shown in the UI so a figure is always attributable. */
  source: string
  priceUsd: number | null
  liquidityUsd: number | null
  volume24hUsd: number | null
  marketCapUsd: number | null
  priceChange24hPct: number | null
  /** Unix seconds the pool/token was created, when the source reports it. */
  createdAtUnix: number | null
  holderCount: number | null
  /** Deployer, when known — keys the reputation heuristic shared with the sniper. */
  dev: string | null
  poolAddress: string | null
}

/** Verdicts from the vault program. Mirrors `solvency()` in lib/escrow/program.ts. */
export type SolvencyVerdict = 'backed' | 'donated' | 'SHORTFALL'

/**
 * What the chain says stands behind a token. Deliberately carries NO price and no
 * percentage: the vault program stores neither, so deriving one here would invent
 * precision the source does not have.
 *
 * `measuredAtSlot` travels with the amounts because a reserve figure whose slot is
 * unknown is not checkable, and an uncheckable figure is exactly what this replaces.
 */
export interface BackingClaim {
  backingMint: string
  backingSymbol: string | null
  /** Raw base units as a STRING. A u64 reaches ~1.8e19 against Number.MAX_SAFE_INTEGER
   *  ~9e15, so any coercion to `number` silently corrupts satoshi-denominated wBTC. */
  recordedBacking: string
  actualBacking: string
  backingDecimals: number
  recordedToken: string
  actualToken: string
  tokenDecimals: number
  verdict: SolvencyVerdict
  measuredAtSlot: number
  positionsOpen: number
}

/**
 * One row of the market. `backing: null` is the normal case — most tokens are backed by
 * nothing, and the union makes that impossible to forget at a call site.
 */
export interface MarketRow {
  token: MarketToken
  backing: BackingClaim | null
}

/**
 * A source that failed is rendered, not dropped.
 *
 * Silently omitting a DEX whose API is down makes the board quietly wrong: the reader
 * sees four venues and concludes those are all of them. `/alchem-link` takes the same
 * position on unreadable feeds, and for the same reason.
 */
export interface SourceStatus {
  dex: Dex
  source: string
  ok: boolean
  count: number
  error: string | null
  fetchedAt: number
}

export interface MarketSnapshot {
  rows: MarketRow[]
  sources: SourceStatus[]
  fetchedAt: number
}

/** Base units → display string, without ever touching a float. See formatAmount rules. */
export function formatUnits(raw: string, decimals: number): string {
  const neg = raw.startsWith('-')
  const digits = (neg ? raw.slice(1) : raw).replace(/^0+(?=\d)/, '')
  if (decimals === 0) return (neg ? '-' : '') + (digits || '0')
  const padded = digits.padStart(decimals + 1, '0')
  const whole = padded.slice(0, padded.length - decimals)
  const frac = padded.slice(padded.length - decimals).replace(/0+$/, '')
  return `${neg ? '-' : ''}${whole}${frac ? `.${frac}` : ''}`
}
