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
import { ARRIVAL_MS, arrivalPoint, landed, type Arrival } from './arrivals.ts'
import { AGGRO_RANGE, SENSOR_MULTIPLIER } from './scale.ts'

/**
 * How far a reinforcement is placed when it does **not** warp in.
 *
 * Still the rule for the opening roster and the fallback when there is no player to warp relative
 * to. Beyond sensor range: a ship appearing there out of nothing would put a contact on the sensor
 * board with no cause, and the board is the one surface the player is trained to believe.
 *
 * Reinforcements during play use `arrivals.ts` instead, which supplies the cause. See that module
 * for why a witnessed warp-in dissolves the objection rather than trading against it.
 */
export const SPAWN_CLEARANCE = Math.round(AGGRO_RANGE * SENSOR_MULTIPLIER * 1.6)

/** Milliseconds between raider wings. Long enough that clearing a region is worth doing. */
export const RAIDER_INTERVAL_MS = 22_000

/**
 * How many craft drop out of hyperspace together.
 *
 * Fewer than a generated wing carries, because these arrive *near* the player rather than
 * somewhere in the volume. Four hostiles materialising inside engagement range is an ambush the
 * player had no way to avoid; three announced by a visible entry is an encounter they can decline.
 */
const WARP_WING = 3

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
  /**
   * Craft mid-warp: drawn, not yet in the swarm.
   *
   * Here rather than on the swarm because an arrival is **not a craft**. It cannot be shot, cannot
   * shoot, and cannot be collided with — the honest reading of something that has not arrived, and
   * it removes the unpleasant case of killing a reinforcement before it finishes materialising.
   */
  arriving: Arrival[]
}

export function newWaves(): Waves {
  return { raiders: WINGS, marshals: 0, nextRaiderMs: -1e9, nextMarshalMs: -1e9, arriving: [] }
}

/**
 * A deterministic unit vector for a wave, so an entry bearing is a function of the seed and the
 * wave number rather than of a clock.
 *
 * Hand-rolled rather than reusing `Rng`: this needs three components from one integer, and the
 * generator is a *stream* — taking draws from it here would desynchronise the placement streams
 * that `raiders.ts` and `factions.ts` seek through by index.
 */
function bearing(seed: string, tag: string, n: number): Vec3 {
  let h = 2166136261 >>> 0
  for (const t of [seed, tag, String(n)]) {
    for (let i = 0; i < t.length; i += 1) {
      h ^= t.charCodeAt(i)
      h = Math.imul(h, 16777619) >>> 0
    }
  }
  const x = ((h % 17) - 8) / 8
  const y = (((h >>> 5) % 13) - 6) / 6
  const z = (((h >>> 10) % 19) - 9) / 9
  const l = Math.hypot(x, y, z) || 1
  return { x: x / l, y: y / l, z: z / l }
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
  playerFacing: Vec3,
  nowMs: number,
): Replenished {
  let out = swarm
  let notice: string | null = null
  let arriving = waves.arriving
  let raiders = waves.raiders
  let marshals = waves.marshals
  let nextRaiderMs = waves.nextRaiderMs
  let nextMarshalMs = waves.nextMarshalMs

  // ── anything that finished warping in becomes a craft ──────────────────────
  const due = arriving.filter((a) => landed(a, nowMs))
  if (due.length > 0) {
    arriving = arriving.filter((a) => !landed(a, nowMs))
    for (const a of due) {
      if (a.faction === 'raider') {
        // One craft, placed exactly where its streak ended. `raiderWing` still supplies the class
        // roll and the provenance, so an arrival is the same *kind* of thing as a raider that was
        // there at generation — it only got here differently. A clearance of zero because the
        // position is already decided.
        const one = raiderWing(seed, raiders, a.at, 0)
          .slice(0, 1)
          .map((c) => ({ ...c, id: a.id, at: a.at }))
        out = Enemy.reinforce(out, Enemy.swarmOf(one, seed).craft)
      } else {
        const civ = marshalReinforcement(space, seed, marshals, a.at, 0)
        out = Enemy.reinforce(
          out,
          Enemy.withTraffic({ craft: [], shots: [] }, [{ ...civ, id: a.id, at: a.at }]).craft,
        )
      }
    }
  }

  // ── open a new entry when the sector is short ──────────────────────────────
  // Counted against what is already **on its way** as well as what is flying, or a wing is ordered
  // several times over while the first of it is still materialising.
  const pending = (f: 'raider' | 'marshal') => arriving.filter((a) => a.faction === f).length

  // Fighters only. A dead capital is not a shortfall — see rule 2.
  if (nowMs >= nextRaiderMs && countOf(swarm, 'raider', false) + pending('raider') < RAIDER_FLOOR) {
    const dir = bearing(seed, ':raider-entry:', raiders)
    // A wing arrives as a wing: several entries at once along one bearing, so what the player sees
    // is a formation dropping out of hyperspace rather than a ship appearing.
    const wing: Arrival[] = []
    for (let i = 0; i < WARP_WING; i += 1) {
      const jitter = bearing(seed, ':raider-spread:', raiders * 8 + i)
      wing.push({
        id: `raider:warp:${raiders}:${i}`,
        faction: 'raider',
        at: arrivalPoint(playerAt, playerFacing, jitter, 0.75),
        dir,
        dueMs: nowMs + ARRIVAL_MS,
      })
    }
    arriving = [...arriving, ...wing]
    raiders += 1
    nextRaiderMs = nowMs + RAIDER_INTERVAL_MS
    notice = 'hyperspace signature — raider wing inbound'
  }

  if (
    nowMs >= nextMarshalMs &&
    countOf(swarm, 'marshal', false) + pending('marshal') < MARSHAL_STRENGTH
  ) {
    const jitter = bearing(seed, ':marshal-spread:', marshals)
    arriving = [
      ...arriving,
      {
        id: `marshal:warp:${marshals}`,
        faction: 'marshal',
        at: arrivalPoint(playerAt, playerFacing, jitter, 0.9),
        dir: bearing(seed, ':marshal-entry:', marshals),
        dueMs: nowMs + ARRIVAL_MS,
      },
    ]
    marshals += 1
    nextMarshalMs = nowMs + MARSHAL_INTERVAL_MS
    // The raider line wins the notice if both fired this frame. A wing of hostiles is the one the
    // player has to act on, and two notices in one frame means only the last is read.
    notice = notice ?? 'hyperspace signature — patrol inbound'
  }

  return {
    swarm: out,
    waves: { raiders, marshals, nextRaiderMs, nextMarshalMs, arriving },
    notice,
  }
}
