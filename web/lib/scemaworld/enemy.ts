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
import { civilian, hostileTo, nextStop, routeNodes, type Civilian, type Faction } from './factions.ts'
import type { Node, Space } from './generate.ts'
import { separate, steerAround, type Grid } from './collide.ts'
import {
  AGGRO_RANGE, ENGAGE_RANGE, LIFE_ENEMY_SHOT, R_PLAYER, SPEED_ENEMY_SHOT,
} from './scale.ts'

export { AGGRO_RANGE, ENGAGE_RANGE }
export const ENEMY_SHOT_SPEED = SPEED_ENEMY_SHOT

export type Behaviour = 'patrol' | 'pursue' | 'attack' | 'overshoot' | 'evade'

export interface Craft {
  /** The contact this craft is. Ids match `Space.contacts`, `Space.raiders`, or traffic. */
  id: string
  /**
   * Who it flies for.
   *
   * Drives *everything* about its behaviour, because "hostile" was never one question. A raider
   * wants the player; a marshal wants the nearest raider and does not care about the player at
   * all; a courier wants to be somewhere else. One flag decides which of those a craft is, and
   * `hostileTo` is the only place that decides whether it will shoot at you.
   */
  faction: Faction
  /** Node it is routing to, for traffic. Null for anything that fights. */
  destination: number | null
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
  /**
   * Range to the player at the end of the previous step.
   *
   * The only way to tell that a craft has *flown past*. The obvious signal is alignment — the
   * player being behind the nose — and it does not work: a fighter turns fast enough that it is
   * always pointing at the player, so alignment inside the standoff band never once dropped
   * below 0.74 across a forty-second engagement, and `overshoot` was a state the state machine
   * could not reach. Range rate is the physical fact; alignment was a proxy for it that holds
   * only for something that turns slowly.
   */
  lastRange: number
  /**
   * The craft this one is fighting, or `null` for the player (or for nothing).
   *
   * **Remembered, not recomputed every frame**, and that is two things at once. As AI it is
   * target *commitment*: a fighter that re-picked the nearest opponent sixty times a second
   * oscillates between two equidistant enemies and closes on neither, which reads as indecision
   * and plays as a ship that never arrives. As arithmetic it is what makes a bigger roster
   * affordable — the opponent search is linear over the swarm, so doing it per craft per frame is
   * quadratic, and at two hundred craft that is forty thousand distance checks every tick.
   */
  target: string | null
  /** When this craft may look for a better target. Staggered per craft; see `RETARGET_MS`. */
  retargetMs: number
  alive: boolean
}

/**
 * A round in flight that the player did not fire.
 *
 * ## Every shot names who fired it and what it is aimed at, and that is what made firefights visible
 *
 * A marshal's rounds used to never exist. Its damage was applied directly to its quarry, and the
 * stated reason was a real one: a stray friendly round hitting the player would make an ally
 * indistinguishable from an enemy at the only moment it counts. But the cost was that the sector's
 * ambient violence was **invisible** — raider counts fell over time and nothing was ever on screen
 * to explain it, so the one thing that makes the place feel inhabited rather than staged happened
 * entirely in the arithmetic.
 *
 * Carrying a `target` solves the original problem exactly, and better than hiding the round did: a
 * shot is resolved against the one craft it was aimed at, or against the player when `target` is
 * null. A marshal's round *cannot* hit the player because it is not aimed at them, not because it
 * was never drawn. And `owner` lets the renderer colour it, so a distant exchange reads as
 * yellow-into-orange rather than as two dots near each other.
 *
 * The honest limitation, stated because it is a real one: a shot passes through anything that is
 * not its target. That is the same rule the player's own fire follows — nothing is cover, for
 * either side — so it is at least symmetric, which is the property that matters most here.
 */
export interface EnemyShot {
  at: Vec3
  dir: Vec3
  life: number
  damage: number
  /** Who fired it. Drives colour only; it has no effect on what the round can hit. */
  owner: Faction
  /** The craft it was aimed at, or `null` for one aimed at the player. */
  target: string | null
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
 * Per *mille* rather than per cent, so the table can express something rarer than one in a
 * hundred. The titan needs it, and a hundred buckets is how the titan was unreachable too.
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
  return h % 1000
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
        // A named class, for the sector's own capital garrison. Honoured **only** for an
        // unlogged contact — everything the record reported keeps its rolled class, so this
        // cannot become a way for a producer to name its own opposition. An unrecognised name
        // falls through to the roll rather than throwing: the field is typed `string` on purpose
        // (see `generate.ts`), and a bad value must be inert.
        const named = Boolean(c.unlogged && c.klass && c.klass in CLASSES)
        if (named) spec = CLASSES[c.klass as keyof typeof CLASSES]
        // ## Only a *named* class may be a capital
        //
        // This used to read "a signal the record reported is never a capital", which left rolled
        // raiders able to become one — so a wing of four could contain a destroyer, and the
        // sector's war classes arrived partly by garrison and partly by lottery. Two consequences,
        // both bad: reinforcement could quietly hand back a capital the player had spent minutes
        // killing, and "how many capitals does a sector have" had no answer.
        //
        // The rule is now flat: **a capital is placed, a fighter is rolled.** The garrison
        // (`raiders.ts::GARRISON`) is the whole of the sector's heavy hostile presence, it is the
        // same in every world, and nothing replaces it. The original protection is strictly
        // contained in this — a record's own signal is never named, so it is never a capital.
        if (spec.capital && !named) spec = CLASSES.gunship
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
          faction: 'raider' as Faction,
          destination: null,
          behaviour: 'patrol' as Behaviour,
          lastFire: -1e9,
          burstLeft: 0,
          since: 0,
          lastRange: Infinity,
          target: null,
          retargetMs: -1e9,
          alive: true,
        }
      }),
    shots: [],
  }
}

/** Fold the sector's traffic into a swarm, so one loop steps everything that flies. */
export function withTraffic(swarm: Swarm, traffic: Civilian[]): Swarm {
  return {
    ...swarm,
    craft: [
      ...swarm.craft,
      ...traffic.map((c) => ({
        id: c.id,
        faction: c.faction,
        destination: c.destination,
        spec: c.spec,
        at: c.at,
        facing: { x: 0, y: 0, z: 1 },
        speed: 0,
        hull: c.spec.hull,
        shield: c.spec.shield,
        lastHitMs: -1e9,
        solid: true,
        behaviour: 'patrol' as Behaviour,
        lastFire: -1e9,
        burstLeft: 0,
        since: 0,
        lastRange: Infinity,
        target: null,
        retargetMs: -1e9,
        alive: true,
      })),
    ],
  }
}

/**
 * Fold new craft into a live swarm. How reinforcement arrives.
 *
 * Ids already present are dropped rather than duplicated. That is not defensive tidiness: the
 * respawn caller counts what is *alive* and raises the difference, so a wave arriving on the same
 * frame a craft is recorded dead is an ordinary race rather than a bug, and two craft sharing an
 * id would be a genuine one — every id-keyed structure in the game (hit resolution, hit flashes,
 * the sensor board, a projectile's target) would then be describing the wrong ship half the time.
 */
export function reinforce(swarm: Swarm, added: Craft[]): Swarm {
  const known = new Set(swarm.craft.map((c) => c.id))
  const fresh = added.filter((c) => !known.has(c.id))
  if (fresh.length === 0) return swarm
  return { ...swarm, craft: [...swarm.craft, ...fresh] }
}

export interface StepResult {
  swarm: Swarm
  /** Damage dealt to the player this step, before their shields absorb any of it. */
  damage: number
  /** Ids that fired this step, so the renderer can flash a muzzle. */
  fired: string[]
}

/** Live craft of a given faction. */
export function of(swarm: Swarm, faction: Faction): Craft[] {
  return swarm.craft.filter((c) => c.alive && c.faction === faction)
}

/** How long a craft commits to an overshoot before turning back. */
const OVERSHOOT_MS = 900

/**
 * How much the range must grow in one step to count as having flown past.
 *
 * A plain "range increased" flickers: a craft holding station at a third throttle oscillates
 * around its standoff by a few parts in a thousand, and with the commit above each wobble would
 * lock it into nine hundred milliseconds of flying away. Two percent in a sixtieth of a second is
 * a departure, not a wobble.
 */
const RECEDING = 1.02

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
  // Flew past: close, and the range is now opening. It cannot turn on a coin, so it commits to
  // the pass, and that commitment is the window the player is meant to notice and use.
  //
  // The test is on **range rate**, not on where the nose points. Pointing was the obvious signal
  // and it never fired: a fighter turns fast enough to keep the player in front throughout the
  // pass, so alignment inside the band never dropped below 0.74 and the manoeuvre was quietly
  // absent from every engagement. `closing` is still taken for the signature's sake and for
  // capitals, which turn slowly enough that it means what it looks like.
  if (range < c.spec.standoff * 2 && range > c.lastRange * RECEDING) return 'overshoot'
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
/**
 * Whether two factions will shoot at each other. The only place that question is answered.
 *
 * Marshals and raiders, and nothing else. Couriers and freighters are unarmed and a marshal has
 * no quarrel with them; two raiders do not fight, or a sector's hostiles would consume themselves
 * before anybody arrived.
 */
export function opposes(a: Faction, b: Faction): boolean {
  return (a === 'marshal' && b === 'raider') || (a === 'raider' && b === 'marshal')
}

/**
 * The nearest live craft of an opposing faction, within `reach`.
 *
 * Linear over the swarm, which is a couple of hundred entries. A spatial index here would be a
 * structure to keep correct in exchange for a fraction of a millisecond.
 */
export function nearestOpponent(swarm: Swarm, c: Craft, reach: number): Craft | null {
  let best: Craft | null = null
  let bestD = reach
  for (const other of swarm.craft) {
    if (!other.alive || other.id === c.id) continue
    if (!opposes(c.faction, other.faction)) continue
    const d = len(sub(other.at, c.at))
    if (d < bestD) {
      bestD = d
      best = other
    }
  }
  return best
}

/**
 * The nearest live craft of the same faction, for a fleeing craft to run toward.
 *
 * Deliberately unbounded in range — a craft with nowhere to run gets `null` only when its whole
 * faction is dead, and at that point running in a straight line is as good an answer as any.
 */
function nearestAlly(swarm: Swarm, c: Craft): Craft | null {
  let best: Craft | null = null
  let bestD = Infinity
  for (const other of swarm.craft) {
    if (!other.alive || other.id === c.id || other.faction !== c.faction) continue
    const d = len(sub(other.at, c.at))
    if (d < bestD) {
      bestD = d
      best = other
    }
  }
  return best
}

/**
 * How much further a marshal will reach for a raider than it will engage one.
 *
 * Generous, because hunting is its whole job and a patrol that only notices what flies into it is
 * indistinguishable from one that is not looking.
 */
const MARSHAL_REACH = 2.5

/**
 * How far a raider will look for a marshal to fight back against.
 *
 * Deliberately tighter than the marshal's reach. A raider is here for the player and turns on the
 * patrol only when the patrol is genuinely on it — the asymmetry is what stops the sector's
 * ambient violence from becoming the whole of the sector, and it is what leaves the player as the
 * thing raiders are for.
 */
const RAIDER_REACH = 1.2

/**
 * What a craft is currently interested in: where it is, how fast, and which craft it is — or
 * `null` for the player.
 *
 * ## Why this exists as its own function
 *
 * The steering, the shot lead and the fire gate each independently decided who the subject was,
 * and two of the three answered "the player" unconditionally. So a marshal was hunting a raider
 * with `nearestEnemyOf` while *flying at the player* and computing a firing solution on the
 * player, and only the damage went to the raider. Nothing looked wrong on screen because nothing
 * was on screen — the rounds were invisible. One function, consulted once per craft per step, is
 * what makes "who is this ship fighting" a single fact rather than three coincidences.
 */
/**
 * How long a craft holds a target before looking for a better one.
 *
 * Long enough to be *commitment* rather than a poll. Re-picking the nearest opponent every frame
 * makes a fighter oscillate between two equidistant enemies and arrive at neither — the classic
 * nearest-target flip-flop, which reads as indecision and plays as a ship that never closes.
 *
 * The retarget clocks are staggered by a hash of the craft's id (`retargetPhase`), so two hundred
 * craft do not all run their opponent search on the same frame. Without the stagger the amortised
 * cost is the same and the *worst* frame is the whole quadratic, which is what a frame-time budget
 * actually cares about.
 */
const RETARGET_MS = 900

function retargetPhase(id: string): number {
  let h = 2166136261 >>> 0
  for (let i = 0; i < id.length; i += 1) {
    h ^= id.charCodeAt(i)
    h = Math.imul(h, 16777619) >>> 0
  }
  return h % RETARGET_MS
}

/**
 * A per-craft offset perpendicular to its approach, so a wing converges from several angles.
 *
 * Four craft all steering at the same point arrive in a queue along one line: the first one is in
 * the fight and the other three are behind it, contributing nothing and unable to shoot without
 * hitting each other. Offsetting each one's *aim point* by a fixed, deterministic vector spreads
 * the approach into a pincer without any craft needing to know what the others are doing — the
 * cheapest possible flocking, and the only kind that stays deterministic.
 *
 * Derived from the id, so a given craft always flanks from the same side and the manoeuvre is
 * reproducible for two players holding one record.
 */
export function flankOffset(id: string, spread: number): Vec3 {
  let h = 2166136261 >>> 0
  for (let i = 0; i < id.length; i += 1) {
    h ^= id.charCodeAt(i)
    h = Math.imul(h, 16777619) >>> 0
  }
  // Three coprime-ish moduli so the components do not repeat together, mapped to [-1, 1].
  const x = ((h % 13) - 6) / 6
  const y = (((h >>> 4) % 9) - 4) / 4
  const z = (((h >>> 8) % 11) - 5) / 5
  const l = Math.hypot(x, y, z) || 1
  return { x: (x / l) * spread, y: (y / l) * spread, z: (z / l) * spread }
}

function focusOf(
  swarm: Swarm,
  c: Craft,
  playerAt: Vec3,
  playerVel: Vec3,
  nowMs: number,
  byId: Map<string, Craft>,
  space?: Space,
): { at: Vec3; vel: Vec3; target: string | null; routeTo: Node | null; retargetMs: number } {
  if (civilian(c.faction)) {
    const route = space ? routeNodes(space, c.faction) : []
    const routeTo = route.find((n) => n.id === c.destination) ?? route[0] ?? null
    return {
      at: routeTo ? routeTo.at : add(c.at, scale(c.facing, c.spec.aggro * 4)),
      vel: { x: 0, y: 0, z: 0 },
      target: null,
      routeTo,
      retargetMs: c.retargetMs,
    }
  }

  const reach = c.spec.aggro * (c.faction === 'marshal' ? MARSHAL_REACH : RAIDER_REACH)

  // Hold the current quarry while it is alive and still in reach. This is the commitment half of
  // `RETARGET_MS`: a craft does not drop a target simply because another drifted a little nearer.
  const held = c.target ? byId.get(c.target) : null
  const holdable = held?.alive && len(sub(held.at, c.at)) < reach * 1.4
  const due = nowMs >= c.retargetMs

  let quarry: Craft | null = holdable ? held! : null
  let retargetMs = c.retargetMs
  if (!quarry || due) {
    const found = nearestOpponent(swarm, c, reach)
    // A held target is only displaced by something meaningfully closer, or the search re-creates
    // the flip-flop it exists to prevent.
    if (found && (!quarry || len(sub(found.at, c.at)) < len(sub(quarry.at, c.at)) * 0.7)) {
      quarry = found
    }
    retargetMs = nowMs + RETARGET_MS + retargetPhase(c.id)
  }

  if (c.faction === 'marshal') {
    // Nothing to hunt: hold a patrol heading rather than drifting toward the player, which would
    // look exactly like a hostile closing in.
    if (!quarry) {
      return {
        at: add(c.at, scale(c.facing, c.spec.aggro * 4)),
        vel: { x: 0, y: 0, z: 0 },
        target: null,
        routeTo: null,
        retargetMs,
      }
    }
    return {
      at: quarry.at,
      vel: scale(quarry.facing, quarry.speed),
      target: quarry.id,
      routeTo: null,
      retargetMs,
    }
  }

  // A raider. It fights back against a marshal that is genuinely on it, and otherwise it is here
  // for the player. "Genuinely on it" is decided by range: whichever of the two is nearer, with
  // the marshal held to the tighter reach above.
  if (quarry && len(sub(quarry.at, c.at)) < len(sub(playerAt, c.at))) {
    return {
      at: quarry.at,
      vel: scale(quarry.facing, quarry.speed),
      target: quarry.id,
      routeTo: null,
      retargetMs,
    }
  }
  return { at: playerAt, vel: playerVel, target: null, routeTo: null, retargetMs }
}

export function step(
  swarm: Swarm,
  playerAt: Vec3,
  playerVel: Vec3,
  dt: number,
  nowMs: number,
  grid?: Grid,
  space?: Space,
): StepResult {
  const craft: Craft[] = []
  const shots: EnemyShot[] = []
  const fired: string[] = []
  let damage = 0

  // Built once and shared by every craft's `focusOf`, so holding a remembered target is a map
  // lookup rather than a scan. Positions are last frame's, which is exactly right: a craft decides
  // who it is fighting from what it could see at the start of the step, not from where everything
  // ended up after it.
  const before = new Map(swarm.craft.map((k) => [k.id, k]))

  for (const c of swarm.craft) {
    if (!c.alive) {
      craft.push(c)
      continue
    }

    // ── who this craft is interested in ──────────────────────────────────────
    // One answer, used by the steering, the lead and the fire gate alike. See `focusOf` for what
    // went wrong when each of the three decided for itself.
    const focus = focusOf(swarm, c, playerAt, playerVel, nowMs, before, space)
    const focusAt = focus.at
    const focusVel = focus.vel
    const routeTo = focus.routeTo

    const toFocus = sub(focusAt, c.at)
    const range = len(toFocus)
    const bearing = norm(toFocus)
    // Positive when the craft is getting closer along its own nose.
    const closing = dot(c.facing, bearing)

    // Traffic does not fight. It flies its route at cruise and runs when something armed is
    // close — which is the whole of its behaviour, and deliberately so: a courier that duelled
    // would be a fighter with a different colour.
    const behaviour: Behaviour = civilian(c.faction)
      ? 'pursue'
      : decide(c, range, closing, nowMs)
    const since = behaviour === c.behaviour ? c.since : nowMs

    // Where it wants to point, and how hard it is pushing.
    let want = c.facing
    let throttle = 0
    switch (behaviour) {
      case 'patrol':
        throttle = 0.25
        break
      case 'pursue':
        // Steered at a point *offset* from the target rather than at the target itself, so a wing
        // converges as a pincer instead of a queue. Four craft aimed at one point arrive in single
        // file: the leader fights and the other three sit behind it unable to shoot past it. The
        // offset shrinks as the range closes, so the approach is a spread that collapses into a
        // firing position rather than a permanent miss.
        want = norm(
          sub(
            add(focusAt, flankOffset(c.id, Math.min(range * 0.35, c.spec.standoff * 2))),
            c.at,
          ),
        )
        // Full burn unless it is pointing the wrong way. A craft at full throttle while facing
        // away flies *away* from what it is chasing for as long as its turn takes, which at
        // these speeds loses the target entirely — but scaling smoothly with alignment was the
        // over-correction: a craft circling at constant range reads a closing rate near zero
        // forever, throttles down to a crawl, and never converges.
        throttle = closing > 0.1 ? 1 : 0.25
        break
      case 'attack':
        // Led onto **the focus**, not onto the player. It used to be the player unconditionally,
        // which meant a marshal hunting a raider flew at the player the whole time it was doing
        // it — a friendly patrol that reads, from the cockpit, as something closing on you.
        want = norm(sub(leadPoint(c.at, focusAt, focusVel, ENEMY_SHOT_SPEED), c.at))
        // Eased off as it arrives, which is what lets it hold a firing position instead of
        // sailing past. It is also the only thing that makes the position reachable at all:
        // turn radius is `speed / turn`, so a third of the throttle is a third of the radius.
        // Eased only once it is *at* the standoff, not on the way in. Braking early made the
        // approach so gentle that `overshoot` stopped happening at all — the manoeuvre was still
        // in the state machine and no craft ever entered it, which is the quietest way for a
        // behaviour to disappear from a game.
        throttle = range < c.spec.standoff ? 0.3 : 0.75
        break
      case 'overshoot':
        // Committed to the pass: it keeps its heading and burns through.
        throttle = 1
        break
      case 'evade':
        // Away from what is shooting at it, and — when there is one — *toward a friend*. A craft
        // that flees in a straight line is a craft that dies tired; one that runs toward its own
        // side drags the pursuer into somebody else's guns, which is both better play and the
        // thing that makes a scattered fight collapse back into a real engagement.
        {
          const refuge = nearestAlly(swarm, c)
          want = refuge
            ? norm(add(scale(bearing, -1), scale(norm(sub(refuge.at, c.at)), 1.2)))
            : scale(bearing, -1)
        }
        throttle = 1
        break
    }

    // Obstacle avoidance overrides the tactical intent, and does so *before* the turn rather
    // than after the move. A craft that only stops when it touches something has visibly given
    // up on the fight; one that starts its turn while it can still make it looks piloted. The
    // lookahead is in seconds, so a destroyer begins earlier than an interceptor without either
    // number being tuned by hand.
    // Capitals do not swerve. A leviathan is fifteen times a station across, so "avoiding" one
    // is neither believable nor cheap — the probe box covers a large fraction of the sector and
    // the query walks hundreds of cells for a manoeuvre nothing would credit anyway.
    if (grid && !c.spec.capital) {
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

    // Traffic that has arrived picks its next stop, deterministically from its own id and the
    // node it just left. A random pick would make two players' sectors diverge the moment
    // anything docked — the same failure the placement rules exist to prevent, delayed a minute.
    let destination = c.destination
    if (civilian(c.faction) && space && routeTo && range < c.spec.radius * 30) {
      destination = nextStop(routeNodes(space, c.faction), c.id, routeTo.id)
    }

    const facing = turnToward(c.facing, want, c.spec.turn * dt)
    // Acceleration rather than a snapped velocity, so a craft has mass and a heavy one reads
    // as heavy.
    const target = c.spec.speed * throttle
    const accel = c.spec.speed * (c.spec.capital ? 0.35 : 1.6) * dt
    const speed = c.speed + Math.max(-accel, Math.min(accel, target - c.speed))
    // Nodes are open structures, so a craft passes through one exactly as the player does. It
    // still *steers* around them above, which costs nothing and reads as piloting; hard-stopping
    // it here would trap a craft inside a station it can legitimately occupy — and record-signal
    // craft start inside one, since contacts sit on nodes.
    const at = add(c.at, scale(facing, speed * dt))

    // ── firing ────────────────────────────────────────────────────────────────
    let lastFire = c.lastFire
    let burstLeft = c.burstLeft
    // Aimed at the focus, for the same reason the steering is. A craft that steers at one thing
    // and shoots at another is not a pilot, it is two bugs agreeing.
    const aim = norm(sub(leadPoint(at, focusAt, focusVel, ENEMY_SHOT_SPEED), at))
    // It only fires while actually pointing at the solution. A craft that can shoot sideways
    // makes manoeuvre pointless, which is the entire game here.
    const onTarget = dot(facing, aim) > 0.985
    // Unarmed factions never fire, whatever state they are in. Checked on `damage` rather than
    // on the faction so a class with a gun cannot be made peaceful by accident, or the reverse.
    const canFire =
      c.spec.damage > 0 && behaviour === 'attack' && range < c.spec.aggro && onTarget
    // Within a burst the rounds come fast; between bursts is the full cooldown. That rhythm is
    // most of what makes incoming fire feel like an event rather than a drip.
    const gap = burstLeft > 0 ? 90 : c.spec.cooldownMs
    if (canFire && nowMs - lastFire > gap) {
      if (burstLeft <= 0) burstLeft = c.spec.burst
      // Every round is a projectile now, including a marshal's. It carries the id of the one
      // craft it can hit (`null` meaning the player), so a friendly round is incapable of
      // hurting you *and* is visible while it fails to — which is the entire reason a firefight
      // between two other parties is something you can see happening. See `EnemyShot`.
      shots.push({
        at,
        dir: aim,
        life: LIFE_ENEMY_SHOT,
        damage: c.spec.damage,
        owner: c.faction,
        target: focus.target,
      })
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
      speed,
      behaviour,
      destination,
      since,
      lastFire,
      burstLeft,
      shield,
      lastRange: range,
      target: focus.target,
      retargetMs: focus.retargetMs,
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
      c.at = { x: c.at.x + p.x, y: c.at.y + p.y, z: c.at.z + p.z }
    })
  }

  // Advance existing shots and resolve each against the one thing it was aimed at.
  //
  // Resolved after every craft has moved, so a round cannot kill something this loop has not
  // finished stepping — and so a shot is tested against where its target *is* this frame rather
  // than where it was.
  const byId = new Map(craft.map((c) => [c.id, c]))
  const craftHits: { id: string; damage: number }[] = []
  for (const s of swarm.shots) {
    const at = add(s.at, scale(s.dir, ENEMY_SHOT_SPEED * dt))
    const life = s.life - dt
    if (s.target === null) {
      // Swept, for the same reason player shots are: at this speed an endpoint test misses.
      if (nearSegment(playerAt, s.at, at) <= R_PLAYER) {
        damage += s.damage
        continue
      }
    } else {
      const t = byId.get(s.target)
      // A round whose target died keeps flying and expires. Deleting it would be tidier and
      // would make a kill silently swallow the rounds already in the air toward it, which is
      // the sort of small dishonesty that adds up into a fight not matching what you saw.
      if (t && t.alive && nearSegment(t.at, s.at, at) <= t.spec.radius) {
        craftHits.push({ id: s.target, damage: s.damage })
        continue
      }
    }
    // Nothing blocks fire, for anyone. A wireframe frame is not cover, and what matters is that
    // the rule is *symmetric*: a player who learns that hiding works only for the other side has
    // learned something worse than either consistent answer.
    if (life > 0) shots.push({ ...s, at, life })
  }

  let out: Swarm = { craft, shots }
  for (const h of craftHits) {
    out = hit(out, h.id, h.damage, nowMs).swarm
  }

  return { swarm: out, damage, fired }
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
  // Only factions that will actually shoot at *you*. A marshal on the sensor board is not a
  // threat, and counting one would inhibit the jump drive because a friendly patrol flew past —
  // which is the kind of bug that reads as the mechanic being broken.
  for (const c of living(swarm).filter((k) => hostileTo(k.faction))) {
    const range = len(sub(c.at, at))
    if (!best || range < best.range) best = { craft: c, range }
  }
  return best
}
