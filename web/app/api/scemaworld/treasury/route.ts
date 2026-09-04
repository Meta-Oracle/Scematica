// GET /api/scemaworld/treasury — what the $SCEMA treasury actually holds, and what it will pay.
//
// A real chain read or an error. **No simulation branch**, for the same reason `/escrow` has none
// and a sharper one: this figure is what a player decides whether to withdraw against, and an
// invented balance is a promise of money that is not there.
//
// Every limit in the policy is returned, not just the balance. A cap the player cannot see is a
// cap that reads as the button being broken — the same lesson as the station panel that states
// *why* a service is unavailable instead of refusing into a notice that fades in three seconds.

import { NextResponse } from 'next/server'

import { policy, readTreasury } from '@/lib/scemaworld/treasury'

export const dynamic = 'force-dynamic'

export async function GET() {
  const result = await readTreasury()
  const limits = policy()

  if (!result.ok) {
    // The reason is carried through rather than flattened. "The mint could not be read", "the
    // treasury has no token account" and "the RPC failed" send an operator to three different
    // places, and collapsing them into one message is how a healthy deployment with a typo in an
    // address gets diagnosed as a network problem.
    return NextResponse.json(
      { ok: false, reason: result.reason, detail: result.detail, host: result.host, policy: limits },
      { status: result.reason === 'rpc_failed' ? 503 : 502 },
    )
  }

  return NextResponse.json({ ok: true, treasury: result.reading, policy: limits })
}
