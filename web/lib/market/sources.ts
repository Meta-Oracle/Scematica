// Market data sources for the Escrow Market boards.
//
// Every function here reads a real, keyless, third-party API and returns either data or
// a recorded failure. There is NO simulation branch and no synthesised row — the same
// rule `/escrow` and `/alchem-link` hold, and for the sharper reason that this page sits
// next to a custody claim. A fabricated volume beside a real reserve figure lends the
// fabrication the reserve's credibility.
//
// Source notes, all verified against live responses rather than assumed:
//
//   Raydium   api-v3.raydium.io/pools/info/list — keyless, CORS-open, returns
//             {data:{data:[...]}} with tvl, day.volume, price, openTime, burnPercent.
//             Pools are Concentrated (CLMM) or Standard (AMM).
//
//   pump.fun  THE DIRECT API IS DEAD. frontend-api.pump.fun answers HTTP 530,
//             Cloudflare "Error 1016: Origin DNS error" — the origin does not resolve,
//             so there is nothing to fall back to and nothing to retry. pump.fun rows
//             therefore come from Jupiter's `launchpad` field, which really does tag
//             them (28 of 30 recent mints at time of writing). That is a different
//             provenance and is labelled as such in `source`, because "pump.fun says"
//             and "Jupiter says this came from pump.fun" are not the same sentence.
//
//   Jupiter   lite-api.jup.ag/tokens/v2 — keyless. `recent` for launches,
//             `toporganicscore/24h` for the ranked board.

import type { Dex, MarketToken, SourceStatus } from './types'

const TIMEOUT_MS = 10_000

const RAYDIUM_POOLS =
  'https://api-v3.raydium.io/pools/info/list?poolType=all&poolSortField=volume24h&sortType=desc&pageSize=100&page=1'
const JUP_RECENT = 'https://lite-api.jup.ag/tokens/v2/recent'
const JUP_TOP = 'https://lite-api.jup.ag/tokens/v2/toporganicscore/24h?limit=100'

/** Quote assets — the side of a pair that is never the "token" being listed. */
const QUOTES = new Set([
  'So11111111111111111111111111111111111111112', // WSOL
  'EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v', // USDC
  'Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB', // USDT
]);

/** `null`, not `0`, for anything non-finite — "unknown" must not render as "none". */
function num(v: unknown): number | null {
  const n = typeof v === 'string' ? Number(v) : typeof v === 'number' ? v : NaN
  return Number.isFinite(n) ? n : null
}

async function getJson(url: string): Promise<unknown> {
  const res = await fetch(url, {
    signal: AbortSignal.timeout(TIMEOUT_MS),
    headers: { accept: 'application/json' },
    cache: 'no-store',
  })
  if (!res.ok) throw new Error(`HTTP ${res.status}`)
  return res.json()
}

function status(dex: Dex, source: string, count: number, error: string | null): SourceStatus {
  return { dex, source, ok: error === null, count, error, fetchedAt: Date.now() }
}

export interface SourceResult {
  tokens: MarketToken[]
  status: SourceStatus
}

// ── Raydium ──────────────────────────────────────────────────────────────────

interface RayMint { address?: string; symbol?: string; name?: string; decimals?: number }
interface RayPool {
  id?: string
  type?: string
  mintA?: RayMint
  mintB?: RayMint
  price?: number
  tvl?: number
  openTime?: string
  burnPercent?: number
  day?: { volume?: number }
}

/**
 * Pick the listed side of a pair. A WSOL/USDC pool has no "token" in the sense this
 * board means, so it is skipped rather than arbitrarily listing one leg — the boards
 * rank tokens, and a quote pair is not a token listing.
 */
function listedSide(p: RayPool): { mint: RayMint; isB: boolean } | null {
  const a = p.mintA?.address
  const b = p.mintB?.address
  if (!a || !b) return null
  const aQuote = QUOTES.has(a)
  const bQuote = QUOTES.has(b)
  if (aQuote && bQuote) return null
  if (aQuote) return { mint: p.mintB!, isB: true }
  if (bQuote) return { mint: p.mintA!, isB: false }
  return { mint: p.mintA!, isB: false }
}

export async function fetchRaydium(): Promise<SourceResult> {
  try {
    const j = (await getJson(RAYDIUM_POOLS)) as { data?: { data?: RayPool[] } }
    const pools = j?.data?.data
    if (!Array.isArray(pools)) throw new Error('unexpected response shape')

    const tokens: MarketToken[] = []
    for (const p of pools) {
      const side = listedSide(p)
      if (!side?.mint.address) continue
      const openUnix = num(p.openTime)
      tokens.push({
        mint: side.mint.address,
        symbol: side.mint.symbol || '???',
        name: side.mint.name || '',
        decimals: side.mint.decimals ?? 9,
        dex: 'raydium',
        source: `raydium api-v3 (${p.type ?? 'pool'})`,
        // Raydium quotes price as B-per-A. When the listed token is the A side the
        // figure is denominated in the quote asset, not USD, so it is not a USD price
        // and must not be presented as one.
        priceUsd: null,
        liquidityUsd: num(p.tvl),
        volume24hUsd: num(p.day?.volume),
        marketCapUsd: null,
        priceChange24hPct: null,
        createdAtUnix: openUnix && openUnix > 0 ? openUnix : null,
        holderCount: null,
        dev: null,
        poolAddress: p.id ?? null,
        // Raydium reports LP burn but not token authorities. `null` for the ones it
        // does not cover — the commitment grader treats those as unevaluated rather
        // than failed, so a Raydium row is not penalised for Raydium's silence.
        mintRenounced: null,
        freezeRevoked: null,
        devBalancePct: null,
        lpBurnedPct: num(p.burnPercent),
      })
    }
    return { tokens, status: status('raydium', 'api-v3.raydium.io', tokens.length, null) }
  } catch (e) {
    return {
      tokens: [],
      status: status('raydium', 'api-v3.raydium.io', 0, e instanceof Error ? e.message : String(e)),
    }
  }
}

// ── Jupiter (also the provenance for pump.fun and Meteora launchpad rows) ─────

interface JupAudit {
  mintAuthorityDisabled?: boolean
  freezeAuthorityDisabled?: boolean
  devBalancePercentage?: number
}

interface JupTok {
  id?: string
  name?: string
  symbol?: string
  decimals?: number
  dev?: string
  holderCount?: number
  liquidity?: number
  usdPrice?: number
  mcap?: number
  launchpad?: string
  createdAt?: string
  firstPool?: { id?: string; createdAt?: string }
  audit?: JupAudit
  stats24h?: { priceChange?: number; buyVolume?: number; sellVolume?: number }
}

/** Jupiter's `launchpad` string → our venue taxonomy. Unrecognised stays `jupiter`. */
function dexOfLaunchpad(launchpad: string | undefined): Dex {
  if (!launchpad) return 'jupiter'
  const l = launchpad.toLowerCase()
  if (l.includes('pump')) return 'pumpfun'
  if (l.includes('met')) return 'meteora'
  if (l.includes('ray')) return 'raydium'
  if (l.includes('orca') || l.includes('whirl')) return 'orca'
  return 'jupiter'
}

function toMarketToken(t: JupTok, sourceLabel: string): MarketToken | null {
  if (!t.id) return null
  const dex = dexOfLaunchpad(t.launchpad)
  const created = t.firstPool?.createdAt ?? t.createdAt
  const createdMs = created ? Date.parse(created) : NaN
  const s24 = t.stats24h ?? {}
  const buy = num(s24.buyVolume)
  const sell = num(s24.sellVolume)
  return {
    mint: t.id,
    symbol: t.symbol || '???',
    name: t.name || '',
    decimals: t.decimals ?? 6,
    dex,
    // Provenance, not branding: these rows are Jupiter's report of a pump.fun launch,
    // not pump.fun's own API, which no longer resolves.
    source: dex === 'jupiter' ? sourceLabel : `${sourceLabel} (launchpad=${t.launchpad})`,
    priceUsd: num(t.usdPrice),
    liquidityUsd: num(t.liquidity),
    volume24hUsd: buy === null && sell === null ? null : (buy ?? 0) + (sell ?? 0),
    marketCapUsd: num(t.mcap),
    // Jupiter reports priceChange as a fraction over the window.
    priceChange24hPct: s24.priceChange === undefined ? null : (num(s24.priceChange) ?? 0) * 100,
    createdAtUnix: Number.isFinite(createdMs) ? Math.floor(createdMs / 1000) : null,
    holderCount: num(t.holderCount),
    dev: t.dev || null,
    poolAddress: t.firstPool?.id ?? null,
    // `=== true` rather than truthiness: an absent audit field must stay `null`
    // ("not reported"), never collapse to `false` ("authority is live"). The sniper's
    // own feed adapter takes the same position, for the same reason.
    mintRenounced: t.audit?.mintAuthorityDisabled === undefined
      ? null
      : t.audit.mintAuthorityDisabled === true,
    freezeRevoked: t.audit?.freezeAuthorityDisabled === undefined
      ? null
      : t.audit.freezeAuthorityDisabled === true,
    devBalancePct: num(t.audit?.devBalancePercentage),
    lpBurnedPct: null, // Jupiter does not report LP burn.
  }
}

async function fetchJup(url: string, label: string, dex: Dex): Promise<SourceResult> {
  try {
    const raw = (await getJson(url)) as JupTok[]
    if (!Array.isArray(raw)) throw new Error('unexpected response shape')
    const tokens = raw.map(t => toMarketToken(t, label)).filter((t): t is MarketToken => t !== null)
    return { tokens, status: status(dex, label, tokens.length, null) }
  } catch (e) {
    return { tokens: [], status: status(dex, label, 0, e instanceof Error ? e.message : String(e)) }
  }
}

/** Newest mints across every launchpad — the live-launch stream. */
export const fetchLaunches = () => fetchJup(JUP_RECENT, 'jupiter tokens/v2/recent', 'pumpfun')

/** Ranked board by 24h organic score. */
export const fetchTopTokens = () =>
  fetchJup(JUP_TOP, 'jupiter tokens/v2/toporganicscore', 'jupiter')

// NOTE ON pump.fun. There is deliberately no direct pump.fun source here.
// `frontend-api.pump.fun` answers HTTP 530 (Cloudflare 1016, "Origin DNS error") — the
// origin does not resolve, so there is nothing to call and nothing to retry.
//
// pump.fun rows therefore come from Jupiter's `launchpad` tag. That substitution stays
// visible WITHOUT a permanently-red status row, because `toMarketToken` stamps each row's
// `source` as `... (launchpad=pump.fun)`. Provenance travels per row, which is the
// stronger guarantee: a banner can be ignored, a column cannot. If a real pump.fun
// endpoint reappears, add it as a source here rather than reviving a dead-end stub.
