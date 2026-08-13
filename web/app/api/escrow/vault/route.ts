import { NextResponse, type NextRequest } from 'next/server'
import { PublicKey } from '@solana/web3.js'

import { ESCROW_PROGRAM_ID, solvency } from '@/lib/escrow/program'
import { readVault, resolveRpc } from '@/lib/escrow/rpc'

// Proof of reserve for one Escrow Market vault.
//
//   GET /api/escrow/vault?token=<mint>&backing=<mint>
//
// Answers exactly one question: how much reserve is actually sitting behind this token
// right now, and at which slot. Everything it reports comes from the chain.
//
// Three rules this route does not bend:
//
//  1. **No simulation, ever.** Unlike the sniper endpoints there is no fallback that
//     invents a figure when the RPC is unreachable — a fabricated reserve is worse than
//     no page. Failures return an error status with the reason.
//  2. **No price, no USD, no "percent backed".** The vault program stores no price and
//     consults no oracle, and neither does this. Raw amounts plus decimals go out; any
//     valuation is the reader's to compute with their own source. There is no feed here
//     to manipulate and no derived number to argue with.
//  3. **`balance >= recorded`, not `==`.** Anyone can donate SPL tokens into any
//     account, so a surplus is normal and permanently stuck. A *shortfall* is the
//     alarming case and is reported as `SHORTFALL` rather than smoothed away.

export const dynamic = 'force-dynamic'
export const runtime = 'nodejs'

export async function GET(request: NextRequest) {
  if (!ESCROW_PROGRAM_ID) {
    // Not deployed yet. Say so plainly rather than deriving PDAs from a placeholder
    // and rendering a page of confident "not found" rows.
    return NextResponse.json(
      {
        ok: false,
        reason: 'not_configured',
        detail:
          'NEXT_PUBLIC_ESCROW_PROGRAM_ID is unset — the escrow program has not been deployed.',
      },
      { status: 503 },
    )
  }

  const params = request.nextUrl.searchParams
  const tokenRaw = params.get('token')?.trim()
  const backingRaw = params.get('backing')?.trim()
  if (!tokenRaw || !backingRaw) {
    return NextResponse.json(
      { ok: false, reason: 'bad_request', detail: 'token and backing mint params are required' },
      { status: 400 },
    )
  }

  let tokenMint: PublicKey
  let backingMint: PublicKey
  try {
    tokenMint = new PublicKey(tokenRaw)
    backingMint = new PublicKey(backingRaw)
  } catch {
    return NextResponse.json(
      { ok: false, reason: 'bad_request', detail: 'token/backing must be base58 mint addresses' },
      { status: 400 },
    )
  }

  const { connection, host, authenticated } = resolveRpc()

  try {
    const reading = await readVault(connection, ESCROW_PROGRAM_ID, tokenMint, backingMint)
    return NextResponse.json({
      ok: true,
      programId: ESCROW_PROGRAM_ID.toBase58(),
      vault: reading.vault,
      state: reading.state,
      balances: {
        token: reading.tokenBalance,
        backing: reading.backingBalance,
        tokenDecimals: reading.tokenDecimals,
        backingDecimals: reading.backingDecimals,
      },
      solvency: {
        token: solvency(reading.state.totalTokenLocked, reading.tokenBalance),
        backing: solvency(reading.state.totalBackingLocked, reading.backingBalance),
      },
      // Provenance travels with the number. A reserve figure whose slot and source are
      // unknown is not checkable, and an uncheckable figure is the thing this whole
      // product exists to replace.
      measuredAt: { slot: reading.slot, fetchedAt: reading.fetchedAt },
      rpc: { host, authenticated },
    })
  } catch (error) {
    const detail = error instanceof Error ? error.message : String(error)
    return NextResponse.json(
      { ok: false, reason: 'read_failed', detail, rpc: { host, authenticated } },
      { status: 502 },
    )
  }
}
