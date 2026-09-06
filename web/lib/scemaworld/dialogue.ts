/**
 * What the factions say when they meet you, and when they lose.
 *
 * ## Why this is a table and not a generator
 *
 * A line is the cheapest thing in the game to add and the easiest to make worthless. Two failure
 * modes, both common, both avoided here by construction:
 *
 * - **A line that could have been said by anybody** tells the player nothing. Every line below is
 *   keyed to a *faction* and an *event*, and the four factions want different things: raiders are
 *   opportunists who talk about cargo and odds, marshals are professionals reading you a warning
 *   they are required to read, couriers are civilians who did not sign up for this, and a capital
 *   speaks for a crew rather than for itself.
 * - **A line that repeats** stops being read within a minute. Selection is a pure function of
 *   `(seed, speaker id, event)`, so one craft says one thing consistently — it has a *voice*
 *   rather than a shuffle — and two different craft in the same wing say different things.
 *
 * ## It reads no record, and it is not paid for
 *
 * Same rule as everything else in the reward path's neighbourhood: nothing here looks at
 * `blind_spots`, `magnitude` or `legibility`, and no line is attached to a payout. A story beat may
 * read a record (`SCEMA-WORLD-CAMPAIGN.md` is explicit that the narrative may read as deeply as it
 * likes); **a line spoken by a raider in a firefight is not a story beat**, it is furniture, and
 * furniture that varied with what a world claimed would be one more reason to misreport one.
 *
 * ## What it is deliberately not
 *
 * Not a conversation. There is no reply, no branch and no state. A craft says one thing when it
 * notices you and one thing when it dies, and a capital says one more when its shields go. The
 * moment this grows a tree it becomes a system that has to be kept consistent with a plot nobody
 * has written yet — and the campaign bible is explicit that the story is written *after* the
 * mechanics settle.
 */

import type { Faction } from './factions.ts'
import type { ClassSpec } from './classes.ts'

/** When something is said. Each is an event the game already detects. */
export type Beat =
  /** It has just noticed you and is closing. */
  | 'engage'
  /** Its shields are gone and its hull is open. Capitals only — see `linesFor`. */
  | 'broken'
  /** It is destroyed. */
  | 'destroyed'
  /** You have entered a standing battle (`clusters.ts`). Said once, by whoever is nearest. */
  | 'cluster'

export interface Line {
  /** Who said it, for the HUD. */
  speaker: string
  text: string
  /** Which side, so the renderer can colour it without re-deriving hostility. */
  faction: Faction
}

/**
 * The pools. Read as a table, edited as a table.
 *
 * Raiders are the largest pool because they are what a player mostly fights, and a pool that runs
 * dry is the repetition problem arriving late instead of early.
 */
const RAIDER: Record<Beat, string[]> = {
  engage: [
    'Hull on the scope. Cut it out of the lane.',
    'You are a long way from a seal out here.',
    'That is a fat one. Take the drive first.',
    'Freehold colours. Turn around or do not.',
    'No witness, no manifest. Close it.',
    'We do not certify. We collect.',
  ],
  broken: [
    'Plating is open — hold the line, hold it—',
    'Shields are gone. Somebody get on the guns.',
    'She is holed. Keep firing.',
  ],
  destroyed: [
    'Tell the freehold where we fell.',
    'Not certified. Not sorry.',
    'We were the only ones who saw this place—',
    'It was a good enough map.',
    'Somebody else will fly it.',
  ],
  cluster: [
    'Yellow wing in the volume. All hulls, engage.',
    'Marshals. Of course it is marshals.',
    'Hold this pocket. Nobody seals anything here.',
  ],
}

const MARSHAL: Record<Beat, string[]> = {
  engage: [
    'Assay patrol. Hold your heading and be identified.',
    'You are inside a sealed observation. Conduct yourself accordingly.',
    'Patrol wing. State your certification.',
    'We do not want this. Cut your drive.',
    'Assay authority. This is your notice.',
  ],
  broken: [
    'Hull breach — we are still on station.',
    'Shields down. Patrol holds.',
    'Taking it. Not leaving.',
  ],
  destroyed: [
    'Patrol down. Log it.',
    'Somebody carry the seal home—',
    'We witnessed it. That was the job.',
    'Assay… mark the position…',
  ],
  cluster: [
    'Raider concentration. All patrol, converge.',
    'This is a contested volume. Do not stray.',
    'Freehold hulls, in numbers. Engage.',
  ],
}

const CIVILIAN: Record<Beat, string[]> = {
  engage: [
    'We are carriage. We are carriage, do not—',
    'Compact courier. We are not armed.',
    'There is nothing aboard worth this.',
  ],
  broken: ['We are holed — we are only carrying—', 'Please. We do not read what we carry.'],
  destroyed: [
    'The Compact carried it. That is all we did.',
    'It was somebody else’s record…',
    'Neutral. We were neutral.',
  ],
  cluster: ['Getting clear. Getting clear.', 'This is not a lane any more.'],
}

/**
 * A capital speaks for a crew, and the difference is the whole reason it has its own pool.
 *
 * A leviathan saying an interceptor's line makes the largest thing in the sector sound like one
 * pilot in a cockpit, which is the single easiest way to make a war hull feel small.
 */
const CAPITAL: Record<Beat, string[]> = {
  engage: [
    'All batteries. The contact is inside the envelope.',
    'Gunnery, you have the solution. Fire as you bear.',
    'Bring us round. It cannot keep this up.',
    'Bridge to all decks. One hull. Break it.',
  ],
  broken: [
    'Shields collapsed across the beam. Damage control, everywhere at once.',
    'We are open along the spine. Keep the turrets fed.',
    'Batteries three and four are gone. Fight the ship.',
  ],
  destroyed: [
    'All hands — she is coming apart—',
    'Abandon. Abandon. It was one hull—',
    'Log it. One hull did this.',
    'Bridge is gone. Somebody is still firing—',
  ],
  cluster: [
    'Fleet action. All hulls, form on the flagship.',
    'This volume is contested. Hold formation.',
  ],
}

/**
 * The pool for a speaker.
 *
 * A capital's own pool outranks its faction's, because "the biggest thing here" is a more
 * important fact about a speaker than which side it is on — a raider leviathan and a marshal
 * bastion are the same kind of voice pointed in opposite directions.
 */
export function linesFor(faction: Faction, spec: ClassSpec, beat: Beat): string[] {
  if (spec.capital) return CAPITAL[beat]
  if (faction === 'raider') return RAIDER[beat]
  if (faction === 'marshal') return MARSHAL[beat]
  return CIVILIAN[beat]
}

/**
 * A stable index from a speaker id and a beat.
 *
 * Hand-rolled, and not `Rng`, for the reason every other module here gives: the generator is a
 * stream that other placement code seeks through by index, and drawing from it for a line would
 * move a ship.
 */
function pick(seed: string, id: string, beat: Beat, n: number): number {
  if (n <= 0) return 0
  let h = 2166136261 >>> 0
  for (const t of [seed, id, beat]) {
    for (let i = 0; i < t.length; i += 1) {
      h ^= t.charCodeAt(i)
      h = Math.imul(h, 16777619) >>> 0
    }
  }
  return (h >>> 8) % n
}

/**
 * What this craft says at this beat, or `null` when it says nothing.
 *
 * `null` is a real answer and the common one. **Unarmed traffic has no `broken` line** — a courier
 * whose shields fail is not making a stand, and giving it a defiant line would turn the one
 * genuinely pitiable thing in the sector into another combatant. Silence there is the
 * characterisation.
 */
export function say(
  seed: string,
  id: string,
  faction: Faction,
  spec: ClassSpec,
  beat: Beat,
): Line | null {
  // A craft that cannot shoot does not announce an engagement, and does not narrate its own hull
  // failing. It runs, and it says so when it dies.
  if (spec.damage === 0 && (beat === 'engage' || beat === 'broken')) return null
  // Only a capital talks about its shields going. On a fighter it is one hit out of a handful and
  // a line for it would fire constantly, which is the repetition failure arriving through a door
  // the pool size cannot close.
  if (beat === 'broken' && !spec.capital) return null
  const pool = linesFor(faction, spec, beat)
  if (pool.length === 0) return null
  return { speaker: spec.label, text: pool[pick(seed, id, beat, pool.length)], faction }
}
