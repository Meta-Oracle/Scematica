/**
 * SCEMA/SOL price oracle.
 * Primary:  Jupiter Price API v6 (vsToken=WSOL — direct SOL-denominated price)
 * Fallback: DexScreener pair data (priceNative field)
 * Cache TTL: 60 s
 */

const SCEMA_MINT = 'AbKiP2Jc6nM7937jTDfqoJC1bsg5FQ24Buk2iqRFpump'
const WSOL_MINT  = 'So11111111111111111111111111111111111111112'
const CACHE_TTL  = 60_000

let _price: number | null = null
let _fetchedAt = 0

async function fromJupiter(): Promise<number> {
  const url = `https://price.jup.ag/v6/price?ids=${SCEMA_MINT}&vsToken=${WSOL_MINT}`
  const res  = await fetch(url, { signal: AbortSignal.timeout(5_000) })
  const json = await res.json()
  const p = json?.data?.[SCEMA_MINT]?.price
  if (typeof p === 'number' && p > 0) return p
  throw new Error('jupiter: no price')
}

async function fromDexScreener(): Promise<number> {
  const url = `https://api.dexscreener.com/latest/dex/tokens/${SCEMA_MINT}`
  const res  = await fetch(url, { signal: AbortSignal.timeout(5_000) })
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

  // Return last known value or a conservative default (1 SCEMA = 0.000001 SOL)
  return _price ?? 0.000001
}
