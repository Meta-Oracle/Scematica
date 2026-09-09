// Funding and sweeping the session key.
//
// PURE arithmetic, split from the transfer for the same reason `transferPlan` is split
// from `settle` in the treasury path: settling for real needs a funded key, so otherwise
// the arithmetic bounding every payout would only ever be exercised by moving money.
//
// ── The rule that shapes this file ───────────────────────────────────────────
//
// **A sweep may only return funds to the wallet that supplied them.** The destination is
// not an input; it is read from the session record. A sweep that took an address would be
// a one-click drain of the hot key to anywhere, which is precisely the capability the
// caps exist to deny — and it would be reachable by anything that can run in the page,
// which is the same threat model the key itself lives under.

/** Cost of one `SystemProgram.transfer`. Five thousand lamports, plus headroom. */
export const TRANSFER_FEE_LAMPORTS = 5_000
/**
 * Left behind on a sweep so the transaction can pay for itself.
 *
 * Generous on purpose: a sweep that under-reserves fails, and a failed sweep leaves the
 * whole balance in a key the operator has already decided to stop trusting.
 */
export const SWEEP_RESERVE_LAMPORTS = 15_000

/** Below this a top-up is not worth its own fee. */
export const MIN_FUND_LAMPORTS = 10_000_000 // 0.01 SOL

export type FundingRefusal =
  | 'over-balance-cap'
  | 'below-minimum'
  | 'nothing-to-sweep'
  | 'no-funder'
  | 'insufficient-funder'

export type FundingPlan =
  | { ok: true; lamports: number; note: string }
  | { ok: false; refusal: FundingRefusal; detail: string }

/**
 * How much may be moved into the session key.
 *
 * The cap is on the RESULTING balance, not on the transfer, so repeated top-ups cannot
 * walk past it one increment at a time. That is the same shape as the spend ledger's
 * `committed`: a limit measured against the total rather than the delta is the only kind
 * that holds under repetition.
 */
export function fundingPlan(
  currentBalanceLamports: number,
  requestedLamports: number,
  maxBalanceLamports: number,
  funderBalanceLamports: number | null,
): FundingPlan {
  if (funderBalanceLamports === null) {
    return { ok: false, refusal: 'no-funder', detail: 'connect a wallet to fund from' }
  }
  if (requestedLamports < MIN_FUND_LAMPORTS) {
    return {
      ok: false,
      refusal: 'below-minimum',
      detail: `${requestedLamports} lamports is below the ${MIN_FUND_LAMPORTS} minimum — the fee would dominate`,
    }
  }
  const resulting = currentBalanceLamports + requestedLamports
  if (resulting > maxBalanceLamports) {
    const room = Math.max(0, maxBalanceLamports - currentBalanceLamports)
    return {
      ok: false,
      refusal: 'over-balance-cap',
      detail:
        room === 0
          ? `the session key is already at its ${maxBalanceLamports} lamport cap`
          : `that would take the key to ${resulting}, over the ${maxBalanceLamports} cap — room for ${room}`,
    }
  }
  if (funderBalanceLamports < requestedLamports + TRANSFER_FEE_LAMPORTS) {
    return {
      ok: false,
      refusal: 'insufficient-funder',
      detail: `your wallet holds ${funderBalanceLamports}, and this needs ${requestedLamports + TRANSFER_FEE_LAMPORTS} including the fee`,
    }
  }
  return {
    ok: true,
    lamports: requestedLamports,
    note: `session key will hold ${resulting} of a ${maxBalanceLamports} cap`,
  }
}

/**
 * How much comes back on a sweep.
 *
 * Everything except the fee reserve. The session account is a plain system account, so it
 * may legitimately end at zero — there is no rent-exemption floor to respect and leaving
 * one behind would strand lamports in a key the operator is abandoning.
 */
export function sweepPlan(balanceLamports: number): FundingPlan {
  const movable = balanceLamports - SWEEP_RESERVE_LAMPORTS
  if (movable <= 0) {
    return {
      ok: false,
      refusal: 'nothing-to-sweep',
      detail:
        balanceLamports === 0
          ? 'the session key is empty'
          : `${balanceLamports} lamports is not enough to cover the ${SWEEP_RESERVE_LAMPORTS} needed to send it`,
    }
  }
  return {
    ok: true,
    lamports: movable,
    note: `returning ${movable} lamports; ${SWEEP_RESERVE_LAMPORTS} stays to pay for the transfer`,
  }
}

/**
 * What a sweep CANNOT do, stated so the UI can say it.
 *
 * A sweep moves SOL. It does not close token positions, and it must not pretend to: a key
 * swept while holding an open position leaves that position on chain, owned by a key with
 * no SOL to sell it with. The UI refuses the sweep in that case rather than producing a
 * stranded position, and this function is why the refusal has a sentence.
 */
export function sweepBlockedBy(openPositions: number): string | null {
  if (openPositions === 0) return null
  return (
    `${openPositions} position(s) are still open. Sweeping the SOL out leaves them owned by a key ` +
    `with nothing to pay a sell fee with. Close them first, or accept that they can only be ` +
    `recovered by re-funding this key.`
  )
}
