/**
 * What the renderer draws, decided in one place.
 *
 * Same arrangement as `lib/mesh/view.ts::toneFor`: the rule that encodes a claim about trust
 * gets exactly one implementation, and the thing with a GPU in it only places geometry. A
 * `Role` is named here; a colour is looked up here; `gl.ts` never chooses either.
 *
 * The rule this file exists to protect: **a ghost contact must not be able to look like a
 * solid one.** It is the em-dash rule at the point a player acts on it, and if the difference
 * ever comes down to a slightly different shade passed at a call site, it will be lost the
 * first time somebody tunes the palette.
 */

import { EXTENT, type Contact, type Node, type Space, type Vec3 } from './generate.ts'
import { FAR_PLANE } from './scale.ts'
import {
  R_CONTACT, R_CONTACT_SPAN, R_DEPOT, R_DERELICT, R_DOCK, R_LASER, R_MARKER, R_MARKET,
  R_ORIGIN, R_PHANTOM, R_PHOTON, R_RIFT, R_STATION,
} from './scale.ts'
import type { ClassSpec, Shape } from './classes.ts'

/**
 * Every role the renderer can draw. A role is a claim about what a thing *is*, not a colour.
 */
export type Role =
  | 'origin'
  | 'station'
  | 'dock'
  | 'depot'
  | 'market'
  | 'derelict'
  | 'marker'
  | 'phantom'
  | 'rift'
  | 'lane'
  | 'lane-severed'
  | 'hostile'
  | 'salvage'
  | 'ghost-hostile'
  | 'ghost-salvage'
  | 'laser'
  | 'photon'
  | 'enemy-shot'
  | 'raider'
  | 'waypoint'
  | 'capital'

/** RGB in 0..1, for a shader. One table, so a palette change moves every surface at once. */
export const PALETTE: Record<Role, readonly [number, number, number]> = {
  origin: [0.66, 0.42, 1.0],
  station: [0.55, 0.78, 1.0],
  derelict: [0.55, 0.5, 0.42],
  // A marker is where something should be and is not. Deliberately close to the void colour:
  // it should be hard to see, because that is the honest rendering of an absence.
  marker: [0.3, 0.28, 0.36],
  phantom: [0.42, 0.36, 0.6],
  rift: [0.44, 0.4, 0.56],
  // Services are cyan-ward so they read as *infrastructure* rather than as a contact. A
  // player scanning for somewhere to refuel should not have to distinguish a depot from a
  // hostile by shade.
  dock: [0.42, 0.86, 0.95],
  depot: [0.34, 0.68, 0.8],
  market: [0.95, 0.78, 0.38],
  lane: [0.36, 0.3, 0.55],
  'lane-severed': [0.44, 0.4, 0.56],
  hostile: [1.0, 0.35, 0.38],
  salvage: [0.35, 0.9, 0.62],
  'ghost-hostile': [1.0, 0.35, 0.38],
  'ghost-salvage': [0.35, 0.9, 0.62],
  laser: [1.0, 0.86, 0.5],
  photon: [0.7, 0.85, 1.0],
  // Enemy fire is the same red as a hostile, so incoming reads as *theirs* at a glance.
  'enemy-shot': [1.0, 0.42, 0.42],
  // A raider is not from the record. Orange rather than red keeps that visible from the
  // cockpit: red things are signals somebody reported, orange things are just out here.
  raider: [1.0, 0.55, 0.2],
  // A capital is the same orange family, pushed toward bronze so it reads as a different
  // *weight* of thing rather than a bigger fighter. Silhouette carries the rest.
  capital: [1.0, 0.72, 0.34],
  // The nav computer's marker. Deliberately the brightest thing in the palette — it is the
  // only body on screen the player put there.
  waypoint: [0.6, 1.0, 0.85],
}

/**
 * Whether a role is drawn as a solid body or as an outline that pulses.
 *
 * **This is the load-bearing distinction in the file.** Ghosts share a hue with their solid
 * counterpart — a hostile ghost is still red, because it is still a hostile if it is there —
 * but they are never filled. Colour alone would fail exactly the way the sniper TUI's
 * `Depth::Mono` test exists to prevent: on a bad monitor, at a glance, or for a colour-blind
 * player, two shades of red are one thing.
 */
export function isGhost(role: Role): boolean {
  return role === 'ghost-hostile' || role === 'ghost-salvage'
}

/**
 * The silhouette for a body. The **only** place a shape is chosen.
 *
 * Same arrangement as `PALETTE` and for the same reason: a renderer that decided the fast one
 * was the small triangle would be a second home for the class table, and the two would drift.
 */
export function shapeOf(b: Body): Shape {
  if (b.shape) return b.shape
  if (b.role === 'laser' || b.role === 'photon' || b.role === 'enemy-shot') return 'bolt'
  return b.solid ? 'sphere' : 'shell'
}

/** How faint a lane is drawn. See `Segment.alpha`. */
export const LANE_ALPHA = 0.07
/** A severed lane is a claim about a blind spot, so it gets a little more presence. */
export const LANE_SEVERED_ALPHA = 0.14

export function roleOfNode(n: Node): Role {
  return n.kind
}

export function roleOfContact(c: Contact): Role {
  // Checked before solidity, because an unlogged hostile is never an estimate: the sector knows
  // it is there. The distinction it carries is a different one — nobody *reported* it.
  if (c.unlogged) return 'raider'
  if (c.hostility === 'hostile') return c.solid ? 'hostile' : 'ghost-hostile'
  return c.solid ? 'salvage' : 'ghost-salvage'
}

/** One thing to draw. */
export interface Body {
  at: Vec3
  role: Role
  /** World-space radius. */
  radius: number
  /** False draws an outline instead of a filled body. */
  solid: boolean
  label: string
  /**
   * Which way it points. Absent for anything that has no facing — a station does not have one.
   *
   * Load-bearing for craft and for bolts. A wireframe hull with no facing is a shape with no
   * information in it, and the whole reason ships are line models is that you can see which way
   * an opponent is about to break.
   */
  facing?: Vec3
  /**
   * Hit flash, 0..1. Raised for a few frames after damage lands and decayed by the tick.
   *
   * This is the entirety of the game's feedback that a shot connected against a hull, and it is
   * deliberately loud: a shooter where you cannot tell a hit from a miss is a shooter with no
   * skill expression in it, however good the ballistics underneath are.
   */
  flash?: number
  /** Silhouette. Defaults from the role; a craft overrides it from its class. */
  shape?: Shape
}

export interface Segment {
  from: Vec3
  to: Vec3
  role: Role
  /**
   * How strongly to draw it, 0..1.
   *
   * Lanes are structure, not traffic. At a thousand nodes the lane mesh was a bright cage that
   * hid everything inside it — the sector read as a diagram of itself rather than as a place —
   * so lanes are drawn at the edge of visibility: present enough to follow a route deliberately,
   * faint enough to disappear when you are not looking for one.
   */
  alpha: number
}

export interface DrawList {
  bodies: Body[]
  segments: Segment[]
  /**
   * How far the renderer may draw, in world units.
   *
   * A constant now, covering the whole generated sector. It used to come from sensor range, and
   * that conflated two different things: what the *record* knows and what the *window* shows. A
   * record that perceived little should leave you flying blind to what is coming — which is
   * `sensorFar`, on the sensor panel — not unable to see the space you are in.
   */
  far: number
}

/** Radius for a node, from what it is. Stations are landmarks; markers barely register. */
function nodeRadius(role: Role): number {
  switch (role) {
    case 'origin':
      return R_ORIGIN
    case 'station':
      return R_STATION
    case 'derelict':
      return R_DERELICT
    case 'phantom':
      return R_PHANTOM
    case 'rift':
      return R_RIFT
    case 'market':
      return R_MARKET
    case 'dock':
      return R_DOCK
    case 'depot':
      return R_DEPOT
    default:
      return R_MARKER
  }
}

/**
 * Build the draw list for a space.
 *
 * Pure: no GL, no clock, no camera. Culling by distance belongs to the renderer, which knows
 * where the eye is; this decides what exists and how each thing must look.
 */
export interface Dynamic {
  /** Player projectiles in flight, with the direction they are travelling. */
  shots: { at: Vec3; kind: 'laser' | 'photon'; dir: Vec3 }[]
  /** Enemy fire in flight. */
  incoming: { at: Vec3; dir: Vec3 }[]
  /** Live enemy craft, by contact id, with everything the renderer needs to draw a hull. */
  craft: {
    id: string
    at: Vec3
    solid: boolean
    facing: Vec3
    spec: ClassSpec
    /** Hit flash, decayed by the tick. */
    flash: number
  }[]
  /** Contact ids destroyed, so they stop being drawn. */
  destroyed: string[]
  /** The nav computer's selected node, drawn as a ring you can steer at. */
  waypoint?: Vec3 | null
}

export const NOTHING: Dynamic = { shots: [], incoming: [], craft: [], destroyed: [] }

/**
 * Magnitude is nominally 0..1, but it comes from a hand-written producer and nothing in the
 * importer enforces the range. Clamping keeps a mistyped record from drawing a station-sized
 * contact — an over-large body would *understate* the danger by making a ghost easy to see.
 */
function clamp01(v: number): number {
  return Math.max(0, Math.min(1, v))
}

export function drawList(space: Space, dyn: Dynamic = NOTHING): DrawList {
  const byId = new Map<number, Node>()
  for (const n of space.nodes) byId.set(n.id, n)

  const bodies: Body[] = space.nodes.map((n) => {
    const role = roleOfNode(n)
    return {
      at: n.at,
      role,
      radius: nodeRadius(role),
      // A phantom is simulated — something the observer modelled rather than saw. Drawn
      // hollow for the same reason a ghost is: it is not a thing that was there.
      solid: role !== 'phantom' && role !== 'marker',
      label: n.label,
    }
  })

  // Hostiles are drawn from the swarm, which knows where they have moved to; a hostile that
  // is not in the swarm has been destroyed. Salvage contacts are static and drawn as-is.
  const craftAt = new Map(dyn.craft.map((c) => [c.id, c]))
  for (const c of [...space.contacts, ...space.raiders]) {
    if (dyn.destroyed.includes(c.id)) continue
    const role = roleOfContact(c)
    const live = craftAt.get(c.id)
    if (c.hostility === 'hostile' && dyn !== NOTHING && !live) continue
    if (live) {
      // An armed craft is drawn as its class: a wireframe hull, at the class's own size, facing
      // where it is going. Magnitude does not size a craft — its class does — because a craft's
      // size is now a claim about how dangerous it is, and that must never come from a number
      // in the record.
      bodies.push({
        at: live.at,
        role: live.spec.capital ? 'capital' : role,
        radius: live.spec.radius,
        solid: !isGhost(role),
        label: `${live.spec.label} ${c.label}`,
        facing: live.facing,
        flash: live.flash,
        shape: live.spec.shape,
      })
      continue
    }
    bodies.push({
      at: c.at,
      role,
      // Magnitude drives size for an inert contact. Never damage, never yield — a signal's
      // magnitude measures a concern, and turning it into a number that rewards would invite
      // tuning the record.
      radius: R_CONTACT + Math.round(clamp01(c.magnitude) * R_CONTACT_SPAN),
      solid: !isGhost(role),
      label: c.label,
    })
  }

  // Projectiles. Their absence was the bug that made the whole game look broken: shots were
  // created, stepped and resolved, and nothing ever drew them.
  for (const s of dyn.shots) {
    bodies.push({
      at: s.at,
      role: s.kind,
      radius: s.kind === 'photon' ? R_PHOTON : R_LASER,
      solid: true,
      label: s.kind,
      facing: s.dir,
    })
  }
  for (const s of dyn.incoming) {
    bodies.push({
      at: s.at,
      role: 'enemy-shot',
      radius: R_LASER,
      solid: true,
      label: 'incoming',
      facing: s.dir,
    })
  }

  // The waypoint, drawn hollow: it marks a place, and a filled body would read as a thing.
  if (dyn.waypoint) {
    bodies.push({
      at: dyn.waypoint,
      role: 'waypoint',
      radius: R_MARKET,
      solid: false,
      label: 'waypoint',
    })
  }

  const segments: Segment[] = []
  for (const l of space.lanes) {
    const a = byId.get(l.from)
    const b = byId.get(l.to)
    if (!a || !b) continue
    segments.push({
      from: a.at,
      to: b.at,
      role: l.severed ? 'lane-severed' : 'lane',
      alpha: l.severed ? LANE_SEVERED_ALPHA : LANE_ALPHA,
    })
  }

  return { bodies, segments, far: FAR_PLANE }
}

/**
 * Contact range from sensor legibility. **No longer the draw distance.**
 *
 * Legibility used to gate how far the renderer drew, and the result was a wall of fog around a
 * volume the whole design is about the size of — you could not see the sector you were flying
 * in, which made "expansive" arrive as "empty". The far plane is now a constant that clears the
 * entire generated space (`FAR_PLANE`), and legibility expresses itself where it belongs: in how
 * far out the *sensor panel* resolves a hostile. A poorly-perceived world is one you fly through
 * blind to what is coming, not one you fly through unable to see.
 *
 * The floor stays, for the same reason it always did: an unperceived world must still tell you
 * something about your immediate surroundings, or "the game did not load" becomes the lesson.
 */
export function sensorFar(range: number): number {
  const MIN = Math.round(EXTENT * 0.05)
  const MAX = Math.round(EXTENT * 0.45)
  return Math.round(MIN + (MAX - MIN) * Math.max(0, Math.min(1, range)))
}

/**
 * One line of HUD, under the same rule as everywhere else in this project.
 *
 * An unmeasured quantity prints an em dash and never a zero. A player told "sensor range 0"
 * concludes their ship is damaged; one told "—" concludes nobody measured it, which is what
 * actually happened.
 */
export function sensorLabel(space: Space): string {
  if (space.sensorRange === null) return '—'
  return `${Math.round(space.sensorRange * 100)}%`
}

/** What the HUD says about the map's edge. */
export function boundaryLabel(space: Space): string {
  return space.unbounded ? 'NO KNOWN BOUNDARY' : 'BOUNDED'
}
