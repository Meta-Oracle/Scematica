/**
 * The jump drive.
 *
 * ## Why a game with a fast ship still needs one
 *
 * The main drive crosses the sector in eleven seconds and that is not the problem a jump drive
 * solves. The interesting decision here is *which* of a thousand nodes to be at — a rift out on
 * the frontier, the one market that stocks what you are short of — and any travel time long
 * enough to be felt turns that decision into a chore. Making the ship faster still would flatten
 * the space instead, which is the trade this project has already made in the wrong direction
 * once.
 *
 * So: near-instant travel to a waypoint, and every cost loaded onto the *decision* rather than
 * the duration.
 *
 * ## The three costs, and what each one buys
 *
 * **A separate fuel.** Jump charges are counted in single digits and refill only at a dock,
 * where main fuel refills at any of six times as many depots. That makes a route a thing you
 * plan rather than a thing you improvise, and it makes a dock somewhere worth remembering.
 *
 * **A spin-up.** Two and a half seconds during which you are flying straight and slow. Without
 * it a jump is a button that deletes every consequence in the game.
 *
 * **An inhibitor.** The drive will not charge with a hostile inside `JUMP_INHIBIT`, and an
 * interruption spends nothing. This is the load-bearing one: it is what stops the jump being an
 * escape hatch, and therefore what makes choosing to commit to a fight mean anything. Running is
 * still possible — that is the main drive's job, and `classes.ts` guarantees no craft can catch
 * you — but running is a manoeuvre, not a keystroke.
 *
 * Everything here is pure. The charge is advanced by a caller-supplied `dt` and compared against
 * a caller-supplied clock, so a jump is reproducible and testable with no frame loop.
 */

import type { Node, Vec3 } from './generate.ts'
import { JUMP_CHARGE_MS, JUMP_INHIBIT, JUMP_STANDOFF } from './scale.ts'
import { jumpCharge } from './ship.ts'

export type Phase = 'idle' | 'charging' | 'inhibited'

export interface Drive {
  phase: Phase
  /** Node the drive is spun up for. Changing waypoint mid-charge aborts it. */
  target: number | null
  /** Milliseconds charged so far. */
  charged: number
}

export const IDLE: Drive = { phase: 'idle', target: null, charged: 0 }

export interface Situation {
  /** Range to the nearest live hostile, or null when nothing is on sensors. */
  threat: number | null
  /** Jump charges remaining. */
  charges: number
  /** Drive component level, which shortens the spin-up. */
  driveLevel: number
  /** The selected waypoint, or null. */
  waypoint: number | null
}

/** Why a jump cannot be started. `null` when it can. */
export function refusal(s: Situation): string | null {
  if (s.waypoint === null) return 'no waypoint — pick one with 1-4 first'
  if (s.charges <= 0) return 'no jump charges — a dock will refill them'
  if (s.threat !== null && s.threat < JUMP_INHIBIT) return 'jump inhibited — hostiles in range'
  return null
}

export interface Advance {
  drive: Drive
  /** Set on the frame the jump completes: where the ship arrives. */
  arriveAt: Vec3 | null
  /** True on the frame a charge is spent. */
  spent: boolean
  notice: string | null
}

/**
 * Advance the drive by `dt` seconds.
 *
 * `holding` is whether the jump key is down. Releasing aborts and refunds, because a charge
 * consumed by a keystroke the player took back is the kind of loss that teaches somebody not
 * to touch the mechanic again.
 */
export function advance(
  drive: Drive,
  s: Situation,
  target: Node | null,
  holding: boolean,
  dt: number,
): Advance {
  const idle = { drive: IDLE, arriveAt: null, spent: false, notice: null }

  if (!holding || !target) {
    // Aborting mid-charge is worth saying out loud: the spin-up is long enough that a player
    // who let go early will otherwise think the drive is broken.
    const notice = drive.phase === 'charging' ? 'jump aborted' : null
    return { ...idle, notice }
  }

  const why = refusal({ ...s, waypoint: target.id })
  if (why) {
    // An inhibited drive is a *distinct* phase rather than an idle one, so the HUD can say why
    // nothing is happening. "Nothing happened" and "the drive refused" are different facts and
    // only one of them tells the player what to do about it.
    return { drive: { phase: 'inhibited', target: target.id, charged: 0 }, arriveAt: null, spent: false, notice: why }
  }

  // Retargeting mid-charge starts over. A drive that kept its progress across a new destination
  // would let a player charge somewhere safe and arrive somewhere else.
  const charged = drive.target === target.id ? drive.charged + dt * 1000 : dt * 1000
  const need = jumpCharge(s.driveLevel, JUMP_CHARGE_MS)

  if (charged < need) {
    return {
      drive: { phase: 'charging', target: target.id, charged },
      arriveAt: null,
      spent: false,
      notice: null,
    }
  }

  // Arrive offset from the node rather than inside it, and offset along a fixed axis so two
  // players jumping to the same node from anywhere end up in the same place — the determinism
  // rule, in the one mechanic that could quietly break it.
  return {
    drive: IDLE,
    arriveAt: {
      x: target.at.x + JUMP_STANDOFF,
      y: target.at.y,
      z: target.at.z + JUMP_STANDOFF,
    },
    spent: true,
    notice: `jumped to ${target.label}`,
  }
}

/** Charge as a 0..1 fraction, for the HUD bar. */
export function progress(drive: Drive, driveLevel: number): number {
  if (drive.phase !== 'charging') return 0
  return Math.max(0, Math.min(1, drive.charged / jumpCharge(driveLevel, JUMP_CHARGE_MS)))
}
