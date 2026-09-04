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
import { forward, rotate, translate, type Camera } from './camera.ts'

import {
  AGGRO_RANGE, DOCK_RANGE, EXTENT, JUMP_INHIBIT, R_ORIGIN, R_PLAYER, SENSOR_MULTIPLIER,
  SPEED_THRUST,
} from './scale.ts'
import * as Hyper from './hyper.ts'
import { CLASSES } from './classes.ts'
import { hostileTo } from './factions.ts'
import * as Economy from './economy.ts'
import * as Respawn from './respawn.ts'
import * as Arrivals from './arrivals.ts'
import { HULLS, type HullId } from './hulls.ts'
import { trafficOf } from './factions.ts'
import * as Collide from './collide.ts'
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
 * How much of a capital's drawn radius is actually solid.
 *
 * The rest is superstructure. It is what makes flying *inside* a dreadnought possible, and
 * flying inside one is the tactic that makes it beatable: a hull that turns at a twentieth of a
 * radian per second cannot bring a gun to bear on something already past its nose.
 */
const CAPITAL_CORE = 0.22

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
}

export function newGame(space: Space): GameState {
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
    swarm: Enemy.withTraffic(
      Enemy.swarmOf([...space.contacts, ...space.raiders], space.seed),
      trafficOf(space, space.seed),
    ),
    nearby: null,
    noticeAt: -1e9,
    waypoint: null,
    drive: Hyper.IDLE,
    velocity: { x: 0, y: 0, z: 0 },
    flashes: {},
    shake: 0,
    touching: [],
    notice: null,
    waves: Respawn.newWaves(),
    nowMs: 0,
    lost: false,
  }
}

function v3(a: readonly [number, number, number]): Vec3 {
  return { x: a[0], y: a[1], z: a[2] }
}

function dist(a: Vec3, b: Vec3): number {
  return Math.hypot(a.x - b.x, a.y - b.y, a.z - b.z)
}

/** Nearest node within docking range. */
export function nearestService(space: Space, at: Vec3): Node | null {
  let best: Node | null = null
  let bestD = DOCK_RANGE
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
export function tick(state: GameState, space: Space, input: TickInput): GameState {
  const { keys, firing, dt, nowMs } = input
  if (state.lost) return state
  const was = state.camera.position

  // ── attitude ───────────────────────────────────────────────────────────────
  const rate = 1.4 * dt
  let camera = state.camera
  const pitch = (keys.has('KeyS') ? 1 : 0) - (keys.has('KeyW') ? 1 : 0)
  const yaw = (keys.has('KeyA') ? 1 : 0) - (keys.has('KeyD') ? 1 : 0)
  const roll = (keys.has('KeyQ') ? 1 : 0) - (keys.has('KeyE') ? 1 : 0)
  if (pitch || yaw || roll) camera = rotate(camera, pitch * rate, yaw * rate, roll * rate)

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

  if (effective > 0) {
    camera = translate(camera, [0, 0, -Ship.limits(ship).speed * effective * dt])
  }
  // Lateral thrusters are free of the main drive and cost nothing: they are for docking, and
  // a player unable to line up on a depot because they ran dry is stuck forever.
  const strafe = (keys.has('ArrowRight') ? 1 : 0) - (keys.has('ArrowLeft') ? 1 : 0)
  const lift = (keys.has('Space') ? 1 : 0) - (keys.has('ShiftLeft') ? 1 : 0)
  if (strafe || lift) {
    const t = SPEED_THRUST * dt
    camera = translate(camera, [strafe * t, lift * t, 0])
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
  const crossed = Collide.crossed(space, v3(was), v3(camera.position), R_PLAYER)
  if (crossed) notice = Collide.passageNote(crossed.kind, crossed.label)

  // ── the jump drive ─────────────────────────────────────────────────────────
  // Resolved before weapons so a jump that lands this frame puts the ship at its destination
  // before anything is aimed from it — otherwise the first frame after arrival fires from the
  // old position, which reads as a shot coming out of nowhere.
  const threat = Enemy.nearestThreat(state.swarm, v3(camera.position))
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
  // What the ship actually did this tick, including a jump. Enemies lead off this.
  const velocity: Vec3 = jump.arriveAt
    ? { x: 0, y: 0, z: 0 }
    : {
        x: (camera.position[0] - was[0]) / Math.max(dt, 1e-6),
        y: (camera.position[1] - was[1]) / Math.max(dt, 1e-6),
        z: (camera.position[2] - was[2]) / Math.max(dt, 1e-6),
      }

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
    .filter((k) => hostileTo(k.faction))
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
  for (const h of advanced.hits) {
    const res = Enemy.hit(swarm, h.contact, h.damage, nowMs)
    swarm = res.swarm
    // A hit on hull flashes harder than one soaked by a shield. That is the only cue telling a
    // player whether they are making progress or wasting rounds on a buffer, and without it a
    // heavily-shielded gunship reads as invulnerable.
    flashes[h.contact] = res.throughShield ? 1 : 0.45
    if (res.killed) {
      ship = Ship.bounty(ship, res.bounty)
      // `combat.destroyed` is what stops it being drawn. Written here rather than in
      // `weapons.ts` for the same reason the damage is: one authority over one fact.
      combat = { ...combat, destroyed: [...combat.destroyed, h.contact] }
      notice = `destroyed — +${res.bounty} salvage`
    }
  }

  // ── the enemy's turn ───────────────────────────────────────────────────────
  // Craft still avoid nodes even though they cannot hit them. A wing flying *through* a station
  // ring is technically correct and looks like the geometry is decorative; steering round one
  // costs nothing and reads as piloting.
  const enemyStep = Enemy.step(swarm, at, velocity, dt, nowMs, grid, space)
  swarm = enemyStep.swarm
  let shake = Math.max(0, state.shake - dt * 2.2)
  if (enemyStep.damage > 0) {
    const before = ship.hull
    ship = Ship.damage(ship, enemyStep.damage, nowMs)
    // The screen kicks harder when the hit reached hull. Same reasoning as the enemy flash:
    // shields absorbing and hull being opened must not feel identical.
    shake = Math.min(1, shake + (ship.hull < before ? 0.85 : 0.35))
    notice = ship.hull < before ? 'HULL BREACHED' : 'shields holding'
  }
  // ── ramming ────────────────────────────────────────────────────────────────
  // Both parties pay, and the player is pushed clear. A craft you can fly through is a craft
  // that is not there, and at these closing speeds an interceptor crossing your nose ought to be
  // an event rather than a texture.
  //
  // ## Capitals are volumes you fly *through*, not walls
  //
  // A leviathan's radius is a quarter of a sector. Treating that sphere as a hull meant flying
  // into one put the ship permanently inside a hurtbox: every frame re-collided, every frame
  // charged damage and zeroed the throttle, and the push-out teleported the ship a quarter of a
  // sector in a near-arbitrary direction. Stuck, and then dead.
  //
  // So a capital has a **core** — a small fraction of its drawn radius — and only that collides.
  // The rest is superstructure you can fly between, which is also the tactic that makes a
  // dreadnought survivable: get inside its guns' arc, where a hull that turns at a twentieth of a
  // radian per second cannot bring anything to bear.
  //
  // And a ram is charged **on entry**, not while overlapping. Charging per frame is what turns
  // one mistake into a death.
  let rammed: string | null = null
  const touching: string[] = []
  for (const c of Enemy.living(swarm)) {
    const gap = Math.hypot(c.at.x - at.x, c.at.y - at.y, c.at.z - at.z)
    const core = c.spec.capital ? c.spec.radius * CAPITAL_CORE : c.spec.radius
    const touch = R_PLAYER + core
    if (gap >= touch) continue
    touching.push(c.id)
    // Already inside it last frame: no second charge, no second push. Flying out is the player's
    // problem and takes as long as it takes.
    if (state.touching.includes(c.id)) continue

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
      if (hostileTo(c.faction)) ship = Ship.bounty(ship, res.bounty)
      combat = { ...combat, destroyed: [...combat.destroyed, c.id] }
    }
    flashes[c.id] = 1
    shake = Math.min(1, shake + 0.9)
    // Nudged clear along the line between them rather than placed on the surface. A placement is
    // a teleport at capital scale; a nudge lets the ship keep flying the way it was going.
    const dir = gap < 1e-6 ? { x: 1, y: 0, z: 0 } : {
      x: (at.x - c.at.x) / gap,
      y: (at.y - c.at.y) / gap,
      z: (at.z - c.at.z) / gap,
    }
    const out = touch * 1.02
    camera = {
      ...camera,
      position: [c.at.x + dir.x * out, c.at.y + dir.y * out, c.at.z + dir.z * out],
    }
    // The drive is cut for a fighter-sized impact only. Cutting it inside a capital would strand
    // the ship in the one place it most needs to be able to leave.
    if (!c.spec.capital) throttle = 0
    rammed = `collision — ${c.spec.label} (−${cost})`
    break
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

  const nearby = nearestService(space, v3(camera.position))

  return {
    camera,
    throttle,
    ship,
    combat,
    swarm,
    waves: topUp.waves,
    nowMs,
    nearby,
    waypoint: state.waypoint,
    drive: jump.drive,
    velocity,
    flashes,
    shake,
    touching,
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
    case 'Digit0':
      return { ...state, waypoint: null, noticeAt: PENDING, notice: 'course cleared' }
    default:
      return null
  }
}

/** Every key `command` answers to, so the pause menu and the tests share one list. */
export const COMMAND_KEYS = [
  'KeyF', 'KeyR', 'KeyV', 'Digit1', 'Digit2', 'Digit3', 'Digit4', 'Digit0',
]

/**
 * Convert salvage to SCEMA at a market.
 *
 * Only at a market, because the spread is the point: an exchange available anywhere makes the two
 * currencies one resource with two labels, and the choice between a component now and a hull
 * later stops being a question.
 */
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
  const threat = Enemy.nearestThreat(state.swarm, v3(state.camera.position))
  return Hyper.refusal({
    threat: threat ? threat.range : null,
    charges: state.ship.jumpFuel,
    driveLevel: state.ship.levels.drive,
    waypoint: state.waypoint,
  })
}

/** How far the nearest hostile is, and what it is. `null` when sensors are clear. */
export function contact(state: GameState) {
  const t = Enemy.nearestThreat(state.swarm, v3(state.camera.position))
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
  const t = Enemy.nearestThreat(state.swarm, v3(state.camera.position))
  return t !== null && t.range < JUMP_INHIBIT
}
