import { NextResponse, type NextRequest } from 'next/server'

import { decodeReading, listFeeds } from '@/lib/alchem/feeds'
import { redact, resolveEndpoint } from '@/lib/alchem/endpoint'
import { DEFAULT_NETWORK, getNetwork } from '@/lib/alchem/networks'
import { gwei, RpcClient } from '@/lib/alchem/rpc'

// End-to-end readiness check for one network — a port of `alchem_link/health.py`.
//
//   GET /api/alchem/doctor?network=ethereum
//
// This exists because the three ways the toolkit fails in practice are all silent: you
// are on the keyless fallback and getting rate limited, you are pointed at the wrong
// chain, or a feed technically responds but has not published in hours. Each check turns
// one of those into a visible line.

export const dynamic = 'force-dynamic'
export const runtime = 'nodejs'

export interface Check {
  name: string
  ok: boolean
  detail: string
  hint?: string
}

export interface DoctorResponse {
  ok: boolean
  network: string
  networkLabel: string
  endpoint: string
  endpointSource: string
  authenticated: boolean
  checks: Check[]
  error?: string
}

const message = (err: unknown) => (err instanceof Error ? err.message : String(err))

export async function GET(request: NextRequest) {
  const network = (request.nextUrl.searchParams.get('network') || DEFAULT_NETWORK).toLowerCase()

  let net
  try {
    net = getNetwork(network)
  } catch (err) {
    return NextResponse.json({ ok: false, error: message(err) }, { status: 400 })
  }

  const endpoint = resolveEndpoint(net.key)
  const client = new RpcClient(endpoint)
  const checks: Check[] = []

  const respond = () =>
    NextResponse.json({
      ok: checks.every(c => c.ok),
      network: net.key,
      networkLabel: net.label,
      endpoint: redact(endpoint.url),
      endpointSource: endpoint.source,
      authenticated: endpoint.authenticated,
      checks,
    })

  checks.push(
    endpoint.authenticated
      ? { name: 'credentials', ok: true, detail: `using ${endpoint.source}` }
      : {
          name: 'credentials',
          ok: true,
          detail: 'using the keyless public endpoint',
          hint: 'Set ALCHEMY_API_KEY on the server for higher rate limits and reliability.',
        },
  )

  // Latency is measured here rather than inside the client: it is the number a developer
  // actually wants ("is this endpoint slow?"), and it belongs to this check alone.
  const started = Date.now()
  let block: number
  try {
    block = await client.blockNumber()
  } catch (err) {
    checks.push({
      name: 'rpc reachable',
      ok: false,
      detail: message(err),
      hint: 'Check the URL, your key, and any proxy.',
    })
    // Nothing below can succeed once the endpoint is unreachable, so stop here instead
    // of emitting three more failures that all say the same thing.
    return respond()
  }
  checks.push({
    name: 'rpc reachable',
    ok: true,
    detail: `block ${block.toLocaleString('en-US')} in ${Date.now() - started} ms`,
  })

  try {
    const actual = await client.chainId()
    const matches = actual === net.chainId
    checks.push({
      name: 'chain id',
      ok: matches,
      detail: `expected ${net.chainId}, got ${actual}`,
      hint: matches
        ? undefined
        : 'The endpoint is serving a different chain than the network you asked for.',
    })
  } catch (err) {
    checks.push({ name: 'chain id', ok: false, detail: message(err) })
  }

  try {
    checks.push({
      name: 'gas price',
      ok: true,
      detail: `${gwei(await client.gasPriceWei()).toFixed(3)} gwei`,
    })
  } catch (err) {
    checks.push({ name: 'gas price', ok: false, detail: message(err) })
  }

  const feeds = listFeeds(net.key)
  if (feeds.length === 0) {
    checks.push({ name: 'feed read', ok: true, detail: 'no feeds registered for this network' })
    return respond()
  }

  const probe = feeds[0]
  try {
    const raw = await client.readAggregators([probe.address])
    const entry = raw.get(probe.address)
    if (!entry || !entry.ok) {
      checks.push({ name: 'feed read', ok: false, detail: entry?.ok === false ? entry.error : 'no response' })
    } else {
      const reading = decodeReading(probe, net.key, entry.raw)
      const fresh = !reading.stale
      checks.push({
        name: 'feed read',
        ok: fresh,
        detail:
          `${reading.pair} = ${reading.price.toLocaleString('en-US', { maximumFractionDigits: 4 })} ` +
          `(${reading.status}, ${reading.ageSecs}s old)`,
        hint: fresh
          ? undefined
          : `Last update exceeds the ${reading.heartbeatSecs}s heartbeat — do not trade on it.`,
      })
    }
  } catch (err) {
    checks.push({ name: 'feed read', ok: false, detail: message(err) })
  }

  return respond()
}
