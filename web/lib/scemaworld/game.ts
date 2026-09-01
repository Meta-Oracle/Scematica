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

import { DOCK_RANGE, JUMP_INHIBIT, R_ORIGIN, R_PLAYER, SPEED_THRUST } from './scale.ts'
import * as Hyper from './hyper.ts'
import { CLASSES } from './classes.ts'
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
 * Damage from flying into something, per unit of closing speed.
 *
 * Tuned so a graze at cruise costs a few points and a full-burn nose-first impact into a dock
 * takes most of a stock hull. Ramming has to be survivable — a player who dies to one mistimed
 * approach stops approaching anything — and it has to be expensive enough that docking is a
 * manoeuvre rather than a collision you perform on purpose.
 */
const IMPACT_DAMAGE = 26

/** Damage for ramming a craft, per unit of closing speed. Both parties pay. */
const RAM_DAMAGE = 34

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
    // Raiders fly alongside the record's own hostiles and are stepped by the same code. The
    // separation between them is a claim about provenance, not about behaviour.
    swarm: Enemy.swarmOf([...space.contacts, ...space.raiders], space.seed),
    nearby: null,
    noticeAt: -1e9,
    waypoint: null,
    drive: Hyper.IDLE,
    velocity: { x: 0, y: 0, z: 0 },
    flashes: {},
    shake: 0,
    notice: null,
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
    camera = translate(camera, [0, 0, -Ship.topSpeed(ship.levels.engine) * effective * dt])
  }
  // Lateral thrusters are free of the main drive and cost nothing: they are for docking, and
  // a player unable to line up on a depot because they ran dry is stuck forever.
  const strafe = (keys.has('ArrowRight') ? 1 : 0) - (keys.has('ArrowLeft') ? 1 : 0)
  const lift = (keys.has('Space') ? 1 : 0) - (keys.has('ShiftLeft') ? 1 : 0)
  if (strafe || lift) {
    const t = SPEED_THRUST * dt
    camera = translate(camera, [strafe * t, lift * t, 0])
  }

  // ── the ship against the furniture ─────────────────────────────────────────
  // Resolved once, after every source of movement — main drive, thrusters — and before anything
  // is aimed from the ship's position. Resolving per-source would let a thruster nudge you
  // through a wall the drive had just been stopped by.
  const grid = gridFor(space)
  const wanted = v3(camera.position)
  const landing = Collide.resolve(grid, v3(was), wanted, R_PLAYER, dt)
  if (landing.hit) {
    camera = { ...camera, position: [landing.at.x, landing.at.y, landing.at.z] }
    const cost = Math.round((landing.impact / Ship.topSpeed(0)) * IMPACT_DAMAGE)
    if (cost > 0) {
      ship = Ship.damage(ship, cost, nowMs)
      notice = `impact — ${landing.hit.node.label} (−${cost})`
    }
    // The drive is cut on impact. Leaving it running grinds the hull against the surface for as
    // long as the key is held, which turns one mistake into a death and reads as the ship being
    // stuck rather than stopped.
    throttle = 0
  } else {
    // Nothing solid — but the segment may have passed through something the observer never saw.
    // This is the sentence that teaches a player what a provenance is, from the cockpit.
    const ghosted = Collide.passedThrough(space, v3(was), wanted, R_PLAYER)
    if (ghosted) notice = Collide.permeableNote(ghosted.kind)
  }

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
  const alive = Enemy.living(state.swarm)
  // Only live hostiles. Weapons no longer adjudicates death — it reports damage and the swarm
  // decides — so an inert salvage contact in this list would produce hits that nothing consumes.
  const targets = [...space.contacts, ...space.raiders].filter(
    (c) => c.hostility === 'hostile' && alive.some((k) => k.id === c.id),
  )
  // Hostiles have moved, so aim at where they are rather than where the record put them.
  const moved = targets.map((c) => {
    const k = alive.find((x) => x.id === c.id)
    return k ? { ...c, at: k.at } : c
  })

  if (firing) combat = Weapons.fire(combat, at, nose, nowMs, moved, ship.levels)
  const advanced = Weapons.step(combat, dt, moved, space.seed, (from, to) =>
    Collide.sweep(grid, from, to, 0) !== null,
  )
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
  const enemyStep = Enemy.step(swarm, at, velocity, dt, nowMs, grid)
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
  let rammed: string | null = null
  for (const c of Enemy.living(swarm)) {
    const gap = Math.hypot(c.at.x - at.x, c.at.y - at.y, c.at.z - at.z)
    const touch = R_PLAYER + c.spec.radius
    if (gap >= touch) continue
    const closing = Math.hypot(velocity.x, velocity.y, velocity.z) + c.speed
    const cost = Math.max(1, Math.round((closing / Ship.topSpeed(0)) * RAM_DAMAGE))
    ship = Ship.damage(ship, cost, nowMs)
    const res = Enemy.hit(swarm, c.id, cost, nowMs)
    swarm = res.swarm
    if (res.killed) {
      ship = Ship.bounty(ship, res.bounty)
      combat = { ...combat, destroyed: [...combat.destroyed, c.id] }
    }
    flashes[c.id] = 1
    shake = Math.min(1, shake + 0.9)
    // Push out along the line between them, so the two do not sit inside each other next frame.
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
    throttle = 0
    rammed = `collision — ${c.spec.label} (−${cost})`
    break
  }
  if (rammed) notice = rammed

  ship = Ship.recharge(ship, dt, nowMs)

  const nearby = nearestService(space, v3(camera.position))

  return {
    camera,
    throttle,
    ship,
    combat,
    swarm,
    nearby,
    waypoint: state.waypoint,
    drive: jump.drive,
    velocity,
    flashes,
    shake,
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
  const res =
    service === 'refuel'
      ? // A dock charges the jump drive too; a depot does thrust fuel only. Six times as many
        // depots as docks is what makes a jump charge worth planning around.
        Ship.refuel(state.ship, node.kind === 'dock')
      : service === 'repair'
        ? Ship.repair(state.ship)
        : service === 'scavenge'
          ? Ship.scavenge(state.ship, node.id)
          : { ship: state.ship, message: 'trading', ok: true }
  return { ...state, ship: res.ship, notice: res.message, noticeAt: PENDING }
}

/** Buy an upgrade at a node that trades. */
export function purchase(state: GameState, c: Ship.Component | string): GameState {
  const node = state.nearby
  if (!node || !servicesOf(node.kind).includes('trade')) {
    return { ...state, noticeAt: PENDING, notice: 'no market in range' }
  }
  const res = Ship.buy(state.ship, c as Ship.Component)
  // A magazine upgrade that did not also load the rounds would look like it did nothing until
  // the next resupply, which is indistinguishable from a market that took your salvage.
  const combat =
    res.ok && c === 'missiles'
      ? { ...state.combat, photonsLeft: Ship.photonMagazine(res.ship.levels.missiles) }
      : state.combat
  return { ...state, ship: res.ship, combat, notice: res.message, noticeAt: PENDING }
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
    incoming: state.swarm.shots.map((s) => ({ at: s.at, dir: s.dir })),
    craft: Enemy.living(state.swarm).map((c) => ({
      id: c.id,
      at: c.at,
      solid: c.solid,
      facing: c.facing,
      spec: c.spec,
      flash: state.flashes[c.id] ?? 0,
    })),
    destroyed: state.combat.destroyed,
    waypoint: wp,
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

/** True while a hostile is close enough to inhibit the jump drive. */
export function inhibited(state: GameState): boolean {
  const t = Enemy.nearestThreat(state.swarm, v3(state.camera.position))
  return t !== null && t.range < JUMP_INHIBIT
}
