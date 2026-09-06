/**
 * Hyperspace arrivals: reinforcements that come *in*, where you can watch them do it.
 *
 * ## Why this replaces "spawn beyond sensor range"
 *
 * The first version of reinforcement placed a wave outside sensor range, and the stated reason was
 * a good one: a ship appearing inside it is the clearest possible statement that nothing on screen
 * is real. But it made the entire mechanic invisible. The sector's population was maintained by
 * bookkeeping — craft blinked into a volume nobody was looking at, then took a minute to fly
 * somewhere, and the player's only evidence that the sector was alive was that it had not gone
 * quiet. A mechanic you can only detect by its absence is not a mechanic.
 *
 * A hyperspace entry dissolves the objection rather than trading against it. The problem with a
 * ship materialising in front of you is not the *position*, it is the **absence of a cause**: it
 * asserts that a thing was always there when it was not. A warp-in supplies the cause. It is the
 * same fiction the player's own jump drive already runs on (`hyper.ts`), which means the sector is
 * not being given a power the player lacks — it is being shown using the one they have.
 *
 * So arrivals now happen **inside sensor range and in front of the player where possible**, and
 * they are loud: a bright streak along the entry vector that collapses into the craft. That is the
 * point. A reinforcement you did not see arrive may as well have been there all along.
 *
 * ## What is still not allowed
 *
 * - **Nothing here reads the record.** Same rule as `raiders.ts`, `factions.ts` and `respawn.ts`,
 *   and this is a place it would be easy to break: scale the arrival rate by node count and a
 *   record has bought itself a quieter sector. Timings and distances are constants; only the seed
 *   decides the direction a wave comes in from.
 * - **An arrival is never a contact.** It is furniture the game placed, exactly like the raider it
 *   becomes, and it is drawn from `space.raiders`-style provenance rather than being mixed into
 *   anything the record reported.
 * - **No arrival lands on top of the player.** `MIN_ARRIVAL` is a floor, not a suggestion: warping
 *   a gunship into the player's hull would be a collision the game charges them for.
 */

import type { Vec3 } from './generate.ts'
import type { Faction } from './factions.ts'
import { AGGRO_RANGE, SENSOR_MULTIPLIER } from './scale.ts'

/**
 * How long the entry streak is on screen before the craft exists.
 *
 * Long enough to be *seen and reacted to* — a warp-in that resolves in a couple of frames is a
 * pop-in with extra steps. Short enough that a wing is not a light show.
 */
export const ARRIVAL_MS = 1_400

/**
 * Where an arrival appears, as a fraction of engagement range.
 *
 * Inside the player's sensor envelope on purpose — the whole point is that it is witnessed — but
 * comfortably outside knife range, so it announces a fight rather than starting one.
 * In practice that means beyond every rolled fighter's aggro, while still inside stock sensors.
 */
/**
 * The smallest forward component an arrival may have, in units of the facing vector.
 *
 * A backstop, not a tuning knob: with `ARRIVAL_SPREAD` below the geometry already keeps every
 * arrival well inside the frustum, and this is what makes "never behind the player" a property of
 * the function rather than of the numbers passed to it.
 */
export const FORWARD_FLOOR = 0.6

/**
 * How far off the nose an arrival may be placed, as a weight against the facing.
 *
 * 0.30 puts the worst case about 23 degrees off the nose, comfortably inside the 33-degree half
 * angle of the camera. It was 0.75 — 72 degrees, and therefore mostly off-screen.
 */
export const ARRIVAL_SPREAD = 0.3

export const MIN_ARRIVAL = Math.round(AGGRO_RANGE * 1.8)
export const MAX_ARRIVAL = Math.round(AGGRO_RANGE * (SENSOR_MULTIPLIER - 0.35))

/**
 * A craft on its way in. Drawn, but not yet in the swarm.
 *
 * It cannot be shot, cannot shoot, and cannot be collided with — it is not there yet. That is the
 * honest reading of a thing that has not arrived, and it also removes the unpleasant case where a
 * player kills a reinforcement before it finishes materialising.
 */
export interface Arrival {
  id: string
  faction: Faction
  /** Where the craft will be when it finishes. */
  at: Vec3
  /** Unit vector the streak runs along — the direction it came *from*, so it reads as decelerating. */
  dir: Vec3
  /** Milliseconds at which it becomes a craft. */
  dueMs: number
}

/**
 * How far along its entry an arrival is, 0..1.
 *
 * Used only for the streak's brightness and length. Kept here rather than in the renderer so the
 * curve is testable and so `view.ts` keeps its rule of placing geometry rather than deciding
 * anything.
 */
export function progress(a: Arrival, nowMs: number): number {
  const t = 1 - (a.dueMs - nowMs) / ARRIVAL_MS
  return Math.max(0, Math.min(1, t))
}

/** True once the craft should exist. */
export function landed(a: Arrival, nowMs: number): boolean {
  return nowMs >= a.dueMs
}

/**
 * Pick an arrival point near the player.
 *
 * Biased **ahead** of them, because an arrival behind the camera is one they will never see and
 * the entire reason this exists is to be seen. Not exactly ahead: a wave that always warps in dead
 * centre reads as scripted, so the direction is spread around the nose by the seed-derived vector
 * the caller supplies.
 *
 * The `jitter` argument is a unit-ish vector from a deterministic source (`raiders.ts` and
 * `factions.ts` both have one). This function does no rolling of its own — it must stay pure and
 * reproducible, so two players holding one record see a wave enter from the same bearing.
 */
export function arrivalPoint(
  playerAt: Vec3,
  playerFacing: Vec3,
  jitter: Vec3,
  spread: number,
): Vec3 {
  // ## Ahead, and *visibly* ahead
  //
  // Weighting the nose at 1 and the jitter at `spread` keeps the arrival in a cone rather than
  // anywhere on a sphere — but the cone was far wider than it reads. The jitter is a unit bearing,
  // so at a spread of 0.75 a bearing pointing back along the nose leaves 0.25 forward against 0.75
  // lateral: **72 degrees off the nose**, against a field of view of 66 degrees *total*. Most of
  // that cone is off-screen, so the entry effect — the whole point of which is to announce the
  // arrival — was frequently drawn where the player could not see it, and a wing simply appeared
  // on the sensor board instead.
  //
  // The spread is now narrow enough that the worst case is inside the frustum, and the forward
  // component is clamped positive as a backstop so no arithmetic here can ever put an arrival
  // behind the player.
  const fwd = Math.max(FORWARD_FLOOR, 1)
  let dx = playerFacing.x * fwd + jitter.x * spread
  let dy = playerFacing.y * fwd + jitter.y * spread
  let dz = playerFacing.z * fwd + jitter.z * spread
  // Project out any component that would put the point behind the nose. A dot product at or below
  // zero means the jitter overwhelmed the facing, which the spread above should already prevent —
  // this is the guarantee rather than the tuning.
  const along = dx * playerFacing.x + dy * playerFacing.y + dz * playerFacing.z
  if (along < FORWARD_FLOOR) {
    const need = FORWARD_FLOOR - along
    dx += playerFacing.x * need
    dy += playerFacing.y * need
    dz += playerFacing.z * need
  }
  const l = Math.hypot(dx, dy, dz) || 1
  const range = MIN_ARRIVAL + Math.abs(jitter.x + jitter.y + jitter.z) * (MAX_ARRIVAL - MIN_ARRIVAL)
  // Integer coordinates can shorten the radial distance by less than a unit when each component
  // rounds independently. Keep a tiny cushion so `MIN_ARRIVAL` remains a floor after projection.
  const d = Math.min(MAX_ARRIVAL, Math.max(MIN_ARRIVAL + 2, range))
  return {
    x: Math.round(playerAt.x + (dx / l) * d),
    y: Math.round(playerAt.y + (dy / l) * d),
    z: Math.round(playerAt.z + (dz / l) * d),
  }
}

// ── the entry itself ─────────────────────────────────────────────────────────
//
// **Lines, not a shrinking ball.** The first version drew an arrival as one sphere that got
// smaller as it resolved, and at any real distance a shrinking sphere is a dot that dims — it
// carried no direction, so the player could not tell where the thing had come *from* or which way
// it would be facing when it got here, which is most of what you want to know about something
// arriving.
//
// A hyperspace entry is a bundle of streaks running along the entry vector, converging on the
// point the craft will occupy. Three things do the work, and each is a claim the player can read:
//
//   * **They run along `dir`**, so the entry has a bearing. A wing arriving from your six is
//     visibly arriving from your six.
//   * **They converge**, laterally and longitudinally, onto one point. The eye follows a
//     contraction, so the collapse *is* the announcement of where the craft will be.
//   * **They brighten as they close**, so the moment of arrival is the brightest frame rather
//     than the dimmest. Fading out and then producing a ship reads as two unrelated events.

/** How many streaks in one entry. Enough to read as a bundle, few enough that a wing is not a wall. */
export const STREAK_STRANDS = 6

/** How far back along the entry vector the streaks begin, at t=0. */
export const STREAK_REACH = Math.round(AGGRO_RANGE * 0.85)

/** How long each streak is at t=0. */
export const STREAK_LEN = Math.round(AGGRO_RANGE * 0.55)

/** How far off the entry axis the bundle spreads at t=0, before it converges. */
export const STREAK_SPREAD = Math.round(AGGRO_RANGE * 0.055)

/** One drawn line of an entry. Pure geometry — `view.ts` gives it a role and a colour. */
export interface Strand {
  /** The trailing end, furthest back along the entry vector. */
  from: Vec3
  /** The leading end, closest to where the craft will be. */
  to: Vec3
  alpha: number
}

/**
 * Two unit vectors perpendicular to `dir`, and to each other.
 *
 * The seed axis is chosen against `dir`'s *smallest* component, which is what stops the cross
 * product degenerating: crossing with an axis the vector is nearly parallel to gives a near-zero
 * result, and normalising that yields whatever the floating point noise happened to be — so the
 * bundle would flip orientation between frames for an entry that happened to come in along an
 * axis, and only for those.
 */
export function perpBasis(dir: Vec3): [Vec3, Vec3] {
  const ax = Math.abs(dir.x)
  const ay = Math.abs(dir.y)
  const az = Math.abs(dir.z)
  const seed: Vec3 =
    ax <= ay && ax <= az ? { x: 1, y: 0, z: 0 } : ay <= az ? { x: 0, y: 1, z: 0 } : { x: 0, y: 0, z: 1 }
  let ux = dir.y * seed.z - dir.z * seed.y
  let uy = dir.z * seed.x - dir.x * seed.z
  let uz = dir.x * seed.y - dir.y * seed.x
  const ul = Math.hypot(ux, uy, uz) || 1
  ux /= ul
  uy /= ul
  uz /= ul
  return [
    { x: ux, y: uy, z: uz },
    {
      x: dir.y * uz - dir.z * uy,
      y: dir.z * ux - dir.x * uz,
      z: dir.x * uy - dir.y * ux,
    },
  ]
}

/**
 * The streaks of one entry at progress `t`, 0..1.
 *
 * Pure, and pure of the record as well: nothing here reads a world, so an entry looks the same
 * whatever was perceived. The only inputs are where the craft will be and which way it came from.
 *
 * At `t = 1` every strand has collapsed to a zero-length segment at `at`, which is the frame the
 * craft appears — so there is never a gap between the last streak and the first hull.
 */
export function streak(at: Vec3, dir: Vec3, t: number): Strand[] {
  const k = Math.max(0, Math.min(1, t))
  const ease = 1 - k
  const [u, v] = perpBasis(dir)

  // Squared, so the bundle spends most of the entry closing the last of the distance rather than
  // crossing the first of it. A linear approach reads as a constant slide; this reads as something
  // decelerating hard into the sector, which is the fiction.
  const head = STREAK_REACH * ease * ease
  const len = STREAK_LEN * ease
  const spread = STREAK_SPREAD * ease

  const out: Strand[] = []
  for (let i = 0; i < STREAK_STRANDS; i += 1) {
    const a = (i / STREAK_STRANDS) * Math.PI * 2
    const ox = (u.x * Math.cos(a) + v.x * Math.sin(a)) * spread
    const oy = (u.y * Math.cos(a) + v.y * Math.sin(a)) * spread
    const oz = (u.z * Math.cos(a) + v.z * Math.sin(a)) * spread
    out.push({
      from: {
        x: Math.round(at.x + dir.x * (head + len) + ox),
        y: Math.round(at.y + dir.y * (head + len) + oy),
        z: Math.round(at.z + dir.z * (head + len) + oz),
      },
      to: {
        x: Math.round(at.x + dir.x * head + ox),
        y: Math.round(at.y + dir.y * head + oy),
        z: Math.round(at.z + dir.z * head + oz),
      },
      // Brightest at the end. An entry that faded out and *then* produced a ship would read as
      // two unrelated events rather than as one thing happening.
      alpha: 0.3 + 0.7 * k,
    })
  }
  return out
}
