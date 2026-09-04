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
import { COURSE_CLEAR, COURSE_DASHES, FAR_PLANE } from './scale.ts'
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
  | 'ally-shot'
  | 'raider'
  | 'waypoint'
  | 'course'
  | 'self'
  | 'capital'
  | 'courier'
  | 'freighter'
  | 'marshal'

/** RGB in 0..1, for a shader. One table, so a palette change moves every surface at once. */
export const PALETTE: Record<Role, readonly [number, number, number]> = {
  // Node colours are a *legend*, and the legend is the point: a player should be able to answer
  // "where can I refuel" by looking out of the window rather than by opening a map. Fuel is
  // green, trade is red, the places you dock are purple, and everything else — an ordinary
  // station the observer perceived and that sells nothing — is blue.
  //
  // Silhouette still carries it independently (`meshes.ts`), because colour alone fails on a bad
  // monitor, at a glance, and for a colour-blind player. The two agree; neither depends on the
  // other.
  origin: [0.72, 0.45, 1.0],
  station: [0.42, 0.62, 1.0],
  derelict: [0.55, 0.5, 0.42],
  // A marker is where something should be and is not. Deliberately close to the void colour:
  // it should be hard to see, because that is the honest rendering of an absence.
  marker: [0.3, 0.28, 0.36],
  phantom: [0.45, 0.7, 0.95],
  rift: [0.44, 0.4, 0.56],
  // Services are cyan-ward so they read as *infrastructure* rather than as a contact. A
  // player scanning for somewhere to refuel should not have to distinguish a depot from a
  // hostile by shade.
  dock: [0.78, 0.5, 1.0],
  depot: [0.3, 1.0, 0.5],
  market: [1.0, 0.35, 0.38],
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
  // A marshal's fire, in the marshal's own yellow. It exists because the sector's firefights are
  // drawn now rather than resolved in the arithmetic (`enemy.ts::EnemyShot`), and a distant
  // exchange is only legible if the two sides of it are different colours — otherwise a patrol
  // killing a raider a third of a sector away looks exactly like something shooting at you.
  //
  // A friendly round **cannot** hit the player: it carries the id of the one craft it was aimed
  // at. The colour is how that fact reaches the cockpit, and it is why making these visible did
  // not reintroduce the ambiguity that hiding them was avoiding.
  'ally-shot': [1.0, 0.92, 0.45],
  // A raider is not from the record. Orange rather than red keeps that visible from the
  // cockpit: red things are signals somebody reported, orange things are just out here.
  raider: [1.0, 0.55, 0.2],
  // A capital is the same orange family, pushed toward bronze so it reads as a different
  // *weight* of thing rather than a bigger fighter. Silhouette carries the rest.
  capital: [1.0, 0.72, 0.34],
  // Traffic. Blues for the two civilian factions and yellow for the patrol, chosen so that the
  // three of them together are unmistakable against the orange-red the hostiles occupy — at a
  // glance, across a sector, the question "is that coming for me" has to answer itself.
  //
  // Colour is doing real work here and it is still not doing it alone: a courier is a fighter
  // silhouette, a freighter is a gunship silhouette, and a marshal carries a hostile's shape
  // with a friendly hue precisely because it *is* an armed ship — one that is not after you.
  courier: [0.35, 0.85, 1.0],
  freighter: [0.36, 0.55, 0.95],
  marshal: [1.0, 0.9, 0.3],
  // The nav computer's marker. Deliberately the brightest thing in the palette — it is the
  // only body on screen the player put there.
  waypoint: [0.6, 1.0, 0.85],
  // The course line. Green, and the only green in the palette, so it can never be confused with
  // anything the sector contains — a marker the *player* placed must not look like a reading.
  course: [0.3, 1.0, 0.45],
  // Your own hull. Near-white, and the only near-white in the palette: in third person it is on
  // screen every frame, so it has to be the one thing you never mistake for something else.
  self: [0.92, 0.94, 1.0],
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
 * Same arrangement as `PALETTE` and for the same reason: a renderer that decided the fast one was
 * the small triangle would be a second home for the class table, and the two would drift.
 *
 * **Nodes have their own shapes now.** They were shaded spheres, which told you a thing was there
 * and nothing else — a market and a rift were the same ball in different colours, so the whole
 * vocabulary the record carries arrived as a palette. That fails the way colour-only distinctions
 * fail everywhere in this project: on a bad monitor, at a glance, or for a colour-blind player,
 * two shades are one thing. A market is now a hexagonal platform and a rift is a jagged shell
 * around nothing, and neither needs its colour to be read.
 */
export function shapeOf(b: Body): Shape {
  if (b.shape) return b.shape
  if (
    b.role === 'laser' ||
    b.role === 'photon' ||
    b.role === 'enemy-shot' ||
    b.role === 'ally-shot'
  ) {
    return 'bolt'
  }
  // The course rides the bolt pass to get its glow: additive, depth-writes off, so it brightens
  // where it overlaps and is occluded by whatever it passes behind.
  if (b.role === 'course') return 'bolt'
  switch (b.role) {
    case 'origin':
    case 'station':
    case 'market':
    case 'dock':
    case 'depot':
    case 'derelict':
    case 'rift':
    case 'phantom':
    case 'marker':
      return b.role
    case 'self':
      // Never reached: the player's body always carries an explicit `shape` from its hull, and
      // the early return above takes it. Present so the switch is exhaustive rather than relying
      // on a caller always setting the field.
      return 'interceptor'
    default:
      // Contacts and the waypoint keep the sphere. A signal is a *reading*, not a structure, and
      // giving it architecture would claim the observer saw a thing where it counted an event.
      return b.solid ? 'sphere' : 'shell'
  }
}

/**
 * A node's orientation, from its id.
 *
 * Deterministic and cheap. Without it every station ring in the sector faces the same way, which
 * makes a thousand of them read as a printed pattern rather than as a place — and the ring is
 * edge-on from the same direction for all of them, so from one axis the whole sector becomes
 * lines.
 */
export function nodeFacing(id: number): Vec3 {
  // Three coprime-ish moduli, so the three components do not repeat together.
  const x = ((id * 7) % 13) - 6
  const y = ((id * 11) % 9) - 4
  const z = ((id * 5) % 11) - 5 || 3
  const l = Math.hypot(x, y, z) || 1
  return { x: x / l, y: y / l, z: z / l }
}

/**
 * The dashes marking a course.
 *
 * Spacing is a fixed fraction of the distance, so the dashes crowd as they recede and the line
 * reads as a road going away. Size scales with the leg so a course across the sector is visible
 * and one across a docking approach is not a wall of light.
 */
export function course(from: Vec3, to: Vec3): Body[] {
  const dx = to.x - from.x
  const dy = to.y - from.y
  const dz = to.z - from.z
  const dist = Math.hypot(dx, dy, dz)
  if (dist < 1) return []
  const dir = { x: dx / dist, y: dy / dist, z: dz / dist }
  const out: Body[] = []
  // Dash radius from the leg, floored so a short hop still shows one, and capped so a very long
  // one does not put beach balls across the window.
  const radius = Math.max(R_LASER * 0.6, Math.min(R_PHOTON * 1.4, dist / 900))
  for (let i = 1; i <= COURSE_DASHES; i += 1) {
    const t = (i / (COURSE_DASHES + 1)) * COURSE_CLEAR
    out.push({
      at: { x: from.x + dx * t, y: from.y + dy * t, z: from.z + dz * t },
      role: 'course',
      radius,
      solid: true,
      label: 'course',
      facing: dir,
    })
  }
  return out
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

/**
 * Radius for a node, from what it is. Stations are landmarks; markers barely register.
 *
 * Exported because `collide.ts` builds its obstacle grid from exactly these numbers. A hit test
 * that disagrees with the picture is the worst kind of bug in a game — the player flies through
 * something visibly in the way, or bounces off empty space — so there is one table and both the
 * renderer and the physics read it.
 */
export function nodeRadius(role: Role): number {
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
  /**
   * Fire in flight that the player did not send. `owner` is who sent it.
   *
   * Optional, and defaulting to a hostile round, because that is what every caller meant before
   * the sector had visible firefights in it — and because defaulting the *other* way would paint
   * an unlabelled round friendly, which is the one mistake here that could get somebody killed.
   */
  incoming: { at: Vec3; dir: Vec3; owner?: string }[]
  /** Live enemy craft, by contact id, with everything the renderer needs to draw a hull. */
  craft: {
    id: string
    at: Vec3
    solid: boolean
    facing: Vec3
    spec: ClassSpec
    /** Hit flash, decayed by the tick. */
    flash: number
    /** Who it flies for. Drives colour for anything with no contact behind it. */
    faction?: string
  }[]
  /** Contact ids destroyed, so they stop being drawn. */
  destroyed: string[]
  /**
   * The player's own ship.
   *
   * Absent in first person and present in third, which is the whole of the difference as far as
   * this file is concerned: the camera's placement is `camera.ts`'s problem, and what is *in* the
   * world is this one's. Drawing yourself is not a camera feature, it is another body.
   */
  self?: { at: Vec3; facing: Vec3; shape: Shape; radius: number } | null
  /** The nav computer's selected node, drawn as a ring you can steer at. */
  waypoint?: Vec3 | null
  /**
   * Where the ship is, so the course line can start from it.
   *
   * The line is drawn in world space from the ship to the waypoint. Drawing it as a screen-space
   * overlay would be cheaper and would be a different thing: an overlay sits in front of the
   * sector, a course lies *in* it and is occluded by whatever it passes behind — which is what
   * makes it read as a route rather than as a HUD element.
   */
  from?: Vec3 | null
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
      // Every node is hollow now. They are open structures you fly *through* (see `collide.ts`),
      // and a wireframe is the honest rendering of one: a shaded sphere claims a surface where
      // there is a frame. `solid` survives because it still separates a *contact* that was
      // measured from one that was estimated, which is a different claim entirely.
      solid: false,
      label: n.label,
      // A fixed heading, so a station's ring is not edge-on to the same axis everywhere. Derived
      // from the node id rather than a clock or a random draw: two players holding the same
      // record see the same sector, orientations included.
      facing: nodeFacing(n.id),
    }
  })

  // Hostiles are drawn from the swarm, which knows where they have moved to; a hostile that
  // is not in the swarm has been destroyed. Salvage contacts are static and drawn as-is.
  const craftAt = new Map(dyn.craft.map((c) => [c.id, c]))

  /**
   * A live craft, drawn as its class.
   *
   * Shared by the two paths below because there are two: a craft that *has* a contact behind it
   * (a record signal, or a raider) and one that does not (traffic). Traffic was stepped, hunted,
   * shot at and reported on the sensor board — and never once drawn, because this loop iterated
   * the contact lists and traffic is in neither. Exactly the bug that made the projectiles
   * invisible, in a new place: the thing existed everywhere except on screen.
   */
  const hull = (
    live: NonNullable<ReturnType<typeof craftAt.get>>,
    role: Role,
    label: string,
  ): Body => ({
    at: live.at,
    // ## The capital bronze is a *hostile* weight, not a size class
    //
    // It used to apply to anything with `capital: true`, which was harmless while every capital
    // in the sector was hostile. The patrol has war classes of its own now (`classes.ts::warden`,
    // `::bastion`), and painting one bronze would put the sector's largest friendly ship in the
    // hostile family — the single worst thing this palette can get wrong, because the question
    // colour is here to answer is "is that coming for me".
    //
    // Faction wins, and the silhouette carries the weight instead: a warden is a marshal-yellow
    // *dreadnought*, which is legible without the colour and is the rule everywhere else here.
    role: live.spec.capital && (role === 'raider' || role === 'hostile') ? 'capital' : role,
    radius: live.spec.radius,
    solid: !isGhost(role),
    label: `${live.spec.label} ${label}`,
    facing: live.facing,
    flash: live.flash,
    shape: live.spec.shape,
  })

  if (dyn.self) {
    bodies.push({
      at: dyn.self.at,
      role: 'self',
      radius: dyn.self.radius,
      solid: false,
      label: 'you',
      facing: dyn.self.facing,
      shape: dyn.self.shape,
    })
  }

  // Traffic first: everything in the swarm that no contact accounts for.
  const accounted = new Set([...space.contacts, ...space.raiders].map((c) => c.id))
  for (const live of dyn.craft) {
    if (accounted.has(live.id)) continue
    // Faction is the role, so a courier is neon blue and a marshal is yellow — the same table the
    // sensor board reads, so the window and the board can never disagree about who is friendly.
    bodies.push(hull(live, (live.faction ?? 'raider') as Role, ''))
  }

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
      bodies.push(hull(live, role, c.label))
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
    // A marshal's round is yellow and cannot touch you; anything else is red and can. Anything
    // unlabelled is treated as hostile — see the note on `Dynamic.incoming` for why the default
    // falls that way rather than the other.
    const ally = s.owner === 'marshal'
    bodies.push({
      at: s.at,
      role: ally ? 'ally-shot' : 'enemy-shot',
      radius: R_LASER,
      solid: true,
      label: ally ? 'patrol fire' : 'incoming',
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
    // And the course to it, as glowing dashes. They ride the bolt pass, which is additive with
    // depth writes off, so the line brightens where it overlaps itself and is hidden by anything
    // it passes behind — a route lying in the sector rather than an overlay drawn on glass.
    if (dyn.from) bodies.push(...course(dyn.from, dyn.waypoint))
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
