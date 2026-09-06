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
import { EXTENT, SECTOR_REACH } from './scale.ts'
import { CLASSES, type ClassId, type ClassSpec } from './classes.ts'

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
 * The sector's standing roster of everything that is not a raider and not the player.
 *
 * Counts are constants, not rates — see the module note. Couriers outnumber everything because
 * they are the cheapest way to make the sector look busy: small, fast, and usually crossing your
 * path rather than sitting in it.
 *
 * **A faction may field more than one class**, which is why this is a list rather than a map of
 * faction to count. The marshals now bring war classes of their own (`classes.ts::warden`,
 * `::bastion`) — a patrol of eighteen interceptors against a hostile roster that tops out at a
 * titan was a gesture, and it made every large silhouette in the sector mean the same thing. With
 * a warden and a bastion out there, a capital on the horizon is a question.
 */
const ROSTER: { faction: Exclude<Faction, 'raider'>; klass: ClassId; count: number }[] = [
  { faction: 'courier', klass: 'courier', count: 34 },
  { faction: 'freighter', klass: 'freighter', count: 14 },
  { faction: 'marshal', klass: 'marshal', count: 18 },
  { faction: 'marshal', klass: 'warden', count: 3 },
  { faction: 'marshal', klass: 'bastion', count: 1 },
]

/**
 * How many marshal interceptors the sector tries to keep flying.
 *
 * The patrol is *meant* to take losses — that is the whole of what makes an ambient firefight
 * legible — but a sector that runs out of marshals half an hour in stops having one, and the
 * faction quietly disappears from a game that never says it has. Reinforcement is what keeps the
 * ambient violence ambient rather than a one-off event early in a session.
 *
 * The capitals are deliberately **not** replaced. A warden is a thing that was there and is now
 * gone, and respawning one would make killing it meaningless in the one place the sector has
 * something at stake.
 */
export const MARSHAL_STRENGTH = strengthOf('marshal', 'marshal')

/**
 * How many of one class of one faction the sector tries to keep flying, read off `ROSTER`.
 *
 * Derived rather than restated. `MARSHAL_STRENGTH` used to be a separate `18` sitting next to a
 * roster line that also said 18, which is two places to change and one of them silently wins.
 * Capitals are excluded by the callers, not here — the roster is the count that was placed, and
 * whether a thing is replaced is a different decision from how many there were.
 */
export function strengthOf(faction: Faction, klass: ClassId): number {
  return ROSTER.find((e) => e.faction === faction && e.klass === klass)?.count ?? 0
}

/**
 * The civilian classes the sector tops back up, and how many of each.
 *
 * **Traffic was never replenished at all.** Couriers and freighters were placed once at
 * generation and that was the whole of it: raiders hunt them, so a sector left running long
 * enough simply ran out of civilians — measured at 34 couriers and 14 freighters down to zero,
 * permanently, with no path back. The two factions the game uses to make the sector feel
 * inhabited quietly stopped existing, and nothing said so.
 *
 * Marshals are not in this list because they have their own timer: a patrol replacement is a
 * response to violence and arrives on its own cadence, where traffic is just traffic.
 */
export const TRAFFIC: { faction: Faction; klass: ClassId }[] = [
  { faction: 'courier', klass: 'courier' },
  { faction: 'freighter', klass: 'freighter' },
]

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

  for (const entry of ROSTER) {
    const faction = entry.faction
    const spec = CLASSES[entry.klass]
    // A capital does not fly a delivery route. Freezing `route` to empty for one keeps the
    // placement path below identical for every roster line rather than branching it.
    const route = spec.capital ? [] : routeNodes(space, faction)
    for (let i = 0; i < entry.count; i += 1) {
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
            // The no-route case, which is every marshal and every capital: scattered across the
            // sector rather than across one `EXTENT`. The box used to be ±1 extent, and once the
            // sector grew to ±6 that quietly became "the middle of the map" — the patrol was
            // placed in a small central cube while the raiders it exists to hunt were spread over
            // fifty times the volume, so the two populations barely overlapped and the ambient war
            // stopped. Three checks caught it at once, all of them about the same thing: marshals
            // killing raiders with nobody watching.
            x: rng.below(SECTOR_REACH * 2) - SECTOR_REACH,
            y: Math.trunc((rng.below(SECTOR_REACH * 2) - SECTOR_REACH) * 0.7),
            z: rng.below(SECTOR_REACH * 2) - SECTOR_REACH,
          }

      out.push({
        // The class is in the id because a faction fields more than one now. Without it a
        // marshal interceptor and a warden would both be `marshal:0`, and every id-keyed
        // structure in the game — flashes, hit resolution, the sensor board — would treat two
        // different ships as one.
        id: `${faction}:${spec.id}:${i}`,
        faction,
        spec,
        at,
        destination: route.length > 1 ? route[rng.below(route.length)].id : null,
      })
    }
  }
  return out
}

/**
 * One replacement marshal, deterministic in the seed and in how many have been raised before it.
 *
 * ## What is and is not deterministic here, stated because the distinction is load-bearing
 *
 * The sector's *initial* population is a pure function of the record, and that property is the
 * one the whole game rests on. Reinforcements cannot be: whether a marshal died at all depends on
 * how the player flew, so two players holding the same record diverge the moment either of them
 * touches anything. What is preserved is the *sequence* — reinforcement number `n` for a given
 * record always has the same class, the same route and the same offset — so the divergence is
 * confined to when a wave arrives rather than leaking into what the sector is made of.
 *
 * They arrive well away from the player. A ship materialising inside sensor range is the single
 * clearest way to tell a player that nothing they are looking at is real.
 */
export function marshalReinforcement(
  space: Space,
  seed: string,
  index: number,
  awayFrom: Vec3,
  clearance: number,
): Civilian {
  return civilianReinforcement(space, seed, 'marshal', 'marshal', index, awayFrom, clearance)
}

/**
 * A replacement civilian of any class, placed and given somewhere to go.
 *
 * Generalised from the marshal-only version so traffic can be topped up the same way. The stream
 * is keyed by faction *and* class as well as by the wave number: sharing one stream across
 * factions would make a courier's replacement position a function of how many freighters had
 * died, which is both wrong and untraceable when it goes wrong.
 */
export function civilianReinforcement(
  space: Space,
  seed: string,
  faction: Faction,
  klass: ClassId,
  index: number,
  awayFrom: Vec3,
  clearance: number,
): Civilian {
  // Its own stream, keyed by the wave number, so a reinforcement never draws from the stream the
  // initial placement used — reusing it would make the first replacement land exactly where a
  // dead one started.
  const rng = new Rng(seed.slice(24, 32) || seed.slice(16, 24) || seed)
  const route = routeNodes(space, faction)
  // Advance the stream deterministically to this wave's slot, so wave 7 is wave 7 whether it is
  // raised at four minutes or forty. The faction and class are folded in so two factions
  // replacing their seventh loss do not both land in the same place.
  const offset = [...`${faction}:${klass}`].reduce((a, c) => (a * 31 + c.charCodeAt(0)) % 997, 7)
  for (let i = 0; i < offset + index * 3; i += 1) rng.below(1024)

  // Sized by the sector, not by one extent — same reason as the roster placement above.
  const span = SECTOR_REACH * 2
  let at = {
    x: rng.below(span) - SECTOR_REACH,
    y: Math.trunc((rng.below(span) - SECTOR_REACH) * 0.7),
    z: rng.below(span) - SECTOR_REACH,
  }
  // Pushed outward until it clears the player rather than re-rolled, so the loop terminates on
  // the first pass instead of possibly never — the same shape as `raidersOf`'s spawn guard.
  let guard = 0
  while (
    Math.hypot(at.x - awayFrom.x, at.y - awayFrom.y, at.z - awayFrom.z) < clearance &&
    guard < 24
  ) {
    at = { x: at.x * 2 - awayFrom.x, y: at.y * 2 - awayFrom.y, z: at.z * 2 - awayFrom.z + clearance }
    guard += 1
  }

  return {
    // `+` rather than `:` before the wave number, so a reinforcement id can never collide with a
    // roster id however many of either there are.
    id: `${faction}:${klass}+${index}`,
    faction,
    spec: CLASSES[klass],
    at,
    destination: route.length > 0 ? route[index % route.length].id : null,
  }
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
