/**
 * Who else is out here.
 *
 * A sector used to contain the player, the record's signals, and raiders. Everything that moved
 * was trying to kill you, which makes a volume this size read as a shooting range with long gaps
 * — the emptiness between fights is not *space*, it is waiting. Traffic is what turns it into a
 * place: ships going somewhere for their own reasons, most of which have nothing to do with you.
 *
 * ## The four factions, and what each one is for
 *
 * - **Raider** (orange). Hostile. Hunts the player. Already existed.
 * - **Courier** (neon blue). Fast, unarmed, runs between markets. The commonest thing in the
 *   sector and the reason the markets feel like they are *for* something.
 * - **Freighter** (blue). Slow, heavy, runs between depots. Visible from a long way off.
 * - **Marshal** (yellow). Anti-raider patrol. Hunts raiders and ignores the player entirely.
 *
 * ## Marshals fight raiders whether or not you are watching
 *
 * This is the part that makes the sector feel inhabited rather than staged. A marshal picks the
 * nearest raider and goes for it; a raider inside a marshal's range fights back. Arrive at a
 * fight already in progress and you may pick a side, or leave — and the outcome differs depending
 * on whether you were there, which is the difference between a world and a backdrop.
 *
 * ## None of this comes from the record
 *
 * Same rule as `raiders.ts`, and worth restating because this is where it would be easiest to
 * break: traffic density is a **constant**. Deriving it from the record's markets or signals would
 * make a record's contents worth misreporting — more markets, more couriers, more of something.
 * Civilians carry `unlogged: true` for the same reason raiders do: they are furniture the game
 * placed, and furniture must never be mistakable for a signal somebody counted.
 *
 * Routes run between real service nodes, which is a *use* of the record's contents rather than a
 * reward derived from them. A sector with more depots has freighters flying between more places;
 * it does not have more freighters.
 */

import type { Node, Space, Vec3 } from './generate.ts'
import { servicesOf } from './generate.ts'
import { Rng } from '../omni/fractal.ts'
import { EXTENT } from './scale.ts'
import { CLASSES, type ClassSpec } from './classes.ts'

export type Faction = 'raider' | 'courier' | 'freighter' | 'marshal'

/** True for factions that will shoot at the player. */
export function hostileTo(f: Faction): boolean {
  return f === 'raider'
}

/** True for factions that fly routes rather than fight. */
export function civilian(f: Faction): boolean {
  return f === 'courier' || f === 'freighter'
}

/**
 * How many of each. Constants, not rates — see the module note.
 *
 * Couriers outnumber everything because they are the cheapest way to make the sector look busy:
 * small, fast, and usually crossing your path rather than sitting in it.
 */
const COUNTS: Record<Exclude<Faction, 'raider'>, number> = {
  courier: 34,
  freighter: 14,
  marshal: 18,
}

/** A ship the sector placed, with somewhere it is going. */
export interface Civilian {
  id: string
  faction: Faction
  spec: ClassSpec
  at: Vec3
  /** Node id it is heading for. Civilians route; marshals hunt and leave this null. */
  destination: number | null
}

/** Service nodes a given faction routes between. */
export function routeNodes(space: Space, faction: Faction): Node[] {
  if (faction === 'courier') {
    return space.nodes.filter((n) => servicesOf(n.kind).includes('trade'))
  }
  if (faction === 'freighter') {
    return space.nodes.filter((n) => servicesOf(n.kind).includes('refuel'))
  }
  return []
}

/**
 * Populate the sector with everything that is not a raider and not the player.
 *
 * Deterministic in the seed, so two players holding the same record meet the same traffic — the
 * property the whole game rests on, applied to the part of it that is easiest to treat as
 * decoration and therefore easiest to make random by accident.
 */
export function trafficOf(space: Space, seed: string): Civilian[] {
  // A third slice of the digest, so traffic shares a stream with neither the fractal nor the
  // raiders. `Rng` reads only the first eight hex characters, so a suffix would not do it.
  const rng = new Rng(seed.slice(16, 24) || seed.slice(8, 16) || seed)
  const out: Civilian[] = []

  const specOf: Record<Exclude<Faction, 'raider'>, ClassSpec> = {
    courier: CLASSES.courier,
    freighter: CLASSES.freighter,
    marshal: CLASSES.marshal,
  }

  for (const faction of ['courier', 'freighter', 'marshal'] as const) {
    const route = routeNodes(space, faction)
    for (let i = 0; i < COUNTS[faction]; i += 1) {
      // Civilians start *on* their route, which is both the cheaper placement and the more
      // legible one: traffic that begins at a station and heads for another reads as traffic,
      // where traffic scattered at random reads as debris that happens to be moving.
      const start = route.length > 0 ? route[rng.below(route.length)] : null
      const at = start
        ? {
            // Offset off the node, so a station does not appear to have ships embedded in it.
            x: start.at.x + rng.below(EXTENT / 40) - EXTENT / 80,
            y: start.at.y + rng.below(EXTENT / 40) - EXTENT / 80,
            z: start.at.z + rng.below(EXTENT / 40) - EXTENT / 80,
          }
        : {
            x: rng.below(EXTENT * 2) - EXTENT,
            y: Math.trunc((rng.below(EXTENT * 2) - EXTENT) * 0.7),
            z: rng.below(EXTENT * 2) - EXTENT,
          }

      out.push({
        id: `${faction}:${i}`,
        faction,
        spec: specOf[faction],
        at,
        destination: route.length > 1 ? route[rng.below(route.length)].id : null,
      })
    }
  }
  return out
}

/**
 * The next destination after arriving at one.
 *
 * Deterministic in the ship's own id and the node it just left, so a courier's whole itinerary is
 * fixed by the record. A random pick here would make two players' sectors diverge the moment
 * anything docked, which is the same failure the placement rules exist to prevent — just delayed
 * by a minute.
 */
export function nextStop(route: Node[], id: string, from: number): number | null {
  if (route.length === 0) return null
  if (route.length === 1) return route[0].id
  let h = 2166136261 >>> 0
  for (const t of [id, ':', String(from)]) {
    for (let i = 0; i < t.length; i += 1) {
      h ^= t.charCodeAt(i)
      h = Math.imul(h, 16777619) >>> 0
    }
  }
  const pick = route[h % route.length]
  // Never the node just left, or a ship parks and shuttles between two points a metre apart.
  return pick.id === from ? route[(h + 1) % route.length].id : pick.id
}
