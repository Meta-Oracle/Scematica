// POST /api/scemaworld/claim — withdraw in-game SCEMA as real $SCEMA.
//
// The only route on this site that moves money. Three things it must never do, in order of how
// badly each would go wrong:
//
// 1. **Report a success it did not achieve.** A deployment with no signer answers 501 and says
//    so. A failed transfer answers 502 with the chain's own error. There is no branch anywhere in
//    this path that returns a signature it did not get back from `sendAndConfirmTransaction`.
// 2. **Trust the client's number without bounding it.** It cannot verify it — the balance lives
//    in a browser tab (see `lib/scemaworld/claim.ts`, which says so at length). What it can do is
//    cap it, per claim, per wallet, per deployment, and behind a cooldown, and write every payment
//    to a ledger first.
// 3. **Refuse without saying why.** Each refusal names its own cause, because "capped at 250 per
//    withdrawal", "this wallet has drawn its lifetime limit" and "this build cannot pay" are three
//    different instructions and only one of them means try again later.
//
// `?quote=1` prices a claim without settling it, using the same policy and the same ledger — one
// implementation consulted at both ends, so the preview cannot disagree with the payer.

import { NextResponse } from 'next/server'

import { looksLikeAddress } from '@/lib/scemaworld/claim'
import { policy, quote, settle } from '@/lib/scemaworld/treasury'

export const dynamic = 'force-dynamic'

/** HTTP status per refusal. The distinctions are the point; see the header note. */
const STATUS: Record<string, number> = {
  bad_wallet: 400,
  nothing_to_claim: 400,
  below_minimum: 400,
  wallet_limit: 429,
  cooling_down: 429,
  budget_exhausted: 503,
  treasury_short: 503,
  // Not the player's fault and not the chain's: this build can quote and cannot pay.
  not_configured: 501,
  signer_mismatch: 500,
  ledger_unreadable: 500,
  // Another writer held the ledger, so this claim was refused *before* anything was sent. A retry
  // is safe, which is why this is a 503 and not one of the 5xx codes that mean "state unknown".
  ledger_busy: 503,
  transfer_failed: 502,
  // Accepted, outcome unobserved. **202, not an error code.** The request was taken and a
  // transaction was broadcast; what is missing is knowledge of whether it landed. Answering 502
  // here would tell a client the transfer failed, which is a claim nobody is in a position to
  // make and which invites the one retry that could pay twice.
  unconfirmed: 202,
  mint_unreadable: 502,
  not_a_mint: 502,
  account_unreadable: 502,
  rpc_failed: 503,
}

export async function POST(req: Request) {
  let body: { wallet?: unknown; scema?: unknown; world?: unknown }
  try {
    body = await req.json()
  } catch {
    return NextResponse.json({ ok: false, reason: 'bad_request', detail: 'expected JSON' }, { status: 400 })
  }

  const wallet = typeof body.wallet === 'string' ? body.wallet.trim() : ''
  const scema = typeof body.scema === 'number' ? body.scema : Number.NaN
  // The world commitment the claim was made against, when the client names one. It does **not**
  // affect the amount — every world pays identically, which is the property that stops a forged
  // record being worth writing — and is recorded purely so the ledger is auditable per record.
  const world = typeof body.world === 'string' && body.world.trim() ? body.world.trim() : null

  if (!looksLikeAddress(wallet)) {
    return NextResponse.json(
      { ok: false, reason: 'bad_wallet', detail: 'that does not look like a Solana address' },
      { status: 400 },
    )
  }
  if (!Number.isFinite(scema) || scema <= 0) {
    return NextResponse.json(
      { ok: false, reason: 'nothing_to_claim', detail: 'no SCEMA offered' },
      { status: 400 },
    )
  }

  const nowMs = Date.now()

  if (new URL(req.url).searchParams.get('quote') === '1') {
    const q = await quote(scema, wallet, nowMs)
    if (!q.ok) {
      // The same refusal the settle path gives for the same condition. A preview that quotes an
      // amount the payer will not pay is worse than no preview — and quoting against an
      // unreadable ledger means quoting against caps that have silently reset to full.
      return NextResponse.json(
        { ok: false, reason: q.reason, detail: q.detail },
        { status: STATUS[q.reason] ?? 500 },
      )
    }
    return NextResponse.json({
      ok: true,
      quote: q.entitlement,
      policy: policy(),
      dispensed: q.dispensed,
      // Whether this build could settle the quote it just gave. Surfaced so the panel can label
      // the button honestly rather than letting a player press it and meet a 501.
      configured: q.treasury.ok ? q.treasury.reading.configured : false,
      treasury: q.treasury.ok ? q.treasury.reading : null,
      treasuryError: q.treasury.ok ? null : { reason: q.treasury.reason, detail: q.treasury.detail },
    })
  }

  const result = await settle({ scema, wallet, world, nowMs })
  if (!result.ok) {
    return NextResponse.json(
      {
        ok: false,
        reason: result.reason,
        detail: result.detail,
        // Carried on `unconfirmed` and on an on-chain failure, so the player has the one thing
        // that lets them find out what actually happened. A refusal that hides the signature of a
        // transaction it just broadcast is worse than useless.
        signature: 'signature' in result ? (result.signature ?? null) : null,
        quote: 'entitlement' in result ? (result.entitlement ?? null) : null,
      },
      { status: STATUS[result.reason] ?? 500 },
    )
  }
  return NextResponse.json({
    ok: true,
    tokens: result.tokens,
    // What the client must debit locally. Returned rather than assumed, because a capped claim
    // spends less than was offered and a client that debited the offer would burn the difference.
    spend: result.spend,
    signature: result.signature,
    createdAccount: result.created,
  })
}
