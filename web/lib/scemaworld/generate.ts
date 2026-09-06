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
import { raidersOf } from './raiders.ts'
import type { WorldState } from '../omni/types.ts'

import { EXTENT, MIN_BRANCH, MIN_NODE_GAP, TRUNK } from './scale.ts'

/**
 * Scale of the whole volume, re-exported because it is what most callers want from here.
 *
 * Sixty times the first version, which was crossable in about four seconds at full throttle
 * and therefore read as a diagram rather than a sector. Distance is the cheapest way to make
 * a space feel like one: at this size a lane between two stations is a journey with a fuel
 * cost, which is what makes docking matter and what makes a rift somewhere out on the frontier
 * a thing you decide whether to visit.
 *
 * Everything derived from it — station radii, weapon reach, aggro range — lives in `scale.ts`.
 * They were bare literals once, and the sixty-fold enlargement moved the positions and left
 * them all behind. See that file.
 */
export { EXTENT }

export interface Vec3 {
  x: number
  y: number
  z: number
}

export type NodeKind =
  | 'origin'
  | 'station'
  | 'dock'
  | 'depot'
  | 'market'
  | 'derelict'
  | 'marker'
  | 'phantom'
  | 'rift'

/** What a node will do for you if you approach it. */
export type Service = 'refuel' | 'repair' | 'trade' | 'scavenge'

/**
 * Which services a node offers, from its provenance.
 *
 * The mapping is the epistemics again, and the `phantom` row is the one worth keeping: a
 * **simulated** object looks exactly like a station on approach and offers nothing, because it
 * was modelled rather than observed. Something the observer imagined cannot refuel you.
 *
 * A `derelict` is stale — it is really there and no longer answering — so it can be scavenged
 * once and cannot be traded with. An `absent` marker offers nothing at all: the observer
 * expected something and found nothing, and flying to it is how you learn that first-hand.
 */
export function servicesOf(kind: NodeKind): Service[] {
  switch (kind) {
    case 'dock':
      return ['refuel', 'repair', 'trade']
    case 'depot':
      return ['refuel']
    case 'market':
      return ['trade', 'repair']
    case 'station':
      return ['refuel']
    case 'derelict':
      return ['scavenge']
    case 'origin':
      // The home station is the one node that does everything. It is where the ship starts and
      // where a new player learns what the service keys are for, and a first station that
      // answers "core does not offer refuel" teaches that the keys do not work.
      return ['refuel', 'repair', 'trade']
    // origin, marker, phantom, rift: nothing. A phantom especially — it is a mirage.
    default:
      return []
  }
}

/** A navigable point. Stations sit on the branch nodes the world tree already produces. */
export interface Node {
  id: number
  at: Vec3
  /** Recursion depth this node sits at. 0 is the origin. */
  depth: number
  kind: NodeKind
  /** Services this node offers. Empty for anything that cannot be approached. */
  services: Service[]
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
  /**
   * True for a hostile the *sector* placed rather than one the record reported.
   *
   * Set only by `raiders.ts`. It keeps the two apart at the type level, because the one thing
   * that must not happen is a piece of game furniture becoming indistinguishable from a signal
   * somebody actually counted — at which point the record's contents stop being checkable by
   * looking at the screen.
   */
  unlogged?: boolean
  /**
   * Hit radius, when this target is a live craft rather than an inert signal.
   *
   * Set by `game.ts` from the craft's class, because that is what the renderer draws. Absent for
   * a static contact, which falls back to the magnitude formula — a signal is a reading and its
   * size is a claim about how big a concern somebody counted.
   */
  radius?: number
  /**
   * The craft's facing and silhouette, so the hit test can be the *shape* rather than a sphere
   * around it. Absent for an inert contact, which is genuinely round.
   */
  facing?: Vec3
  shape?: string
  /**
   * A named class, for a hostile the *sector* placed deliberately.
   *
   * Set only by `raiders.ts`, and only for the capital garrison — everything else has its class
   * rolled from the seed by `enemy.ts::classRoll`. It exists because a roll cannot express "every
   * sector has a leviathan in it somewhere": `classFor` reaches one about once in a hundred and
   * fifty, so on a roster of seventy-two the war classes turned up by accident or not at all.
   *
   * A `string` rather than a `ClassId`, deliberately, and for the same reason `shape` above is
   * one: `generate.ts` describes what a *record* becomes and must not acquire a dependency on the
   * combat tables. `swarmOf` validates it against `CLASSES` and ignores anything it does not
   * recognise, so a stray value is inert rather than a crash.
   *
   * **Never set from anything a record reported.** A record that could name its own opposition
   * would be a record worth writing carefully, which is the failure this whole file is arranged
   * to prevent.
   */
  klass?: string
}

export interface Space {
  /** The record's world commitment. The space is a function of this and nothing else. */
  seed: string
  nodes: Node[]
  lanes: Lane[]
  contacts: Contact[]
  /**
   * Hostiles the sector carries that the record never mentioned. See `raiders.ts`.
   *
   * Separate from `contacts` deliberately: a raider is not a claim about anything, and mixing
   * the two would let furniture pass for evidence.
   */
  raiders: Contact[]
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

  const nodes: Node[] = [
    {
      id: 0,
      at: { x: 0, y: 0, z: 0 },
      depth: 0,
      // The core of a world is always a market. It is where you arrive, it is the one place
      // that is reachable without a journey, and a sector you cannot outfit from is a sector
      // you can only lose in.
      // Its own kind, not a market. The home station does everything — refuel, repair and trade
      // — because it is where a new player learns what the service keys are for, and a first
      // station that answers "core does not offer refuel" teaches that the keys do not work. It
      // also gets its own silhouette, which is worth having for the one node you keep coming back
      // to.
      kind: 'origin',
      services: servicesOf('origin'),
      label: 'core',
    },
  ]
  const lanes: Lane[] = []

  // Which objects dress which node. Walked in order so the mapping is stable.
  const objects = world.objects
  let objectCursor = 0

  // Rifts land on one level, exactly as the fractal cuts one level — so the count on screen
  // and the count in the legend cannot disagree.
  const riftLevel = Math.max(1, g.depth - 2)
  let riftsPlaced = 0

  // A long trunk and a slow taper: the tree has to reach the edges of a volume this size or
  // the sector is a dense knot with emptiness around it, which reads as smaller than a map
  // half the size that fills its space.
  //
  // The length itself lives in `scale.ts` with every other world distance. It was the one that
  // had escaped, and it is the only lever that actually makes the sector bigger — see `TRUNK`.
  const trunk = TRUNK

  /**
   * A coarse spatial index of placed nodes, so the gap check below is not quadratic.
   *
   * Cells are `MIN_NODE_GAP` across, which means a candidate can only be too close to something
   * in its own cell or one of the twenty-six around it.
   */
  const placed = new Map<string, Vec3[]>()
  const cellKey = (v: Vec3) =>
    `${Math.floor(v.x / MIN_NODE_GAP)},${Math.floor(v.y / MIN_NODE_GAP)},${Math.floor(v.z / MIN_NODE_GAP)}`

  /**
   * Whether a candidate position is far enough from every node already placed.
   *
   * `MIN_BRANCH` alone does not give this. It bounds the distance between a node and its own
   * *parent*, and says nothing about two branches from different parents folding back toward
   * each other — which is precisely how a sector ends up with stations a few hundred thousand
   * units apart in a volume three thousand million across. Enlarging `EXTENT` cannot fix that
   * either: a longer trunk moves the whole knot further out and leaves it just as tight.
   */
  const clear = (v: Vec3): boolean => {
    const cx = Math.floor(v.x / MIN_NODE_GAP)
    const cy = Math.floor(v.y / MIN_NODE_GAP)
    const cz = Math.floor(v.z / MIN_NODE_GAP)
    for (let dx = -1; dx <= 1; dx += 1) {
      for (let dy = -1; dy <= 1; dy += 1) {
        for (let dz = -1; dz <= 1; dz += 1) {
          const bucket = placed.get(`${cx + dx},${cy + dy},${cz + dz}`)
          if (!bucket) continue
          for (const o of bucket) {
            if (Math.hypot(v.x - o.x, v.y - o.y, v.z - o.z) < MIN_NODE_GAP) return false
          }
        }
      }
    }
    return true
  }

  const remember = (v: Vec3) => {
    const k = cellKey(v)
    const bucket = placed.get(k)
    if (bucket) bucket.push(v)
    else placed.set(k, [v])
  }

  remember(nodes[0].at)

  const grow = (parent: number, len: number, yaw: number, pitch: number, depth: number) => {
    if (depth <= 0 || len < MIN_BRANCH || nodes.length > 3600) return

    const at = add(nodes[parent].at, polar(len, yaw, pitch))
    // Too close to something already placed: this branch stops rather than being nudged. Nudging
    // would break determinism's easiest guarantee — that the tree is a pure function of the
    // record — by making a node's position depend on the order its siblings were visited.
    // Stopping depends on that order too, but stopping is *visible* as a shorter branch rather
    // than as a station somewhere it does not belong.
    if (!clear(at)) return
    remember(at)
    const id = nodes.length
    const level = g.depth - depth

    // A rift: the lane exists, the far side does not. One per reported blind spot, a count and
    // never a rate — the same rule the fractal's severed limbs follow.
    const isRift = level === riftLevel && riftsPlaced < riftCount
    if (isRift) riftsPlaced += 1

    const obj = !isRift && objectCursor < objects.length ? objects[objectCursor++] : null
    let kind: NodeKind = isRift ? 'rift' : obj ? kindOf(obj.provenance.kind) : 'station'

    // Service nodes among the live stations. Which one becomes what is drawn from the seed
    // rather than from anything the record measured — a dock is a convenience the sector
    // happens to have, and deriving it from a reported quantity would make that quantity
    // worth misreporting. Same reasoning as combat durability.
    if (kind === 'station') {
      // Rare, common, commonest. A market on every tenth node makes outfitting a formality;
      // it should be somewhere you plan a route around.
      const roll = rng.below(25)
      if (roll === 0) kind = 'market'
      else if (roll <= 3) kind = 'dock'
      else if (roll <= 8) kind = 'depot'
    }

    nodes.push({
      id,
      at,
      depth: level,
      kind,
      services: servicesOf(kind),
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

  // Contacts sit on nodes, spread through the map.
  //
  // **Never on node 0.** That is the origin, and the origin is where the player spawns: the
  // first signal in the record was placed exactly on top of the ship, so the game opened with
  // a hostile already inside the cockpit and the first shot fired hit it before leaving the
  // muzzle. A spawn point has to be a place you can look around from.
  const contacts: Contact[] = []
  const placeable = nodes.length - 1
  const n = Math.max(1, world.signals.length)
  world.signals.forEach((s, i) => {
    const node = placeable > 0 ? nodes[1 + Math.trunc((i * placeable) / n)] : undefined
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
    raiders: raidersOf(digest),
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
