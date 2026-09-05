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
import { AGGRO_RANGE, EXTENT } from './scale.ts'

/** The spawn point, which is what a generated wing is held clear of. */
const ORIGIN: Vec3 = { x: 0, y: 0, z: 0 }

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
  // Deterministic seek to this wing's slot. Three draws for the anchor plus four per craft.
  for (let i = 0; i < wing * (3 + PER_WING * 4); i += 1) rng.below(1024)

  const out: Contact[] = []
  // A cube rather than a sphere: the fractal fills a boxy volume, so a sphere would leave the
  // corners — where the longest branches end — unpopulated.
  const span = EXTENT * 2
  let anchor = {
    x: rng.below(span) - EXTENT,
    y: Math.trunc((rng.below(span) - EXTENT) * 0.7),
    z: rng.below(span) - EXTENT,
  }
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
  const span = EXTENT * 2
  GARRISON.forEach((klass, i) => {
    let at = {
      x: rng.below(span) - EXTENT,
      y: Math.trunc((rng.below(span) - EXTENT) * 0.7),
      z: rng.below(span) - EXTENT,
    }
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
