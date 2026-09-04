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
import { HULLS, type HullId } from './hulls.ts'
import { capsuleOf, strikes } from './hitbox.ts'
import type { Shape } from './classes.ts'
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
  /**
   * `null` for unlimited. Lasers are; missiles are not.
   *
   * For the photon this is the **fallback only** — the real magazine is the hull's tube count
   * (`HULLS[frame].tubes`, and `photonMagazine` below). A weapon table cannot know what ship it
   * is bolted to, and `check:scemaworld` fires shots with no ship anywhere in scope, so the
   * constant stays as the answer for a caller that has not named a hull.
   */
  magazine: number | null
  /** World-unit radius counted as a hit. */
  calibre: number
  /**
   * Damage per hit.
   *
   * A laser is a drip and a photon is an event — nine rounds of one against a shielded gunship
   * or a single missile. That contrast is the whole reason there are two weapons, and it is
   * what a burst of laser fire failing to break a shield is supposed to teach.
   */
  damage: number
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
  damage: 5,
}

/**
 * The photon missile, after the buff — and the buff is a *change of role*, not a bigger number.
 *
 * It used to be twelve rounds of forty-two: enough to be a slightly better laser and never
 * enough to be an event, so the sane play was to hold the trigger and ignore the other weapon
 * entirely. It is now **one to six rounds of two hundred and forty**, and how many you carry is
 * a property of the hull you bought rather than of a component you levelled (`hulls.ts::tubes`,
 * `photonMagazine` below). A skiff carries one. A marauder carries six.
 *
 * That combination is what makes it a decision rather than a resource: firing it is a claim that
 * *this* target is worth the round, and there is no version of "spam it" available at a magazine
 * of one. A gunship dies to a single hit. A war class does not, and is not meant to — six rounds
 * is a serious opening against a leviathan and nowhere near a titan, which stays a fight you have
 * to actually fly.
 *
 * The wide calibre is part of the buff and part of the fairness: a tracking round with a magazine
 * of one that can still *miss* a manoeuvring interceptor would be a coin toss, so the round is
 * forgiving about where it connects while remaining dodgeable by breaking the lock's steering
 * (`step` turns at a finite rate, exactly like a craft).
 */
export const PHOTON: Weapon = {
  kind: 'photon',
  name: 'PHOTON MISSILE',
  automatic: false,
  cooldownMs: 700,
  speed: SPEED_PHOTON,
  tracking: true,
  magazine: 1,
  calibre: Math.round(R_CONTACT * 7),
  damage: 240,
}

export const WEAPONS: Weapon[] = [LASER, PHOTON]

/**
 * How many photons a hull carries. **The magazine is the hull, and nothing else touches it.**
 *
 * Six on a marauder, four on a lancer, two on a corvette, one on a skiff or a scout. Flat counts
 * a pilot can hold in their head, because that count *is* the decision every time the tube is
 * loaded — a magazine that came out of a formula over a component level would be a number you
 * have to look up, which is the opposite of what a scarce weapon needs.
 *
 * The MISSILES component buys **yield** instead (`photonDamage`). That split is what keeps the
 * counts above literally true at every level of progression, and it is why levelling missiles
 * no longer needs to hand back rounds: nothing it does changes how many there are.
 */
export function photonMagazine(frame: HullId = 'skiff'): number {
  return HULLS[frame].tubes
}

/**
 * Warhead yield at a given MISSILES level — what the component actually buys now.
 *
 * Per-level rather than per-round, so a fully upgraded marauder's six tubes hit for a great deal
 * and a stock skiff's single tube still hurts. Kept here rather than in `ship.ts` because
 * `ship.ts` must not import this module: `weapons.ts` already reads `laserCooldown` from it, and
 * the pair would be a cycle.
 */
export function photonDamage(missiles: number): number {
  return Math.round(PHOTON.damage * (1 + missiles * 0.3))
}

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
  /**
   * What this round carries, decided **at the muzzle**.
   *
   * Stamped when the shot is fired rather than looked up when it lands, because the MISSILES
   * component now buys yield: a round already in flight was paid for at the level the ship had
   * when it left the tube. Reading the level at impact would let a purchase made mid-flight
   * retroactively improve a missile — a small thing that would be indistinguishable, in a log,
   * from the damage table being wrong.
   */
  damage: number
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

export function newCombat(frame: HullId = 'skiff'): Combat {
  return {
    selected: 0,
    projectiles: [],
    lastFire: { laser: -1e9, photon: -1e9 },
    photonsLeft: photonMagazine(frame),
    nextId: 1,
    destroyed: [],
    hits: {},
  }
}

/**
 * Fill the tubes. Used by a dock's ordnance service and by taking delivery of a new hull.
 *
 * Missiles became *scarce* with this buff, which makes replenishing them a mechanic rather than
 * a detail — a magazine of one that never comes back is a weapon you fire once per session and
 * then forget the game has. It refills to the hull's own count, so changing ship changes the
 * answer immediately rather than at the next resupply.
 */
export function reload(c: Combat, frame: HullId): Combat {
  return { ...c, photonsLeft: photonMagazine(frame) }
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
  frame: HullId = 'skiff',
): Combat {
  const w = selected(c)
  const cooldown = w.kind === 'laser' ? laserCooldown(levels.laser, frame) : w.cooldownMs
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
    // Yield is stamped at the muzzle — see `Projectile.damage`. Only the photon scales; a laser
    // upgrade buys rate of fire, which is already spent above in `cooldown`.
    damage: w.kind === 'photon' ? photonDamage(levels.missiles) : w.damage,
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
  /** How much damage the round carried. The *swarm* decides what that does to a hull. */
  damage: number
}

/** Advance projectiles by `dt` seconds and resolve hits. Pure. */
export function step(
  c: Combat,
  dt: number,
  contacts: Contact[],
  seed: string,
  /**
   * Whether the segment a projectile crossed this frame was blocked by geometry.
   *
   * Optional so this module keeps no dependency on the collision grid — the same reason it takes
   * `contacts` rather than reaching for a `Space`. Without it a shot passes through a station and
   * kills something on the far side, which reads as the geometry being decorative.
   */
  blocked?: (from: Vec3, to: Vec3) => boolean,
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

    // Tested before the targets, so a hostile standing behind a dock is genuinely in cover.
    if (blocked && blocked(p.at, at)) continue

    let consumed = false
    for (const contact of contacts) {
      if (destroyed.includes(contact.id)) continue
      // Swept along the whole step, so a fast shot cannot pass through a target between frames.
      //
      // ## The target is a shape, not a sphere around one
      //
      // An armed craft is tested as a **capsule along its own hull**, built from the mesh the
      // renderer draws. A sphere of the class radius was the second version of this bug: the
      // `dreadnought` mesh reaches 2.1 along its nose and 0.72 across, so a sphere of radius 1
      // misses the entire prow and the entire stern while covering a lot of empty space beside
      // the ship — and the larger the hull, the worse it gets, which is exactly backwards. Fire
      // aimed squarely at the middle of a visible war hull kept missing.
      //
      // The first version of the same bug sized craft from a *signal's* magnitude while the
      // renderer sized them by class. Both are the one failure this file's oldest comment warns
      // about: a hit test that disagrees with the picture.
      //
      // An inert contact keeps the sphere, because a signal genuinely is round — its size is a
      // claim about how big a concern somebody counted, not the shape of an object.
      const size =
        contact.radius ??
        R_CONTACT + Math.round(Math.max(0, Math.min(1, contact.magnitude)) * R_CONTACT_SPAN)
      const struck =
        contact.facing && contact.shape
          ? strikes(
              capsuleOf(contact.at, contact.facing, size, contact.shape as Shape),
              p.at,
              at,
              w.calibre,
            )
          : distToSegment(contact.at, p.at, at) <= w.calibre + size
      if (struck) {
        // Counted, but **not** adjudicated. This module used to decide death here from a
        // seed-derived durability, and `enemy.ts` now owns hull and shields — two authorities
        // over one fact, which is how a craft ends up dead on one side and firing on the other.
        // A hit is a report; the swarm decides what it costs.
        hitCount[contact.id] = (hitCount[contact.id] ?? 0) + 1
        // The round's own stamped yield, not the table's. A photon fired before a MISSILES
        // upgrade must land for what it was loaded with.
        hits.push({ contact: contact.id, damage: p.damage })
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
