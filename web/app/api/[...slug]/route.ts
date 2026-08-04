import { type NextRequest, NextResponse } from 'next/server'

import { getSnapshot } from '@/lib/sim/engine'

// Self-contained API. Resolution order per request:
//
//   1. An operator's own bot, if `RUST_API_URL` points at a reachable
//      `scematica-api` — real trades, real money, full control routes.
//   2. Otherwise the built-in simulation engine (`lib/sim/engine.ts`), so the
//      dashboard is fully explorable with **no backend of any kind**. This is what
//      makes a public deploy standalone: no Rust service, no database, no external
//      API — the Deep Q*™ network runs inside this Next.js server.
//
// Simulated responses always carry `simulated: true` and an
// `X-Scematica-Source: simulation` header. The UI turns that into a permanent
// SIMULATION badge — simulated PnL must never read as real money.

export const dynamic = 'force-dynamic'

const RUST_API = process.env.RUST_API_URL || 'http://localhost:3001'
const UPSTREAM_TIMEOUT_MS = 2_000

/**
 * Remember a failed upstream probe briefly. Without this, every panel poll on a
 * deploy with no bot would pay the full connect timeout before falling back.
 */
let upstreamDownUntil = 0
const UPSTREAM_RETRY_MS = 15_000

function simHeaders(extra?: HeadersInit): Headers {
  const h = new Headers(extra)
  h.set('X-Scematica-Source', 'simulation')
  h.set('Cache-Control', 'no-store, max-age=0')
  return h
}

function simJson(body: unknown, status = 200): NextResponse {
  return NextResponse.json(body, { status, headers: simHeaders() })
}

/** Build the simulated response for a given `/api/<path>`, or null if unknown. */
function localResponse(path: string, search: URLSearchParams): NextResponse | null {
  const snap = getSnapshot()
  const limit = Number(search.get('limit') ?? '0') || undefined
  const lines = Number(search.get('lines') ?? '0') || undefined
  const take = <T,>(arr: T[], n?: number) => (n && n > 0 ? arr.slice(0, n) : arr)

  switch (path) {
    case 'metrics':
      return simJson({ ...snap.metrics, simulated: true })
    case 'filters':
      return simJson({ ...snap.filters, simulated: true })
    case 'nn':
      return simJson({ ...snap.nn, simulated: true })
    case 'nn-advice':
      return simJson({ ...snap.advice, simulated: true })
    case 'positions':
      return simJson(snap.positions)
    case 'tournament':
      return simJson({ ...snap.tournament, simulated: true })
    case 'pools':
      return simJson({ pools: take(snap.pools, limit), total: snap.pools.length, simulated: true })
    case 'trades':
      return simJson({ trades: take(snap.trades, limit), simulated: true })
    case 'decisions':
      return simJson({ decisions: take(snap.decisions, limit), simulated: true })
    case 'tx-telemetry':
      return simJson({ telemetry: take(snap.telemetry, limit), simulated: true })
    case 'logs':
      return simJson({ lines: snap.logs.slice(-(lines ?? 200)), simulated: true })
    case 'intelligence':
      return simJson({
        nn: snap.nn,
        advice: snap.advice,
        decisions: take(snap.decisions, limit),
        telemetry: take(snap.telemetry, limit),
        simulated: true,
      })
    case 'health':
      return simJson({
        api: 'simulation',
        sniper_running: true,
        simulated: true,
        note: 'Simulated session — no bot is paired. Pair your own instance for live data.',
      })
    case 'controls':
      return simJson({
        sell_mode: false,
        dump_mode: false,
        rate_mode: 'balanced',
        builder_mode: 'off',
        high_speed: false,
        moon_chase: false,
        simulated: true,
      })
    default:
      return null
  }
}

async function tryUpstream(request: NextRequest, path: string): Promise<NextResponse | null> {
  if (Date.now() < upstreamDownUntil) return null

  const url = `${RUST_API}/api/${path}${request.nextUrl.search}`
  try {
    const isPost = request.method === 'POST'
    const body = isPost ? await request.text() : undefined
    const auth = request.headers.get('authorization')

    const res = await fetch(url, {
      method: request.method,
      headers: {
        Accept: 'application/json',
        ...(isPost ? { 'Content-Type': 'application/json' } : {}),
        ...(auth ? { Authorization: auth } : {}),
      },
      body,
      cache: 'no-store',
      signal: AbortSignal.timeout(UPSTREAM_TIMEOUT_MS),
    })

    // A reachable-but-erroring bot is still a real bot: pass its status through
    // rather than silently masking the failure with simulated data.
    const data = await res.json().catch(() => ({ error: `upstream ${res.status}` }))
    return NextResponse.json(data, {
      status: res.status,
      headers: { 'Cache-Control': 'no-store, max-age=0', 'X-Scematica-Source': 'live' },
    })
  } catch {
    upstreamDownUntil = Date.now() + UPSTREAM_RETRY_MS
    return null
  }
}

async function handle(request: NextRequest, params: { slug: string[] }) {
  const path = params.slug.join('/')

  const upstream = await tryUpstream(request, path)
  if (upstream) return upstream

  // No bot reachable — fall back to the self-contained engine.
  if (request.method === 'POST') {
    // Controls need something to control. Failing loudly beats pretending a
    // toggle took effect against a simulation.
    return simJson(
      {
        error: 'no_instance_paired',
        simulated: true,
        hint: 'Controls require a live sniper. Pair your own instance to send commands.',
      },
      503,
    )
  }

  const local = localResponse(path, request.nextUrl.searchParams)
  if (local) return local

  return simJson({ error: `unknown endpoint: ${path}`, simulated: true }, 404)
}

export async function GET(req: NextRequest, { params }: { params: { slug: string[] } }) {
  return handle(req, params)
}

export async function POST(req: NextRequest, { params }: { params: { slug: string[] } }) {
  return handle(req, params)
}
