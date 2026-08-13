// Pure aggregation for the Escrow Market. No fetching, no RPC — every function here is
// a total function of its inputs, so the ranking rules are testable without a network.
//
// The ordering rules encode the product's thesis, so they are worth stating plainly:
// this market ranks by VERIFIABLE BACKING first and market activity second. Every other
// venue ranks by volume, which is the number easiest to manufacture. A token with a real
// reserve behind it outranks a token with a hundred times the volume and nothing behind
// it, because the first claim is checkable by a stranger and the second is a vibe.

import type { BackingClaim, Dex, MarketRow, MarketSnapshot, MarketToken, SourceStatus } from './types'

/**
 * Merge duplicate mints across sources, keeping the most informative record.
 *
 * The same mint legitimately appears on several boards — a pump.fun launch that migrated
 * to Raydium is one token on two venues. Deduping by mint alone would silently drop one
 * venue's view; keeping both would double-count the market. So: one row per mint, the
 * winner being whichever record carries more populated fields, with its `dex` preserved.
 *
 * `null` counts as absent, which is why `MarketToken` uses `number | null` rather than 0.
 */
export function dedupeByMint(tokens: MarketToken[]): MarketToken[] {
  const best = new Map<string, { token: MarketToken; score: number }>()
  for (const t of tokens) {
    const score =
      (t.priceUsd !== null ? 1 : 0) +
      (t.liquidityUsd !== null ? 1 : 0) +
      (t.volume24hUsd !== null ? 1 : 0) +
      (t.marketCapUsd !== null ? 1 : 0) +
      (t.createdAtUnix !== null ? 1 : 0) +
      (t.holderCount !== null ? 1 : 0) +
      (t.dev !== null ? 1 : 0)
    const cur = best.get(t.mint)
    if (!cur || score > cur.score) best.set(t.mint, { token: t, score })
  }
  return [...best.values()].map(v => v.token)
}

const VERDICT_RANK = { SHORTFALL: 0, backed: 1, donated: 1 } as const

/**
 * The headline ordering: backed tokens first, then by market activity.
 *
 * SHORTFALL sorts to the very top rather than the bottom. It is the one condition on
 * this page that should alarm somebody, and burying an alarm below a hundred healthy
 * rows is how alarms get missed.
 */
export function rankRows(rows: MarketRow[]): MarketRow[] {
  return [...rows].sort((a, b) => {
    const aShort = a.backing?.verdict === 'SHORTFALL'
    const bShort = b.backing?.verdict === 'SHORTFALL'
    if (aShort !== bShort) return aShort ? -1 : 1

    const aBacked = a.backing !== null
    const bBacked = b.backing !== null
    if (aBacked !== bBacked) return aBacked ? -1 : 1

    if (aBacked && bBacked) {
      const r = VERDICT_RANK[a.backing!.verdict] - VERDICT_RANK[b.backing!.verdict]
      if (r !== 0) return r
    }
    // Within a tier, fall back to observable activity. `null` sorts last — a source that
    // did not report volume must not outrank one that reported zero.
    return (b.token.volume24hUsd ?? -1) - (a.token.volume24hUsd ?? -1)
  })
}

/** Newest first, for the live-launch stream. Tokens with no creation time are excluded. */
export function rankByAge(rows: MarketRow[]): MarketRow[] {
  return rows
    .filter(r => r.token.createdAtUnix !== null)
    .sort((a, b) => (b.token.createdAtUnix ?? 0) - (a.token.createdAtUnix ?? 0))
}

export function groupByDex(rows: MarketRow[]): Map<Dex, MarketRow[]> {
  const out = new Map<Dex, MarketRow[]>()
  for (const r of rows) {
    const list = out.get(r.token.dex)
    if (list) list.push(r)
    else out.set(r.token.dex, [r])
  }
  return out
}

export interface MarketStats {
  totalTokens: number
  backedTokens: number
  shortfalls: number
  /** Share of listed tokens with any reserve behind them, 0-1. The product's headline. */
  backedShare: number
  dexCounts: { dex: Dex; count: number }[]
}

export function computeStats(rows: MarketRow[]): MarketStats {
  const backedTokens = rows.filter(r => r.backing !== null).length
  const shortfalls = rows.filter(r => r.backing?.verdict === 'SHORTFALL').length
  const dexCounts = [...groupByDex(rows)]
    .map(([dex, list]) => ({ dex, count: list.length }))
    .sort((a, b) => b.count - a.count)
  return {
    totalTokens: rows.length,
    backedTokens,
    shortfalls,
    backedShare: rows.length === 0 ? 0 : backedTokens / rows.length,
    dexCounts,
  }
}

/** Assemble the snapshot. `backingFor` returns null for the overwhelming majority. */
export function buildSnapshot(
  tokens: MarketToken[],
  sources: SourceStatus[],
  backingFor: (mint: string) => BackingClaim | null,
): MarketSnapshot {
  const rows = dedupeByMint(tokens).map<MarketRow>(token => ({
    token,
    backing: backingFor(token.mint),
  }))
  return { rows: rankRows(rows), sources, fetchedAt: Date.now() }
}
