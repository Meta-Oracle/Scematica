/**
 * Scema-World — a sealed decision record, as a volume of space.
 *
 * The world tree is already a deterministic function of the record's commitment: the same
 * digest grows the same fractal, in Rust and in the browser, byte for byte. That property is
 * what makes a *game* possible without a server. The record is the seed, the map, and the
 * proof that the map is what it claims to be — so two players holding the same record fly
 * the same space, and neither has to trust the other or anybody hosting it.
 *
 * This module is the whole load-bearing half: record → space. It is pure, has no WebGL in it,
 * and is tested by `check:scemaworld`. A renderer is a view of this; it is not this.
 *
 * ## The epistemics are the mechanics
 *
 * The temptation with a generator is to reach for "interesting" numbers and tune until the
 * space feels good. That would make Scema-World a skin over a random seed, and the record
 * would be decoration. Every quantity below instead comes from something an observer
 * actually reported, and the project's central rule survives into play:
 *
 * | In the record | In the space |
 * |---|---|
 * | `blind_spots` (count) | a **rift** — a dead end you cannot see into, one per blind spot |
 * | signal `measured: false` | a **ghost contact** — reads on sensors, may not be there |
 * | signal `polarity: risk` | hostile |
 * | signal `polarity: opportunity` | salvage |
 * | `Provenance::Stale` | derelict station, still there, no longer answering |
 * | `Provenance::Absent` | a marker where something should be, and nothing is |
 * | legibility | sensor range — an illegible world is literally dark |
 * | `extent.total === null` | the map has **no boundary**, and says so |
 *
 * A ghost contact is the em-dash rule as a game mechanic. An unmeasured signal must not
 * render as a solid enemy, because that is exactly the lie the whole runtime is built to
 * refuse — so it renders as something the player is told they cannot trust.
 *
 * ## Not an economy
 *
 * There is no currency, no market, no yield, and no reason to add one. Owning the record gets
 * you the space it describes; that is the entire relationship between the token and the game.
 * Anything that priced a world would make a record's *content* worth misreporting, and a
 * producer with an incentive to lie is the one failure mode this project cannot absorb.
 */

import { growthOf, plannedCuts, Rng, type Growth } from '../omni/fractal.ts'
import type { WorldState } from '../omni/types.ts'

/** Integer millis of a space unit, so positions stay exact and reproducible. */
const UNIT = 1000

/** Scale of the whole volume. Arbitrary but fixed — a renderer scales it, never the generator. */
export const EXTENT = 4000 * UNIT

export interface Vec3 {
  x: number
  y: number
  z: number
}

/** A navigable point. Stations sit on the branch nodes the world tree already produces. */
export interface Node {
  id: number
  at: Vec3
  /** Recursion depth this node sits at. 0 is the origin. */
  depth: number
  kind: 'origin' | 'station' | 'derelict' | 'marker' | 'phantom' | 'rift'
  /** The object this came from, when it came from one. */
  label: string
}

/** A traversable lane between two nodes. */
export interface Lane {
  from: number
  to: number
  /** A lane into a rift is one-way in the sense that it ends: nothing is on the far side. */
  severed: boolean
}

/** Something the player meets. */
export interface Contact {
  id: string
  at: Vec3
  hostility: 'hostile' | 'salvage'
  /**
   * False when the signal's magnitude was estimated rather than counted.
   *
   * A ghost reads on sensors and may not be there. This is the single most important field in
   * the file: rendering an unmeasured signal as a solid contact would be the em-dash bug, in
   * a place where the player would act on it.
   */
  solid: boolean
  /** Signal magnitude, 0..1. Drives size, never damage — see the note on economy. */
  magnitude: number
  label: string
}

export interface Space {
  /** The record's world commitment. The space is a function of this and nothing else. */
  seed: string
  nodes: Node[]
  lanes: Lane[]
  contacts: Contact[]
  /**
   * How far the player can see, 0..1 of `EXTENT`. From legibility.
   *
   * `null` when no objects were perceived: sensor range is *unknown*, not zero. A renderer
   * must show that differently from a dark map — the difference between "you cannot see" and
   * "nobody knows how far you can see".
   */
  sensorRange: number | null
  /** True when `extent.total` was null: the volume has no known boundary. */
  unbounded: boolean
  /** Rift count, exactly the reported blind spots (capped by geometry, and it says which). */
  rifts: number
  riftsCapped: boolean
  growth: Growth
}

/** Integer trig, shared with the fractal so the two agree on where a branch points. */
function polar(len: number, yawDeg: number, pitchDeg: number): Vec3 {
  const yaw = sin(yawDeg + 90)
  const yawS = sin(yawDeg)
  const pitch = sin(pitchDeg + 90)
  const pitchS = sin(pitchDeg)
  return {
    x: Math.trunc((len * yaw * pitch) / 1_000_000_000_000),
    y: Math.trunc((len * pitchS) / 1_000_000),
    z: Math.trunc((len * yawS * pitch) / 1_000_000_000_000),
  }
}

/** Integer sine in millionths, whole degrees. Reuses the fractal's table via its Rng module. */
function sin(deg: number): number {
  // Normalised into [0, 360) first, so negative pitches behave.
  const d = ((deg % 360) + 360) % 360
  if (d <= 90) return TABLE[d]
  if (d <= 180) return TABLE[180 - d]
  if (d <= 270) return -TABLE[d - 180]
  return -TABLE[360 - d]
}

/** sin(0°..90°) in millionths. Integer, so no platform disagrees about a coordinate. */
const TABLE: number[] = (() => {
  // Generated once from the same definition the Rust table uses: round(sin(d) * 1e6).
  const t: number[] = []
  for (let d = 0; d <= 90; d += 1) {
    t.push(Math.round(Math.sin((d * Math.PI) / 180) * 1_000_000))
  }
  return t
})()

function add(a: Vec3, b: Vec3): Vec3 {
  return { x: a.x + b.x, y: a.y + b.y, z: a.z + b.z }
}

/** Which node kind a perceived object becomes. Provenance is the whole mapping. */
function kindOf(provenance: string): Node['kind'] {
  switch (provenance) {
    case 'live':
      return 'station'
    case 'stale':
      return 'derelict'
    case 'simulated':
      return 'phantom'
    // Absent: something should be here and is not. A marker, not an empty space — the player
    // is told the observer expected something, which is a different fact from nothing.
    default:
      return 'marker'
  }
}

/**
 * Build the space a record describes.
 *
 * Deterministic: same record, same space, on every machine, forever. No clock, no
 * `Math.random`, no floats in any coordinate.
 */
export function generate(world: WorldState, digest: string): Space {
  const g = growthOf(world)
  const rng = new Rng(digest)
  const [riftCount, riftsCapped] = plannedCuts(g)

  const nodes: Node[] = [{ id: 0, at: { x: 0, y: 0, z: 0 }, depth: 0, kind: 'origin', label: 'origin' }]
  const lanes: Lane[] = []

  // Which objects dress which node. Walked in order so the mapping is stable.
  const objects = world.objects
  let objectCursor = 0

  // Rifts land on one level, exactly as the fractal cuts one level — so the count on screen
  // and the count in the legend cannot disagree.
  const riftLevel = Math.max(1, g.depth - 2)
  let riftsPlaced = 0

  const trunk = Math.trunc(EXTENT / 3)

  const grow = (parent: number, len: number, yaw: number, pitch: number, depth: number) => {
    if (depth <= 0 || len < UNIT * 40 || nodes.length > 4000) return

    const at = add(nodes[parent].at, polar(len, yaw, pitch))
    const id = nodes.length
    const level = g.depth - depth

    // A rift: the lane exists, the far side does not. One per reported blind spot, a count and
    // never a rate — the same rule the fractal's severed limbs follow.
    const isRift = level === riftLevel && riftsPlaced < riftCount
    if (isRift) riftsPlaced += 1

    const obj = !isRift && objectCursor < objects.length ? objects[objectCursor++] : null
    nodes.push({
      id,
      at,
      depth: level,
      kind: isRift ? 'rift' : obj ? kindOf(obj.provenance.kind) : 'station',
      label: isRift ? 'rift' : (obj?.label ?? `waypoint ${id}`),
    })
    lanes.push({ from: parent, to: id, severed: isRift })
    if (isRift) return

    const next = Math.trunc((len * g.decay) / 100)
    for (let i = 0; i < g.arity; i += 1) {
      const offset = g.arity === 1 ? 0 : -g.spread + Math.trunc((2 * g.spread * i) / (g.arity - 1))
      // Pitch fans on a different axis than yaw so the tree occupies a volume rather than a
      // plane. Jitter comes from the same RNG the fractal uses, in the same order.
      const j = rng.jitter(5)
      grow(id, next, yaw + offset + j, pitch + Math.trunc(offset / 2), depth - 1)
    }
  }

  grow(0, trunk, 0, 0, g.depth)

  // Contacts sit on nodes, spread through the map rather than clustered at the origin.
  const contacts: Contact[] = []
  const n = Math.max(1, world.signals.length)
  world.signals.forEach((s, i) => {
    const node = nodes[Math.trunc((i * nodes.length) / n)]
    if (!node) return
    contacts.push({
      id: s.id,
      at: node.at,
      hostility: s.polarity === 'risk' ? 'hostile' : 'salvage',
      solid: s.measured,
      magnitude: s.magnitude,
      label: s.label,
    })
  })

  // Sensor range from legibility. `null` when nothing was perceived — unknown, not zero.
  const sensorRange = world.objects.length === 0 ? null : legibility(world)

  return {
    seed: digest,
    nodes,
    lanes,
    contacts,
    sensorRange,
    unbounded: g.unbounded,
    rifts: riftsPlaced,
    riftsCapped,
    growth: g,
  }
}

function legibility(w: WorldState): number {
  const actionable = w.objects.filter((o) => o.provenance.kind === 'live').length
  return w.objects.length === 0 ? 0 : actionable / w.objects.length
}
