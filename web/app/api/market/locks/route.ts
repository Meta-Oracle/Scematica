import { NextResponse, type NextRequest } from 'next/server'

import { resolveRpc } from '@/lib/escrow/rpc'
import { LOCK_PROGRAMS, lookupLocksBatch } from '@/lib/market/locks'

// Tier-3 commitment lookup: which mints are referenced by a third-party lock contract.
//
//   GET /api/market/locks?mints=<mint>,<mint>,...
//
// Separate from /api/market on purpose. The board sweep must stay fast; these are
// indexed memcmp queries at ~65-250ms each against programs holding 800k+ accounts, so
// folding them into the 30s board poll would make the whole page wait on them.
//
// Capped at MAX_MINTS per request. A caller asking for 200 mints would fire 400 RPC
// queries, and an endpoint that lets an anonymous caller do that is a way to burn
// somebody's RPC quota. The cap is enforced by truncation with `truncated: true` rather
// than a 400, so the client renders what it got instead of nothing.

export const dynamic = 'force-dynamic'
export const runtime = 'nodejs'

// Kept low deliberately. These queries are not uniform: a mint with a handful of locks
// answers in ~70ms, while JUP walks 8,946 matching accounts and dominates a whole batch.
// Measured warm, 3 mints (JUP, PUMP, WIF) took ~14s of RPC time. At 25 mints a single
// request could exceed a serverless function's execution limit and return nothing —
// worse than returning fewer answers, because the client then shows "not checked" for
// everything rather than for the tail. The client asks in chunks instead.
const MAX_MINTS = 8

export async function GET(request: NextRequest) {
  const raw = request.nextUrl.searchParams.get('mints')?.trim()
  if (!raw) {
    return NextResponse.json(
      { ok: false, reason: 'bad_request', detail: 'mints param is required (comma separated)' },
      { status: 400 },
    )
  }

  const requested = [...new Set(raw.split(',').map(m => m.trim()).filter(Boolean))]
  const mints = requested.slice(0, MAX_MINTS)

  const { connection, host, authenticated } = resolveRpc()

  try {
    const locks = await lookupLocksBatch(connection, mints)
    return NextResponse.json(
      {
        ok: true,
        locks,
        programs: LOCK_PROGRAMS.map(p => ({ key: p.key, label: p.label, url: p.url })),
        truncated: requested.length > mints.length,
        requested: requested.length,
        checked: mints.length,
        rpc: { host, authenticated },
      },
      { headers: { 'cache-control': 'no-store' } },
    )
  } catch (error) {
    // A failed lookup must not render as "no locks" — that would demote a genuinely
    // locked token to tier 0 and print a claim we did not verify.
    return NextResponse.json(
      {
        ok: false,
        reason: 'read_failed',
        detail: error instanceof Error ? error.message : String(error),
        rpc: { host, authenticated },
      },
      { status: 502 },
    )
  }
}
