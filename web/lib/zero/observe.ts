// Did the swap land? — and the third answer.
//
// ── Never `sendAndConfirmTransaction` ────────────────────────────────────────
//
// It waits on a WebSocket `signatureSubscribe`. The treasury path already paid for this:
// `ws`'s bufferutil binding does not survive Next's bundler, so the promise never settles
// — on a transaction that already finalized. A successful trade presenting as a dead bot
// is the worst pair of facts this feature can produce, and the obvious retry pays twice.
//
// So: send raw, poll `getSignatureStatuses` over HTTP. A subscription would leak a socket
// per trade anyway.
//
// ── Unknown is an outcome, not an error ──────────────────────────────────────
//
// `scema-effect`'s `Outcome::Unknown`. An attempt whose result nobody could observe is
// neither success nor failure. Zero records the position as `unknown`, keeps the session
// reservation (releasing it is how a paid trade gets paid again), tells the operator, and
// **does not retry**. `exits.ts` refuses to price an unknown position, so nothing
// downstream acts on a number it does not have.

export type Observation =
  | { outcome: 'landed'; signature: string; slot: number }
  | { outcome: 'failed'; signature: string; reason: string }
  | { outcome: 'unknown'; signature: string; reason: string }

/** Status shape from `getSignatureStatuses`, narrowed to what matters. */
export interface SignatureStatus {
  slot: number
  confirmationStatus?: 'processed' | 'confirmed' | 'finalized'
  err: unknown | null
}

/**
 * Interpret one status response.
 *
 * `null` means the cluster has no record of the signature. That is genuinely ambiguous:
 * it can mean "never landed" or "landed and the status has already been pruned", and the
 * two are indistinguishable from here. Which one it is depends on how long we have been
 * asking — hence `elapsedSecs`.
 */
export function interpret(
  signature: string,
  status: SignatureStatus | null,
  elapsedSecs: number,
  blockhashValidSecs: number,
): Observation {
  if (status) {
    if (status.err !== null && status.err !== undefined) {
      // The chain saw it and rejected it. Nothing moved: this is the one case where
      // releasing the reservation is safe, because we have positive evidence of failure.
      return { outcome: 'failed', signature, reason: `transaction reverted: ${JSON.stringify(status.err)}` }
    }
    if (status.confirmationStatus === 'confirmed' || status.confirmationStatus === 'finalized') {
      return { outcome: 'landed', signature, slot: status.slot }
    }
    return { outcome: 'unknown', signature, reason: `seen but only ${status.confirmationStatus ?? 'processed'}` }
  }

  // No record. Before the blockhash could possibly have expired, absence is just
  // propagation delay — keep waiting rather than declaring anything.
  if (elapsedSecs < blockhashValidSecs) {
    return { outcome: 'unknown', signature, reason: 'not yet visible to this endpoint' }
  }

  // Past the blockhash's validity a transaction that was going to land has landed. This
  // is the strongest "it failed" available without a full-history node — and it is still
  // not certain, which is why it stays `failed` only when we also never saw it at all.
  return {
    outcome: 'failed',
    signature,
    reason: `no record after ${Math.round(elapsedSecs)}s; the blockhash can no longer be valid`,
  }
}

/**
 * How long a blockhash stays usable, in seconds.
 *
 * 150 slots at ~400ms. Deliberately a named constant rather than an inline 60: the number
 * decides when Zero is allowed to call a trade dead, and calling one dead too early is
 * how a landed trade gets retried.
 */
export const BLOCKHASH_VALID_SECS = 90

export interface PollPlan {
  /** Milliseconds to wait before the next status read. */
  delayMs: number
  /** False once the caller should stop and treat the result as final. */
  keepPolling: boolean
}

/**
 * Backoff for the status poll.
 *
 * This is a timer, and it is allowed to be one: it does not DECIDE anything. It only
 * asks again. Z-1 forbids a timer from deciding money — an exit rule, a size, a veto —
 * not from scheduling a read whose *answer* is what decides. The distinction is the whole
 * reason Z-1 is expressible as a rule rather than a ban on `setTimeout`.
 */
export function pollPlan(attempt: number, elapsedSecs: number): PollPlan {
  if (elapsedSecs > BLOCKHASH_VALID_SECS + 30) return { delayMs: 0, keepPolling: false }
  // 400ms, 800ms, 1.6s, then every 3s. Fast at first because most land in a slot or two.
  const delayMs = Math.min(3000, 400 * 2 ** Math.min(attempt, 3))
  return { delayMs, keepPolling: true }
}

/**
 * What the operator is told about an unknown fill.
 *
 * Deliberately not phrased as an error. It names the signature, says what is and is not
 * known, and says what Zero will not do — because the failure mode is a human retrying by
 * hand on the assumption that nothing happened.
 */
export function unknownNotice(signature: string, reason: string): string {
  return (
    `A swap was submitted but could not be observed (${reason}). ` +
    `Signature ${signature}. It may have landed. Zero has NOT retried and will not: ` +
    `the position is marked unknown and its budget stays reserved. Check the signature ` +
    `before doing anything by hand.`
  )
}
