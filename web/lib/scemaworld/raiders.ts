/**
 * Raiders: the sector's own population, which the record never mentions.
 *
 * ## The problem this solves
 *
 * Hostiles came from the record's signals, and a record has a handful of them. The parity
 * fixture has five. Scaled up to a sector three hundred and seventy-five million units across,
 * that is an empty volume with five things in it — expansive in the arithmetic and deserted in
 * play. A sector needs a population.
 *
 * ## Why raiders are allowed to exist, and why they are labelled differently
 *
 * Everything else in the space is a claim: a station is an object the observer perceived, a
 * contact is a signal it counted, a marker is something it looked for and did not find. A
 * raider is **not a claim about anything**. It is furniture the game placed, and the honest
 * thing is to say so rather than to quietly mix it in with `space.contacts` — where it would
 * become indistinguishable from a signal somebody actually reported.
 *
 * So raiders live in their own field, carry `unlogged: true`, and the HUD names them `RAIDER`
 * rather than giving them a label from a record that never described them.
 *
 * ## Density is a constant, and getting there took two attempts
 *
 * This is the same rule as `ship.ts`, applied where it is easiest to break. Tying raider
 * density to `blind_spots` is the obvious idea and it is backwards: a producer who hides blind
 * spots would get an easier game, which is a reward paid for misreporting.
 *
 * So the first version made it a rate *per node*, which looked uniform and was not. Blind spots
 * are placed into the sector as rift nodes, and reported extent drives the fractal's depth — so
 * a record claiming less of both grew a smaller node list and bought a quieter sector. The test
 * caught it; the reasoning had not.
 *
 * Raiders therefore do not touch the node list at all. A fixed number of them are scattered
 * through the volume at seed-derived positions, which is also the better fiction: a raider is
 * not a station's tenant, it is out in the black. Every world is exactly as dangerous as every
 * other, and only the arrangement varies.
 *
 * A raider's threat reads as a real number, unlike a ghost contact's. That is not an
 * inconsistency: the em dash marks a quantity *the record left unmeasured*, and the record
 * makes no statement about a raider at all. There is no unmeasured claim here to misrepresent.
 * The inversion is a nice one to play — the things the record described are the uncertain ones.
 */

import type { Contact, Vec3 } from './generate.ts'
import { Rng } from '../omni/fractal.ts'
import type { ClassId } from './classes.ts'
import { AGGRO_RANGE, SECTOR_REACH } from './scale.ts'

/** The spawn point, which is what a generated wing is held clear of. */
const ORIGIN: Vec3 = { x: 0, y: 0, z: 0 }

/**
 * The resolution a scattered coordinate is drawn at. See `scatter`.
 *
 * Small enough that `Rng.below` is nowhere near its ceiling, and fine enough that a millionth of
 * the sector is far below anything a player could perceive as a grid.
 */
const FRACTION = 1_000_000

/** A seed-derived fraction in [0, 1). One draw, so callers can count their draws. */
const frac = (rng: Rng): number => rng.below(FRACTION) / FRACTION

/**
 * A seed-derived point in the sector, pulled toward the middle.
 *
 * ## The bug this exists to fix, which shipped twice
 *
 * `Rng` is a **32-bit** xorshift: `next()` cannot exceed 4,294,967,295, and `below(n)` is
 * `next() % n`. So for any `n` above 2^32 the modulo does nothing at all and the draw is simply
 * `next()` — uniform over a range that has no relationship to the one the caller asked for.
 *
 * `EXTENT` is 3.2e9, so this ceiling sits at about 1.34 extents and *every* scatter here was over
 * it. The failure was invisible both times because it produces plausible coordinates:
 *
 *   * `below(EXTENT * 2) - EXTENT` asked for ±1 extent and returned [-1.00, +0.34] — skewed, but
 *     close to the origin, so the sector looked populated and nobody had reason to look.
 *   * `below(SECTOR_REACH * 2) - SECTOR_REACH` asked for ±5.89 and returned [-5.89, -4.55]: a thin
 *     shell in one corner, all three axes negative. Measured, the nearest raider sat 1.9 extents
 *     from the spawn and the median 8.5, with nothing whatsoever inside sensor range. The whole
 *     roster had been exiled to a corner of the map.
 *
 * No check caught either, because the suite asserts counts, clearance from the spawn, and that
 * marshals eventually find raiders — and the last of those still passed, since both populations
 * were exiled to the *same* corner.
 *
 * `Rng` itself is deliberately not touched: `lib/omni/fractal.ts` shares it with the plate
 * renderer, whose output is pinned byte-for-byte against Rust by `check:omni`. Widening `below`
 * would change every drawn plate. So the composition happens here, in the caller.
 *
 * ## Why the point is pulled inward rather than spread evenly
 *
 * Uniform scatter through a cube puts most of its volume *far away* — the shells grow as `r^2`,
 * so a correct uniform draw over ±5.89 extents still leaves a player at the origin with nothing
 * nearby to meet. `pull` is the exponent on the radius: 1 is uniform-in-cube, higher values
 * concentrate the roster near the middle where the player starts, while keeping a tail that
 * reaches the corners so the outer sector is not bare.
 *
 * The shape stays a **cube**, not a sphere, for the reason it always was: the fractal fills a
 * boxy volume, and a sphere leaves the corners — where the longest branches end — unpopulated.
 *
 * Costs **four draws**, one per axis plus one for the radius. Callers that seek deterministically
 * into this stream by index must count them.
 */
export function scatter(rng: Rng, reach: number, pull: number, floor: number): Vec3 {
  let x = frac(rng) * 2 - 1
  let y = (frac(rng) * 2 - 1) * 0.7
  let z = frac(rng) * 2 - 1

  // ## The direction is normalised onto the cube's surface first
  //
  // Without this, `floor` is not a floor on anything. A raw cube point has a length between 0 and
  // ~1.58, so scaling it by a radius makes the radius a *scale factor* rather than a distance,
  // and a draw whose three components all happen to land near zero lands near the origin however
  // large the floor is. Measured with the floor already in place: a nearest raider at 0.241
  // extents against a floor of 0.448, and eight of them inside sensor range at load-in.
  //
  // Dividing by the largest component (rather than by the length) puts the direction on the
  // surface of the **cube**, not the sphere — which is the corner-filling property this wanted in
  // the first place. Distance is then between `radius` and about 1.58 of it, so the floor holds.
  const longest = Math.max(Math.abs(x), Math.abs(y), Math.abs(z))
  if (longest < 1e-9) return { x: Math.trunc(floor), y: 0, z: 0 }
  x /= longest
  y /= longest
  z /= longest

  // The radius runs from `floor` to `reach`, biased by `pull`.
  const radius = floor + Math.pow(frac(rng), pull) * Math.max(0, reach - floor)
  return {
    x: Math.trunc(x * radius),
    y: Math.trunc(y * radius),
    z: Math.trunc(z * radius),
  }
}

/**
 * How hard the opening roster is pulled toward the player's starting position.
 *
 * Shared by the raiders here and the patrol in `factions.ts` — **the two must match**, or one
 * population sits where the other is not and the ambient war between them stops happening. That
 * is not hypothetical: it is what a mismatched scatter volume did, and it cost three checks.
 *
 * The value is not a taste call. The placement it replaced was measured by replaying it, 32-bit
 * truncation and all, and this was fitted to it:
 *
 * |  | nearest | p25 | median | on sensors at load-in | max |
 * |---|---|---|---|---|---|
 * | the original | 0.49 | 0.70 | 0.95 | 0 | 1.44 |
 * | this | 0.46 | 0.54 | 1.03 | 0 | 8.00 |
 *
 * So the half a player meets is the half they had before — a clear board on the first frame and
 * the first wing a short flight out — while the tail now runs to the edge of the sector instead
 * of stopping at 1.44, which is what the corrected sampling buys and what keeps the outer map
 * from being bare.
 */
export const SCATTER_PULL = 4.6

/**
 * How close to the spawn point the opening roster may be placed.
 *
 * Just outside sensor range (`AGGRO_RANGE * SENSOR_MULTIPLIER` is 3.4 of these), which is the
 * property the original placement had and which reads as deliberate: **you load in with a clear
 * board and find the sector, rather than loading into a contact.** A first frame with hostiles
 * already resolved on the sensor panel is indistinguishable from being ambushed by the loading
 * screen.
 *
 * This is a floor on the *placement*, distinct from the `clearance` push-out below, which is a
 * floor on the result after a wing has been spread around its anchor and which also applies to
 * respawns, where "the player" is wherever they have flown to.
 */
export const SPAWN_STANDOFF = Math.round(AGGRO_RANGE * 4)

/**
 * How many raiders a sector carries, and how they are arranged.
 *
 * A constant, not a rate — see the module note.
 *
 * **They come in wings.** Scattered uniformly, sixty craft in a volume this size sit two
 * hundred million units apart, which means you essentially never meet one: the sector reads as
 * empty and the entire combat system goes unused. Clustering fixes both halves at once — long
 * genuinely empty stretches, and then a *wing*, which is an encounter rather than a loner.
 *
 * It is also the better fight. Four craft of mixed classes arrive with different turn rates and
 * different standoffs, so the engagement has a shape: the interceptors are on you first and the
 * gunship arrives late and hits far harder.
 *
 * ## It did not move when the sector grew, and that is a measurement rather than an oversight
 *
 * `TRUNK` grew the volume by a factor of about fifty-five, which drops the density of a fixed
 * roster through the floor, so raising this looks obviously right. It was tried at 28 and then at
 * 22, and the per-tick cost is what settled it:
 *
 * | wings | craft in the swarm | ms per tick |
 * |---|---|---|
 * | 28 | ~190 | (suite timed out) |
 * | 22 | 164 | 5.59 |
 * | 18 | 148 | 3.74 |
 *
 * The bar in `check:scemaworld` is 4 ms against a 16.7 ms frame, and it exists because this same
 * arithmetic already produced a 0.68 → 5.17 ms regression once. Craft interact pairwise, so the
 * cost climbs faster than the roster does.
 *
 * **What makes 18 safe is that density is not the number a player experiences.** `respawn.ts`
 * raises reinforcements *relative to where the player is*, on a timer, so how often you meet
 * something is set by that timer and not by how thinly the opening roster is spread over the
 * volume. What the enlarged sector genuinely broke was not the count but the *placement* — the
 * roster was scattered across one `EXTENT` while the nodes ran to six, which is fixed below by
 * scattering across `SECTOR_REACH`.
 */
export const WINGS = 18
export const PER_WING = 4

/**
 * The raider capital garrison.
 *
 * Placed rather than rolled, for the same reason wings are a constant: `classFor` reaches a
 * dreadnought once in seventy rolls and a titan once in a thousand, so on a roster of seventy-two
 * craft a sector's war classes were a lottery — most sectors had none, and the ones that did had
 * them by accident. Naming them here makes "there is a hostile capital out there" a property of
 * the *game* rather than of a dice roll, which is what the marshals' own war classes
 * (`classes.ts::warden`, `::bastion`) now exist to answer.
 *
 * Every sector gets the same garrison. A record cannot buy itself a quieter one.
 */
const GARRISON: ClassId[] = ['dreadnought', 'dreadnought', 'leviathan', 'titan']

/**
 * How far apart a wing's craft sit, as a fraction of engagement range.
 *
 * Small on purpose: a wing has to arrive *together* to be an encounter rather than four separate
 * one-on-ones trickling in.
 */
const WING_SPREAD = 0.3

/**
 * One raider wing, from the seed and a wing index.
 *
 * Factored out of `raidersOf` so respawning can continue the same deterministic sequence past the
 * sector's opening roster — wing `n` is wing `n` whether it was placed at generation or raised
 * forty minutes in. Advancing the stream by index rather than carrying it means a wing's
 * composition does not depend on how many wings happened to be raised before it in this session.
 */
export function raiderWing(
  seed: string,
  wing: number,
  awayFrom: Vec3 = ORIGIN,
  clearance: number = AGGRO_RANGE * 2,
): Contact[] {
  // `Rng` reads only the first eight hex characters, so appending a suffix would hand the
  // raiders the fractal's own stream. A different slice of the digest gives an independent one.
  const rng = new Rng(seed.slice(8, 16) || seed)
  // Deterministic seek to this wing's slot. Four draws for the anchor (`scatter` costs one per
  // axis plus one for the radius) plus four per craft.
  for (let i = 0; i < wing * (4 + PER_WING * 4); i += 1) rng.below(1024)

  const out: Contact[] = []
  // Reaches the whole sector and is pulled toward the middle, so a player at the spawn point has
  // something to meet without the corners being empty. `SECTOR_REACH` is a constant rather than a
  // measurement of the generated tree — see its note on why that may not be measured off a record.
  let anchor = scatter(rng, SECTOR_REACH, SCATTER_PULL, SPAWN_STANDOFF)
  // Never within sensor range of the player. At generation that is the spawn point — opening the
  // game already inside an engagement reads as the game being broken before you have touched a
  // control — and on a respawn it is wherever they actually are, because a wing materialising in
  // front of somebody is the clearest possible statement that none of this is a place.
  const spread = Math.round(AGGRO_RANGE * WING_SPREAD)
  // The clearance is owed by the nearest *craft*, not by the anchor — a wing is scattered around
  // its anchor, so a wing anchored exactly at the limit puts its closest ship a full spread
  // inside it. Off by exactly `spread`, which is small enough to look like rounding and large
  // enough to drop a hostile inside sensor range on a respawn.
  const keepOut = clearance + spread
  let guard = 0
  while (
    Math.hypot(anchor.x - awayFrom.x, anchor.y - awayFrom.y, anchor.z - awayFrom.z) < keepOut &&
    guard < 24
  ) {
    anchor = {
      x: anchor.x * 2 - awayFrom.x,
      y: anchor.y * 2 - awayFrom.y,
      z: anchor.z * 2 - awayFrom.z + keepOut,
    }
    guard += 1
  }

  for (let i = 0; i < PER_WING; i += 1) {
    // Magnitude drives size and hit radius only, exactly as it does for a contact, and comes
    // from the seed rather than any reported figure. See `weapons.ts` on why a reported
    // magnitude may never become a hit-point pool.
    const magnitude = 0.25 + rng.below(60) / 100
    out.push({
      id: `raider:${wing}:${i}`,
      at: {
        x: anchor.x + rng.below(spread * 2) - spread,
        y: anchor.y + rng.below(spread * 2) - spread,
        z: anchor.z + rng.below(spread * 2) - spread,
      },
      hostility: 'hostile',
      // Solid: the sector knows these are there. A raider is not an estimate.
      solid: true,
      magnitude,
      label: 'RAIDER',
      unlogged: true,
    })
  }
  return out
}

/**
 * Place the sector's raiders: the wings, plus the capital garrison.
 *
 * Deterministic in the seed, so two players holding the same record fight the same sector —
 * the property the whole game rests on, and one a randomly-populated sector would break
 * silently while still looking correct on one machine.
 *
 * Takes the seed and nothing else. It used to take the node list, which is how the record got
 * a say in how dangerous its own sector was.
 */
export function raidersOf(seed: string): Contact[] {
  const out: Contact[] = []
  for (let w = 0; w < WINGS; w += 1) out.push(...raiderWing(seed, w))

  // The garrison, on its own stream so adding or removing a capital cannot shuffle the wings.
  const rng = new Rng(seed.slice(4, 12) || seed.slice(8, 16) || seed)
  GARRISON.forEach((klass, i) => {
    let at = scatter(rng, SECTOR_REACH, SCATTER_PULL, SPAWN_STANDOFF)
    let guard = 0
    // A capital's own aggro range reaches a third of the sector, so it is held much further from
    // the spawn than a fighter wing is. A titan on top of a new player is not a difficulty
    // setting, it is the game ending before it starts.
    const clear = AGGRO_RANGE * 5
    while (Math.hypot(at.x, at.y, at.z) < clear && guard < 24) {
      at = { x: at.x * 2, y: at.y * 2, z: at.z * 2 + clear }
      guard += 1
    }
    out.push({
      id: `raider:capital:${i}`,
      at,
      hostility: 'hostile',
      solid: true,
      // Size comes from the class, not from this, for anything that becomes a craft — see
      // `view.ts`. Kept in range so the field never carries a nonsense value.
      magnitude: 0.8,
      label: 'RAIDER',
      unlogged: true,
      // The one place a hostile's class is *named* rather than rolled. `swarmOf` honours it.
      klass,
    })
  })
  return out
}

/**
 * How many raiders the sector tries to keep flying, and how a shortfall is made up.
 *
 * Without this the sector is a resource that depletes: clear the wings near you and the volume
 * you are in goes permanently quiet, which turns the back half of a session into flying through
 * an empty box. Worse, the marshals win by default — the ambient firefights that make the place
 * feel inhabited stop happening because there is nothing left for them to fight.
 *
 * A wing at a time rather than a craft at a time, for the same reason the sector opens with
 * wings: four craft arriving together is an encounter, and one craft appearing alone every so
 * often is a leak.
 *
 * **The capital garrison is not replaced.** A leviathan you killed stays killed, or the largest
 * thing you can do in the sector becomes a chore with a respawn timer.
 */
export const RAIDER_FLOOR = Math.round(WINGS * PER_WING * 0.6)

/**
 * How many raider fighters the sector tries to get back to.
 *
 * **The floor is a trigger, not a target**, and conflating the two is what made the sector
 * permanently emptier after every fight. Reinforcement fired only below `RAIDER_FLOOR` and
 * stopped the moment it was reached, so a player who cleared thirty raiders got twenty-nine of
 * them back and the sector settled at 43 of the 72 it opened with — for the rest of the session,
 * with nothing anywhere saying so. Measured: a purged sector recovered to exactly 43 and sat
 * there.
 *
 * So the target is the full complement and the floor keeps a different job: below it the sector
 * is contested and reinforcement **surges** (see `RAIDER_SURGE_MS`). Clearing a region is still
 * worth doing, because what it buys is time rather than a permanent dent — the deficit comes back
 * over minutes, which is a pace, where a dent is just less game.
 *
 * The capital garrison is still never replaced, which is why this counts fighters only.
 */
export const RAIDER_STRENGTH = WINGS * PER_WING
