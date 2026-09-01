/**
 * Enemy craft: hostiles that move, close, and shoot back.
 *
 * A static contact is a target; a craft that hunts you is an opponent. Pure and stepped by a
 * caller-supplied `dt`, so a fight is reproducible and testable without a frame loop.
 *
 * ## A ghost still never resolves, even while it is shooting at you
 *
 * A hostile from an *estimated* signal is a craft whose strength nobody measured. It flies, it
 * fires, and its threat reads `—` for the whole engagement. The pressure to make it resolve is
 * much stronger here than it was for a static contact — a player being shot at wants a number
 * — and giving them one would be inventing it. The uncertainty *is* the encounter.
 *
 * What a ghost does not get is invented aggression either: its behaviour comes from the seed,
 * exactly like durability, and never from the magnitude it does not have.
 */

import type { Contact, Vec3 } from './generate.ts'
import { durability } from './weapons.ts'
import {
  AGGRO_RANGE, ENGAGE_RANGE, LIFE_ENEMY_SHOT, R_PLAYER, SPEED_CRAFT, SPEED_CRAFT_PER_TIER,
  SPEED_ENEMY_SHOT,
} from './scale.ts'

export { AGGRO_RANGE, ENGAGE_RANGE }
export const ENEMY_SHOT_SPEED = SPEED_ENEMY_SHOT
export const ENEMY_COOLDOWN_MS = 1_400

export interface Craft {
  /** The contact this craft is. Ids match `Space.contacts`. */
  id: string
  at: Vec3
  vel: Vec3
  /** Hits remaining. From the seed, never from a reported magnitude. */
  integrity: number
  solid: boolean
  /** Milliseconds of its last shot. */
  lastFire: number
  alive: boolean
}

export interface EnemyShot {
  at: Vec3
  dir: Vec3
  life: number
}

export interface Swarm {
  craft: Craft[]
  shots: EnemyShot[]
}

function sub(a: Vec3, b: Vec3): Vec3 {
  return { x: a.x - b.x, y: a.y - b.y, z: a.z - b.z }
}

function norm(v: Vec3): Vec3 {
  const l = Math.hypot(v.x, v.y, v.z) || 1
  return { x: v.x / l, y: v.y / l, z: v.z / l }
}

function len(v: Vec3): number {
  return Math.hypot(v.x, v.y, v.z)
}

/** Speed for a craft, from the seed. Bounded so nothing outruns an un-upgraded engine. */
export function craftSpeed(seed: string, id: string): number {
  return SPEED_CRAFT + (durability(seed, id) % 5) * SPEED_CRAFT_PER_TIER
}

/** Build the swarm from a space's hostile contacts. Salvage contacts stay inert. */
export function swarmOf(contacts: Contact[], seed: string): Swarm {
  return {
    craft: contacts
      .filter((c) => c.hostility === 'hostile')
      .map((c) => ({
        id: c.id,
        at: c.at,
        vel: { x: 0, y: 0, z: 0 },
        integrity: durability(seed, c.id),
        solid: c.solid,
        lastFire: -1e9,
        alive: true,
      })),
    shots: [],
  }
}

export interface StepResult {
  swarm: Swarm
  /** Hull damage dealt to the player this step. */
  damage: number
}

/**
 * Advance the swarm.
 *
 * Craft outside `AGGRO_RANGE` do nothing at all — they do not drift toward the player from
 * across the sector, because a sector where every hostile converges on you is one big fight
 * rather than a place with dangerous regions in it.
 */
export function step(
  swarm: Swarm,
  playerAt: Vec3,
  dt: number,
  nowMs: number,
  seed: string,
): StepResult {
  const craft: Craft[] = []
  const shots: EnemyShot[] = []
  let damage = 0

  for (const c of swarm.craft) {
    if (!c.alive) {
      craft.push(c)
      continue
    }
    const toPlayer = sub(playerAt, c.at)
    const range = len(toPlayer)

    if (range > AGGRO_RANGE) {
      craft.push(c)
      continue
    }

    const dir = norm(toPlayer)
    const speed = craftSpeed(seed, c.id)
    // Close to engagement range, then hold. Multiplying by the signed gap makes it ease in
    // rather than oscillating across the threshold every frame.
    const approach = range > ENGAGE_RANGE ? 1 : -0.4
    const vel = {
      x: dir.x * speed * approach,
      y: dir.y * speed * approach,
      z: dir.z * speed * approach,
    }
    const at = { x: c.at.x + vel.x * dt, y: c.at.y + vel.y * dt, z: c.at.z + vel.z * dt }

    let lastFire = c.lastFire
    if (range < AGGRO_RANGE * 0.7 && nowMs - c.lastFire > ENEMY_COOLDOWN_MS) {
      shots.push({ at, dir, life: LIFE_ENEMY_SHOT })
      lastFire = nowMs
    }
    craft.push({ ...c, at, vel, lastFire })
  }

  // Advance existing shots and resolve those that reach the player.
  const PLAYER_RADIUS = R_PLAYER
  for (const s of swarm.shots) {
    const at = {
      x: s.at.x + s.dir.x * ENEMY_SHOT_SPEED * dt,
      y: s.at.y + s.dir.y * ENEMY_SHOT_SPEED * dt,
      z: s.at.z + s.dir.z * ENEMY_SHOT_SPEED * dt,
    }
    const life = s.life - dt
    // Swept, for the same reason player shots are: at this speed an endpoint test misses.
    if (nearSegment(playerAt, s.at, at) <= PLAYER_RADIUS) {
      damage += 8
      continue
    }
    if (life > 0) shots.push({ at, dir: s.dir, life })
  }

  return { swarm: { craft, shots }, damage }
}

function nearSegment(p: Vec3, a: Vec3, b: Vec3): number {
  const ab = sub(b, a)
  const ap = sub(p, a)
  const l2 = ab.x * ab.x + ab.y * ab.y + ab.z * ab.z
  if (l2 === 0) return len(sub(p, a))
  let t = (ap.x * ab.x + ap.y * ab.y + ap.z * ab.z) / l2
  t = Math.max(0, Math.min(1, t))
  return len(sub(p, { x: a.x + ab.x * t, y: a.y + ab.y * t, z: a.z + ab.z * t }))
}

/** Register a player hit. Returns the swarm and whether the craft died this hit. */
export function hit(swarm: Swarm, id: string): { swarm: Swarm; killed: boolean } {
  let killed = false
  const craft = swarm.craft.map((c) => {
    if (c.id !== id || !c.alive) return c
    const integrity = c.integrity - 1
    if (integrity <= 0) {
      killed = true
      return { ...c, integrity: 0, alive: false }
    }
    return { ...c, integrity }
  })
  return { swarm: { ...swarm, craft }, killed }
}

/** Live craft, for rendering and for targeting. */
export function living(swarm: Swarm): Craft[] {
  return swarm.craft.filter((c) => c.alive)
}
