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

/**
 * The upstream API root, with a trailing `/api` stripped.
 *
 * Every request below appends `/api/<path>`, so `RUST_API_URL=https://host/api` asks for
 * `https://host/api/api/mesh` and 404s on every endpoint. On Vercel that failure is
 * invisible: the env var is server-only, the 404 body is empty, and the dashboard just
 * looks empty. The API mounts at the server root, so a base legitimately ending in
 * `/api` cannot exist and stripping is always safe — and a reverse proxy that maps
 * `/api/*` onto the API root is served correctly by the stripped form.
 */
const RUST_API = (process.env.RUST_API_URL || 'http://localhost:3001')
  .trim()
  .replace(/\/+$/, '')
  .replace(/\/api$/i, '')

/**
 * A Vercel lambda reaching an operator's home tunnel (cloudflared / ngrok / Tailscale)
 * is a wide-area round trip through a relay, not the localhost hop this originally
 * assumed. 2s timed out a working instance often enough to look like an outage, and the
 * negative cache below then suppressed the next 15s of polls behind it. Override with
 * `UPSTREAM_TIMEOUT_MS` when a tunnel is unusually slow.
 */
const UPSTREAM_TIMEOUT_MS = Number(process.env.UPSTREAM_TIMEOUT_MS) || 8_000

/**
 * Remember a failed upstream probe briefly. Without this, every panel poll on a
 * deploy with no bot would pay the full connect timeout before falling back.
 *
 * Only *connection* failures arm this. An upstream that answers — even with a 404 or a
 * 500 — is a reachable bot, and suppressing the next 15s of polls after one bad status
 * turns a single hiccup into a rolling outage across every panel.
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
    case 'mesh':
      // THE ONE ENDPOINT HERE WITH NO SIMULATION, AND THE REASON IS A CATEGORY
      // DIFFERENCE. A simulated metric is a fake number wearing a SIMULATED badge, and a
      // reader discounts it accordingly. A simulated *topology* would assert that a
      // particular set of units exists, is wired a particular way, and is healthy — a
      // claim about the operator's machine rather than about a value on it. There is no
      // honest way to badge that, so it is not offered.
      //
      // Note this is NOT the same as an empty mesh: a collector run against a directory
      // with no state files returns a complete topology with every node dark, which is a
      // true statement. Only the web layer, which has no directory to read at all, has
      // nothing truthful to say.
      return simJson(
        {
          error: 'no_instance_paired',
          simulated: true,
          hint: 'The mesh is a picture of a running system. Pair a sniper instance — there is no simulated topology, by design.',
        },
        503,
      )
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
    //
    // A 404 is the one status worth translating. The upstream body is empty, so the
    // browser would receive `{"error":"upstream 404"}` and no way to tell a wrong
    // RUST_API_URL from a route this build does not have — which is exactly the state
    // a mis-rooted base URL produces on every single endpoint. Name the URL that was
    // actually requested; it is server-side, but the *shape* of it is the diagnosis.
    const data = await res.json().catch(() =>
      res.status === 404
        ? {
            error: 'upstream_route_missing',
            status: 404,
            hint: `The paired instance answered, but has no route at /api/${path}. This is usually RUST_API_URL pointing at a sub-path — it must be the API root (e.g. https://host, not https://host/api), because this proxy appends /api/... itself.`,
          }
        : { error: `upstream ${res.status}`, status: res.status },
    )
    return NextResponse.json(data, {
      status: res.status,
      headers: { 'Cache-Control': 'no-store, max-age=0', 'X-Scematica-Source': 'live' },
    })
  } catch {
    // Connection-level failure only — a bad status returns above and never lands here.
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
