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
 * ## Bounties are flat per class and come from the act
 *
 * Never from anything the record reports. See `ship.ts` — this is where the rule would be
 * easiest to break, because scaling a bounty by a signal's magnitude is one line and would
 * quietly pay somebody to inflate a record.
 */

import { EXTENT } from './scale.ts'

/** A silhouette. `gl.ts` owns one mesh per value; nothing else may choose one. */
export type Shape = 'sphere' | 'shell' | 'bolt' | 'interceptor' | 'gunship' | 'capital'

export type ClassId = 'skiff' | 'interceptor' | 'lancer' | 'gunship' | 'frigate' | 'destroyer'

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
    speed: S(1 / 20), turn: 1.5, aggro: S(0.05), standoff: S(0.012),
    damage: 4, cooldownMs: 1400, burst: 1, bounty: 15, capital: false,
  },
  // The knife fighter. Turns faster than you, so it will get behind — the answer is to break
  // away and come back on your terms, not to out-turn it.
  interceptor: {
    id: 'interceptor', label: 'INTERCEPTOR', shape: 'interceptor',
    radius: S(0.0028), hull: 14, shield: 8, shieldRegen: 3,
    speed: S(1 / 15), turn: 2.4, aggro: S(0.075), standoff: S(0.009),
    damage: 6, cooldownMs: 620, burst: 2, bounty: 30, capital: false,
  },
  // Fast and fragile, fights at a distance. Punishes sitting still.
  lancer: {
    id: 'lancer', label: 'LANCER', shape: 'interceptor',
    radius: S(0.0032), hull: 12, shield: 14, shieldRegen: 5,
    speed: S(1 / 14), turn: 1.8, aggro: S(0.10), standoff: S(0.028),
    damage: 11, cooldownMs: 1500, burst: 1, bounty: 45, capital: false,
  },
  // Slow, heavily shielded, hits hard in bursts. You beat it by out-turning it, which is the
  // first time the turn/speed trade is the whole answer.
  gunship: {
    id: 'gunship', label: 'GUNSHIP', shape: 'gunship',
    radius: S(0.0055), hull: 34, shield: 26, shieldRegen: 4,
    speed: S(1 / 26), turn: 0.85, aggro: S(0.09), standoff: S(0.016),
    damage: 9, cooldownMs: 900, burst: 3, bounty: 90, capital: false,
  },
  // A capital. It does not chase and it does not need to.
  frigate: {
    id: 'frigate', label: 'FRIGATE', shape: 'capital',
    radius: S(0.016), hull: 140, shield: 90, shieldRegen: 8,
    speed: S(1 / 90), turn: 0.22, aggro: S(0.13), standoff: S(0.045),
    damage: 16, cooldownMs: 1100, burst: 4, bounty: 350, capital: true,
  },
  // Star-destroyer class. Visible from most of the sector, survivable only with upgrades, and
  // deliberately worth more than everything else combined — it is the thing to build toward.
  destroyer: {
    id: 'destroyer', label: 'DESTROYER', shape: 'capital',
    radius: S(0.038), hull: 460, shield: 300, shieldRegen: 14,
    speed: S(1 / 150), turn: 0.1, aggro: S(0.18), standoff: S(0.07),
    damage: 24, cooldownMs: 800, burst: 6, bounty: 1200, capital: true,
  },
}

export const CLASS_IDS: ClassId[] = [
  'skiff', 'interceptor', 'lancer', 'gunship', 'frigate', 'destroyer',
]

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
  const r = roll % 100
  if (r < 26) return CLASSES.skiff
  if (r < 58) return CLASSES.interceptor
  if (r < 78) return CLASSES.lancer
  if (r < 93) return CLASSES.gunship
  if (r < 99) return CLASSES.frigate
  return CLASSES.destroyer
}
