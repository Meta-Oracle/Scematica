import { NextResponse, type NextRequest } from 'next/server'

import { decodeReading, listFeeds } from '@/lib/alchem/feeds'
import { redact, resolveEndpoint } from '@/lib/alchem/endpoint'
import { DEFAULT_NETWORK, getNetwork } from '@/lib/alchem/networks'
import { RpcClient } from '@/lib/alchem/rpc'

// Confirm each registered address still reports the pair it is filed under.
//
//   GET /api/alchem/verify?network=base
//
// This is the check that caught the Base "BTC/USD" address actually reporting
// `WBTC / USD` on-chain. It is also the only thing keeping `lib/alchem/feeds.ts` honest
// against the Python registry it was ported from: both are just tables, and this route
// asks the chain instead of either one.

export const dynamic = 'force-dynamic'
export const runtime = 'nodejs'

export interface VerifyEntry {
  pair: string
  address: string
  ok: boolean
  /** The contract's own description(), when it could be read. */
  description?: string
  decimals?: number
  declaredDecimals: number
  price?: number
  status?: string
  error?: string
}

const normalise = (value: string) => value.replace(/\s+/g, '').toUpperCase()

export async function GET(request: NextRequest) {
  const network = (request.nextUrl.searchParams.get('network') || DEFAULT_NETWORK).toLowerCase()

  let net
  try {
    net = getNetwork(network)
  } catch (err) {
    return NextResponse.json(
      { ok: false, error: err instanceof Error ? err.message : String(err) },
      { status: 400 },
    )
  }

  const endpoint = resolveEndpoint(net.key)
  const feeds = listFeeds(net.key)
  const base = {
    network: net.key,
    networkLabel: net.label,
    endpoint: redact(endpoint.url),
    endpointSource: endpoint.source,
  }

  if (feeds.length === 0) {
    return NextResponse.json({ ...base, ok: true, entries: [] })
  }

  const client = new RpcClient(endpoint)

  let raw
  try {
    raw = await client.readAggregators(feeds.map(f => f.address))
  } catch (err) {
    return NextResponse.json(
      { ...base, ok: false, entries: [], error: err instanceof Error ? err.message : String(err) },
      { status: 502 },
    )
  }

  const entries: VerifyEntry[] = feeds.map(f => {
    const entry = raw.get(f.address)
    if (!entry || !entry.ok) {
      return {
        pair: f.pair,
        address: f.address,
        ok: false,
        declaredDecimals: f.decimals,
        error: entry?.ok === false ? entry.error : 'no response for this address',
      }
    }
    try {
      const reading = decodeReading(f, net.key, entry.raw)
      return {
        pair: f.pair,
        address: f.address,
        // Whitespace differs between registries ("ETH / USD" vs "ETH/USD"); the pair
        // identity is what must match, not the formatting.
        ok: normalise(reading.description) === normalise(f.pair),
        description: reading.description,
        decimals: reading.decimals,
        declaredDecimals: f.decimals,
        price: reading.price,
        status: reading.status,
      }
    } catch (err) {
      return {
        pair: f.pair,
        address: f.address,
        ok: false,
        declaredDecimals: f.decimals,
        error: err instanceof Error ? err.message : String(err),
      }
    }
  })

  return NextResponse.json({ ...base, ok: entries.every(e => e.ok), entries })
}
