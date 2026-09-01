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

import type { Contact } from './generate.ts'
import { Rng } from '../omni/fractal.ts'
import { AGGRO_RANGE, EXTENT } from './scale.ts'

/**
 * How many raiders a sector carries. A constant, not a rate — see the module note.
 *
 * Forty over a volume this size works out to one every few seconds of travel along a busy
 * axis and long stretches of nothing elsewhere, which is the density that makes the emptiness
 * read as distance rather than as absence.
 */
const RAIDERS = 40

/**
 * Place the sector's raiders.
 *
 * Deterministic in the seed, so two players holding the same record fight the same sector —
 * the property the whole game rests on, and one a randomly-populated sector would break
 * silently while still looking correct on one machine.
 *
 * Takes the seed and nothing else. It used to take the node list, which is how the record got
 * a say in how dangerous its own sector was.
 */
export function raidersOf(seed: string): Contact[] {
  // `Rng` reads only the first eight hex characters, so appending a suffix would hand the
  // raiders the fractal's own stream. A different slice of the digest gives an independent one.
  const rng = new Rng(seed.slice(8, 16) || seed)
  const out: Contact[] = []

  for (let i = 0; i < RAIDERS; i += 1) {
    // A cube rather than a sphere: the fractal fills a boxy volume, so a sphere would leave the
    // corners — where the longest branches end — unpopulated.
    const span = EXTENT * 2
    let at = {
      x: rng.below(span) - EXTENT,
      y: Math.trunc((rng.below(span) - EXTENT) * 0.7),
      z: rng.below(span) - EXTENT,
    }
    // Never within sensor range of the spawn point. Opening the game already inside an
    // engagement reads as the game being broken before you have touched a control.
    while (Math.hypot(at.x, at.y, at.z) < AGGRO_RANGE * 1.5) {
      at = { x: at.x * 2, y: at.y * 2, z: at.z * 2 + AGGRO_RANGE * 2 }
    }

    // Magnitude here drives size and hit radius only, exactly as it does for a contact, and it
    // comes from the seed rather than from any reported figure. See `weapons.ts` on why a
    // reported magnitude may never become a hit-point pool.
    const magnitude = 0.25 + rng.below(60) / 100

    out.push({
      id: `raider:${i}`,
      at,
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
