/**
 * One tick of the whole game, as a pure function.
 *
 * The frame loop used to hold flight, combat, enemies and the ship inline, and the result was
 * a bug that made the entire game look broken: shots were created, stepped and resolved, and
 * nothing ever drew them, because the draw list was uploaded once outside the loop. Pulling
 * the tick out here makes the state transition testable without a GPU or a clock — which is
 * the only reason that class of bug is now catchable.
 *
 * Everything is passed in and everything comes back. No refs, no timers, no `Date.now`.
 */

import type { Space, Node, Service, Vec3 } from './generate.ts'
import { servicesOf } from './generate.ts'
import * as Nav from './nav.ts'
import * as Enemy from './enemy.ts'
import type { Swarm } from './enemy.ts'
import * as Weapons from './weapons.ts'
import type { Combat } from './weapons.ts'
import * as Ship from './ship.ts'
import type { Ship as ShipState } from './ship.ts'
import { forward, right, rotate, translate, up, type Camera } from './camera.ts'

import {
  ACCEL_MAIN, AGGRO_RANGE, ASSIST, DOCK_RANGE, EXTENT, JUMP_INHIBIT, R_ORIGIN,
  RATE_PITCH, RATE_ROLL, RATE_YAW, SENSOR_MULTIPLIER, SPIN_ACCEL, SPIN_DAMP, SPEED_THRUST,
} from './scale.ts'
import * as Hyper from './hyper.ts'
import { CLASSES } from './classes.ts'
import { hostileTo } from './factions.ts'
import * as Quests from './quests.ts'
import { DEFAULT_ROLE, bountyScale, hostileToPlayer, type RoleId } from './roles.ts'
import * as Economy from './economy.ts'
import * as Respawn from './respawn.ts'
import * as Arrivals from './arrivals.ts'
import { HULLS, type HullId } from './hulls.ts'
import { trafficOf } from './factions.ts'
import * as Collide from './collide.ts'
import * as Clusters from './clusters.ts'
import * as Fx from './fx.ts'
import * as Dialogue from './dialogue.ts'
import { capsuleOf, nearestOnAxis, segmentDistance } from './hitbox.ts'
import { nodeRadius, roleOfNode } from './view.ts'

export { DOCK_RANGE }

/** How fast a hit flash decays, in units per second. Two or three frames of white. */
const FLASH_DECAY = 5.5

/** How long a HUD notice stays up. Long enough to read once, short enough not to become furniture. */
export const NOTICE_MS = 3_200

/**
 * "Stamp me on the next tick."
 *
 * `useService`, `purchase` and `route` are called from an input handler, between ticks, and have
 * no clock — a single press must not become sixty refuels a second, so they cannot live inside
 * the tick either. Threading a timestamp through three signatures to serve one field would put
 * the burden on every caller; a sentinel puts it in the one place that already has the clock.
 */
const PENDING = -1

/** Keep a fresh notice, or expire an old one. */
function noticeState(
  state: GameState,
  raised: string | null,
  nowMs: number,
): { notice: string | null; noticeAt: number } {
  if (raised) return { notice: raised, noticeAt: nowMs }
  if (!state.notice) return { notice: null, noticeAt: state.noticeAt }
  // Raised between ticks by a key press: this is the first tick that has seen it.
  if (state.noticeAt === PENDING) return { notice: state.notice, noticeAt: nowMs }
  if (nowMs - state.noticeAt < NOTICE_MS) {
    return { notice: state.notice, noticeAt: state.noticeAt }
  }
  return { notice: null, noticeAt: state.noticeAt }
}

/**
 * The obstacle grid, built once per space and cached against it.
 *
 * A `WeakMap` rather than a field on `GameState`, so `tick` keeps its signature and the grid
 * cannot go stale relative to the space it describes — the two are the same object or they are
 * not. Nodes never move, so there is nothing to invalidate.
 *
 * Radii come from `view.ts::nodeRadius`, the same table the renderer draws with. One table, or
 * the player flies through something visibly in the way and bounces off empty space.
 */
const GRIDS = new WeakMap<Space, Collide.Grid>()

export function gridFor(space: Space): Collide.Grid {
  const cached = GRIDS.get(space)
  if (cached) return cached
  const built = Collide.gridOf(space, (n) => nodeRadius(roleOfNode(n)))
  GRIDS.set(space, built)
  return built
}

/**
 * Damage for ramming a craft, per unit of closing speed. Both parties pay.
 *
 * Ramming has to be survivable — a player who dies to one mistimed pass stops flying near
 * anything — and expensive enough that closing to knife range is a decision.
 */
const RAM_DAMAGE = 34

/**
 * How far past the hull a collision keeps counting as one collision, as a multiple of the contact
 * distance.
 *
 * See the ramming loop for the measurement that set it. The number only has to exceed how far a
 * ship at cruise travels in a frame relative to the hull it is scraping along, and 1.6 is
 * comfortably past that at every hull size — the alternative is a sequence of separate collisions
 * with one surface, which is a death sentence rather than a mistake.
 */
const CONTACT_HYSTERESIS = 1.6

/**
 * A stable seed for one burst, from what it happened to and when.
 *
 * The burst's shards are derived from this rather than stored, so it has to be the same number on
 * every machine that reaches this frame — `Math.random` here would make two players watching one
 * explosion see two different explosions, which is the same class of defect as two machines
 * generating different sectors.
 */
function burstSeed(id: string, nowMs: number): number {
  let h = 2166136261 >>> 0
  for (let i = 0; i < id.length; i += 1) {
    h ^= id.charCodeAt(i)
    h = Math.imul(h, 16777619) >>> 0
  }
  h ^= Math.round(nowMs) & 0xffff
  return Math.imul(h, 16777619) >>> 0
}

/**
 * How far the ship starts from the origin node.
 *
 * Outside the market's own hull with room to spare, and inside docking range, so the first thing
 * a new player can do is dock — which is also the first thing they need to learn.
 */
const SPAWN_CLEARANCE = Math.round(R_ORIGIN * 2.4)

/** Held keys, by `KeyboardEvent.code`. */
export type Keys = ReadonlySet<string>

export interface GameState {
  camera: Camera
  /**
   * Commanded throttle, 0..1. **A level, not a button.**
   *
   * The first version accelerated only while a key was held, so the ship was a car with the
   * pedal being tapped. A cruise setting is what a vessel has, and at this sector's size it is
   * the difference between flying and steering: you set 40% and go and do something else.
   */
  throttle: number
  ship: ShipState
  combat: Combat
  swarm: Swarm
  /** Node within docking range, if any. */
  nearby: Node | null
  /** The nav computer's selected node id, or null. Set by `route`. */
  waypoint: number | null
  /** The jump drive. See `hyper.ts` for why it costs what it costs. */
  drive: Hyper.Drive
  /**
   * The player's world velocity this tick, so enemies can lead their shots.
   *
   * Kept on the state rather than recomputed, because the camera is a position and an
   * orientation — there is nowhere else the velocity exists, and passing zero would make every
   * enemy shot aim at where the player *was*, which is a silent way to make the game trivial.
   */
  velocity: Vec3
  /**
   * Angular velocity in the ship's **own** frame: pitch about its right, yaw about its up, roll
   * about its nose. Radians per second.
   *
   * State rather than a per-frame rate, which is the whole fix. Attitude used to be
   * `rotate(camera, key * 1.4 * dt, …)` on all three axes: the ship snapped to a fixed rate while
   * a key was down and stopped dead the instant it came up, and every axis shared one number. So
   * there was no reason to ever roll — yaw turned you just as fast and kept the horizon — and
   * nothing about the ship had any mass. Rates are per-axis now (`RATE_ROLL` ≫ `RATE_PITCH` ≫
   * `RATE_YAW`) and reaching or leaving one takes time.
   */
  spin: Vec3
  /** Hit flash per craft id, 0..1, decayed each tick. The game's whole "you hit it" feedback. */
  flashes: Record<string, number>
  /** Raised when the player takes damage, so the HUD can shake and redden. */
  shake: number
  /**
   * Craft the ship is currently overlapping.
   *
   * A ram is charged on *entry*. Without this the ship inside a leviathan was charged every
   * frame it stayed there and pushed a quarter of a sector on each one — stuck, then dead, which
   * is the reported "you get stuck inside their hurtboxes".
   */
  touching: string[]
  /** Transient line for the HUD — a hit, a refuel, a refusal. */
  notice: string | null
  /**
   * When the current notice was raised, so it can expire.
   *
   * It used to be `notice ?? state.notice`, which never cleared: an impact message from four
   * minutes ago sat under the crosshair for the rest of the session, and a player reading it had
   * no way to tell whether they had just hit something or once had. A stale message is worse
   * than none — it is the interface asserting something that is no longer true.
   */
  noticeAt: number
  /**
   * How many reinforcement waves have been raised, and when the next may be.
   *
   * On the state rather than in a module-level counter, because two records can be open in one
   * session — the fleet view loads several — and a counter outside the state would have one
   * sector's losses reinforcing another's. See `respawn.ts` for what determinism survives.
   */
  waves: Respawn.Waves
  /**
   * The clock the last tick ran at.
   *
   * Carried because `dynamicOf` is a pure projection of the state and has no clock of its own, and
   * a hyperspace entry is the one thing on screen whose appearance is a function of *time* rather
   * than of position. Passing a clock into `dynamicOf` instead would give the renderer a second
   * source of truth about when "now" is, and the two would disagree on any frame the tick was
   * skipped — which is every frame while the game is paused.
   */
  nowMs: number
  /** True once the hull is gone. The sector keeps rendering; you just cannot act. */
  lost: boolean
  /**
   * What the player declared themselves to be. See `roles.ts`.
   *
   * On the state rather than in a module-level variable for the same reason `waves` is: a session
   * can hold two records open, and a role stored outside the state would make one sector's choice
   * govern the other's hostility.
   */
  role: RoleId
  /** Contracts. See `quests.ts`. */
  quests: Quests.QuestState
  /**
   * Live sparks and detonations (`fx.ts`).
   *
   * Origins and start times only — the shards themselves are derived by the renderer from a seed,
   * so a burst costs one small object rather than thirty integrated particles a frame. It is
   * capped (`Fx.MAX_BURSTS`) and swept once a tick, because an uncapped effect list is an
   * unbounded allocation whose symptom is the frame rate rather than anything legible.
   */
  bursts: Fx.Burst[]
  /**
   * The last thing a faction said, and when.
   *
   * One line at a time. A queue would be a chat log, and the moment two ships are talking over
   * each other neither gets read — the same reasoning as `notice`, which this deliberately sits
   * beside rather than inside: a notice is the game telling you something, and chatter is somebody
   * in the sector talking. Conflating them would let a raider's last words overwrite "HULL
   * BREACHED".
   */
  chatter: Dialogue.Line | null
  chatterAt: number
  /** Which cluster the ship is currently inside, so entering one is an event and being in one is not. */
  inCluster: number | null
}

export function newGame(space: Space, role: RoleId = DEFAULT_ROLE): GameState {
  return {
    // Offset from the origin and pointed *away* from it. Two bugs live here and both were found
    // by tests rather than by looking. Spawning at [0,0,0] put the ship inside the origin market,
    // so the moment collision existed every frame was an impact — throttle cut, hull ticking
    // down, and shots blocked at the muzzle by a station the player could not see they were
    // standing in. Moving the ship to +Z then had the camera, which looks along −Z, staring
    // straight into that same station: the first press of the throttle flew into it.
    //
    // So: behind the market, looking out. The station is astern, in docking range, and the whole
    // sector is ahead.
    camera: { position: [0, 0, -SPAWN_CLEARANCE], orientation: [0, 0, 0, 1] },
    throttle: 0,
    ship: Ship.newShip(),
    combat: Weapons.newCombat(),
    // Raiders fly alongside the record's own hostiles and are stepped by the same code; the
    // separation between them is a claim about provenance, not about behaviour. Traffic —
    // couriers, freighters and marshals — joins the same swarm for the same reason: one loop
    // steps everything that flies, so an ally and an enemy cannot drift apart in how they move.
    // Plus the three firefight clusters (`clusters.ts`): a lot of both sides, in one place,
    // already fighting. The scattered roster produces real fights and almost no *findable* ones —
    // eighteen wings and eighteen marshals meeting in ones and twos across twelve extents — so a
    // cluster is the thing the sector was missing: a destination that is not a station. They join
    // the same swarm as everything else, because one loop steps everything that flies.
    swarm: Enemy.withTraffic(
      Enemy.swarmOf(
        [...space.contacts, ...space.raiders, ...Clusters.clusterRaiders(space.seed)],
        space.seed,
      ),
      [...trafficOf(space, space.seed), ...Clusters.clusterMarshals(space.seed)],
    ),
    nearby: null,
    noticeAt: -1e9,
    waypoint: null,
    drive: Hyper.IDLE,
    velocity: { x: 0, y: 0, z: 0 },
    spin: { x: 0, y: 0, z: 0 },
    flashes: {},
    shake: 0,
    touching: [],
    notice: null,
    waves: Respawn.newWaves(),
    nowMs: 0,
    lost: false,
    role,
    // **One contract, already accepted.** A new pilot used to spawn with an empty board and no way
    // to learn there was one: contracts live at citadels, citadels are scattered over twelve
    // extents, and the panel that names them is one you have to be docked at to read. See
    // `quests.ts::opening` — it obeys every rule the citadel boards obey, and is deliberately the
    // smallest job of its kind.
    quests: Quests.openingState(space.seed, role, space.nodes),
    bursts: [],
    chatter: null,
    chatterAt: -1e9,
    inCluster: null,
  }
}

function v3(a: readonly [number, number, number]): Vec3 {
  return { x: a[0], y: a[1], z: a[2] }
}

function dist(a: Vec3, b: Vec3): number {
  return Math.hypot(a.x - b.x, a.y - b.y, a.z - b.z)
}

/**
 * How close this hull has to be to a node to use its services.
 *
 * **Measured from the hull, never from its centre**, which is why it is a function of the frame
 * rather than the bare `DOCK_RANGE` constant. A dominion has a radius of 0.135 extents against a
 * docking range of 0.075: with a centre-to-centre test the ship would have to swallow the station
 * whole to dock with it, and the panel would report `nothing in range` while the structure was
 * visibly inside the hull. That is the same failure as the original refuelling bug — a dockable
 * shell the ship could not occupy — arriving from the other direction.
 *
 * The invariant `check:scemaworld` pins therefore generalises rather than moving: the dockable
 * band clears the largest node for **every** hull, not just the stock one.
 */
export function dockRange(frame: HullId): number {
  return DOCK_RANGE + Ship.hullRadius(frame)
}

/** Nearest node within this hull's docking range. */
export function nearestService(space: Space, at: Vec3, frame: HullId = 'skiff'): Node | null {
  let best: Node | null = null
  let bestD = dockRange(frame)
  for (const n of space.nodes) {
    // Axis rejects before the square root. This runs over a thousand nodes every frame, and a
    // `Math.hypot` per node was most of what it cost.
    if (Math.abs(n.at.x - at.x) > bestD) continue
    if (Math.abs(n.at.y - at.y) > bestD) continue
    if (Math.abs(n.at.z - at.z) > bestD) continue
    if (servicesOf(n.kind).length === 0) continue
    const d = dist(n.at, at)
    if (d < bestD) {
      bestD = d
      best = n
    }
  }
  return best
}

export interface TickInput {
  keys: Keys
  firing: boolean
  dt: number
  nowMs: number
}

/**
 * Advance everything by `dt`.
 *
 * Order matters and is fixed: rotate, then throttle, then burn, then move, then shoot, then
 * let the enemy act. Moving before burning would let a dry ship travel one free frame; letting
 * the enemy act first would let it shoot a player who has already died this tick.
 */
/**
 * Move an angular velocity toward a commanded one, one axis at a time.
 *
 * Two rates, not one, and that asymmetry is the feel of the ship. Pushing *toward* a commanded
 * rate uses `SPIN_ACCEL`; coasting back to rest with the stick centred uses the gentler
 * `SPIN_DAMP`, so a released control bleeds off over a moment instead of stopping the ship dead.
 *
 * Clamped so a step can never overshoot its target — at a large `dt` (an alt-tabbed tab handing
 * back a half-second frame) a plain `v += a * dt` sails past the commanded rate and the ship
 * snaps into a spin the player never asked for.
 */
function approach(cur: Vec3, cmd: Vec3, dt: number, agility: number): Vec3 {
  const step = (c: number, want: number): number => {
    // Scaled by the hull, exactly as the commanded rates are. Scaling only the peak gives a ship
    // that snaps instantly into a slow rotation, which reads as a bug rather than as mass — the
    // felt weight of a capital is in how long it takes to *start* turning at all.
    const rate = (want === 0 ? SPIN_DAMP : SPIN_ACCEL) * agility
    const d = want - c
    const max = rate * dt
    if (Math.abs(d) <= max) return want
    return c + Math.sign(d) * max
  }
  return { x: step(cur.x, cmd.x), y: step(cur.y, cmd.y), z: step(cur.z, cmd.z) }
}

export function tick(state: GameState, space: Space, input: TickInput): GameState {
  const { keys, firing, dt, nowMs } = input
  if (state.lost) return state
  const was = state.camera.position

  // ── attitude ───────────────────────────────────────────────────────────────
  //
  // A commanded *rate* per axis, approached over time rather than snapped to. Holding a key spins
  // the ship up toward that axis's peak; releasing it lets the rotation bleed off. `SPIN_DAMP` is
  // lower than `SPIN_ACCEL`, so the ship carries its turn for a moment after the stick is
  // centred — which is the difference between flying something and pointing a camera.
  let camera = state.camera
  //
  // Every rate is multiplied by the hull's `agility`, which is the stat that makes a tier feel
  // like a tier. It cannot be top speed: `scale.ts` and `classes.ts` both pin that every hull
  // outruns every hostile, because disengaging has to stay possible. So what a heavy hull pays is
  // the delay between deciding to point somewhere and pointing there — which is also the axis the
  // whole dogfight model turns on, so a fighter that stays out of a capital's arc is a real
  // problem for the largest ship in the game.
  const agility = Ship.agilityOf(state.ship.frame)
  const cmd = {
    x: ((keys.has('KeyS') ? 1 : 0) - (keys.has('KeyW') ? 1 : 0)) * RATE_PITCH * agility,
    y: ((keys.has('KeyA') ? 1 : 0) - (keys.has('KeyD') ? 1 : 0)) * RATE_YAW * agility,
    z: ((keys.has('KeyQ') ? 1 : 0) - (keys.has('KeyE') ? 1 : 0)) * RATE_ROLL * agility,
  }
  const spin = approach(state.spin, cmd, dt, agility)
  if (spin.x || spin.y || spin.z) {
    camera = rotate(camera, spin.x * dt, spin.y * dt, spin.z * dt)
  }

  // ── throttle as a level ────────────────────────────────────────────────────
  const trim = (keys.has('ArrowUp') ? 1 : 0) - (keys.has('ArrowDown') ? 1 : 0)
  let throttle = state.throttle
  if (trim) throttle = Math.max(0, Math.min(1, throttle + trim * dt * 0.9))
  if (keys.has('KeyX')) throttle = 0 // full stop, because 0.9/s from cruise is a long wait

  // ── fuel, then movement ────────────────────────────────────────────────────
  let ship = state.ship
  let notice: string | null = null
  const wantsThrust = throttle > 0
  const dry = !Ship.hasFuel(ship)
  const effective = dry ? 0 : throttle
  if (wantsThrust && !dry) ship = Ship.burn(ship, dt, throttle)
  if (wantsThrust && dry) notice = 'tanks dry — find a depot'

  // ── momentum ───────────────────────────────────────────────────────────────
  //
  // Velocity is state. Thrust changes it, it carries the ship, and the ship's nose and its course
  // are two different things. Position used to be `translate(camera, [0, 0, -speed * dt])` — the
  // ship was wherever it was pointing, instantly, so turning did not cost anything and there was
  // no such thing as drifting wide off a hard reversal.
  //
  // `ASSIST` is what keeps that flyable: it drags velocity onto the nose so a turn is *followed*
  // rather than merely watched. See `scale.ts` for why full Newtonian flight is a worse game.
  const noseV = forward(camera)
  const rightV = right(camera)
  const upV = up(camera)
  const top = Ship.limits(ship).speed
  let vel = state.velocity

  if (effective > 0) {
    vel = {
      x: vel.x + noseV[0] * ACCEL_MAIN * effective * dt,
      y: vel.y + noseV[1] * ACCEL_MAIN * effective * dt,
      z: vel.z + noseV[2] * ACCEL_MAIN * effective * dt,
    }
  }
  // Lateral thrusters are free of the main drive and cost nothing: they are for docking, and
  // a player unable to line up on a depot because they ran dry is stuck forever.
  const strafe = (keys.has('ArrowRight') ? 1 : 0) - (keys.has('ArrowLeft') ? 1 : 0)
  const lift = (keys.has('Space') ? 1 : 0) - (keys.has('ShiftLeft') ? 1 : 0)
  if (strafe || lift) {
    const a = SPEED_THRUST * dt * 2
    vel = {
      x: vel.x + (rightV[0] * strafe + upV[0] * lift) * a,
      y: vel.y + (rightV[1] * strafe + upV[1] * lift) * a,
      z: vel.z + (rightV[2] * strafe + upV[2] * lift) * a,
    }
  }
  // The assist, and the one condition on it: **a dry ship keeps its momentum.** Damping without
  // fuel would let a stranded player brake to a stop for free, which is both wrong and the least
  // interesting reading of running out of fuel.
  if (!dry) {
    const want = {
      x: noseV[0] * top * effective,
      y: noseV[1] * top * effective,
      z: noseV[2] * top * effective,
    }
    const k = Math.min(1, ASSIST * dt)
    vel = {
      x: vel.x + (want.x - vel.x) * k,
      y: vel.y + (want.y - vel.y) * k,
      z: vel.z + (want.z - vel.z) * k,
    }
  }
  // Speed ceiling. Applied to the magnitude rather than per axis, or the cap would be higher on a
  // diagonal than along an axis and the fastest way anywhere would be a corner of the box.
  const sp = Math.hypot(vel.x, vel.y, vel.z)
  if (sp > top) {
    const f = top / sp
    vel = { x: vel.x * f, y: vel.y * f, z: vel.z * f }
  }
  if (vel.x || vel.y || vel.z) {
    camera = {
      ...camera,
      position: [
        camera.position[0] + vel.x * dt,
        camera.position[1] + vel.y * dt,
        camera.position[2] + vel.z * dt,
      ],
    }
  }

  // ── flying through the furniture ───────────────────────────────────────────
  // Nodes do not block. They are open structures at a scale where solid ones would be a maze
  // rather than scenery — a sector whose landmarks are also walls is one where the interesting
  // thing about a market is that it is in the way.
  //
  // What they do is *register*. Passing through something the observer perceived puts it on the
  // sensors; passing through a phantom, a marker or a rift puts nothing there and says so. Same
  // claim the solid/permeable split used to carry, expressed as a reading rather than a wall —
  // and a better home for it, since a wall is a fact about the world and a sensor return is a
  // fact about what somebody knows.
  const grid = gridFor(space)
  // The hull's own radius, not the stock constant. A capital touches structures a fighter passes
  // beside, and that is the point of flying one.
  const selfR = Ship.hullRadius(state.ship.frame)
  const crossed = Collide.crossed(space, v3(was), v3(camera.position), selfR)
  if (crossed) notice = Collide.passageNote(crossed.kind, crossed.label)

  // ── the jump drive ─────────────────────────────────────────────────────────
  // Resolved before weapons so a jump that lands this frame puts the ship at its destination
  // before anything is aimed from it — otherwise the first frame after arrival fires from the
  // old position, which reads as a shot coming out of nowhere.
  const threat = Enemy.nearestThreat(state.swarm, v3(camera.position), state.role)
  const waypointNode =
    state.waypoint === null ? null : (space.nodes.find((n) => n.id === state.waypoint) ?? null)
  const jump = Hyper.advance(
    state.drive,
    {
      threat: threat ? threat.range : null,
      charges: ship.jumpFuel,
      driveLevel: ship.levels.drive,
      waypoint: state.waypoint,
    },
    waypointNode,
    keys.has('KeyJ'),
    dt,
  )
  if (jump.arriveAt) {
    camera = { ...camera, position: [jump.arriveAt.x, jump.arriveAt.y, jump.arriveAt.z] }
    // Arriving at rest. A jump that preserved momentum would drop you into a station at a
    // tenth of the sector per second, which is a collision the game has no answer for.
    throttle = 0
  }
  if (jump.spent) ship = { ...ship, jumpFuel: ship.jumpFuel - 1 }
  if (jump.notice) notice = jump.notice

  const at = v3(camera.position)
  const nose = v3(forward(camera))
  // The ship's real velocity, which enemies lead off. It is integrated state now rather than a
  // position delta — the delta also picked up collision push-outs and jump translations, so a
  // ram used to hand every gunner in the sector a lead solution built from a teleport.
  //
  // A jump ends with the ship stationary: arriving at a hundredth of a sector per second and
  // ploughing on through the destination is not an arrival.
  let velocity: Vec3 = jump.arriveAt ? { x: 0, y: 0, z: 0 } : vel

  // ── the player's weapons ───────────────────────────────────────────────────
  let combat = state.combat
  // ## The target list is built from the **swarm**, not from the record's contact lists
  //
  // It used to be the other way round — filter `space.contacts` and `space.raiders` down to those
  // still alive — and that quietly made reinforcement impossible: a wing raised by `respawn.ts`
  // is not in either list, because `Space` is a pure function of the record and must not be
  // mutated to hold ships the record never described. Such a raider was drawn, would shoot at you,
  // was counted on the sensor board, and could not be hit by anything. The swarm is the authority
  // on what exists; the record is looked up only for what a *signal* claimed about it.
  //
  // Only hostiles. Weapons no longer adjudicates death — it reports damage and the swarm decides —
  // so an inert salvage contact in this list would produce hits that nothing consumes, and traffic
  // in it would let a stray round start a war with the patrol.
  const contactById = new Map([...space.contacts, ...space.raiders].map((c) => [c.id, c]))
  const moved = Enemy.living(state.swarm)
    // Everything the player's own weapons may resolve against. Role-dependent: a pirate's rounds
    // have to be able to reach a marshal, and a bounty hunter's must not start a war with one.
    .filter((k) => hostileToPlayer(k.faction, state.role))
    .map((k) => {
      const c = contactById.get(k.id)
      return {
        id: k.id,
        at: k.at,
        hostility: 'hostile' as const,
        // Provenance comes from the contact when there is one. A reinforcement has none, and is
        // solid: the sector knows it is there, exactly as `raiders.ts` says of a placed raider.
        // Defaulting it to `false` would invent a ghost — a craft claiming somebody estimated it,
        // when nobody reported it at all.
        solid: c ? c.solid : true,
        // Only ever read as a fallback size, and `radius` below always supersedes it here. A
        // reinforcement has no magnitude because nobody counted anything about it — which is a
        // different statement from a signal measured at zero, and the reason nothing on screen
        // is allowed to render this number for an unlogged craft.
        magnitude: c ? c.magnitude : 0,
        label: c ? c.label : 'RAIDER',
        unlogged: c ? c.unlogged : true,
        // The craft's *class* radius, facing and silhouette travel with it, so the hit test is the
        // hull the renderer draws rather than a sphere around it. A sphere misses the prow and the
        // stern of anything long, and the bigger the ship the worse it gets — a leviathan once had
        // a ten-million-unit hitbox inside a seven-hundred-million-unit silhouette.
        radius: k.spec.radius,
        facing: k.facing,
        shape: k.spec.shape,
      }
    })

  // Fired from the ship's **nose**, not from the camera. In third person the camera sits behind
  // and above, so a shot spawned at the lens starts inside your own hull and appears to come out
  // of the middle of the screen rather than out of the guns. The offset is the hull's own length,
  // so a marauder's rounds leave a marauder's prow.
  const muzzle = {
    x: at.x + nose.x * Ship.noseOffset(ship.frame),
    y: at.y + nose.y * Ship.noseOffset(ship.frame),
    z: at.z + nose.z * Ship.noseOffset(ship.frame),
  }
  if (firing) combat = Weapons.fire(combat, muzzle, nose, nowMs, moved, ship.levels, ship.frame)
  // No geometry blocks a shot any more: a wireframe frame is not cover. The seam stays because
  // the alternative is deleting a parameter that a future obstacle would have to reintroduce.
  const advanced = Weapons.step(combat, dt, moved, space.seed)
  combat = advanced.combat

  // Flashes decay every tick and are re-lit by a hit. Decaying first means a hit landing this
  // frame is at full brightness rather than one frame stale.
  const flashes: Record<string, number> = {}
  for (const [id, v] of Object.entries(state.flashes)) {
    const next = v - FLASH_DECAY * dt
    if (next > 0) flashes[id] = next
  }

  let swarm = state.swarm
  let quests = state.quests

  // Sparks, detonations and chatter. Declared here because the hit loop below is the first thing
  // that raises any of them, and a `let` used before its declaration is a runtime error rather
  // than a hoisting nicety.
  //
  // Bursts age out once a tick, so the list cannot grow without bound; `Fx.add` caps it as well,
  // because a cluster firefight resolves dozens of hits a second and a sweep alone is not a bound.
  let bursts = Fx.live(state.bursts ?? [], nowMs)
  let chatter = state.chatter ?? null
  let chatterAt = state.chatterAt ?? -1e9
  const speak = (
    id: string,
    faction: string,
    spec: (typeof CLASSES)[keyof typeof CLASSES],
    beat: Dialogue.Beat,
  ) => {
    const line = Dialogue.say(space.seed, id, faction as never, spec, beat)
    if (!line) return
    chatter = line
    chatterAt = nowMs
  }

  for (const h of advanced.hits) {
    // Looked up **before** the hit resolves, because a kill removes it and a burst has to be
    // drawn where the ship was rather than where nothing is.
    const target = Enemy.living(swarm).find((c) => c.id === h.contact) ?? null
    const res = Enemy.hit(swarm, h.contact, h.damage, nowMs)
    swarm = res.swarm
    if (target) {
      // Shield or hull, and the two look different on purpose. It is the same distinction the
      // flash below carries and the only cue telling a player whether they are making progress or
      // wasting rounds on a buffer — a burst that looked identical either way would undo it.
      // **A photon always detonates.** It is one decisive round out of a magazine a pilot counts,
      // and giving it the same spark a laser bolt gets would make the most expensive thing you can
      // fire the least legible. The weapon says so itself (`h.kind`) rather than being inferred
      // from the damage figure — a `damage > 500` threshold would have silently stopped meaning
      // anything the moment `PHOTON.damage` tripled, which it just did.
      bursts = Fx.add(bursts, {
        at: target.at,
        kind: h.kind === 'photon' ? 'detonation' : res.throughShield ? 'hull' : 'shield',
        startedMs: nowMs,
        seed: burstSeed(h.contact, nowMs),
      })
      // A capital says something the first time its shields fail. Only a capital: on a fighter
      // that is one hit out of a handful and the line would fire constantly.
      if (res.throughShield && target.spec.capital && target.shield <= 0) {
        speak(h.contact, target.faction, target.spec, 'broken')
      }
    }
    // A hit on hull flashes harder than one soaked by a shield. That is the only cue telling a
    // player whether they are making progress or wasting rounds on a buffer, and without it a
    // heavily-shielded gunship reads as invulnerable.
    flashes[h.contact] = res.throughShield ? 1 : 0.45
    if (res.killed) {
      // Paid only for what the role hunts — see `roles.ts`. The kill still happens either way;
      // a pirate can shoot a raider, it simply is not work anybody commissioned.
      const faction = res.faction ?? 'raider'
      const paid = Math.round(res.bounty * bountyScale(faction, state.role))
      ship = Ship.bounty(ship, paid)
      // `combat.destroyed` is what stops it being drawn. Written here rather than in
      // `weapons.ts` for the same reason the damage is: one authority over one fact.
      combat = { ...combat, destroyed: [...combat.destroyed, h.contact] }
      if (target) {
        bursts = Fx.add(bursts, {
          at: target.at,
          kind: 'detonation',
          startedMs: nowMs,
          seed: burstSeed(`${h.contact}:dead`, nowMs),
        })
        speak(h.contact, target.faction, target.spec, 'destroyed')
      }

      // A contract advances on the **act**, not on a survey of the world. See `quests.ts`.
      const adv = Quests.recordKill(quests, faction, res.capital)
      quests = adv.state
      if (adv.completed) {
        ship = Ship.bounty(ship, adv.completed.reward)
        notice = `contract complete — +${adv.completed.reward} salvage`
      } else {
        notice = paid > 0 ? `destroyed — +${paid} salvage` : 'destroyed'
      }
    }
  }

  // ── the enemy's turn ───────────────────────────────────────────────────────
  // Craft still avoid nodes even though they cannot hit them. A wing flying *through* a station
  // ring is technically correct and looks like the geometry is decorative; steering round one
  // costs nothing and reads as piloting.
  // The role reaches the AI, not just the sensor board. Without it a pirate is hunted by the
  // raiders who count them as one of their own and ignored by the patrol they are paid to fight.
  const enemyStep = Enemy.step(swarm, at, velocity, dt, nowMs, grid, space, state.role, selfR)
  swarm = enemyStep.swarm
  let shake = Math.max(0, state.shake - dt * 2.2)
  if (enemyStep.damage > 0) {
    const before = ship.hull
    ship = Ship.damage(ship, enemyStep.damage, nowMs)
    // The screen kicks harder when the hit reached hull. Same reasoning as the enemy flash:
    // shields absorbing and hull being opened must not feel identical.
    shake = Math.min(1, shake + (ship.hull < before ? 0.85 : 0.35))
    notice = ship.hull < before ? 'HULL BREACHED' : 'shields holding'
    // On your own hull, at the nose rather than at the camera — the same reason shots leave the
    // muzzle. A burst at the lens is a flash in the middle of the screen with no location in it.
    bursts = Fx.add(bursts, {
      at: muzzle,
      kind: ship.hull < before ? 'hull' : 'shield',
      startedMs: nowMs,
      seed: burstSeed('self', nowMs),
    })
  }
  // ── ramming, against the hull that is drawn ────────────────────────────────
  // Both parties pay, and the player is pushed clear. A craft you can fly through is a craft that
  // is not there, and at these closing speeds an interceptor crossing your nose ought to be an
  // event rather than a texture.
  //
  // ## A capital is solid now, and the sphere was why it was not
  //
  // The reported symptom was flying straight through a dreadnought, and the cause was two
  // approximations compounding. A craft was collided as a **sphere of its class radius**, which is
  // wrong for these hulls in both directions at once: the `dreadnought` mesh reaches 2.1 along its
  // nose and 0.72 across, so a sphere misses the whole prow and the whole stern while claiming a
  // great deal of empty space beside the ship. And because that sphere is enormously too wide —
  // a leviathan's is a quarter of a sector — flying into one used to trap the ship inside a
  // hurtbox that re-collided every frame, so the fix at the time was to shrink it to `CAPITAL_CORE`,
  // 22% of the drawn radius. Which made it *narrower than the hull*: the ship passed through
  // everything you can see and touched an invisible ball somewhere near the middle.
  //
  // The same fix `weapons.ts` already had is the fix here. **Collide against the capsule the
  // renderer draws** (`hitbox.ts`, whose extents are measured from the vertex data rather than
  // declared) and the two problems dissolve together: the hull is solid along its whole length,
  // and the empty volume beside it is not. This is the project's own rule — *a hit test must use
  // the radius the renderer draws* — applied to the one collision that had been exempted from it.
  //
  // **Swept**, not an endpoint test, for the same reason a shot is: a ship crosses a substantial
  // fraction of a hull length in one frame and an endpoint test tunnels straight through a prow.
  //
  // ## What the capsule preserves that the core was protecting
  //
  // Getting *inside* a capital's turret envelope is the counterplay that makes one beatable
  // (`classes.ts::TURRET_MIN_RANGE`, 1.3 radii), and a solid hull could have deleted it. It does
  // not, but the margin is thinner than it looks and is worth stating rather than assuming: the
  // war hull's capsule measures **1.05 radii** across — the sponsons reach further than the plating
  // does, which is why the extents are measured from the vertex data and never declared — so the
  // shell where the guns cannot depress and the ship is not inside anything runs from 1.05 to 1.3
  // radii. Narrow, and real. Hugging the hull is now literally hugging the hull.
  //
  // The trapping that the old 22% core was introduced to prevent cannot recur, because the volume
  // being pushed out of is a hull rather than a sphere the size of a region.
  let rammed: string | null = null
  const touching: string[] = []
  // ## Charge once a frame, but **finish the scan**
  //
  // This loop used to `break` the moment it charged, and with a small sphere per craft that was
  // harmless: at most one thing was ever in contact. Against hulls it is a bug with a spectacular
  // symptom. `touching` is built *by this loop*, so breaking early means every craft after the one
  // that charged is never recorded as being in contact — and next frame it is therefore "new" and
  // charges, and breaks, and the frame after that the next one does. A ship sitting among four
  // overlapping capitals cycles through them forever: measured at **202 charges in 300 frames**,
  // each one a different hull wearing the same label, which is why it read as one capital charging
  // over and over.
  //
  // So the charge is capped at one per frame — the push-out can only put the ship in one place, and
  // two pushes in a frame fight each other — while the scan always runs to completion.
  let charged = false
  for (const c of Enemy.living(swarm)) {
    // Facing rather than velocity: a stationary capital still points somewhere, and a zero-length
    // facing would collapse the capsule onto its own centre — which is the sphere again.
    const cap = capsuleOf(c.at, c.facing, c.spec.radius, c.spec.shape)
    const touch = selfR + cap.radius
    // Swept against the axis. `was` is where the ship started the frame.
    const struck = segmentDistance(v3(was), at, cap.tail, cap.head) < touch
    // ## Contact is sticky, and it has to be
    //
    // "Charged on entry, not while overlapping" is enforced by `state.touching`, and with a sphere
    // that was enough: the push-out cleared a small ball and a ship at cruise was gone from it in
    // one frame. Against a **hull** it is not. A capsule is long, the push is perpendicular to the
    // axis, and a ship under thrust drives straight back into the flank on the next frame — so the
    // ship leaves contact and re-enters it every second frame, and every re-entry is a fresh
    // charge. Measured: a ship parked inside one dreadnought was charged **133 times in 300
    // frames**, which is the exact failure the entry rule exists to prevent, arriving through a
    // door the rule did not cover.
    //
    // So contact persists out to a band well past the hull, and only a genuine crossing of the
    // surface charges. Grinding along a war hull is one collision with a long tail, which is both
    // the honest reading and the one that does not kill somebody for a mistake they made once.
    const near = nearestOnAxis(cap, at)
    const inBand = near.distance < touch * CONTACT_HYSTERESIS
    if (!struck && !inBand) continue
    touching.push(c.id)
    // Already in contact last frame: no second charge, no second push. Flying out is the player's
    // problem and takes as long as it takes.
    if (state.touching.includes(c.id)) continue
    // In the band but not actually through the surface — nothing to charge for.
    if (!struck) continue
    // One charge a frame, and the scan continues regardless — see the note above the loop.
    if (charged) continue
    charged = true

    const closing = Math.hypot(velocity.x, velocity.y, velocity.z) + c.speed
    // Scaled against the *stock* top speed, not this hull's. A marauder is slower, and charging
    // it less for the same collision would make the heaviest ship the safest to fly into things
    // with — a hull's armour already covers that, and doing it twice is how a stat becomes a
    // dominant strategy.
    const cost = Math.max(1, Math.round((closing / Ship.topSpeed(0)) * RAM_DAMAGE))
    ship = Ship.damage(ship, cost, nowMs)
    const res = Enemy.hit(swarm, c.id, cost, nowMs)
    swarm = res.swarm
    if (res.killed) {
      // **Paid only for what the role hunts.** `bountyScale` is 0 for everything else, which is
      // how "the game never pays you to make the sector less policed" survives a pirate role —
      // see `roles.ts`. A ram still kills; it just does not always pay.
      ship = Ship.bounty(ship, Math.round(res.bounty * bountyScale(c.faction, state.role)))
      combat = { ...combat, destroyed: [...combat.destroyed, c.id] }
      speak(c.id, c.faction, c.spec, 'destroyed')
    }
    flashes[c.id] = 1
    shake = Math.min(1, shake + 0.9)
    bursts = Fx.add(bursts, {
      at: near.at,
      kind: 'detonation',
      startedMs: nowMs,
      seed: burstSeed(`${c.id}:ram`, nowMs),
    })
    // Pushed out from the nearest point **on the hull's axis**, not from the craft's centre. On a
    // ship four times longer than it is wide those are different places by most of a hull length,
    // and using the centre shoves somebody who clipped the prow sideways out of the middle of the
    // ship, in a direction they were nowhere near.
    const gap = near.distance
    const dir = gap < 1e-6 ? { x: 1, y: 0, z: 0 } : {
      x: (at.x - near.at.x) / gap,
      y: (at.y - near.at.y) / gap,
      z: (at.z - near.at.z) / gap,
    }
    const out = touch * 1.02
    camera = {
      ...camera,
      position: [near.at.x + dir.x * out, near.at.y + dir.y * out, near.at.z + dir.z * out],
    }
    // Kill the part of the velocity that was carrying the ship *into* the thing it just hit.
    // Nudging the position clear while leaving the momentum pointed inward flies straight back in
    // on the next tick — and since a second entry is not charged while `touching`, the result is a
    // ship pinned inside a hull it is being told it has already left.
    const into = velocity.x * dir.x + velocity.y * dir.y + velocity.z * dir.z
    if (into < 0) {
      velocity = {
        x: velocity.x - dir.x * into,
        y: velocity.y - dir.y * into,
        z: velocity.z - dir.z * into,
      }
    }
    // The drive is cut for a fighter-sized impact only. Cutting it inside a capital would strand
    // the ship in the one place it most needs to be able to leave.
    if (!c.spec.capital) throttle = 0
    rammed = `collision — ${c.spec.label} (−${cost})`
  }
  if (rammed) notice = rammed

  ship = Ship.recharge(ship, dt, nowMs)

  // ── keeping the sector populated ───────────────────────────────────────────
  // Last, so a wave raised this frame is not stepped, shot at or collided with until the next
  // one — a craft that appears and immediately acts has skipped the frame in which the player
  // could have seen it arrive. Both floors are constants and neither reads the record; see
  // `respawn.ts` for why that is a rule rather than a preference.
  const topUp = Respawn.replenish(
    swarm,
    space,
    space.seed,
    state.waves,
    v3(camera.position),
    nose,
    nowMs,
  )
  swarm = topUp.swarm
  // A reinforcement notice never displaces one the player caused. A kill, an impact or a refusal
  // is about something they just did; "a wing is on sensors" can wait for a quiet frame, and
  // overwriting the former with the latter is how feedback stops being trusted.
  if (!notice) notice = topUp.notice

  // ## Entering a standing battle
  //
  // Keyed on the cluster index *changing*, exactly as the docking beat is keyed on `nearby`
  // changing: a line that fired while you were inside would repeat sixty times a second, and one
  // that fired on proximity rather than entry would fire again every time you drifted across the
  // boundary. Whoever is nearest speaks, because a battle is announced by somebody in it.
  const anchors = Clusters.clusterAnchors(space.seed)
  const reach = AGGRO_RANGE * Clusters.CLUSTER_SPREAD
  let inCluster: number | null = null
  for (let i = 0; i < anchors.length; i += 1) {
    const a2 = anchors[i]
    if (Math.hypot(at.x - a2.x, at.y - a2.y, at.z - a2.z) < reach) {
      inCluster = i
      break
    }
  }
  if (inCluster !== null && inCluster !== (state.inCluster ?? null)) {
    const near2 = Enemy.living(swarm)
      .filter((c) => Clusters.clusterOf(c.id) === inCluster)
      .sort(
        (p, q) =>
          Math.hypot(p.at.x - at.x, p.at.y - at.y, p.at.z - at.z) -
          Math.hypot(q.at.x - at.x, q.at.y - at.y, q.at.z - at.z),
      )[0]
    if (near2) speak(near2.id, near2.faction, near2.spec, 'cluster')
  }

  const nearby = nearestService(space, v3(camera.position), state.ship.frame)

  // ## A haul advances on arrival, not on proximity
  //
  // Keyed on `nearby` **changing**, so holding station inside a node's docking shell does not
  // re-trigger it, and on the same `DOCK_RANGE` the services use — a contract that could be
  // completed by flying past would make the delivery half of the trader role a formality.
  if (nearby && nearby.id !== state.nearby?.id) {
    const arrived = Quests.recordDock(quests, nearby.id)
    quests = arrived.state
    if (arrived.completed) {
      ship = Ship.bounty(ship, arrived.completed.reward)
      notice = `contract complete — +${arrived.completed.reward} salvage`
    } else if (quests.active?.picked && !state.quests.active?.picked) {
      notice = 'cargo aboard'
    }
  }

  return {
    camera,
    throttle,
    ship,
    combat,
    role: state.role,
    quests,
    swarm,
    waves: topUp.waves,
    nowMs,
    nearby,
    waypoint: state.waypoint,
    drive: jump.drive,
    velocity,
    // A jump ends the tumble too: arriving mid-roll from a drive that is supposed to have set the
    // ship down somewhere is disorienting for no reason anybody chose.
    spin: jump.arriveAt ? { x: 0, y: 0, z: 0 } : spin,
    flashes,
    shake,
    touching,
    bursts,
    chatter,
    chatterAt,
    inCluster,
    ...noticeState(state, notice, nowMs),
    lost: Ship.destroyed(ship),
  }
}

/** Use a service at the node you are next to. Returns the state and a message. */
export function useService(
  state: GameState,
  service: 'refuel' | 'repair' | 'trade' | 'scavenge',
): GameState {
  const node = state.nearby
  if (!node) return { ...state, noticeAt: PENDING, notice: 'nothing in range' }
  if (!servicesOf(node.kind).includes(service)) {
    // A phantom is the interesting refusal: it looks like a station and offers nothing,
    // because the observer modelled it rather than saw it.
    return {
      ...state,
      notice:
        node.kind === 'phantom'
          ? 'that station was simulated, not observed — there is nothing there'
          : `${node.label} does not offer ${service}`,
    }
  }
  const atDock = node.kind === 'dock'
  const res =
    service === 'refuel'
      ? // A dock charges the jump drive too; a depot does thrust fuel only. Six times as many
        // depots as docks is what makes a jump charge worth planning around.
        Ship.refuel(state.ship, atDock)
      : service === 'repair'
        ? Ship.repair(state.ship)
        : service === 'scavenge'
          ? Ship.scavenge(state.ship, node.id)
          : { ship: state.ship, message: 'trading', ok: true }

  // ## A dock rearms the photon tubes, and that is a consequence of the buff rather than a perk
  //
  // A magazine of one to six (`hulls.ts::tubes`) is only a decision if it can be refilled. Left
  // unreplenishable it is a weapon you fire once per session and then forget the game has, which
  // is exactly the fate the old twelve-round pea-shooter had for the opposite reason. Docks
  // rather than depots, and folded into refuel rather than given a key of its own: a dock is
  // already where the scarce things are topped up, and the sector has six times as many depots,
  // so *where you can rearm* stays a real constraint on a route.
  //
  // Reported separately from the refuel verdict, because "tanks already full" and "tubes loaded"
  // are two facts and a player who is told only the first will conclude nothing happened.
  const rearmed =
    service === 'refuel' &&
    atDock &&
    state.combat.photonsLeft < Weapons.photonMagazine(state.ship.frame)
  const combat = rearmed ? Weapons.reload(state.combat, state.ship.frame) : state.combat
  const message = rearmed ? `${res.message} — photon tubes loaded` : res.message
  return { ...state, ship: res.ship, combat, notice: message, noticeAt: PENDING }
}

/** Buy an upgrade at a node that trades. */
export function purchase(state: GameState, c: Ship.Component | string): GameState {
  const node = state.nearby
  if (!node || !servicesOf(node.kind).includes('trade')) {
    return { ...state, noticeAt: PENDING, notice: 'no market in range' }
  }
  const res = Ship.buy(state.ship, c as Ship.Component)
  // The MISSILES component used to raise the magazine, so buying it had to hand over the rounds
  // or it looked like the market had simply taken your salvage. It buys **yield** now
  // (`weapons.ts::photonDamage`) and the magazine is the hull's tube count, which nothing scales —
  // so there are no rounds to hand over, and the rounds you already carry got better where they
  // sit. `Combat` is untouched here on purpose.
  return { ...state, ship: res.ship, notice: res.message, noticeAt: PENDING }
}

/**
 * Cycle the nav computer to the next node offering `service`.
 *
 * A waypoint is the only thing on screen the player put there, and with a thousand nodes over
 * a sector this size it is what makes a destination reachable on purpose.
 */
export function route(state: GameState, space: Space, service: Service): GameState {
  const next = Nav.cycle(space, state.camera, service, state.waypoint)
  if (next === null) {
    // A real state, not an error: a world whose observer perceived nothing live has no docks.
    return { ...state, noticeAt: PENDING, notice: `this sector has nowhere to ${service}` }
  }
  const fix = Nav.fixOn(space, state.camera, next)
  return {
    ...state,
    waypoint: next,
    notice: fix ? `waypoint: ${fix.node.label} — ${Nav.rangeLabel(fix.range)}` : state.notice,
  }
}

/**
 * Point the nav computer at the active contract.
 *
 * Bound to `7`, beside the four service keys, because a contract's subject is a destination in
 * exactly the way a fuel depot is — and until this existed the only way to find the place a haul
 * named was to read the label off the panel and then cycle `3` until the same name came round.
 *
 * **It refuses with a reason rather than doing nothing**, which is the rule the station panel had
 * to learn: three services were reported broken and all three were declining correctly into a
 * notice that faded in three seconds. A hunting contract genuinely has no waypoint — its subject
 * is a *kind of craft*, not a place — and saying so is the difference between a key that is
 * inapplicable and a key that is dead.
 */
export function routeToQuest(state: GameState, space: Space): GameState {
  const active = state.quests.active
  if (!active) {
    return { ...state, noticeAt: PENDING, notice: 'no contract — dock at a citadel for a board' }
  }
  // Which leg. Before the pickup the destination is the origin of the haul; after it, the drop.
  const target = active.quest.kind === 'haul' || active.quest.kind === 'contraband'
    ? (active.picked ? active.quest.to : active.quest.from)
    : null
  if (target === null) {
    return {
      ...state,
      noticeAt: PENDING,
      notice: `${active.quest.title} — a hunting contract has no waypoint; its quarry moves`,
    }
  }
  const fix = Nav.fixOn(space, state.camera, target)
  if (!fix) {
    // The node named by the contract is not in this sector's node list, which should be
    // impossible — `build` picks from `nodes` — so it is reported rather than silently ignored.
    return { ...state, noticeAt: PENDING, notice: 'the contract names a place this sector has no record of' }
  }
  return {
    ...state,
    waypoint: target,
    noticeAt: PENDING,
    notice: `contract: ${fix.node.label} — ${Nav.rangeLabel(fix.range)}`,
  }
}

/** What the renderer needs from the live state. */
export function dynamicOf(state: GameState, space?: Space) {
  const wp =
    space && state.waypoint !== null
      ? (space.nodes.find((n) => n.id === state.waypoint)?.at ?? null)
      : null
  return {
    shots: state.combat.projectiles.map((p) => ({ at: p.at, kind: p.kind, dir: p.dir })),
    // `owner` rides along so the renderer can colour a round by who fired it. Without it every
    // shot in the sector is the same red, and a marshal shooting a raider three hundred million
    // units away is indistinguishable from a raider shooting at you — which defeats the entire
    // point of making the exchange visible in the first place.
    incoming: state.swarm.shots.map((s) => ({ at: s.at, dir: s.dir, owner: s.owner })),
    // Craft still dropping out of hyperspace. Drawn, and nothing else: they are not in the swarm,
    // so they cannot be shot, cannot shoot and cannot be collided with. See `arrivals.ts`.
    arrivals: state.waves.arriving.map((a) => ({
      at: a.at,
      dir: a.dir,
      faction: a.faction as string,
      progress: Arrivals.progress(a, state.nowMs),
    })),
    // The player's own drive spinning up. `null` on almost every frame, on purpose — see the
    // field on `Dynamic`. `Hyper.progress` is the same figure the HUD reads, so the cage and the
    // readout cannot disagree about how close the jump is.
    jump:
      state.drive.phase === 'charging'
        ? {
            at: v3(state.camera.position),
            facing: v3(forward(state.camera)),
            progress: Hyper.progress(state.drive, state.ship.levels.drive),
          }
        : null,
    // Sparks and detonations. Derived shards, not stored particles — see `fx.ts`.
    bursts: (state.bursts ?? []).flatMap((b) => Fx.shardsOf(b, state.nowMs)),
    craft: Enemy.living(state.swarm).map((c) => ({
      id: c.id,
      at: c.at,
      solid: c.solid,
      facing: c.facing,
      spec: c.spec,
      flash: state.flashes[c.id] ?? 0,
      // Carried so the renderer can colour traffic, which has no contact behind it to take a
      // role from. Without it every courier in the sector was invisible.
      faction: c.faction,
    })),
    destroyed: state.combat.destroyed,
    waypoint: wp,
    from: wp ? v3(state.camera.position) : null,
    self: {
      at: v3(state.camera.position),
      facing: v3(forward(state.camera)),
      shape: HULLS[state.ship.frame].shape,
      radius: Math.round(EXTENT * HULLS[state.ship.frame].size),
    },
  }
}

/**
 * What a key press does, as a pure function.
 *
 * ## Why this is here and not in the component
 *
 * It was in the component, inside a `useEffect` whose dependency array was empty. The effect ran
 * once on mount — *before* a record was loaded — so the handler closed over `loaded === null` for
 * the rest of the session and every service key hit an early return. `F`, `R`, `V`, `M` and the
 * course keys were dead from the first frame.
 *
 * It presented as "refuelling does not work", and it was invisible from every angle: the
 * mechanics were tested and correct, the buttons were wired and correct, and movement worked —
 * because held keys are recorded *before* that early return, so flying and firing were unaffected
 * while every single-press command was not. It also explained the jump drive: the course keys are
 * how a waypoint gets set, so with them dead the drive refused for want of one, forever.
 *
 * Pure, tested, and returning `null` when a key is not a command, so the component is a
 * dispatcher with nothing in it that can be wrong on its own.
 */
export function command(state: GameState, space: Space, code: string): GameState | null {
  switch (code) {
    // Services are single presses rather than held keys — a held `F` should refuel once, not
    // sixty times a second, which is why these are not read from the key set in `tick`.
    case 'KeyF':
      return useService(state, 'refuel')
    case 'KeyR':
      return useService(state, 'repair')
    case 'KeyV':
      return useService(state, 'scavenge')
    case 'Digit1':
      return route(state, space, 'refuel')
    case 'Digit2':
      return route(state, space, 'repair')
    case 'Digit3':
      return route(state, space, 'trade')
    case 'Digit4':
      return route(state, space, 'scavenge')
    // The active contract's subject, which is a destination in exactly the way a fuel depot is.
    // Until this existed the only way to find the place a haul named was to read the label off the
    // panel and cycle `3` until the same name came round.
    case 'Digit7':
      return routeToQuest(state, space)
    case 'Digit0':
      return { ...state, waypoint: null, noticeAt: PENDING, notice: 'course cleared' }
    default:
      return null
  }
}

/** Every key `command` answers to, so the pause menu and the tests share one list. */
export const COMMAND_KEYS = [
  'KeyF', 'KeyR', 'KeyV', 'Digit1', 'Digit2', 'Digit3', 'Digit4', 'Digit7', 'Digit0',
]

/**
 * Convert salvage to SCEMA at a market.
 *
 * Only at a market, because the spread is the point: an exchange available anywhere makes the two
 * currencies one resource with two labels, and the choice between a component now and a hull
 * later stops being a question.
 */
/**
 * What a full magazine of photons costs in salvage, per round.
 *
 * Priced per round rather than per reload so a marauder's twenty-four cost eight times a skiff's
 * three — a flat reload price would make the largest magazine the cheapest ammunition in the
 * sector, which inverts the one decision the tube count exists to create.
 */
export const PHOTON_PRICE = 45

/**
 * Rearm the photon tubes for salvage, at a dock or a market.
 *
 * **The magazine was previously unrefillable anywhere except by buying a different hull.** A dock
 * reloaded it as a side effect of its other services and nothing else did, so a player who spent
 * their rounds mid-sector had no route back to a full magazine that did not involve flying home.
 * With a starting magazine of eight and capitals that need most of it, that is the difference
 * between the photon being a weapon and being a consumable you hoard and never use.
 *
 * Partial reloads are allowed and charged for what they deliver: refusing to sell four rounds to
 * somebody who cannot afford eight is a refusal with no reason behind it.
 */
export function rearm(state: GameState): GameState {
  const node = state.nearby
  if (!node) return { ...state, noticeAt: PENDING, notice: 'nothing in range' }
  const services = servicesOf(node.kind)
  if (!services.includes('trade') && !services.includes('refuel')) {
    return { ...state, noticeAt: PENDING, notice: `${node.label} has no ordnance` }
  }
  const full = Weapons.photonMagazine(state.ship.frame)
  const missing = full - state.combat.photonsLeft
  if (missing <= 0) {
    return { ...state, noticeAt: PENDING, notice: 'tubes already full' }
  }
  const afford = Math.floor(state.ship.salvage / PHOTON_PRICE)
  if (afford <= 0) {
    return {
      ...state,
      noticeAt: PENDING,
      notice: `a photon is ${PHOTON_PRICE} salvage — you have ${state.ship.salvage}`,
    }
  }
  const rounds = Math.min(missing, afford)
  return {
    ...state,
    ship: { ...state.ship, salvage: state.ship.salvage - rounds * PHOTON_PRICE },
    combat: { ...state.combat, photonsLeft: state.combat.photonsLeft + rounds },
    noticeAt: PENDING,
    notice: `+${rounds} photon${rounds === 1 ? '' : 's'} — ${rounds * PHOTON_PRICE} salvage`,
  }
}

/** Take a contract. At most one at a time — see `quests.ts`. */
export function takeContract(state: GameState, quest: Quests.Quest): GameState {
  if (state.quests.active) {
    return { ...state, noticeAt: PENDING, notice: 'you already have a contract' }
  }
  return {
    ...state,
    quests: Quests.accept(state.quests, quest),
    noticeAt: PENDING,
    notice: `contract taken — ${quest.title}`,
  }
}

/** Drop the contract. Free, deliberately — see `quests.ts`. */
export function dropContract(state: GameState): GameState {
  if (!state.quests.active) return state
  return {
    ...state,
    quests: Quests.abandon(state.quests),
    noticeAt: PENDING,
    notice: 'contract abandoned',
  }
}

export function exchangeAt(state: GameState, salvage?: number): GameState {
  const node = state.nearby
  if (!node || !servicesOf(node.kind).includes('trade')) {
    return { ...state, noticeAt: PENDING, notice: 'no market in range' }
  }
  const r = Economy.exchange({ salvage: state.ship.salvage, scema: state.ship.scema }, salvage)
  if (!r.ok) return { ...state, noticeAt: PENDING, notice: r.message }
  return {
    ...state,
    ship: { ...state.ship, salvage: r.wallet.salvage, scema: r.wallet.scema },
    noticeAt: PENDING,
    notice: r.message,
  }
}

/**
 * Buy a hull at a market. Everything you own comes across and the new ship arrives whole.
 *
 * Delivering one empty would mean the first thing a player does after the largest purchase in the
 * game is limp to a depot, which is a strange lesson to attach to a reward.
 */
export function acquire(state: GameState, frame: HullId): GameState {
  const node = state.nearby
  if (!node || !servicesOf(node.kind).includes('trade')) {
    return { ...state, noticeAt: PENDING, notice: 'no shipyard in range' }
  }
  const r = Economy.buyHull(
    { salvage: state.ship.salvage, scema: state.ship.scema },
    state.ship.frame,
    frame,
  )
  if (!r.ok) return { ...state, noticeAt: PENDING, notice: r.message }
  const refitted = Ship.refit({ ...state.ship, scema: r.wallet.scema }, frame)
  // Tubes are a property of the hull, so a new ship arrives with *its own* magazine loaded —
  // six on a marauder however few the skiff had left. Carrying the old count across would mean
  // the largest purchase in the game silently delivered an empty weapon, and a player who bought
  // a marauder for its six tubes would find one round in them.
  return {
    ...state,
    ship: refitted,
    combat: Weapons.reload(state.combat, frame),
    noticeAt: PENDING,
    notice: r.message,
  }
}

/**
 * Debit SCEMA that has been withdrawn as $SCEMA tokens.
 *
 * Called **only** after `POST /api/scemaworld/claim` has returned a confirmed signature, and it
 * debits the `spend` the server reported rather than the amount that was offered — a capped claim
 * pays less than was asked for, and a client that debited its own request would burn the
 * difference. Same rule as `economy.ts::exchange`: only what actually converted is spent.
 *
 * It refuses to go negative and says so rather than clamping silently. A balance that has drifted
 * below what was paid out is a bug somewhere upstream, and a clamp is how it stops being visible.
 */
export function withdrawn(state: GameState, spend: number, tokens: number): GameState {
  if (spend > state.ship.scema) {
    return {
      ...state,
      noticeAt: PENDING,
      notice: `withdrawal of ${spend} exceeds the ${state.ship.scema} SCEMA on this ship`,
    }
  }
  return {
    ...state,
    ship: { ...state.ship, scema: state.ship.scema - spend },
    noticeAt: PENDING,
    notice: `${tokens} $SCEMA sent`,
  }
}

/**
 * Whether the jump drive would refuse right now, and why. For the HUD.
 *
 * Reported rather than merely enforced: a drive that silently does nothing is indistinguishable
 * from a broken key, and "jump inhibited — hostiles in range" is the sentence that teaches the
 * player the mechanic exists at all.
 */
export function jumpRefusal(state: GameState): string | null {
  const threat = Enemy.nearestThreat(state.swarm, v3(state.camera.position), state.role)
  return Hyper.refusal({
    threat: threat ? threat.range : null,
    charges: state.ship.jumpFuel,
    driveLevel: state.ship.levels.drive,
    waypoint: state.waypoint,
  })
}

/** How far the nearest hostile is, and what it is. `null` when sensors are clear. */
export function contact(state: GameState) {
  const t = Enemy.nearestThreat(state.swarm, v3(state.camera.position), state.role)
  if (!t) return null
  return { spec: t.craft.spec, range: t.range, behaviour: t.craft.behaviour, id: t.craft.id }
}

/**
 * Everything on sensors, nearest first, with its faction.
 *
 * Sensor range is `SENSOR_MULTIPLIER` times what anything engages at, and the gap is the point:
 * contact should arrive long before a fight does, because that gap is where the decision to fight
 * or leave actually lives. Detection used to be the same number as aggression, so a sector was
 * quiet until something was already on you — an ambush, not a decision.
 */
export function sensors(state: GameState, limit = 6) {
  const at = v3(state.camera.position)
  const reach = AGGRO_RANGE * SENSOR_MULTIPLIER * Ship.sensorGain(state.ship.levels.sensors)
  const out = Enemy.living(state.swarm)
    .map((c) => ({
      id: c.id,
      faction: c.faction,
      spec: c.spec,
      behaviour: c.behaviour,
      range: Math.hypot(c.at.x - at.x, c.at.y - at.y, c.at.z - at.z),
    }))
    .filter((c) => c.range < reach)
  out.sort((a, b) => a.range - b.range)
  return out.slice(0, limit)
}

/** True while a hostile is close enough to inhibit the jump drive. */
export function inhibited(state: GameState): boolean {
  const t = Enemy.nearestThreat(state.swarm, v3(state.camera.position), state.role)
  return t !== null && t.range < JUMP_INHIBIT
}
