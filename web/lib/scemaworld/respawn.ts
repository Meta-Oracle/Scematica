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
import {
  civilianReinforcement, marshalReinforcement, strengthOf,
  MARSHAL_STRENGTH, TRAFFIC,
} from './factions.ts'
import { raiderWing, RAIDER_FLOOR, RAIDER_STRENGTH, WINGS } from './raiders.ts'
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

/**
 * How far apart, in milliseconds, the ships of one wing finish their entry.
 *
 * A wing used to share a single `dueMs`, so three hulls appeared on the same frame — which does
 * not read as a formation arriving, it reads as the sector gaining three ships at once. Staggering
 * it makes the entry an *event with a duration*: the streaks resolve one after another and the eye
 * gets to follow them.
 *
 * Small enough that the wing is unmistakably one wing. A longer stagger and it becomes three
 * separate arrivals that happen to share a bearing, which is a different and less useful reading.
 */
const WARP_STAGGER_MS = 220

/**
 * Milliseconds between raider wings when the sector is **contested** — below `RAIDER_FLOOR`.
 *
 * A gutted sector coming back at the cruising rate takes a quarter of an hour, which nobody
 * waits through: the measured recovery from a full purge reached the old target in nine minutes
 * and would have needed fifteen to reach a full complement. Surging below the floor makes the
 * deficit close at a pace somebody actually sees, while the ordinary trickle above it keeps a
 * cleared region cleared for long enough to be worth having cleared.
 */
export const RAIDER_SURGE_MS = 8_000

/** Milliseconds between marshal replacements. Shorter: they arrive singly, not four at a time. */
export const MARSHAL_INTERVAL_MS = 13_000

/**
 * Milliseconds between civilian replacements.
 *
 * Traffic is the largest population in the sector and the one with no defence, so it needs the
 * shortest interval of the three or it never keeps up with what the raiders take. It arrives
 * singly: a wing of couriers is not a thing.
 */
export const TRAFFIC_INTERVAL_MS = 7_000

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
  /**
   * Civilian waves raised, per `faction:class`.
   *
   * A record rather than a field per faction, because the roster is data and a shape that has to
   * grow a field every time somebody adds a class is a shape that stops being updated.
   */
  civilians: Record<string, number>
  nextRaiderMs: number
  nextMarshalMs: number
  nextTrafficMs: number
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
  return {
    raiders: WINGS,
    marshals: 0,
    civilians: {},
    nextRaiderMs: -1e9,
    nextMarshalMs: -1e9,
    nextTrafficMs: -1e9,
    arriving: [],
  }
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

/** The wave-counter key for one roster line. */
function keyOf(faction: string, klass: string): string {
  return `${faction}:${klass}`
}

/** Which class a civilian faction flies, or null if it is not one the sector tops up. */
function classOfTraffic(faction: string): (typeof TRAFFIC)[number]['klass'] | null {
  return TRAFFIC.find((t) => t.faction === faction)?.klass ?? null
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
  let nextTrafficMs = waves.nextTrafficMs ?? -1e9
  let civilians = waves.civilians ?? {}

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
      } else if (a.faction === 'marshal') {
        const civ = marshalReinforcement(space, seed, marshals, a.at, 0)
        out = Enemy.reinforce(
          out,
          Enemy.withTraffic({ craft: [], shots: [] }, [{ ...civ, id: a.id, at: a.at }]).craft,
        )
      } else {
        // Traffic. It arrives with a destination, so a courier that drops out of hyperspace next
        // to you immediately sets off for somewhere — which is the difference between the sector
        // gaining a ship and the sector gaining a delivery.
        const klass = classOfTraffic(a.faction)
        if (klass) {
          const civ = civilianReinforcement(
            space, seed, a.faction, klass, civilians[keyOf(a.faction, klass)] ?? 0, a.at, 0,
          )
          out = Enemy.reinforce(
            out,
            Enemy.withTraffic({ craft: [], shots: [] }, [{ ...civ, id: a.id, at: a.at }]).craft,
          )
        }
      }
    }
  }

  // ── open a new entry when the sector is short ──────────────────────────────
  // Counted against what is already **on its way** as well as what is flying, or a wing is ordered
  // several times over while the first of it is still materialising.
  const pending = (f: string) => arriving.filter((a) => a.faction === f).length

  // Fighters only. A dead capital is not a shortfall — see rule 2.
  //
  // Against `RAIDER_STRENGTH`, the **full** complement, not `RAIDER_FLOOR`. The floor used to be
  // both the trigger and the target, so a cleared sector came back to 60% of what it started with
  // and stayed there: measured at 43 of 72, permanently, with nothing saying so. The floor now
  // decides the *pace* instead — below it the sector is contested and reinforcement surges.
  const shortRaiders = countOf(swarm, 'raider', false) + pending('raider')
  if (nowMs >= nextRaiderMs && shortRaiders < RAIDER_STRENGTH) {
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
        dueMs: nowMs + ARRIVAL_MS + i * WARP_STAGGER_MS,
      })
    }
    arriving = [...arriving, ...wing]
    raiders += 1
    nextRaiderMs = nowMs + (shortRaiders < RAIDER_FLOOR ? RAIDER_SURGE_MS : RAIDER_INTERVAL_MS)
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

  // ── traffic ────────────────────────────────────────────────────────────────
  //
  // The population that had no replenishment path at all. Raiders hunt couriers and freighters,
  // so a sector left running lost all of both — 34 and 14 down to zero, permanently — and the two
  // factions that make the place look inhabited quietly stopped existing. Nothing anywhere said
  // so, because nothing was measuring it: the sector still had ships in it, they were just all
  // shooting at each other.
  //
  // **The faction furthest below its roster strength goes first**, rather than a fixed order.
  // Round-robin would spend a slot on a faction that is one short while another is wiped out, and
  // the wiped-out one is the one you can see is missing.
  if (nowMs >= nextTrafficMs) {
    let worst: { faction: string; klass: string; deficit: number } | null = null
    for (const t of TRAFFIC) {
      const want = strengthOf(t.faction, t.klass)
      const have = swarm.craft.filter(
        (c) => c.alive && c.faction === t.faction && !c.spec.capital,
      ).length
      const deficit = want - (have + pending(t.faction))
      if (deficit > 0 && (!worst || deficit > worst.deficit)) {
        worst = { faction: t.faction, klass: t.klass, deficit }
      }
    }
    if (worst) {
      const k = keyOf(worst.faction, worst.klass)
      const n = civilians[k] ?? 0
      arriving = [
        ...arriving,
        {
          id: `${worst.faction}:warp:${n}`,
          faction: worst.faction as Arrival['faction'],
          at: arrivalPoint(playerAt, playerFacing, bearing(seed, `:${k}-spread:`, n), 0.95),
          dir: bearing(seed, `:${k}-entry:`, n),
          dueMs: nowMs + ARRIVAL_MS,
        },
      ]
      civilians = { ...civilians, [k]: n + 1 }
      nextTrafficMs = nowMs + TRAFFIC_INTERVAL_MS
      // Never the headline. A hostile wing and a patrol are both things the player has to decide
      // about; a courier arriving is the sector working, and a notice for it would push the two
      // that matter off the screen.
      notice = notice ?? null
    }
  }

  return {
    swarm: out,
    waves: {
      raiders,
      marshals,
      civilians,
      nextRaiderMs,
      nextMarshalMs,
      nextTrafficMs,
      arriving,
    },
    notice,
  }
}
