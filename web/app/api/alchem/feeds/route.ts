import { NextResponse, type NextRequest } from 'next/server'

import { decodeReading, listFeeds, type FeedFailure, type FeedReading } from '@/lib/alchem/feeds'
import { redact, resolveEndpoint } from '@/lib/alchem/endpoint'
import { DEFAULT_NETWORK, getNetwork } from '@/lib/alchem/networks'
import { RpcClient } from '@/lib/alchem/rpc'

// Live Chainlink feed reads for one network.
//
//   GET /api/alchem/feeds?network=ethereum
//
// Server-side on purpose. ALCHEMY_API_KEY must never reach the browser bundle, and most
// public RPC endpoints send no CORS headers, so reading them from the page would fail
// even without the key concern.
//
// This route reads a chain or it reports the error. There is no simulated branch — the
// whole point of the panel is that a stale oracle is visible, and a fabricated price
// would defeat it more thoroughly than showing nothing.

export const dynamic = 'force-dynamic'
export const runtime = 'nodejs'

export interface FeedsResponse {
  ok: boolean
  network: string
  networkLabel: string
  chainId: number
  explorer: string
  endpoint: string
  endpointSource: string
  authenticated: boolean
  readAt: number
  readings: FeedReading[]
  failures: FeedFailure[]
  error?: string
}

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
  const base = {
    network: net.key,
    networkLabel: net.label,
    chainId: net.chainId,
    explorer: net.explorer,
    endpoint: redact(endpoint.url),
    endpointSource: endpoint.source,
    authenticated: endpoint.authenticated,
    readAt: Math.floor(Date.now() / 1000),
  }

  const feeds = listFeeds(net.key)
  if (feeds.length === 0) {
    return NextResponse.json({ ...base, ok: true, readings: [], failures: [] })
  }

  const client = new RpcClient(endpoint)

  let raw
  try {
    raw = await client.readAggregators(feeds.map(f => f.address))
  } catch (err) {
    // Transport failure takes out every call at once — report it as one endpoint-level
    // error rather than N identical per-feed rows.
    return NextResponse.json(
      {
        ...base,
        ok: false,
        readings: [],
        failures: [],
        error: err instanceof Error ? err.message : String(err),
      },
      { status: 502 },
    )
  }

  const readings: FeedReading[] = []
  const failures: FeedFailure[] = []

  for (const f of feeds) {
    const entry = raw.get(f.address)
    if (!entry) {
      failures.push({ pair: f.pair, address: f.address, error: 'no response for this address' })
      continue
    }
    if (!entry.ok) {
      failures.push({ pair: f.pair, address: f.address, error: entry.error })
      continue
    }
    try {
      readings.push(decodeReading(f, net.key, entry.raw, base.readAt))
    } catch (err) {
      // A decode failure is a real finding — usually the address is not an AggregatorV3
      // contract. Surfacing it beats dropping the row and leaving a silent gap.
      failures.push({
        pair: f.pair,
        address: f.address,
        error: err instanceof Error ? err.message : String(err),
      })
    }
  }

  return NextResponse.json({ ...base, ok: failures.length === 0, readings, failures })
}
