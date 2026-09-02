/**
 * The sector's economy, and a placeholder for $SCEMA.
 *
 * ## Read this before wiring anything real to it
 *
 * The `SCEMA` here is **a name and an exchange rate, and nothing else**. It is not a token, it
 * touches no chain, it has no price feed, and a balance in it is a number in a browser tab that
 * ends when the tab does. Every function below is arithmetic on that number.
 *
 * That is deliberate and it is worth being blunt about, because the moment this is connected to
 * an actual token the game acquires a property it does not currently have: **the things you do in
 * it become worth money outside it.** Two rules this project already holds would then need
 * re-verifying rather than assumed:
 *
 * 1. *No quantity in the record may translate into a reward.* Still true today — salvage comes
 *    from acts (a kill, a derelict stripped), never from record content, and `check:scemaworld`
 *    asserts it on `ship.ts`, `raiders.ts` and `factions.ts`. With a real token behind SCEMA, that
 *    assertion stops being a design preference and becomes the thing standing between a producer
 *    and a financial incentive to misreport a world. It would need to hold under adversarial
 *    reading, not merely under test.
 * 2. *Sealed records are the map.* A record that pays better than another record is a record
 *    worth forging. Today forging one gains nothing, because every world is worth the same and
 *    `raiders.ts` and `factions.ts` read only the seed. That property is load-bearing and would
 *    have to be re-argued, not inherited.
 *
 * Neither is a reason not to build it. Both are reasons the wiring is not something to do quietly
 * as a follow-up.
 *
 * ## Why two currencies
 *
 * Salvage accumulates steadily from fighting and buys components — an incremental decision that
 * suits an incremental resource. SCEMA is converted *from* salvage at a deliberate loss and buys
 * hulls, which are commitments. The spread is what makes changing ship a decision rather than an
 * inevitability, and it is the only place in the game where a number is deliberately unkind.
 */

import { HULLS, type HullId } from './hulls.ts'

/**
 * Salvage per SCEMA.
 *
 * A placeholder, and flagged as one everywhere it is shown. Twelve to one, so a scout is a few
 * dozen kills and a marauder is a campaign.
 */
export const SALVAGE_PER_SCEMA = 12

/**
 * The spread taken on conversion, as a fraction.
 *
 * Deliberately unkind, and it is the only such number in the game. Without it, salvage and SCEMA
 * are the same resource wearing two labels and the choice between a component and a hull is a
 * formality — you would simply buy whichever is cheaper per point of benefit. A loss on the
 * exchange is what makes "spend this on parts now, or bank it toward a hull" an actual question.
 */
export const EXCHANGE_SPREAD = 0.15

/** What `salvage` converts to, after the spread. Never negative, never fractional. */
export function toScema(salvage: number): number {
  if (salvage < SALVAGE_PER_SCEMA) return 0
  return Math.floor((salvage / SALVAGE_PER_SCEMA) * (1 - EXCHANGE_SPREAD))
}

/** How much salvage a given SCEMA amount costs to obtain. The inverse, rounded against you. */
export function salvageFor(scema: number): number {
  return Math.ceil((scema * SALVAGE_PER_SCEMA) / (1 - EXCHANGE_SPREAD))
}

export interface Wallet {
  salvage: number
  scema: number
}

export type Trade = { ok: true; wallet: Wallet; message: string } | { ok: false; message: string }

/** Convert salvage to SCEMA. All of it, or a stated amount. */
export function exchange(w: Wallet, salvage: number = w.salvage): Trade {
  const spend = Math.min(salvage, w.salvage)
  const gained = toScema(spend)
  if (gained <= 0) {
    return {
      ok: false,
      message: `${SALVAGE_PER_SCEMA} salvage buys 1 SCEMA — you have ${w.salvage}`,
    }
  }
  // Only the salvage that actually became SCEMA is spent. Charging the remainder would take
  // salvage and give nothing for it, which is a bug that looks exactly like a design decision.
  const cost = Math.ceil((gained * SALVAGE_PER_SCEMA) / (1 - EXCHANGE_SPREAD))
  const paid = Math.min(cost, w.salvage)
  return {
    ok: true,
    wallet: { salvage: w.salvage - paid, scema: w.scema + gained },
    message: `${paid} salvage → ${gained} SCEMA`,
  }
}

/** Buy a hull. Refuses with the shortfall rather than with a bare "no". */
export function buyHull(w: Wallet, current: HullId, want: HullId): Trade {
  const spec = HULLS[want]
  if (current === want) return { ok: false, message: `already flying a ${spec.label}` }
  if (w.scema < spec.price) {
    return {
      ok: false,
      message: `${spec.label} costs ${spec.price} SCEMA — ${spec.price - w.scema} short`,
    }
  }
  return {
    ok: true,
    wallet: { ...w, scema: w.scema - spec.price },
    message: `${spec.label} acquired`,
  }
}

/**
 * The line the shipyard prints about what SCEMA currently is.
 *
 * On screen, not only in a comment. A player who is told a currency is a placeholder has been
 * told; one who infers it later from a changelog has been misled, and this project's whole
 * argument is about not letting a number imply more than it is.
 */
export const SCEMA_NOTE =
  'SCEMA is a placeholder unit — local to this session, worth nothing outside it, and not ' +
  'connected to any token.'
