// Live new-mint feed — the real market behind the discovery panels.
//
// `lite-api.jup.ag/tokens/v2/recent` returns the most recently created tokens with the
// audit + trade-stats fields the sniper's filter pipeline consumes, and it is both
// keyless and CORS-permissive, so the browser can read it directly. That is what makes
// the discovery half of this dashboard genuinely standalone: no Rust bot, no proxy, no
// API key, and the mints on screen are real.
//
// Honest limits of this source vs. the bot's own listener:
//   • REST polling is seconds behind a Helius WebSocket — fine for a dashboard, useless
//     for actually sniping, where the edge is sub-second.
//   • No LP-burn read and no AMM vault balances, so the buy-pressure ratio the Rust
//     scorer derives from quote_vault/base_vault is unavailable here (see scorer.ts).
//   • Liquidity is quoted in USD and converted to SOL via the shared price cache.

import { getSolUsd } from './price'

const RECENT_URL = 'https://lite-api.jup.ag/tokens/v2/recent'

// ── wire types (subset of the v2 token payload we actually use) ───────────────

interface JupStats {
  buyVolume?: number
  sellVolume?: number
  numBuys?: number
  numSells?: number
  numTraders?: number
  numNetBuyers?: number
}

interface JupAudit {
  mintAuthorityDisabled?: boolean
  freezeAuthorityDisabled?: boolean
  devBalancePercentage?: number
  devMigrations?: number
  devMints?: number
}

export interface JupToken {
  id: string
  name?: string
  symbol?: string
  decimals?: number
  dev?: string
  holderCount?: number
  liquidity?: number
  usdPrice?: number
  mcap?: number
  fdv?: number
  launchpad?: string
  organicScore?: number
  organicScoreLabel?: string
  createdAt?: string
  firstPool?: { id?: string; createdAt?: string }
  audit?: JupAudit
  stats5m?: JupStats
  stats1h?: JupStats
}

// ── normalised shape consumed by the scorer and the panels ───────────────────

/** One freshly-launched pool, normalised out of the Jupiter payload. */
export interface FeedPool {
  mint: string
  symbol: string
  name: string
  decimals: number
  /** Deployer address — keys the reputation heuristic. */
  dev: string
  /** Quote-side depth in SOL, converted from the USD figure. */
  sizeSol: number
  liquidityUsd: number
  /** Seconds since the first pool for this mint was created. */
  ageSecs: number
  createdAtUnix: number
  holderCount: number
  /** True when the mint authority has been revoked. */
  mintRenounced: boolean
  /** True when the freeze authority has been revoked. */
  freezeDisabled: boolean
  /** Share of supply still held by the deployer, in percent. */
  devBalancePct: number
  /** How many mints this deployer has launched — a serial-launcher signal. */
  devMints: number
  devMigrations: number
  buys5m: number
  sells5m: number
  buyVolume5m: number
  sellVolume5m: number
  netBuyers5m: number
  traders5m: number
  organicScore: number
  launchpad: string
  usdPrice: number
}

function unixOf(iso: string | undefined): number {
  if (!iso) return 0
  const ms = Date.parse(iso)
  return Number.isFinite(ms) ? Math.floor(ms / 1000) : 0
}

/** Map one raw token to the normalised pool shape. `solUsd` converts USD depth to SOL. */
export function toFeedPool(t: JupToken, solUsd: number, nowUnix: number): FeedPool {
  const createdAtUnix = unixOf(t.firstPool?.createdAt ?? t.createdAt)
  const s5 = t.stats5m ?? {}
  const liquidityUsd = t.liquidity ?? 0

  return {
    mint: t.id,
    symbol: t.symbol ?? '???',
    name: t.name ?? '',
    decimals: t.decimals ?? 6,
    dev: t.dev ?? '',
    sizeSol: solUsd > 0 ? liquidityUsd / solUsd : 0,
    liquidityUsd,
    ageSecs: createdAtUnix > 0 ? Math.max(0, nowUnix - createdAtUnix) : 0,
    createdAtUnix,
    holderCount: t.holderCount ?? 0,
    // Absent audit data is treated as "not renounced" — the filters must not pass a
    // token just because the source declined to tell us about it.
    mintRenounced: t.audit?.mintAuthorityDisabled === true,
    freezeDisabled: t.audit?.freezeAuthorityDisabled === true,
    devBalancePct: t.audit?.devBalancePercentage ?? 0,
    devMints: t.audit?.devMints ?? 0,
    devMigrations: t.audit?.devMigrations ?? 0,
    buys5m: s5.numBuys ?? 0,
    sells5m: s5.numSells ?? 0,
    buyVolume5m: s5.buyVolume ?? 0,
    sellVolume5m: s5.sellVolume ?? 0,
    netBuyers5m: s5.numNetBuyers ?? 0,
    traders5m: s5.numTraders ?? 0,
    organicScore: t.organicScore ?? 0,
    launchpad: t.launchpad ?? '',
    usdPrice: t.usdPrice ?? 0,
  }
}

/**
 * Fetch the current batch of recently-created mints, newest first.
 * Returns `null` on any failure so the caller can keep its last good snapshot
 * (the shared store treats null as "poll failed, retain previous").
 */
export async function fetchRecentPools(): Promise<FeedPool[] | null> {
  try {
    const [res, solUsd] = await Promise.all([
      fetch(RECENT_URL, { signal: AbortSignal.timeout(8_000) }),
      getSolUsd(),
    ])
    if (!res.ok) return null
    const raw = (await res.json()) as JupToken[]
    if (!Array.isArray(raw)) return null

    // Without a SOL quote every size would be 0 and the scorer would hard-reject the
    // whole batch, which reads as "everything is garbage" rather than "price missing".
    if (!solUsd) return null

    const nowUnix = Math.floor(Date.now() / 1000)
    return raw
      .filter(t => typeof t?.id === 'string')
      .map(t => toFeedPool(t, solUsd, nowUnix))
      .sort((a, b) => b.createdAtUnix - a.createdAtUnix)
  } catch {
    return null
  }
}
