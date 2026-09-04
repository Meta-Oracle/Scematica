/**
 * The sector's economy. **SCEMA is a real token now.**
 *
 * ## What this module still is, and what moved
 *
 * Everything below is arithmetic on an in-browser balance: salvage in, SCEMA out, at a spread, and
 * hulls priced in the result. That has not changed. What changed is what the balance *is* — it is
 * redeemable for the $SCEMA token from a fixed treasury, through `claim.ts` (the policy) and
 * `treasury.ts` (the chain). Spending SCEMA on a hull is still a purely local transaction; only a
 * withdrawal touches a chain, and only at a market.
 *
 * ## The warning this file used to carry, and what happened to it
 *
 * It said: the moment this is connected to an actual token the game acquires a property it does
 * not currently have — **the things you do in it become worth money outside it** — and named two
 * rules that would stop being design preferences and become load-bearing. Both were re-verified
 * rather than inherited when the wiring was built, and both hold:
 *
 * 1. *No quantity in the record may translate into a reward.* Salvage comes from acts (a kill, a
 *    derelict stripped), never from record content; raider and traffic density are constants; so
 *    are the reinforcement floors in `respawn.ts`; and the withdrawal rate reads nothing but a
 *    balance. `check:scemaworld` asserts the reward path, `raiders.ts`, `factions.ts`,
 *    `respawn.ts` and `claim.ts` all read no record field. **A world with more blind spots is
 *    worth exactly the same as one with fewer**, which is what stops a forged record paying.
 * 2. *Sealed records are the map.* Every world pays identically, so forging one gains nothing.
 *
 * ## The rule that could NOT be preserved, and is stated instead
 *
 * A balance lives in a browser tab and is trivially forgeable. Nothing here can tell a balance
 * that was earned from one that was typed, and making it unforgeable would mean running the
 * simulation server-side — a different product. So withdrawals are a **capped faucet**: per claim,
 * per wallet, per deployment, behind a cooldown, ledgered before they are attempted. The caps buy
 * bounded loss, not secrecy. `claim.ts` says this at length and `SCEMA_NOTE` says the short form
 * on screen, because a player who is told a currency is real and not told how it is bounded has
 * been told half of something.
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
 * Twelve to one, so a scout is a few dozen kills and a marauder is a campaign. It was described
 * here as a placeholder while SCEMA was one; it is now the **entry price of the redeemable
 * currency**, which makes it the number that decides how much play a token is worth. It has not
 * been changed for that — the caps in `claim.ts` are where the conservatism lives, and moving both
 * would be the same caution applied twice and legible in neither.
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
 * The line the shipyard prints about what SCEMA is.
 *
 * On screen, not only in a comment — and re-exported from `claim.ts` rather than restated, because
 * this sentence is now a claim about money and two copies of it would eventually disagree.
 *
 * It used to read "a placeholder unit — local to this session, worth nothing outside it, and not
 * connected to any token." Every clause of that is false now. A player who is told a currency is a
 * placeholder has been told; one still reading that line after the wiring landed would have been
 * misled by us specifically, which is the failure this project's whole argument is about.
 */
export { SCEMA_NOTE } from './claim.ts'
