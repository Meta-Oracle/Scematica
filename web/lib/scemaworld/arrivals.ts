/**
 * Hyperspace arrivals: reinforcements that come *in*, where you can watch them do it.
 *
 * ## Why this replaces "spawn beyond sensor range"
 *
 * The first version of reinforcement placed a wave outside sensor range, and the stated reason was
 * a good one: a ship appearing inside it is the clearest possible statement that nothing on screen
 * is real. But it made the entire mechanic invisible. The sector's population was maintained by
 * bookkeeping — craft blinked into a volume nobody was looking at, then took a minute to fly
 * somewhere, and the player's only evidence that the sector was alive was that it had not gone
 * quiet. A mechanic you can only detect by its absence is not a mechanic.
 *
 * A hyperspace entry dissolves the objection rather than trading against it. The problem with a
 * ship materialising in front of you is not the *position*, it is the **absence of a cause**: it
 * asserts that a thing was always there when it was not. A warp-in supplies the cause. It is the
 * same fiction the player's own jump drive already runs on (`hyper.ts`), which means the sector is
 * not being given a power the player lacks — it is being shown using the one they have.
 *
 * So arrivals now happen **inside sensor range and in front of the player where possible**, and
 * they are loud: a bright streak along the entry vector that collapses into the craft. That is the
 * point. A reinforcement you did not see arrive may as well have been there all along.
 *
 * ## What is still not allowed
 *
 * - **Nothing here reads the record.** Same rule as `raiders.ts`, `factions.ts` and `respawn.ts`,
 *   and this is a place it would be easy to break: scale the arrival rate by node count and a
 *   record has bought itself a quieter sector. Timings and distances are constants; only the seed
 *   decides the direction a wave comes in from.
 * - **An arrival is never a contact.** It is furniture the game placed, exactly like the raider it
 *   becomes, and it is drawn from `space.raiders`-style provenance rather than being mixed into
 *   anything the record reported.
 * - **No arrival lands on top of the player.** `MIN_ARRIVAL` is a floor, not a suggestion: warping
 *   a gunship into the player's hull would be a collision the game charges them for.
 */

import type { Vec3 } from './generate.ts'
import type { Faction } from './factions.ts'
import { AGGRO_RANGE, SENSOR_MULTIPLIER } from './scale.ts'

/**
 * How long the entry streak is on screen before the craft exists.
 *
 * Long enough to be *seen and reacted to* — a warp-in that resolves in a couple of frames is a
 * pop-in with extra steps. Short enough that a wing is not a light show.
 */
export const ARRIVAL_MS = 1_400

/**
 * Where an arrival appears, as a fraction of engagement range.
 *
 * Inside the player's sensor envelope on purpose — the whole point is that it is witnessed — but
 * comfortably outside knife range, so it announces a fight rather than starting one.
 * In practice that means beyond every rolled fighter's aggro, while still inside stock sensors.
 */
export const MIN_ARRIVAL = Math.round(AGGRO_RANGE * 1.8)
export const MAX_ARRIVAL = Math.round(AGGRO_RANGE * (SENSOR_MULTIPLIER - 0.35))

/**
 * A craft on its way in. Drawn, but not yet in the swarm.
 *
 * It cannot be shot, cannot shoot, and cannot be collided with — it is not there yet. That is the
 * honest reading of a thing that has not arrived, and it also removes the unpleasant case where a
 * player kills a reinforcement before it finishes materialising.
 */
export interface Arrival {
  id: string
  faction: Faction
  /** Where the craft will be when it finishes. */
  at: Vec3
  /** Unit vector the streak runs along — the direction it came *from*, so it reads as decelerating. */
  dir: Vec3
  /** Milliseconds at which it becomes a craft. */
  dueMs: number
}

/**
 * How far along its entry an arrival is, 0..1.
 *
 * Used only for the streak's brightness and length. Kept here rather than in the renderer so the
 * curve is testable and so `view.ts` keeps its rule of placing geometry rather than deciding
 * anything.
 */
export function progress(a: Arrival, nowMs: number): number {
  const t = 1 - (a.dueMs - nowMs) / ARRIVAL_MS
  return Math.max(0, Math.min(1, t))
}

/** True once the craft should exist. */
export function landed(a: Arrival, nowMs: number): boolean {
  return nowMs >= a.dueMs
}

/**
 * Pick an arrival point near the player.
 *
 * Biased **ahead** of them, because an arrival behind the camera is one they will never see and
 * the entire reason this exists is to be seen. Not exactly ahead: a wave that always warps in dead
 * centre reads as scripted, so the direction is spread around the nose by the seed-derived vector
 * the caller supplies.
 *
 * The `jitter` argument is a unit-ish vector from a deterministic source (`raiders.ts` and
 * `factions.ts` both have one). This function does no rolling of its own — it must stay pure and
 * reproducible, so two players holding one record see a wave enter from the same bearing.
 */
export function arrivalPoint(
  playerAt: Vec3,
  playerFacing: Vec3,
  jitter: Vec3,
  spread: number,
): Vec3 {
  // Ahead, plus a lateral spread. Weighting the nose at 1 and the jitter at `spread` keeps the
  // arrival inside a cone rather than anywhere on a sphere.
  const dx = playerFacing.x + jitter.x * spread
  const dy = playerFacing.y + jitter.y * spread
  const dz = playerFacing.z + jitter.z * spread
  const l = Math.hypot(dx, dy, dz) || 1
  const range = MIN_ARRIVAL + Math.abs(jitter.x + jitter.y + jitter.z) * (MAX_ARRIVAL - MIN_ARRIVAL)
  // Integer coordinates can shorten the radial distance by less than a unit when each component
  // rounds independently. Keep a tiny cushion so `MIN_ARRIVAL` remains a floor after projection.
  const d = Math.min(MAX_ARRIVAL, Math.max(MIN_ARRIVAL + 2, range))
  return {
    x: Math.round(playerAt.x + (dx / l) * d),
    y: Math.round(playerAt.y + (dy / l) * d),
    z: Math.round(playerAt.z + (dz / l) * d),
  }
}
