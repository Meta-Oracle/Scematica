/**
 * Many records, one galaxy.
 *
 * A player who holds several worlds should be able to fly between them rather than reloading
 * the page. `join` places each space at its own offset and links them, so a corpus becomes a
 * single volume without any of the individual spaces changing.
 *
 * ## Placement is derived, never chosen
 *
 * Each world's offset comes from its own commitment. Two consequences, and the second is the
 * one that matters: a world always sits in the same place relative to the others, and **the
 * galaxy does not depend on the order the records were loaded**. Somebody who drops three
 * files in a different sequence gets the same arrangement, so two players comparing notes are
 * describing the same thing.
 *
 * An index-based layout would have been simpler and would have quietly made the map a
 * function of a UI event order.
 *
 * ## What a bridge does not claim
 *
 * Worlds are linked by proximity, and a bridge means only *these two records are both yours*.
 * It is not a claim that the observed things are related — a repository and a set of oracle
 * feeds have nothing to do with each other, and drawing a lane between them must not suggest
 * otherwise. The renderer gives bridges their own role so they cannot be mistaken for lanes
 * inside a world, which *are* structural.
 */

import type { Contact, Lane, Node, Space, Vec3 } from './generate.ts'
import { EXTENT } from './generate.ts'
import { Rng } from '../omni/fractal.ts'

/** How far apart two worlds sit. Comfortably beyond a single world's own extent. */
const SPACING = EXTENT * 3

export interface Bridge {
  /** Index into `Fleet.worlds`. */
  from: number
  to: number
  fromNode: number
  toNode: number
}

export interface Fleet {
  worlds: { seed: string; label: string; origin: Vec3 }[]
  nodes: Node[]
  lanes: Lane[]
  contacts: Contact[]
  bridges: Bridge[]
  /**
   * Lowest sensor range across the fleet, or `null` if *any* world's range is unknown.
   *
   * Unknown wins over any number, because a fleet containing one unmeasured world is a fleet
   * whose visibility nobody has established. Taking the minimum of the known ones would report
   * a confident figure computed over an incomplete set — the coverage mistake, in a new place.
   */
  sensorRange: number | null
}

/** Where a world sits, from its own commitment rather than from its position in a list. */
export function placement(seed: string): Vec3 {
  const rng = new Rng(seed)
  // Three draws on a fixed lattice, so worlds are spread without overlapping and without a
  // float anywhere in the arithmetic.
  const span = 5
  const pick = () => (rng.below(span) - Math.floor(span / 2)) * SPACING
  return { x: pick(), y: Math.trunc(pick() / 2), z: pick() }
}

function shift(v: Vec3, by: Vec3): Vec3 {
  return { x: v.x + by.x, y: v.y + by.y, z: v.z + by.z }
}

function dist(a: Vec3, b: Vec3): number {
  return Math.hypot(a.x - b.x, a.y - b.y, a.z - b.z)
}

/**
 * Join several spaces into one.
 *
 * Node ids are renumbered so they stay unique across the fleet; lanes and contacts follow.
 * Deterministic and order-independent: the worlds are sorted by commitment before placement.
 */
export function join(spaces: Space[], labels: string[] = []): Fleet {
  // Sorted by seed, so the galaxy is a function of *which* records are held rather than of
  // the order somebody happened to drop them.
  const order = spaces
    .map((s, i) => ({ s, label: labels[i] ?? s.seed.slice(0, 12) }))
    .sort((a, b) => (a.s.seed < b.s.seed ? -1 : a.s.seed > b.s.seed ? 1 : 0))

  const worlds: Fleet['worlds'] = []
  const nodes: Node[] = []
  const lanes: Lane[] = []
  const contacts: Contact[] = []
  const originIds: number[] = []

  for (const { s, label } of order) {
    const at = placement(s.seed)
    const base = nodes.length
    worlds.push({ seed: s.seed, label, origin: at })

    for (const n of s.nodes) {
      nodes.push({ ...n, id: base + n.id, at: shift(n.at, at) })
    }
    for (const l of s.lanes) {
      lanes.push({ from: base + l.from, to: base + l.to, severed: l.severed })
    }
    for (const c of s.contacts) {
      // Contact ids are prefixed with the world, because two records can legitimately carry
      // the same signal id and a fleet must not merge two different things into one target.
      contacts.push({ ...c, id: `${s.seed.slice(0, 8)}:${c.id}`, at: shift(c.at, at) })
    }
    originIds.push(base)
  }

  // Bridges: each world to its nearest neighbour, origin to origin. A minimal spanning shape
  // rather than every pair — a complete graph on a dozen worlds is unreadable and says
  // nothing extra.
  const bridges: Bridge[] = []
  for (let i = 1; i < worlds.length; i += 1) {
    let best = 0
    let bestD = Infinity
    for (let j = 0; j < i; j += 1) {
      const d = dist(worlds[i].origin, worlds[j].origin)
      if (d < bestD) {
        bestD = d
        best = j
      }
    }
    bridges.push({ from: best, to: i, fromNode: originIds[best], toNode: originIds[i] })
  }

  const ranges = order.map(({ s }) => s.sensorRange)
  const sensorRange = ranges.some((r) => r === null)
    ? null
    : Math.min(...(ranges as number[]))

  return { worlds, nodes, lanes, contacts, bridges, sensorRange }
}
