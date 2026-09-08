// The session key — bounded autonomy.
//
// An Ed25519 keypair generated in the browser, funded with a bounded amount, signing
// without a prompt. It is a hot key in a browser tab and there is no way to pretend
// otherwise, so this module does not pretend — it BOUNDS.
//
// The precedent is exact. `lib/scemaworld/claim.ts` met the same problem (a balance in a
// browser tab is trivially forgeable) and answered it the same way: caps that buy bounded
// loss rather than secrecy, stated on screen, because a cap the user cannot see reads as
// a broken button.
//
// ── The rule this file exists to enforce ─────────────────────────────────────
//
// **Deciding a spend and reserving it are ONE step.** This is the N-1 finding from the
// security audit and the sharpest lesson on the treasury path: a cap checked against a
// snapshot and enforced by a write that happens after an `await` is not a cap. Two
// entries arriving together each measured themselves against a budget the other had
// already taken. So `authorise()` below is the whole critical section — it reads, decides
// and reserves synchronously, with no `await` anywhere inside it, and returns either a
// reservation or a refusal.
//
// A reservation is released ONLY by an observed failure. An unobserved outcome keeps it,
// because the money may already have moved and a released hold is exactly what lets a
// retry pay twice (`scema-spend`, `Outcome::Unknown`).

import { type Term, absent, measured } from './types.ts'

export interface SessionCaps {
  /** Hard ceiling on what may sit in the key at once. */
  maxBalanceLamports: number
  /** Ceiling on a single entry. */
  maxPerTradeLamports: number
  /** Ceiling on everything this session may ever spend. */
  budgetLamports: number
  /** Wall-clock expiry. After this the key signs nothing. */
  expiresAtUnix: number
  /** Minimum gap between two entries, in seconds. Blunt, and it is meant to be. */
  cooldownSecs: number
}

export interface Reservation {
  id: string
  lamports: number
  atUnix: number
}

export interface SessionLedger {
  /** Monotonic. A lost write shows up as a REFUSED spend rather than a double one. */
  version: number
  /** Settled — the money demonstrably moved. */
  spentLamports: number
  /** Authorised and not yet resolved. Occupies budget from the moment it is taken. */
  reserved: Reservation[]
  lastSpendUnix: number | null
  /** Reservations that resolved as Unknown. Never released, surfaced for a human. */
  strandedIds: string[]
}

export const newLedger = (): SessionLedger => ({
  version: 0,
  spentLamports: 0,
  reserved: [],
  lastSpendUnix: null,
  strandedIds: [],
})

/**
 * `committed = spent + reserved`, and this is what every cap is measured against.
 *
 * An earlier design in this repo tracked only settled spend, on the reasoning that only a
 * settled payment may consume budget. True, and it left the cumulative cap defeated
 * anyway: a spend occupies its allowance from the moment it is authorised, not from
 * whenever a receipt turns up.
 */
export function committed(ledger: SessionLedger): number {
  return ledger.spentLamports + ledger.reserved.reduce((s, r) => s + r.lamports, 0)
}

export function remaining(ledger: SessionLedger, caps: SessionCaps): number {
  return Math.max(0, caps.budgetLamports - committed(ledger))
}

export type RefusalReason =
  | 'expired'
  | 'budget-exhausted'
  | 'over-per-trade-cap'
  | 'cooling-down'
  | 'not-armed'
  | 'zero-amount'

export type Authorisation =
  | { ok: true; ledger: SessionLedger; reservation: Reservation }
  | { ok: false; refusal: RefusalReason; detail: string }

/**
 * The whole critical section. **No `await` may ever appear in this function.**
 *
 * JavaScript is single-threaded, so a synchronous read-decide-write cannot interleave.
 * The moment an `await` is introduced the event loop can run another entry between the
 * check and the reservation, and both will measure themselves against money the other has
 * already taken — with the ledger afterwards reading exactly at the cap, so nothing
 * downstream ever sees a figure that looks wrong.
 *
 * A source scan in `check:zero` asserts the absence, because this is a one-word
 * regression that no behavioural test on a single call can catch.
 */
export function authorise(
  ledger: SessionLedger,
  caps: SessionCaps,
  lamports: number,
  nowUnix: number,
  armed: boolean,
  idSeed: string,
): Authorisation {
  if (!armed) {
    return { ok: false, refusal: 'not-armed', detail: 'session key is not armed' }
  }
  if (nowUnix >= caps.expiresAtUnix) {
    return {
      ok: false,
      refusal: 'expired',
      detail: `session expired at ${caps.expiresAtUnix}; re-arm to continue`,
    }
  }
  if (!(lamports > 0)) {
    return { ok: false, refusal: 'zero-amount', detail: 'nothing to authorise' }
  }
  if (lamports > caps.maxPerTradeLamports) {
    return {
      ok: false,
      refusal: 'over-per-trade-cap',
      detail: `${lamports} exceeds the per-trade cap ${caps.maxPerTradeLamports}`,
    }
  }
  if (
    ledger.lastSpendUnix !== null &&
    nowUnix - ledger.lastSpendUnix < caps.cooldownSecs
  ) {
    const left = caps.cooldownSecs - (nowUnix - ledger.lastSpendUnix)
    return { ok: false, refusal: 'cooling-down', detail: `${Math.ceil(left)}s of cooldown remaining` }
  }
  if (committed(ledger) + lamports > caps.budgetLamports) {
    return {
      ok: false,
      refusal: 'budget-exhausted',
      detail: `${lamports} would exceed the session budget (${committed(ledger)}/${caps.budgetLamports} committed)`,
    }
  }

  const reservation: Reservation = { id: `${idSeed}-${ledger.version}`, lamports, atUnix: nowUnix }
  return {
    ok: true,
    reservation,
    ledger: {
      ...ledger,
      version: ledger.version + 1,
      reserved: [...ledger.reserved, reservation],
      lastSpendUnix: nowUnix,
    },
  }
}

/**
 * The spend landed. Discharge the reservation AS the charge is applied.
 *
 * Both halves, or one payment is committed twice and the symptom is an allowance
 * shrinking on its own.
 */
export function settle(ledger: SessionLedger, id: string, actualLamports: number): SessionLedger {
  const r = ledger.reserved.find(x => x.id === id)
  if (!r) return ledger // already settled; settling twice must be a no-op
  return {
    ...ledger,
    version: ledger.version + 1,
    spentLamports: ledger.spentLamports + actualLamports,
    reserved: ledger.reserved.filter(x => x.id !== id),
  }
}

/**
 * The spend demonstrably did not happen. Only then is the hold released.
 *
 * This is applied as a DELTA, never by writing back a snapshot the decision was made
 * against — restoring a pre-spend ledger erases every reservation taken while this one
 * was in flight, and those may already have been paid.
 */
export function release(ledger: SessionLedger, id: string): SessionLedger {
  if (!ledger.reserved.some(x => x.id === id)) return ledger
  return {
    ...ledger,
    version: ledger.version + 1,
    reserved: ledger.reserved.filter(x => x.id !== id),
  }
}

/**
 * The outcome could not be observed. The hold STAYS.
 *
 * Charging for a spend that may not have happened lets a flaky endpoint drain an
 * allowance; releasing one that may have happened lets a retry pay twice. Between those
 * two, holding is the only choice that cannot lose money — the cost is that an unresolved
 * spend occupies budget forever, which is why it is surfaced rather than swallowed.
 */
export function strand(ledger: SessionLedger, id: string): SessionLedger {
  if (!ledger.reserved.some(x => x.id === id)) return ledger
  return {
    ...ledger,
    version: ledger.version + 1,
    strandedIds: [...ledger.strandedIds, id],
  }
}

// ── what the operator sees ───────────────────────────────────────────────────

export interface SessionReadout {
  armed: boolean
  /** Absent when no key exists — never 0, which would read as a funded, empty key. */
  balanceLamports: Term
  committedLamports: number
  remainingLamports: number
  budgetLamports: number
  secsUntilExpiry: Term
  strandedCount: number
  /** The sentence that must be on screen whenever the key is armed. */
  warning: string
}

export const SESSION_WARNING =
  'This key lives in your browser. Treat its balance as the maximum you can lose.'

export function readout(
  ledger: SessionLedger,
  caps: SessionCaps,
  armed: boolean,
  balanceLamports: number | null,
  nowUnix: number,
): SessionReadout {
  return {
    armed,
    balanceLamports:
      balanceLamports === null ? absent('balance not read') : measured(balanceLamports),
    committedLamports: committed(ledger),
    remainingLamports: remaining(ledger, caps),
    budgetLamports: caps.budgetLamports,
    secsUntilExpiry: armed
      ? measured(Math.max(0, caps.expiresAtUnix - nowUnix))
      : absent('not armed'),
    strandedCount: ledger.strandedIds.length,
    warning: SESSION_WARNING,
  }
}

/** Sensible defaults: small, short, and obviously adjustable. */
export const LAMPORTS_PER_SOL = 1_000_000_000

export function defaultCaps(nowUnix: number): SessionCaps {
  return {
    maxBalanceLamports: LAMPORTS_PER_SOL / 2, // 0.5 SOL
    maxPerTradeLamports: LAMPORTS_PER_SOL / 20, // 0.05 SOL
    budgetLamports: LAMPORTS_PER_SOL / 4, // 0.25 SOL
    expiresAtUnix: nowUnix + 60 * 60, // one hour
    cooldownSecs: 20,
  }
}
