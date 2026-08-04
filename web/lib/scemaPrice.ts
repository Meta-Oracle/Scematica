/**
 * SCEMA/SOL price oracle.
 * Primary:  Jupiter Price API v3 (USD prices for SCEMA + WSOL → SOL-denominated ratio)
 * Fallback: DexScreener pair data (priceNative field)
 * Cache TTL: 60 s
 *
 * NOTE: the old `price.jup.ag/v6` host was retired and no longer resolves — calling it
 * from the browser produced a `net::ERR_NAME_NOT_RESOLVED` on every poll. v3 only quotes
 * in USD, so SOL-denominated price is derived by asking for both mints in one request.
 */

import { getUsdPrices, WSOL_MINT } from './feed/price'
import { SCEMA_MINT } from './ScemaGateContext'

const CACHE_TTL  = 60_000
// After both sources fail, don't retry (and don't log another pair of network errors)
// until this cools off — a persistent outage otherwise spams the console on every poll.
const FAIL_TTL   = 30_000

let _price: number | null = null
let _fetchedAt = 0
let _failedAt = 0

async function fromJupiter(): Promise<number> {
  // Shares the feed's price cache, so the SOL quote the pool feed already fetched is
  // reused rather than re-requested.
  const prices = await getUsdPrices([SCEMA_MINT, WSOL_MINT])
  const scemaUsd = prices[SCEMA_MINT]
  const solUsd   = prices[WSOL_MINT]
  if (!scemaUsd || !solUsd) throw new Error('jupiter: no price')
  return scemaUsd / solUsd
}

async function fromDexScreener(): Promise<number> {
  const url = `https://api.dexscreener.com/latest/dex/tokens/${SCEMA_MINT}`
  const res  = await fetch(url, { signal: AbortSignal.timeout(5_000) })
  if (!res.ok) throw new Error(`dexscreener: HTTP ${res.status}`)
  const json = await res.json()
  // priceNative is SOL-denominated on Solana pairs
  const pairs: Array<{ priceNative?: string; liquidity?: { usd?: number } }> =
    json?.pairs ?? []
  // Pick the most liquid pair that has a SOL price
  const best = pairs
    .filter(p => p.priceNative && Number(p.priceNative) > 0)
    .sort((a, b) => (b.liquidity?.usd ?? 0) - (a.liquidity?.usd ?? 0))[0]
  const p = Number(best?.priceNative)
  if (p > 0) return p
  throw new Error('dexscreener: no price')
}

/**
 * Returns the SCEMA price in SOL (e.g. 0.000001 means 1 SCEMA = 0.000001 SOL).
 * Result is cached for 60 s. Falls back to the last known price, then to a
 * conservative estimate of 0.000001 SOL if no price has ever been fetched.
 */
export async function getScemaPriceInSol(): Promise<number> {
  const now = Date.now()
  if (_price !== null && now - _fetchedAt < CACHE_TTL) return _price
  if (now - _failedAt < FAIL_TTL) return _price ?? 0.000001

  try {
    _price = await fromJupiter()
    _fetchedAt = now
    return _price
  } catch { /* fall through */ }

  try {
    _price = await fromDexScreener()
    _fetchedAt = now
    return _price
  } catch { /* fall through */ }

  _failedAt = now
  // Return last known value or a conservative default (1 SCEMA = 0.000001 SOL)
  return _price ?? 0.000001
}
