/**
 * Weapons, and the one rule combat is not allowed to break.
 *
 * Pure: no GL, no clock, no input. `step` and `fire` take the time they are given, so a fight
 * is reproducible and `check:scemaworld` can pin it.
 *
 * ## Magnitude drives size and aggression — never durability
 *
 * A signal's `magnitude` measures how big a concern the observer counted. Turning it into a
 * hit-point pool would give anybody who can write a record a reason to understate it, and a
 * producer with an incentive to misreport is the single failure this project cannot absorb.
 * So durability comes from the **seed**, deterministically, and has nothing to do with the
 * reported number. Same record, same fight; smaller reported magnitude, no easier enemy.
 *
 * ## A ghost never resolves, and that is the point
 *
 * An estimated signal is one the observer counted but whose magnitude it guessed. The signal
 * is there. What is unknown is how big it is — and nothing in the record can settle that, so
 * neither can this. A ghost's threat reads `—` for as long as you fight it, including while
 * it is shooting at you.
 *
 * The temptation is to have a ghost "resolve" on first hit into a known value, because that
 * feels better to play. It would be the em-dash bug wearing a game-design justification: the
 * number would be invented, and the player would then act on it as if somebody had measured
 * it. A ghost is dangerous precisely because you can never learn how dangerous.
 */

import type { Contact, Vec3 } from './generate.ts'
import { laserCooldown } from './ship.ts'
import {
  LIFE_LASER, LIFE_PHOTON, R_CONTACT, R_CONTACT_SPAN, SPEED_LASER, SPEED_PHOTON,
} from './scale.ts'

export type WeaponKind = 'laser' | 'photon'

export interface Weapon {
  kind: WeaponKind
  name: string
  /** Held-fire versus one shot per press. */
  automatic: boolean
  cooldownMs: number
  /** World units per second. */
  speed: number
  /** Photon missiles steer toward a lock; lasers do not. */
  tracking: boolean
  /** `null` for unlimited. Lasers are; missiles are not. */
  magazine: number | null
  /** World-unit radius counted as a hit. */
  calibre: number
}

export const LASER: Weapon = {
  kind: 'laser',
  name: 'AUTO LASER',
  automatic: true,
  cooldownMs: 110,
  speed: SPEED_LASER,
  tracking: false,
  magazine: null,
  calibre: Math.round(R_CONTACT * 1.4),
}

export const PHOTON: Weapon = {
  kind: 'photon',
  name: 'PHOTON MISSILE',
  automatic: false,
  cooldownMs: 900,
  speed: SPEED_PHOTON,
  tracking: true,
  magazine: 12,
  calibre: Math.round(R_CONTACT * 4),
}

export const WEAPONS: Weapon[] = [LASER, PHOTON]

export interface Projectile {
  id: number
  kind: WeaponKind
  at: Vec3
  dir: Vec3
  speed: number
  /** Seconds left before it expires. Bounded so a miss cannot leak. */
  life: number
  /** Contact id a photon is steering toward, if it had a lock. */
  lock: string | null
}

export interface Combat {
  /** Index into `WEAPONS`. */
  selected: number
  projectiles: Projectile[]
  /** Milliseconds of the last shot, per weapon kind. */
  lastFire: Record<WeaponKind, number>
  photonsLeft: number
  nextId: number
  /** Contacts destroyed so far, by id. */
  destroyed: string[]
  /** Hits landed per contact, so durability can be spent down. */
  hits: Record<string, number>
}

export function newCombat(): Combat {
  return {
    selected: 0,
    projectiles: [],
    lastFire: { laser: -1e9, photon: -1e9 },
    photonsLeft: PHOTON.magazine ?? 0,
    nextId: 1,
    destroyed: [],
    hits: {},
  }
}

export function selected(c: Combat): Weapon {
  return WEAPONS[c.selected % WEAPONS.length]
}

/** Left click. Cycles; never reloads and never fires. */
export function switchWeapon(c: Combat): Combat {
  return { ...c, selected: (c.selected + 1) % WEAPONS.length }
}

/**
 * How many hits a contact takes.
 *
 * From the **seed and the contact id**, never from `magnitude` — see the module note. Small
 * and bounded so a fight is winnable, deterministic so two players meet the same enemy.
 */
export function durability(seed: string, contactId: string): number {
  let h = 2166136261 >>> 0
  for (const s of [seed, contactId]) {
    for (let i = 0; i < s.length; i += 1) {
      h ^= s.charCodeAt(i)
      h = Math.imul(h, 16777619) >>> 0
    }
  }
  return 3 + (h % 6)
}

/**
 * What the HUD may say about a contact's threat.
 *
 * `—` for a ghost, forever. Not until it is hit, not after: nobody measured it, and the game
 * does not get to decide otherwise because a number would be more comfortable.
 */
export function threatLabel(c: Contact): string {
  return c.solid ? c.magnitude.toFixed(2) : '—'
}

function norm(v: Vec3): Vec3 {
  const l = Math.hypot(v.x, v.y, v.z) || 1
  return { x: v.x / l, y: v.y / l, z: v.z / l }
}

function sub(a: Vec3, b: Vec3): Vec3 {
  return { x: a.x - b.x, y: a.y - b.y, z: a.z - b.z }
}

function dist(a: Vec3, b: Vec3): number {
  return Math.hypot(a.x - b.x, a.y - b.y, a.z - b.z)
}

/**
 * Closest distance from a point to the segment `a → b`.
 *
 * A real swept test, not a check of where the shot ended up. A laser covers 43,000 units in a
 * single 60 Hz frame against a station 16,000 across, so an endpoint test misses almost
 * everything and the shots that do connect look like luck. Tunnelling is the classic
 * projectile bug and it is unfixable by tuning a radius — the radius that catches a fast shot
 * is wide enough to hit things the player did not aim at.
 */
function distToSegment(p: Vec3, a: Vec3, b: Vec3): number {
  const ab = sub(b, a)
  const ap = sub(p, a)
  const len2 = ab.x * ab.x + ab.y * ab.y + ab.z * ab.z
  if (len2 === 0) return dist(p, a)
  let t = (ap.x * ab.x + ap.y * ab.y + ap.z * ab.z) / len2
  t = Math.max(0, Math.min(1, t))
  return dist(p, { x: a.x + ab.x * t, y: a.y + ab.y * t, z: a.z + ab.z * t })
}

/**
 * The contact a photon would steer toward: nearest to the nose, within a narrow cone.
 *
 * Ghosts are lockable. Refusing to lock one would leak the answer — the player would learn
 * from the targeting computer what the record does not know.
 */
export function lockOn(from: Vec3, dir: Vec3, contacts: Contact[], destroyed: string[]): string | null {
  let best: string | null = null
  let bestDot = 0.94
  for (const c of contacts) {
    if (destroyed.includes(c.id)) continue
    const to = norm(sub(c.at, from))
    const d = to.x * dir.x + to.y * dir.y + to.z * dir.z
    if (d > bestDot) {
      bestDot = d
      best = c.id
    }
  }
  return best
}

/** Right click, or right click held for an automatic weapon. */
/**
 * Weapon levels from the ship, so an upgrade bought at a market actually does something.
 *
 * Optional, and defaulted to level zero, because `weapons.ts` must stay usable without a ship —
 * `check:scemaworld` fires shots with no game state around them.
 */
export interface WeaponLevels {
  laser: number
  missiles: number
}

const STOCK: WeaponLevels = { laser: 0, missiles: 0 }

export function fire(
  c: Combat,
  from: Vec3,
  dir: Vec3,
  nowMs: number,
  contacts: Contact[],
  levels: WeaponLevels = STOCK,
): Combat {
  const w = selected(c)
  const cooldown = w.kind === 'laser' ? laserCooldown(levels.laser) : w.cooldownMs
  if (nowMs - c.lastFire[w.kind] < cooldown) return c
  if (w.kind === 'photon' && c.photonsLeft <= 0) return c

  const p: Projectile = {
    id: c.nextId,
    kind: w.kind,
    at: from,
    dir: norm(dir),
    speed: w.speed,
    life: w.kind === 'photon' ? LIFE_PHOTON : LIFE_LASER,
    lock: w.tracking ? lockOn(from, norm(dir), contacts, c.destroyed) : null,
  }
  return {
    ...c,
    projectiles: [...c.projectiles, p],
    lastFire: { ...c.lastFire, [w.kind]: nowMs },
    photonsLeft: w.kind === 'photon' ? c.photonsLeft - 1 : c.photonsLeft,
    nextId: c.nextId + 1,
  }
}

export interface Hit {
  contact: string
  destroyed: boolean
}

/** Advance projectiles by `dt` seconds and resolve hits. Pure. */
export function step(
  c: Combat,
  dt: number,
  contacts: Contact[],
  seed: string,
): { combat: Combat; hits: Hit[] } {
  const hits: Hit[] = []
  const alive: Projectile[] = []
  const destroyed = [...c.destroyed]
  const hitCount = { ...c.hits }

  for (const p of c.projectiles) {
    let dir = p.dir
    if (p.lock) {
      const target = contacts.find((x) => x.id === p.lock)
      if (target && !destroyed.includes(target.id)) {
        // Steer, rather than snap. A missile that could not be dodged is not a fight.
        const want = norm(sub(target.at, p.at))
        const turn = 2.6 * dt
        dir = norm({
          x: dir.x + (want.x - dir.x) * turn,
          y: dir.y + (want.y - dir.y) * turn,
          z: dir.z + (want.z - dir.z) * turn,
        })
      }
    }

    const travel = p.speed * dt
    const at: Vec3 = {
      x: p.at.x + dir.x * travel,
      y: p.at.y + dir.y * travel,
      z: p.at.z + dir.z * travel,
    }
    const life = p.life - dt
    const w = p.kind === 'laser' ? LASER : PHOTON

    let consumed = false
    for (const contact of contacts) {
      if (destroyed.includes(contact.id)) continue
      // Swept along the whole step, so a fast shot cannot pass through a target between
      // frames. The radius is then only what it should be — calibre plus the contact's own
      // size — rather than being inflated to paper over tunnelling.
      // The same radius `view.ts` draws. A hit test that disagrees with the picture is the
      // worst kind of bug in a shooter: the player is told they missed something they saw
      // themselves hit.
      const reach =
        w.calibre + R_CONTACT + Math.round(Math.max(0, Math.min(1, contact.magnitude)) * R_CONTACT_SPAN)
      if (distToSegment(contact.at, p.at, at) <= reach) {
        const n = (hitCount[contact.id] ?? 0) + 1
        hitCount[contact.id] = n
        const dead = n >= durability(seed, contact.id)
        if (dead) destroyed.push(contact.id)
        hits.push({ contact: contact.id, destroyed: dead })
        consumed = true
        break
      }
    }

    if (!consumed && life > 0) alive.push({ ...p, at, dir, life })
  }

  return {
    combat: { ...c, projectiles: alive, destroyed, hits: hitCount },
    hits,
  }
}
