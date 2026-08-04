// Shared USD price lookups against Jupiter Price v3.
//
// Keyless and CORS-permissive (the host reflects the caller's Origin), so this works
// straight from the browser with no backend and no API key — which is what lets the
// discovery half of the dashboard stand on its own.
//
// One cache serves every caller: asking for SCEMA and WSOL in the same tick costs a
// single request, and a warm entry costs none.

const JUP_PRICE = 'https://lite-api.jup.ag/price/v3'
const TTL_MS = 60_000
/** Cool-off after a failure so an outage can't spam the console once per poll. */
const FAIL_TTL_MS = 30_000

export const WSOL_MINT = 'So11111111111111111111111111111111111111112'

interface Entry { usd: number; at: number }

const cache = new Map<string, Entry>()
let failedAt = 0

function fresh(id: string, now: number): number | undefined {
  const e = cache.get(id)
  return e && now - e.at < TTL_MS ? e.usd : undefined
}

/**
 * USD price per token for each mint. Mints that could not be priced are omitted
 * rather than reported as 0 — callers must decide what an unknown price means.
 */
export async function getUsdPrices(ids: string[]): Promise<Record<string, number>> {
  const now = Date.now()
  const out: Record<string, number> = {}
  const missing: string[] = []

  for (const id of ids) {
    const hit = fresh(id, now)
    if (hit !== undefined) out[id] = hit
    else missing.push(id)
  }
  if (missing.length === 0) return out
  if (now - failedAt < FAIL_TTL_MS) return out

  try {
    const res = await fetch(`${JUP_PRICE}?ids=${missing.join(',')}`, {
      signal: AbortSignal.timeout(5_000),
    })
    if (!res.ok) throw new Error(`price v3: HTTP ${res.status}`)
    const json = (await res.json()) as Record<string, { usdPrice?: number }>
    for (const id of missing) {
      const usd = json?.[id]?.usdPrice
      if (typeof usd === 'number' && usd > 0) {
        cache.set(id, { usd, at: now })
        out[id] = usd
      }
    }
  } catch {
    failedAt = now
  }
  return out
}

/** SOL/USD, or null when unavailable. Needed to express USD liquidity in SOL. */
export async function getSolUsd(): Promise<number | null> {
  const prices = await getUsdPrices([WSOL_MINT])
  return prices[WSOL_MINT] ?? null
}
