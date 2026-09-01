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

import { DOCK_RANGE, SPEED_THRUST } from './scale.ts'

export { DOCK_RANGE }

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
  /** Transient line for the HUD — a hit, a refuel, a refusal. */
  notice: string | null
  /** True once the hull is gone. The sector keeps rendering; you just cannot act. */
  lost: boolean
}

export function newGame(space: Space): GameState {
  return {
    camera: { position: [0, 0, 0], orientation: [0, 0, 0, 1] },
    throttle: 0,
    ship: Ship.newShip(),
    combat: Weapons.newCombat(),
    // Raiders fly alongside the record's own hostiles and are stepped by the same code. The
    // separation between them is a claim about provenance, not about behaviour.
    swarm: Enemy.swarmOf([...space.contacts, ...space.raiders], space.seed),
    nearby: null,
    waypoint: null,
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

  const at = v3(camera.position)
  const nose = v3(forward(camera))

  // ── the player's weapons ───────────────────────────────────────────────────
  let combat = state.combat
  const alive = Enemy.living(state.swarm)
  const targets = [...space.contacts, ...space.raiders].filter((c) => {
    if (c.hostility !== 'hostile') return !combat.destroyed.includes(c.id)
    return alive.some((k) => k.id === c.id)
  })
  // Hostiles have moved, so aim at where they are rather than where the record put them.
  const moved = targets.map((c) => {
    const k = alive.find((x) => x.id === c.id)
    return k ? { ...c, at: k.at } : c
  })

  if (firing) combat = Weapons.fire(combat, at, nose, nowMs, moved, ship.levels)
  const advanced = Weapons.step(combat, dt, moved, space.seed)
  combat = advanced.combat

  let swarm = state.swarm
  for (const h of advanced.hits) {
    const res = Enemy.hit(swarm, h.contact)
    swarm = res.swarm
    if (res.killed) {
      ship = Ship.bounty(ship)
      notice = `destroyed — +${Ship.BOUNTY} salvage`
    }
  }

  // ── the enemy's turn ───────────────────────────────────────────────────────
  const enemyStep = Enemy.step(swarm, at, dt, nowMs, space.seed)
  swarm = enemyStep.swarm
  if (enemyStep.damage > 0) {
    ship = Ship.damage(ship, enemyStep.damage)
    notice = 'taking fire'
  }

  const nearby = nearestService(space, at)

  return {
    camera,
    throttle,
    ship,
    combat,
    swarm,
    nearby,
    waypoint: state.waypoint,
    notice: notice ?? state.notice,
    lost: Ship.destroyed(ship),
  }
}

/** Use a service at the node you are next to. Returns the state and a message. */
export function useService(
  state: GameState,
  service: 'refuel' | 'repair' | 'trade' | 'scavenge',
): GameState {
  const node = state.nearby
  if (!node) return { ...state, notice: 'nothing in range' }
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
      ? Ship.refuel(state.ship)
      : service === 'repair'
        ? Ship.repair(state.ship)
        : service === 'scavenge'
          ? Ship.scavenge(state.ship, node.id)
          : { ship: state.ship, message: 'trading', ok: true }
  return { ...state, ship: res.ship, notice: res.message }
}

/** Buy an upgrade at a node that trades. */
export function purchase(state: GameState, c: Ship.Component | string): GameState {
  const node = state.nearby
  if (!node || !servicesOf(node.kind).includes('trade')) {
    return { ...state, notice: 'no market in range' }
  }
  const res = Ship.buy(state.ship, c as Ship.Component)
  // A magazine upgrade that did not also load the rounds would look like it did nothing until
  // the next resupply, which is indistinguishable from a market that took your salvage.
  const combat =
    res.ok && c === 'missiles'
      ? { ...state.combat, photonsLeft: Ship.photonMagazine(res.ship.levels.missiles) }
      : state.combat
  return { ...state, ship: res.ship, combat, notice: res.message }
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
    return { ...state, notice: `this sector has nowhere to ${service}` }
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
    shots: state.combat.projectiles.map((p) => ({ at: p.at, kind: p.kind })),
    incoming: state.swarm.shots.map((s) => ({ at: s.at })),
    craft: Enemy.living(state.swarm).map((c) => ({ id: c.id, at: c.at, solid: c.solid })),
    destroyed: state.combat.destroyed,
    waypoint: wp,
  }
}
