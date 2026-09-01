/**
 * Enemy craft: a dogfight rather than a distance check.
 *
 * ## What was wrong with the last version
 *
 * A craft snapped its velocity straight at the player and held a radius. It could not miss, it
 * could not be out-manoeuvred, and it could not be got behind, so a fight was a contest of who
 * had more hit points — decided before it started and identical every time. The fix is not more
 * numbers, it is **finite turn rate**: an opponent that must fly an arc to point at you is an
 * opponent whose arc you can beat, and every interesting thing in a dogfight follows from that
 * one constraint.
 *
 * So a craft has a facing, turns at `spec.turn` radians per second toward where it wants to
 * point, and thrusts along its nose. It cannot strafe. Neither can it stop.
 *
 * ## The behaviours, and why each exists
 *
 * - `patrol` — drifts on its own heading. A sector where everything converges on you the moment
 *   it can see you is one large fight rather than a place with dangerous regions in it.
 * - `pursue` — turn onto the player and close. The state you spend most of a fight in.
 * - `attack` — inside its standoff, holding a firing solution, **leading the target**. Leading
 *   is what makes a fast shot feel aimed rather than lucky, and what makes jinking work: the
 *   lead is computed from your current velocity, so changing it is what breaks the solution.
 * - `overshoot` — it flew past. It cannot turn on a coin, so it commits to the pass and comes
 *   back around, which is the window the player exists to notice.
 * - `evade` — hull is low. It breaks off and runs, and can be let go. A game where every
 *   encounter is to the death is a game with one verb.
 *
 * Capitals never enter `pursue` or `evade`. They hold a heading, fire on anything in arc, and
 * are fought *around* rather than chased.
 *
 * ## A ghost still never resolves, even mid-fight
 *
 * A hostile from an *estimated* signal is a craft whose strength nobody measured. Its threat
 * reads `—` for the whole engagement. The pressure to resolve it is strongest here — a player
 * being shot at wants a number — and giving them one would be inventing it. Its behaviour and
 * its class come from the seed, never from a magnitude it does not have.
 */

import type { Contact, Vec3 } from './generate.ts'
import { durability } from './weapons.ts'
import { CLASSES, classFor, SHIELD_DELAY_MS, type ClassSpec } from './classes.ts'
import { resolve, separate, steerAround, type Grid } from './collide.ts'
import {
  AGGRO_RANGE, ENGAGE_RANGE, LIFE_ENEMY_SHOT, R_PLAYER, SPEED_ENEMY_SHOT,
} from './scale.ts'

export { AGGRO_RANGE, ENGAGE_RANGE }
export const ENEMY_SHOT_SPEED = SPEED_ENEMY_SHOT

export type Behaviour = 'patrol' | 'pursue' | 'attack' | 'overshoot' | 'evade'

export interface Craft {
  /** The contact this craft is. Ids match `Space.contacts` or `Space.raiders`. */
  id: string
  spec: ClassSpec
  at: Vec3
  /** Unit vector. A craft flies where it points; it cannot strafe. */
  facing: Vec3
  /** Current speed along `facing`, so acceleration is visible rather than instant. */
  speed: number
  hull: number
  shield: number
  lastHitMs: number
  solid: boolean
  behaviour: Behaviour
  /** Milliseconds of the last shot. */
  lastFire: number
  /** Rounds left in the burst being fired. */
  burstLeft: number
  /** When the current behaviour was entered, so a pass commits for long enough to be read. */
  since: number
  alive: boolean
}

export interface EnemyShot {
  at: Vec3
  dir: Vec3
  life: number
  damage: number
}

export interface Swarm {
  craft: Craft[]
  shots: EnemyShot[]
}

// ── vector helpers ────────────────────────────────────────────────────────────

function sub(a: Vec3, b: Vec3): Vec3 {
  return { x: a.x - b.x, y: a.y - b.y, z: a.z - b.z }
}
function add(a: Vec3, b: Vec3): Vec3 {
  return { x: a.x + b.x, y: a.y + b.y, z: a.z + b.z }
}
function scale(a: Vec3, k: number): Vec3 {
  return { x: a.x * k, y: a.y * k, z: a.z * k }
}
function len(v: Vec3): number {
  return Math.hypot(v.x, v.y, v.z)
}
function norm(v: Vec3): Vec3 {
  const l = len(v) || 1
  return { x: v.x / l, y: v.y / l, z: v.z / l }
}
function dot(a: Vec3, b: Vec3): number {
  return a.x * b.x + a.y * b.y + a.z * b.z
}

/**
 * Rotate `from` toward `to` by at most `maxRad`.
 *
 * The one function the whole dogfight rests on. A slerp is the textbook answer; this is the
 * same thing built from a normalised tangent, which stays stable when the two are nearly
 * parallel — the case that occurs every frame a craft is on target, and the case where a naive
 * implementation makes it jitter.
 */
export function turnToward(from: Vec3, to: Vec3, maxRad: number): Vec3 {
  const c = Math.max(-1, Math.min(1, dot(from, to)))
  const angle = Math.acos(c)
  if (angle <= maxRad || angle < 1e-6) return to
  // Component of `to` perpendicular to `from`: the direction to rotate in.
  let perp = sub(to, scale(from, c))
  if (len(perp) < 1e-9) {
    // Exactly antiparallel — the player is directly astern. There is no unique perpendicular,
    // and the naive form yields a zero vector, so the craft freezes facing away and never comes
    // around. Any perpendicular will do; the fight only requires that it picks one.
    const seed = Math.abs(from.x) < 0.9 ? { x: 1, y: 0, z: 0 } : { x: 0, y: 1, z: 0 }
    perp = sub(seed, scale(from, dot(seed, from)))
  }
  perp = norm(perp)
  return norm(add(scale(from, Math.cos(maxRad)), scale(perp, Math.sin(maxRad))))
}

/**
 * Where to aim to hit a target moving at `targetVel`.
 *
 * A first-order lead: close enough at these speeds and — more importantly — *beatable*. An
 * exact intercept solution would connect every time against a player flying straight, which is
 * not difficulty, it is a tax on owning a keyboard.
 */
export function leadPoint(from: Vec3, target: Vec3, targetVel: Vec3, shotSpeed: number): Vec3 {
  const t = len(sub(target, from)) / shotSpeed
  return add(target, scale(targetVel, t))
}

// ── building the swarm ────────────────────────────────────────────────────────

/**
 * A 0..99 roll for a contact, from the seed.
 *
 * Its own hash rather than a reuse of `durability`, which returns one of *six* values — a class
 * drawn from it covered about half the distribution and **never produced a capital at all**, so
 * the frigates and destroyers in `classes.ts` existed and could not be met. Nothing failed; the
 * sector was simply missing its top two classes, which is exactly the kind of bug a table with
 * a plausible-looking output hides.
 *
 * Never from the contact's reported magnitude: a class derived from a record's numbers would
 * hand anybody who writes one a reason to understate them.
 */
export function classRoll(seed: string, contactId: string): number {
  let h = 2166136261 >>> 0
  for (const t of [seed, ':class:', contactId]) {
    for (let i = 0; i < t.length; i += 1) {
      h ^= t.charCodeAt(i)
      h = Math.imul(h, 16777619) >>> 0
    }
  }
  return h % 100
}

/**
 * Build the swarm from hostile contacts.
 */
export function swarmOf(contacts: Contact[], seed: string): Swarm {
  return {
    craft: contacts
      .filter((c) => c.hostility === 'hostile')
      .map((c) => {
        const roll = classRoll(seed, c.id)
        // A signal the record actually reported is never a capital. Capitals are sector
        // furniture; letting a reported signal become a destroyer would put the record's
        // contents back in charge of how hard its own sector is.
        let spec = classFor(roll)
        if (!c.unlogged && spec.capital) spec = CLASSES.gunship
        return {
          id: c.id,
          spec,
          at: c.at,
          facing: norm({
            x: (roll % 7) - 3,
            y: (roll % 5) - 2,
            z: (roll % 11) - 5 || 1,
          }),
          speed: 0,
          hull: spec.hull,
          shield: spec.shield,
          lastHitMs: -1e9,
          solid: c.solid,
          behaviour: 'patrol' as Behaviour,
          lastFire: -1e9,
          burstLeft: 0,
          since: 0,
          alive: true,
        }
      }),
    shots: [],
  }
}

export interface StepResult {
  swarm: Swarm
  /** Damage dealt to the player this step, before their shields absorb any of it. */
  damage: number
  /** Ids that fired this step, so the renderer can flash a muzzle. */
  fired: string[]
}

/** How long a craft commits to an overshoot before turning back. */
const OVERSHOOT_MS = 900

/**
 * How much further than `aggro` a craft will follow once it has engaged.
 *
 * Acquiring is harder than losing, and without the hysteresis a craft that starts with the
 * player astern is *permanently* lost: it burns away from the target for the two seconds its
 * turn takes, crosses its own aggro radius doing so, drops back to patrol, and drifts off
 * forever. A test caught exactly that — a skiff that never once fired at a stationary player
 * parked ten million units off its tail.
 */
const DISENGAGE = 1.9

/**
 * Behaviour for a craft given its situation.
 *
 * Pure and exported, so the state machine is one testable function rather than a shape that
 * emerges from the stepping code.
 */
export function decide(c: Craft, range: number, closing: number, nowMs: number): Behaviour {
  if (c.spec.capital) return range < c.spec.aggro ? 'attack' : 'patrol'
  const limit = c.behaviour === 'patrol' ? c.spec.aggro : c.spec.aggro * DISENGAGE
  if (range > limit) return 'patrol'
  // Below a third of hull it breaks off. Being able to *let one go* is what stops every
  // encounter being to the death.
  if (c.hull < c.spec.hull * 0.33) return 'evade'
  if (c.behaviour === 'overshoot' && nowMs - c.since < OVERSHOOT_MS) return 'overshoot'
  // Flew past: close, and now genuinely moving away. It cannot turn on a coin, so it commits to
  // the pass — and that commitment is the window the player is meant to notice and use. The
  // threshold is on the *standoff* band rather than a bare radius, so it moved when the attack
  // band widened; pinned tight to the old number it simply stopped firing and the manoeuvre
  // quietly disappeared from the game.
  if (range < c.spec.standoff * 1.5 && closing < -0.2) return 'overshoot'
  // A wide attack band, and the reason is geometry rather than taste. A craft's turn radius is
  // `speed / turn`, and at cruise every class here has one two or three times its own standoff —
  // so a craft that only entered `attack` once it was *already* inside standoff could never get
  // there: it orbited at constant range, never closing and never giving up, which is exactly
  // what a gunship parked at nineteen million units did for two and a half minutes. Entering
  // `attack` early lets the eased-off throttle below shrink the turn radius enough to actually
  // arrive.
  if (range < c.spec.standoff * 3.5) return 'attack'
  return 'pursue'
}

/**
 * Advance the swarm.
 *
 * `playerVel` is needed for the lead. Passing zero aims every shot at where the player *is*,
 * which at these speeds means everything misses — a silent way to make the game trivial.
 */
export function step(
  swarm: Swarm,
  playerAt: Vec3,
  playerVel: Vec3,
  dt: number,
  nowMs: number,
  grid?: Grid,
): StepResult {
  const craft: Craft[] = []
  const shots: EnemyShot[] = []
  const fired: string[] = []
  let damage = 0

  for (const c of swarm.craft) {
    if (!c.alive) {
      craft.push(c)
      continue
    }
    const toPlayer = sub(playerAt, c.at)
    const range = len(toPlayer)
    const bearing = norm(toPlayer)
    // Positive when the craft is getting closer along its own nose.
    const closing = dot(c.facing, bearing)

    const behaviour = decide(c, range, closing, nowMs)
    const since = behaviour === c.behaviour ? c.since : nowMs

    // Where it wants to point, and how hard it is pushing.
    let want = c.facing
    let throttle = 0
    switch (behaviour) {
      case 'patrol':
        throttle = 0.25
        break
      case 'pursue':
        want = bearing
        // Full burn unless it is pointing the wrong way. A craft at full throttle while facing
        // away flies *away* from what it is chasing for as long as its turn takes, which at
        // these speeds loses the target entirely — but scaling smoothly with alignment was the
        // over-correction: a craft circling at constant range reads a closing rate near zero
        // forever, throttles down to a crawl, and never converges.
        throttle = closing > 0.1 ? 1 : 0.25
        break
      case 'attack':
        want = norm(sub(leadPoint(c.at, playerAt, playerVel, ENEMY_SHOT_SPEED), c.at))
        // Eased off as it arrives, which is what lets it hold a firing position instead of
        // sailing past. It is also the only thing that makes the position reachable at all:
        // turn radius is `speed / turn`, so a third of the throttle is a third of the radius.
        throttle = range < c.spec.standoff * 1.5 ? 0.3 : 0.75
        break
      case 'overshoot':
        // Committed to the pass: it keeps its heading and burns through.
        throttle = 1
        break
      case 'evade':
        want = scale(bearing, -1)
        throttle = 1
        break
    }

    // Obstacle avoidance overrides the tactical intent, and does so *before* the turn rather
    // than after the move. A craft that only stops when it touches something has visibly given
    // up on the fight; one that starts its turn while it can still make it looks piloted. The
    // lookahead is in seconds, so a destroyer begins earlier than an interceptor without either
    // number being tuned by hand.
    if (grid) {
      const swerve = steerAround(
        grid,
        c.at,
        c.facing,
        want,
        c.spec.radius,
        Math.max(c.speed, c.spec.speed * 0.4),
      )
      want = swerve.dir
      // Ease off in proportion to how hard it is having to bend. Full burn into a turn you are
      // making in order to miss something is how a craft clips the thing it was avoiding — and a
      // flat cap here is what made an earlier version crawl for the rest of the fight.
      if (swerve.urgency > 0) throttle = Math.min(throttle, 1 - swerve.urgency * 0.5)
    }

    const facing = turnToward(c.facing, want, c.spec.turn * dt)
    // Acceleration rather than a snapped velocity, so a craft has mass and a heavy one reads
    // as heavy.
    const target = c.spec.speed * throttle
    const accel = c.spec.speed * (c.spec.capital ? 0.35 : 1.6) * dt
    const speed = c.speed + Math.max(-accel, Math.min(accel, target - c.speed))
    const wanted = add(c.at, scale(facing, speed * dt))
    // Avoidance is a heuristic and heuristics miss. The hard resolve is what guarantees a craft
    // is never *inside* a station — a wireframe hull sitting in the middle of a dock is the
    // single cheapest-looking thing this renderer can produce.
    const moved = grid ? resolve(grid, c.at, wanted, c.spec.radius, dt) : null
    const at = moved ? moved.at : wanted

    // ── firing ────────────────────────────────────────────────────────────────
    let lastFire = c.lastFire
    let burstLeft = c.burstLeft
    const aim = norm(sub(leadPoint(at, playerAt, playerVel, ENEMY_SHOT_SPEED), at))
    // It only fires while actually pointing at the solution. A craft that can shoot sideways
    // makes manoeuvre pointless, which is the entire game here.
    const onTarget = dot(facing, aim) > 0.985
    const canFire = behaviour === 'attack' && range < c.spec.aggro && onTarget
    // Within a burst the rounds come fast; between bursts is the full cooldown. That rhythm is
    // most of what makes incoming fire feel like an event rather than a drip.
    const gap = burstLeft > 0 ? 90 : c.spec.cooldownMs
    if (canFire && nowMs - lastFire > gap) {
      if (burstLeft <= 0) burstLeft = c.spec.burst
      shots.push({ at, dir: aim, life: LIFE_ENEMY_SHOT, damage: c.spec.damage })
      fired.push(c.id)
      lastFire = nowMs
      burstLeft -= 1
    }

    // Shields recover between passes, exactly as the player's do.
    const shield =
      nowMs - c.lastHitMs < SHIELD_DELAY_MS
        ? c.shield
        : Math.min(c.spec.shield, c.shield + c.spec.shieldRegen * dt)

    craft.push({
      ...c,
      at,
      facing,
      // A craft that struck something loses its way on. Keeping the speed would have it grinding
      // against the surface at full burn for as long as it stayed pointed at it.
      speed: moved && moved.hit ? speed * 0.5 : speed,
      behaviour,
      since,
      lastFire,
      burstLeft,
      shield,
    })
  }

  // Craft are kept out of each other positionally rather than by steering. A craft that merely
  // steers away still interpenetrates while it turns, and two wireframes occupying one point is
  // exactly what reads as cheap. Applied after movement so it corrects the frame's own overlaps.
  const live = craft.filter((c) => c.alive)
  if (live.length > 1) {
    const push = separate(live.map((c) => ({ at: c.at, radius: c.spec.radius })))
    live.forEach((c, i) => {
      const p = push[i]
      if (p.x === 0 && p.y === 0 && p.z === 0) return
      const shoved = { x: c.at.x + p.x, y: c.at.y + p.y, z: c.at.z + p.z }
      // Separation runs *after* the obstacle resolve, so it can shove a craft straight back into
      // the station it was just pushed out of — which is how a lancer ended up sitting inside a
      // dock with the collision system working correctly at every individual step. Re-resolving
      // makes the station the authority: two craft may end up closer than they would like, and
      // neither ends up inside the furniture.
      c.at = grid ? resolve(grid, c.at, shoved, c.spec.radius, dt).at : shoved
    })
  }

  // Advance existing shots and resolve those that reach the player.
  for (const s of swarm.shots) {
    const at = add(s.at, scale(s.dir, ENEMY_SHOT_SPEED * dt))
    const life = s.life - dt
    // Swept, for the same reason player shots are: at this speed an endpoint test misses.
    if (nearSegment(playerAt, s.at, at) <= R_PLAYER) {
      damage += s.damage
      continue
    }
    // Geometry stops enemy fire as well as yours, which is what makes a station cover rather
    // than scenery — and it has to be the same rule in both directions or the player learns that
    // hiding works only for the other side.
    if (grid && resolve(grid, s.at, at, 0, dt).hit) continue
    if (life > 0) shots.push({ at, dir: s.dir, life, damage: s.damage })
  }

  return { swarm: { craft, shots }, damage, fired }
}

function nearSegment(p: Vec3, a: Vec3, b: Vec3): number {
  const ab = sub(b, a)
  const ap = sub(p, a)
  const l2 = dot(ab, ab)
  if (l2 === 0) return len(sub(p, a))
  const t = Math.max(0, Math.min(1, dot(ap, ab) / l2))
  return len(sub(p, add(a, scale(ab, t))))
}

export interface HitResult {
  swarm: Swarm
  killed: boolean
  /** True when the hit reached hull rather than being absorbed — the HUD reacts differently. */
  throughShield: boolean
  /** Salvage owed for a kill. Class-derived; zero when nothing died. */
  bounty: number
}

/** Register a player hit. Shields absorb first, exactly as the player's do. */
export function hit(swarm: Swarm, id: string, amount: number, nowMs: number): HitResult {
  let killed = false
  let throughShield = false
  let bounty = 0
  const craft = swarm.craft.map((c) => {
    if (c.id !== id || !c.alive) return c
    const absorbed = Math.min(c.shield, amount)
    const toHull = amount - absorbed
    throughShield = toHull > 0
    const hull = c.hull - toHull
    if (hull <= 0) {
      killed = true
      bounty = c.spec.bounty
      return { ...c, shield: 0, hull: 0, alive: false, lastHitMs: nowMs }
    }
    return { ...c, shield: c.shield - absorbed, hull, lastHitMs: nowMs }
  })
  return { swarm: { ...swarm, craft }, killed, throughShield, bounty }
}

/** Live craft, for rendering and for targeting. */
export function living(swarm: Swarm): Craft[] {
  return swarm.craft.filter((c) => c.alive)
}

/** The nearest live hostile — for the sensor panel and for the jump inhibitor. */
export function nearestThreat(swarm: Swarm, at: Vec3): { craft: Craft; range: number } | null {
  let best: { craft: Craft; range: number } | null = null
  for (const c of living(swarm)) {
    const range = len(sub(c.at, at))
    if (!best || range < best.range) best = { craft: c, range }
  }
  return best
}
