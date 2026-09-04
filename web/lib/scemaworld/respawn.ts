/**
 * Keeping the sector populated.
 *
 * ## The problem
 *
 * A sector was a resource that depleted. Eighteen raider wings and eighteen marshals were placed
 * at generation and that was every hostile the world would ever contain, so an hour of play left
 * a volume with almost nothing in it — and the emptiness arrived *unevenly*, concentrated exactly
 * where the player had spent time, which reads as the game running out rather than as a place.
 *
 * The marshals made it worse rather than better. They hunt raiders whether or not anybody is
 * watching, which is the mechanic that makes the sector feel inhabited — and with a fixed
 * population it is also a mechanic that runs to completion. Left alone for long enough the patrol
 * wins, and the ambient violence that the whole faction exists to produce stops happening. The
 * two sides have to be *replenished* for a standing fight to be a standing fight.
 *
 * ## Three rules
 *
 * 1. **Reinforcements arrive far away, and on a timer.** A ship appearing inside sensor range is
 *    the single clearest way to tell a player that nothing they are looking at is real, and an
 *    instant refill means the sector cannot be thinned at all — clearing the space around you has
 *    to be worth something for a while.
 * 2. **Capitals are never replaced.** A leviathan you spent four minutes killing stays killed, on
 *    both sides. Respawning one turns the largest thing available to do in the sector into a chore
 *    with a cooldown, and takes away the only lasting mark the player can leave on the place.
 * 3. **Nothing here reads the record.** Same rule as `raiders.ts` and `factions.ts`, in the place
 *    it would be easiest to break: tie the floor or the interval to `blind_spots` or to the node
 *    count and a producer has bought itself a quieter sector by misreporting. Floors and intervals
 *    are constants; only the seed decides *where* a wave appears. `check:scemaworld` asserts this
 *    file reads no record field.
 *
 * ## What determinism survives, stated precisely
 *
 * The sector's opening population is a pure function of the record and stays that way. Whether a
 * *reinforcement* happens at all cannot be: it depends on who died, which depends on how the
 * player flew. What is preserved is the sequence — wave `n` of a given record always has the same
 * composition and the same anchor — so two players holding one record still fly the same sector
 * made of the same things, and diverge only in when a wave shows up. That is the most that is
 * available, and pretending otherwise would be the more comfortable lie.
 */

import type { Space, Vec3 } from './generate.ts'
import * as Enemy from './enemy.ts'
import type { Swarm } from './enemy.ts'
import { marshalReinforcement, MARSHAL_STRENGTH } from './factions.ts'
import { raiderWing, RAIDER_FLOOR, WINGS } from './raiders.ts'
import { AGGRO_RANGE, SENSOR_MULTIPLIER } from './scale.ts'

/**
 * How far from the player a reinforcement is placed.
 *
 * Beyond sensor range, not merely beyond engagement range. Arriving just outside the aggro radius
 * would still put a ship on the sensor board out of nothing, and the board is the one surface the
 * player is trained to believe.
 */
export const SPAWN_CLEARANCE = Math.round(AGGRO_RANGE * SENSOR_MULTIPLIER * 1.6)

/** Milliseconds between raider wings. Long enough that clearing a region is worth doing. */
export const RAIDER_INTERVAL_MS = 22_000

/** Milliseconds between marshal replacements. Shorter: they arrive singly, not four at a time. */
export const MARSHAL_INTERVAL_MS = 13_000

/**
 * How many waves of each have been raised, and when the next may be.
 *
 * `raiders` starts at `WINGS` because the sector opens with wings `0 .. WINGS-1` already placed —
 * the counter is the *next* index, so a respawned wing continues the same deterministic sequence
 * rather than re-raising one that already flew.
 */
export interface Waves {
  raiders: number
  marshals: number
  nextRaiderMs: number
  nextMarshalMs: number
}

export function newWaves(): Waves {
  return { raiders: WINGS, marshals: 0, nextRaiderMs: -1e9, nextMarshalMs: -1e9 }
}

/** Live craft of a faction, counting only a given class — capitals are excluded from a floor. */
function countOf(swarm: Swarm, faction: 'raider' | 'marshal', capitals: boolean): number {
  return swarm.craft.filter(
    (c) => c.alive && c.faction === faction && c.spec.capital === capitals,
  ).length
}

export interface Replenished {
  swarm: Swarm
  waves: Waves
  /** A line for the HUD when something arrived, so a wave is announced rather than merely true. */
  notice: string | null
}

/**
 * Top the sector back up, at most one wave of each per call.
 *
 * Cheap enough to run every tick: two linear passes over a swarm of a couple of hundred entries,
 * and both are skipped entirely by the timer check on all but a handful of frames.
 */
export function replenish(
  swarm: Swarm,
  space: Space,
  seed: string,
  waves: Waves,
  playerAt: Vec3,
  nowMs: number,
): Replenished {
  let out = swarm
  let next = waves
  let notice: string | null = null

  // Fighters only. A dead capital is not a shortfall — see rule 2.
  if (nowMs >= waves.nextRaiderMs && countOf(swarm, 'raider', false) < RAIDER_FLOOR) {
    const wing = raiderWing(seed, next.raiders, playerAt, SPAWN_CLEARANCE)
    out = Enemy.reinforce(out, Enemy.swarmOf(wing, seed).craft)
    next = { ...next, raiders: next.raiders + 1, nextRaiderMs: nowMs + RAIDER_INTERVAL_MS }
    notice = 'raider wing on long-range sensors'
  }

  if (nowMs >= waves.nextMarshalMs && countOf(swarm, 'marshal', false) < MARSHAL_STRENGTH) {
    const civ = marshalReinforcement(space, seed, next.marshals, playerAt, SPAWN_CLEARANCE)
    out = Enemy.reinforce(out, Enemy.withTraffic({ craft: [], shots: [] }, [civ]).craft)
    next = { ...next, marshals: next.marshals + 1, nextMarshalMs: nowMs + MARSHAL_INTERVAL_MS }
    // The raider line wins the notice if both fired this frame. A wave of four hostiles is the
    // one the player has to act on, and two notices in one frame means only the last is read.
    notice = notice ?? 'marshal patrol reinforced'
  }

  return { swarm: out, waves: next, notice }
}
