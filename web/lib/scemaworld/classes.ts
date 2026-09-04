/**
 * What kinds of thing fly here, and what each one is made of.
 *
 * One flat table. A class is a *silhouette plus a statline*, and both belong in the same place:
 * the moment a renderer decides that the fast one is the small triangle, the table has two
 * homes and they drift. `view.ts` reads `shape` from here and `gl.ts` only places geometry.
 *
 * ## The design the numbers encode
 *
 * A dogfight is a contest of **turn rate against speed**, not of hit points. So every class
 * trades one for the other along a single line — an interceptor turns twice as fast as it
 * flies, a destroyer barely turns at all — and the player sits deliberately near the fast end
 * of speed and the middle of turn. That is what makes "get behind it" a skill rather than a
 * dice roll, and what makes a capital ship a *place* you fight at rather than a duel.
 *
 * ## Shields absorb, hull decides
 *
 * Every armed thing carries both. Shields regenerate after a lull, so a fight has a rhythm:
 * break contact, recover, re-engage. Hull does not, which is why hull is the number the HUD
 * calls health and shields are the number it calls a buffer. A design where the regenerating
 * bar was the primary one would make every fight a war of attrition against a clock.
 *
 * ## Detection range is half of what makes a sector feel alive
 *
 * Every `aggro` here was raised by half again, and `LIFE_LASER` with it so the line the reach
 * invariant draws through this table survives (`scale.ts`). The reason is measured rather than
 * felt: with the old figures, three minutes of play produced **not one round fired within 0.05
 * EXTENT of the player** — everything armed noticed everything else too late and too locally, so
 * the sector's fights happened in scattered private pockets nobody was near. A craft that sees
 * further engages sooner, follows further, and drags its fight across the volume, which is what
 * puts an engagement in front of somebody.
 *
 * ## Bounties are flat per class and come from the act
 *
 * Never from anything the record reports. See `ship.ts` — this is where the rule would be
 * easiest to break, because scaling a bounty by a signal's magnitude is one line and would
 * quietly pay somebody to inflate a record.
 */

import { EXTENT } from './scale.ts'

/**
 * A silhouette. `gl.ts` owns one mesh per value; nothing else may choose one.
 *
 * Nodes have their own now, rather than all being spheres in different colours. A market and a
 * rift used to be the same ball, so the vocabulary the record carries arrived as a *palette* —
 * which fails the same way colour-only distinctions fail everywhere else in this project.
 */
export type Shape =
  | 'sphere'
  | 'shell'
  | 'bolt'
  | 'interceptor'
  | 'gunship'
  | 'capital'
  | 'dreadnought'
  | 'corvette'
  | 'marauder'
  | 'station'
  | 'market'
  | 'dock'
  | 'depot'
  | 'derelict'
  | 'rift'
  | 'phantom'
  | 'marker'
  | 'origin'

export type ClassId =
  | 'skiff'
  | 'interceptor'
  | 'lancer'
  | 'gunship'
  | 'frigate'
  | 'destroyer'
  | 'dreadnought'
  | 'leviathan'
  | 'titan'
  | 'courier'
  | 'freighter'
  | 'marshal'
  | 'warden'
  | 'bastion'

export interface ClassSpec {
  id: ClassId
  label: string
  shape: Shape
  /** Drawn size. */
  radius: number
  /** Hull points. The primary health — it does not come back. */
  hull: number
  /** Shield points. Absorbs first, regenerates after `SHIELD_DELAY_MS` without a hit. */
  shield: number
  shieldRegen: number
  /** Units per second at full burn. */
  speed: number
  /** Radians per second. The number that decides a dogfight. */
  turn: number
  /** How close before it notices you. */
  aggro: number
  /** The range it tries to hold. */
  standoff: number
  damage: number
  cooldownMs: number
  /** Shots per trigger pull. A burst is what makes a hit feel like an event. */
  burst: number
  /** Salvage for a kill. Flat per class — never from the record. */
  bounty: number
  /** True for something too big to dogfight: it lumbers and fires broadsides. */
  capital: boolean
}

const S = (f: number) => Math.round(EXTENT * f)

export const CLASSES: Record<ClassId, ClassSpec> = {
  // Barely armed, and the only thing in the sector you can reliably run down. It exists so a
  // new player has something to win against — a game whose first encounter is unwinnable
  // teaches the wrong lesson about every later one.
  skiff: {
    id: 'skiff', label: 'SKIFF', shape: 'interceptor',
    radius: S(0.0022), hull: 8, shield: 0, shieldRegen: 0,
    speed: S(1 / 20), turn: 1.5, aggro: S(0.075), standoff: S(0.012),
    damage: 4, cooldownMs: 1400, burst: 1, bounty: 15, capital: false,
  },
  // The knife fighter. Turns faster than you, so it will get behind — the answer is to break
  // away and come back on your terms, not to out-turn it.
  interceptor: {
    id: 'interceptor', label: 'INTERCEPTOR', shape: 'interceptor',
    radius: S(0.0028), hull: 14, shield: 8, shieldRegen: 3,
    speed: S(1 / 15), turn: 2.4, aggro: S(0.112), standoff: S(0.009),
    damage: 6, cooldownMs: 620, burst: 2, bounty: 30, capital: false,
  },
  // Fast and fragile, fights at a distance. Punishes sitting still.
  lancer: {
    id: 'lancer', label: 'LANCER', shape: 'interceptor',
    radius: S(0.0032), hull: 12, shield: 14, shieldRegen: 5,
    speed: S(1 / 14), turn: 1.8, aggro: S(0.15), standoff: S(0.028),
    damage: 11, cooldownMs: 1500, burst: 1, bounty: 45, capital: false,
  },
  // Slow, heavily shielded, hits hard in bursts. You beat it by out-turning it, which is the
  // first time the turn/speed trade is the whole answer.
  gunship: {
    id: 'gunship', label: 'GUNSHIP', shape: 'gunship',
    radius: S(0.0055), hull: 34, shield: 26, shieldRegen: 4,
    speed: S(1 / 26), turn: 0.85, aggro: S(0.135), standoff: S(0.016),
    damage: 9, cooldownMs: 900, burst: 3, bounty: 90, capital: false,
  },
  // The smallest capital. It does not chase and it does not need to.
  frigate: {
    id: 'frigate', label: 'FRIGATE', shape: 'capital',
    radius: S(0.028), hull: 140, shield: 90, shieldRegen: 8,
    speed: S(1 / 90), turn: 0.22, aggro: S(0.225), standoff: S(0.055),
    damage: 16, cooldownMs: 1100, burst: 4, bounty: 350, capital: true,
  },
  // Star-destroyer class: four times a station across, and visible from most of the sector.
  destroyer: {
    id: 'destroyer', label: 'DESTROYER', shape: 'capital',
    radius: S(0.075), hull: 460, shield: 300, shieldRegen: 14,
    speed: S(1 / 150), turn: 0.09, aggro: S(0.30), standoff: S(0.1),
    damage: 24, cooldownMs: 800, burst: 6, bounty: 1200, capital: true,
  },
  // War-class. Fifteen stations end to end.
  //
  // The broadside used to one-shot a fully upgraded ship — ten rounds of sixty-two is six
  // hundred damage against a maximum of five hundred and fifty, so no amount of hull made it
  // survivable and there was no tactic to find. A capital has to be beaten by *manoeuvre*: it
  // turns at a twentieth of a radian per second, so a fighter that keeps moving laterally is
  // never in its arc, and one that flies straight at it deserves what it gets. Per-shot damage
  // is now low enough that being caught once is a mistake rather than the end.
  dreadnought: {
    id: 'dreadnought', label: 'DREADNOUGHT', shape: 'dreadnought',
    radius: S(0.135), hull: 1400, shield: 900, shieldRegen: 26,
    speed: S(1 / 260), turn: 0.05, aggro: S(0.39), standoff: S(0.16),
    damage: 15, cooldownMs: 900, burst: 8, bounty: 4000, capital: true,
  },
  // The largest thing in any sector, and rare enough that most worlds have none. At this size
  // the ribbing on the hull is doing real work: it is the only cue for how far away it is, and
  // without it a leviathan at range reads as a nearby triangle.
  //
  // Beating one takes minutes of sustained fire and constant lateral movement. That is the
  // intent: perseverance and a manoeuvre, not a bigger number.
  leviathan: {
    id: 'leviathan', label: 'LEVIATHAN', shape: 'dreadnought',
    radius: S(0.24), hull: 4200, shield: 2600, shieldRegen: 44,
    speed: S(1 / 420), turn: 0.028, aggro: S(0.51), standoff: S(0.26),
    damage: 21, cooldownMs: 800, burst: 10, bounty: 14000, capital: true,
  },

  // The titan. One per sector at most, and most sectors have none.
  //
  // A third of the sector across: from the far side it is a shape on the sky rather than a
  // contact, and closing on one takes long enough that the decision to do it is made well before
  // you arrive. Beating it is a marauder, every component at maximum, and several minutes of
  // holding a firing solution while never once flying straight.
  //
  // Its per-shot damage is deliberately *lower* than a leviathan's. The threat is the volume of
  // fire and the time you must survive inside it, not a number that deletes you — a one-shot is
  // not difficulty, it is a coin toss with extra steps.
  titan: {
    id: 'titan', label: 'TITAN', shape: 'dreadnought',
    radius: S(0.4), hull: 14000, shield: 9000, shieldRegen: 70,
    speed: S(1 / 700), turn: 0.014, aggro: S(0.60), standoff: S(0.4),
    damage: 18, cooldownMs: 500, burst: 14, bounty: 45000, capital: true,
  },

  // ── traffic ────────────────────────────────────────────────────────────────
  // Not hostile, and `classFor` never rolls them: they are placed by `factions.ts` on routes
  // between real service nodes. They carry statlines because they can be shot at — a ship you
  // cannot destroy is scenery, and scenery that dodges is worse than none.

  // Neon blue. Small, quick, and everywhere; the commonest thing in the sector and the reason
  // the markets look like they are for something.
  courier: {
    id: 'courier', label: 'COURIER', shape: 'interceptor',
    radius: S(0.0024), hull: 10, shield: 6, shieldRegen: 3,
    speed: S(1 / 13), turn: 2.0, aggro: S(0.09), standoff: S(0.01),
    damage: 0, cooldownMs: 9_999, burst: 0, bounty: 0, capital: false,
  },
  // Blue. Slow and heavy, running fuel between depots. Visible from a long way off, which is
  // most of its job: a sector where the only distant silhouettes are threats is a tense one for
  // the wrong reason.
  freighter: {
    id: 'freighter', label: 'FREIGHTER', shape: 'gunship',
    radius: S(0.009), hull: 60, shield: 30, shieldRegen: 4,
    speed: S(1 / 42), turn: 0.6, aggro: S(0.09), standoff: S(0.014),
    damage: 0, cooldownMs: 9_999, burst: 0, bounty: 0, capital: false,
  },
  // Yellow. Anti-raider patrol: hunts raiders, ignores the player, and fights whether or not
  // anyone is watching. Deliberately a match for an interceptor rather than an overmatch — a
  // patrol that always wins makes the sector safe, which is not the point of having one.
  marshal: {
    id: 'marshal', label: 'MARSHAL', shape: 'interceptor',
    radius: S(0.0032), hull: 22, shield: 18, shieldRegen: 5,
    speed: S(1 / 14), turn: 2.2, aggro: S(0.165), standoff: S(0.012),
    damage: 8, cooldownMs: 700, burst: 2, bounty: 0, capital: false,
  },

  // ── the patrol's own war classes ───────────────────────────────────────────
  //
  // The sector's capitals used to be exclusively hostile, which made a marshal patrol a
  // gesture: eighteen interceptors against a roster that includes a leviathan is not a police
  // force, it is a rounding error, and every large silhouette on the horizon meant exactly one
  // thing. Giving the yellow faction a dreadnought and a titan of its own changes what a distant
  // capital *is* — a question rather than an answer — and it is what makes a firefight between
  // two other parties worth flying toward and watching.
  //
  // They mirror the hostile war classes rather than exceeding them. A patrol that outguns
  // everything it meets makes the sector safe, which is not the point of having one; the
  // interesting outcome is a leviathan and a warden grinding each other down over minutes while
  // the player decides whether to intervene, and on which side.
  //
  // **Bounty is zero, and that is a rule rather than a balance choice.** These are the good guys.
  // A payout for killing one would put a price on the faction that exists to make the sector feel
  // policed, and the reward rule (`ship.ts`) is about where salvage may come from at all.

  // The marshal dreadnought. What answers a raider capital.
  warden: {
    id: 'warden', label: 'WARDEN', shape: 'dreadnought',
    radius: S(0.135), hull: 1400, shield: 900, shieldRegen: 26,
    speed: S(1 / 260), turn: 0.05, aggro: S(0.39), standoff: S(0.16),
    damage: 15, cooldownMs: 900, burst: 8, bounty: 0, capital: true,
  },
  // The marshal titan. One per sector, and the only thing out here that a hostile titan has to
  // take seriously. Finding the two of them already engaged is the sight this whole faction
  // exists to produce.
  bastion: {
    id: 'bastion', label: 'BASTION', shape: 'dreadnought',
    radius: S(0.4), hull: 14000, shield: 9000, shieldRegen: 70,
    speed: S(1 / 700), turn: 0.014, aggro: S(0.60), standoff: S(0.4),
    damage: 18, cooldownMs: 500, burst: 14, bounty: 0, capital: true,
  },
}

/**
 * The classes `classFor` may roll — the hostile roster only.
 *
 * Traffic is placed by `factions.ts` and never rolled, so it is excluded here. Keeping one list
 * of "everything" and filtering at each use site is how a courier eventually turns up in a raider
 * wing, which would be both a gameplay bug and a lie about who is out there.
 */
export const CLASS_IDS: ClassId[] = [
  'skiff', 'interceptor', 'lancer', 'gunship', 'frigate', 'destroyer', 'dreadnought', 'leviathan',
  'titan',
]

/** Every class, hostile and otherwise. */
export const ALL_CLASS_IDS: ClassId[] = [
  ...CLASS_IDS, 'courier', 'freighter', 'marshal', 'warden', 'bastion',
]

/**
 * The patrol's war classes, placed as a garrison rather than rolled.
 *
 * Same treatment as the hostile capitals: a fixed roster the sector carries, so a record can
 * neither buy itself a quieter sector nor a better-defended one.
 */
export const MARSHAL_CAPITAL_IDS: ClassId[] = ['warden', 'bastion']

/** Milliseconds without taking a hit before shields begin to come back. */
export const SHIELD_DELAY_MS = 4_200

/**
 * Pick a class from a seed-derived roll.
 *
 * The distribution is fixed and lives here rather than in `raiders.ts`, so the sector's
 * composition is one table rather than a chain of thresholds somebody has to reconstruct.
 * Capitals are rare because a sector with several is a sector you cannot cross.
 */
export function classFor(roll: number): ClassSpec {
  // Per *mille*, not per cent. The roll is an integer, so a hundred buckets cannot express
  // anything rarer than one in a hundred — the titan sat behind `r < 99.6` against a value that
  // is only ever a whole number, and could never be rolled at all. Same shape as the bug that
  // hid both capitals behind a six-valued hash, and it survived one round of tests because the
  // threshold *looks* like it works.
  const r = roll % 1000
  if (r < 240) return CLASSES.skiff
  if (r < 540) return CLASSES.interceptor
  if (r < 730) return CLASSES.lancer
  if (r < 880) return CLASSES.gunship
  if (r < 950) return CLASSES.frigate
  if (r < 980) return CLASSES.destroyer
  if (r < 994) return CLASSES.dreadnought
  if (r < 999) return CLASSES.leviathan
  return CLASSES.titan
}
