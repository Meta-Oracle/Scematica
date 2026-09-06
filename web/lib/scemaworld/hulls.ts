/**
 * The ships you can fly, as distinct from the parts you bolt onto one.
 *
 * Component upgrades made a ship *better*. They could not make it a different ship, so every
 * player converged on the same silhouette with bigger numbers, and the whole progression was one
 * axis. A hull is a set of trade-offs you commit to: the scout is faster than anything armed and
 * carries almost no armour, the lancer trades a third of its speed for the shields to survive a
 * capital's arc, the marauder is slow and enormous and can absorb a titan's broadside.
 *
 * ## Multipliers, not replacements
 *
 * A hull scales what the components give rather than substituting for them, so an upgrade is
 * never wasted by a later purchase — buying the marauder does not make your level-four shields
 * irrelevant, it makes them worth more. The alternative (flat stats per hull) makes every
 * component you own evaporate the moment you change ship, which teaches people not to change.
 *
 * ## Why hulls cost SCEMA and components cost salvage
 *
 * Two currencies for two decisions. Salvage is what a fight leaves behind and it accumulates
 * steadily, which suits incremental upgrades. A hull is a commitment, and paying for one in a
 * currency you have to *convert* makes it a decision rather than an inevitability. See
 * `economy.ts`.
 *
 * ## Three tiers, and what a tier actually changes
 *
 * The light hulls were the whole game and the ladder topped out at the marauder — a ship
 * explicitly described as "built to stand in front of a capital", which is a supporting role in
 * somebody else's fight. There are two tiers above it now, and the thing that separates them is
 * **not** a bigger multiplier on the same numbers:
 *
 * - **light** — you point, and the ship is pointing. Attitude is nearly instant, the hull is
 *   smaller than anything it can be shot by, and disengaging is free.
 * - **medium** — the tier the sector already had (`warfighter`) and the player did not. Turns
 *   noticeably, is a target a gunner cannot miss, and cannot pivot out of a mistake.
 * - **capital** — you fly a *volume*. Attitude is measured in seconds, your own hull is wider
 *   than a station, and the fight you are in is the one you committed to a minute ago.
 *
 * ## What a capital deliberately does NOT get
 *
 * **Top speed.** Every hull here still outruns every hostile class in the sector, which is a
 * standing invariant (`scale.ts`, `classes.ts`: *disengaging must always be possible, or the game
 * punishes the exploring it is about*). Mass shows up as `agility` and as the linear inertia the
 * flight model already simulates — a dominion reaches cruise eventually and turns like a
 * continent — never as being run down from behind by an interceptor. A capital that could be
 * cornered by a fighter would make the largest purchase in the game the one that takes your
 * options away.
 *
 * **And it never becomes the largest thing in the sector.** The biggest flyable hull is exactly a
 * hostile `dreadnought` across (`check:scemaworld` pins the equality), which leaves the leviathan
 * and the titan larger than anything you can buy. A game where the top purchase makes you the
 * apex object has nothing left to point at.
 */

import type { Shape } from './classes.ts'

export type HullId =
  // light
  | 'skiff'
  | 'scout'
  | 'corvette'
  | 'lancer'
  | 'marauder'
  // medium
  | 'prowler'
  | 'halberd'
  | 'rampart'
  | 'aegis'
  | 'carrack'
  | 'paladin'
  // capital
  | 'castellan'
  | 'monitor'
  | 'vanguard'
  | 'colossus'
  | 'suzerain'
  | 'dominion'

/**
 * What weight of ship this is.
 *
 * A label rather than a derived thing, because it is read by the shipyard (which groups by it),
 * by the flight model (which asks how much attitude authority a hull has) and by the camera
 * (which asks how far to sit back). Deriving it from `size` would work today and would be a
 * threshold somebody has to reconstruct from three call sites the first time a hull lands near
 * a boundary.
 */
export type HullTier = 'light' | 'medium' | 'capital'

export interface HullSpec {
  id: HullId
  label: string
  tier: HullTier
  /**
   * The silhouette flown in third person.
   *
   * Player hulls get their own shapes rather than borrowing an enemy's, because in third person
   * you look at yours for the whole session — and a ship indistinguishable from the thing
   * shooting at you is a bad thing to identify with. The two heavy tiers have `cruiser`,
   * `bulwark` and `sovereign` for exactly that reason: drawing a `marauder` at fifteen times a
   * marauder's size reads as a rendering fault, not as a bigger ship.
   */
  shape: Shape
  /** Drawn size, as a fraction of `EXTENT`. Also the hull's collision radius and camera scale. */
  size: number
  /** One line, shown in the shipyard. */
  note: string
  /** Multiplier on `hullMax`. */
  armour: number
  /** Multiplier on `shieldMax`. */
  shields: number
  /** Multiplier on `topSpeed`. */
  speed: number
  /**
   * Multiplier on every attitude rate and on how hard the controls bite.
   *
   * The stat that makes a tier feel like a tier. Speed cannot carry that weight — it is pinned
   * from below by the disengagement invariant — so what a heavy hull actually pays is the time
   * between deciding to point somewhere and pointing there. Applied to `RATE_ROLL`/`RATE_PITCH`/
   * `RATE_YAW` *and* to `SPIN_ACCEL`/`SPIN_DAMP` together, so a capital both turns slowly and
   * takes a long moment to start and stop turning: scaling the peak rates alone gives a ship that
   * snaps instantly to a slow rotation, which feels like a bug rather than like mass.
   */
  agility: number
  /** Multiplier on `fuelCapacity`. */
  tanks: number
  /** Multiplier on laser rate of fire — above 1 is *faster*. */
  guns: number
  /**
   * Photon tubes: the whole magazine, and **a flat count rather than a multiplier**.
   *
   * The one stat on a hull that is not scaled by a component, and deliberately so. A photon is a
   * single decisive round rather than one of a dozen pecks (`weapons.ts`), and what decides how
   * many you carry is how much ship there is to carry them. Making this a multiplier over a
   * component level would put the number back in a formula, and the number is the point: a pilot
   * has to be able to answer "how many warheads do I have left" without arithmetic.
   *
   * The heavy tiers grow this **slowly** — a dominion carries under twice a marauder's magazine
   * for fifteen times its mass. A capital's advantage is that it survives long enough to use its
   * lasers, not that it carries a warhead for every problem; letting the magazine scale with the
   * hull would turn the tier into an ammunition check and quietly delete the decision the count
   * exists to create.
   *
   * The MISSILES component buys **yield**, not capacity. See `weapons.ts::photonDamage`.
   */
  tubes: number
  /** Extra jump charges over the drive's own. */
  jump: number
  /**
   * Chase-camera distance and height, as multiples of the hull's own drawn size.
   *
   * Per hull rather than a constant, and the constant is the reason: the camera sat at `7.5 ×
   * size` for every ship, which is right for a dart and absurd for a ship a tenth of a sector
   * across — a dominion would be framed from two-thirds of a sector behind, with the ship a
   * speck in the middle of a volume it is supposed to dominate. Heavier hulls sit proportionally
   * *closer*: a capital's own bow filling the lower third of the frame is the view, and it is
   * what makes flying one feel different from flying a fighter that has been scaled up.
   *
   * These never come close to `FAR_PLANE` (about 13 extents), so no hull can push the camera far
   * enough back to clip the sector — pinned by `check:scemaworld` rather than left to arithmetic
   * somebody would have to redo when a size moves.
   */
  chaseBack: number
  chaseUp: number
  /** Price in SCEMA. Zero for the starter. */
  price: number
}

export const HULLS: Record<HullId, HullSpec> = {
  // ── light ──────────────────────────────────────────────────────────────────

  // What you start in. Deliberately unremarkable: a starter that is good at something teaches
  // that the shipyard is optional.
  skiff: {
    id: 'skiff', label: 'SKIFF', tier: 'light', note: 'the hull you arrived in',
    shape: 'interceptor', size: 0.0022, agility: 1, chaseBack: 7.5, chaseUp: 2.2,
    armour: 1, shields: 1, speed: 1, tanks: 1, guns: 1, tubes: 8, jump: 0, price: 0,
  },
  // Fast and fragile. The exploration hull: it will not survive a wing, and it will reach the
  // far side of the sector on one tank and be gone before anything closes.
  scout: {
    id: 'scout', label: 'SCOUT', tier: 'light', note: 'fast, long-legged, and made of paper',
    shape: 'interceptor', size: 0.0019, agility: 1.2, chaseBack: 7.5, chaseUp: 2.2,
    armour: 0.7, shields: 0.9, speed: 1.5, tanks: 1.8, guns: 0.9, tubes: 8, jump: 2, price: 400,
  },
  // The all-rounder, and the one most people should buy first.
  corvette: {
    id: 'corvette', label: 'CORVETTE', tier: 'light', note: 'no weakness and no speciality',
    shape: 'corvette', size: 0.0032, agility: 0.95, chaseBack: 7.5, chaseUp: 2.2,
    armour: 1.6, shields: 1.5, speed: 1.15, tanks: 1.2, guns: 1.2, tubes: 12, jump: 1, price: 900,
  },
  // The gunboat. Slower than a raider, which means committing: you cannot disengage from a fight
  // in this, so you pick the ones you intend to finish.
  lancer: {
    id: 'lancer', label: 'LANCER', tier: 'light', note: 'guns and shields, at the cost of running away',
    shape: 'gunship', size: 0.0046, agility: 0.85, chaseBack: 7.5, chaseUp: 2.2,
    armour: 2.2, shields: 2.6, speed: 0.92, tanks: 1, guns: 1.7, tubes: 16, jump: 1, price: 2200,
  },
  // What a titan is fought in, at the light tier. Enormous for a fighter and small for a warship,
  // which is exactly the gap the medium tier now fills.
  marauder: {
    id: 'marauder', label: 'MARAUDER', tier: 'light', note: 'built to stand in front of a capital',
    shape: 'marauder', size: 0.0078, agility: 0.72, chaseBack: 7.5, chaseUp: 2.2,
    armour: 4.5, shields: 4.2, speed: 0.8, tanks: 1.6, guns: 2.1, tubes: 24, jump: 2, price: 6500,
  },

  // ── medium ─────────────────────────────────────────────────────────────────
  //
  // The rung the *player's* ladder was missing, and the sector has had one since `warfighter`
  // arrived: something a wing of fighters has to work at, that is still a ship rather than a
  // place. Every hull here is a target a competent gunner cannot miss, and none of them can
  // pivot out of a mistake — the trade the whole tier is built on.

  // The medium you buy first: fast for its weight and long-legged, so the tier opens with a hull
  // that still explores rather than one that only fights.
  prowler: {
    id: 'prowler', label: 'PROWLER', tier: 'medium', note: 'a medium that still runs and still explores',
    shape: 'cruiser', size: 0.011, agility: 0.62, chaseBack: 5.6, chaseUp: 1.7,
    armour: 2.4, shields: 2.2, speed: 1.25, tanks: 2.0, guns: 1.3, tubes: 16, jump: 2, price: 12_000,
  },
  // The gun platform. Fires nearly twice as fast as anything below it and turns like a barn.
  halberd: {
    id: 'halberd', label: 'HALBERD', tier: 'medium', note: 'a gun platform that happens to fly',
    shape: 'gunship', size: 0.014, agility: 0.55, chaseBack: 5.6, chaseUp: 1.7,
    armour: 3.2, shields: 3.0, speed: 0.95, tanks: 1.2, guns: 2.4, tubes: 22, jump: 1, price: 18_000,
  },
  // The brawler. Armour first: built to be inside a capital's envelope and stay there.
  rampart: {
    id: 'rampart', label: 'RAMPART', tier: 'medium', note: 'armour first — made to be hit',
    shape: 'marauder', size: 0.018, agility: 0.50, chaseBack: 5.6, chaseUp: 1.7,
    armour: 5.5, shields: 4.6, speed: 0.86, tanks: 1.4, guns: 1.8, tubes: 18, jump: 1, price: 26_000,
  },
  // Shields, and an absurd amount of them. Regenerating capacity is a different resource from
  // armour — it comes back between engagements — so this is the hull that fights all day and
  // loses any single exchange.
  aegis: {
    id: 'aegis', label: 'AEGIS', tier: 'medium', note: 'shields that come back, armour that does not',
    shape: 'cruiser', size: 0.023, agility: 0.47, chaseBack: 5.6, chaseUp: 1.7,
    armour: 4.2, shields: 8.0, speed: 0.90, tanks: 1.6, guns: 1.5, tubes: 20, jump: 2, price: 38_000,
  },
  // The long-range hull: fuel and jump charges rather than guns. The one that crosses a sector
  // nobody has scouted and comes back.
  carrack: {
    id: 'carrack', label: 'CARRACK', tier: 'medium', note: 'range, fuel and jumps — the deep hull',
    shape: 'corvette', size: 0.030, agility: 0.44, chaseBack: 5.6, chaseUp: 1.7,
    armour: 4.8, shields: 4.4, speed: 1.05, tanks: 3.2, guns: 1.4, tubes: 24, jump: 4, price: 55_000,
  },
  // The top of the tier and the last hull that is unambiguously a ship. If you are going to fight
  // a capital without becoming one, this is what you do it in.
  paladin: {
    id: 'paladin', label: 'PALADIN', tier: 'medium', note: 'the last hull that is still a ship',
    shape: 'cruiser', size: 0.040, agility: 0.40, chaseBack: 5.6, chaseUp: 1.7,
    armour: 6.5, shields: 6.2, speed: 0.92, tanks: 1.8, guns: 2.6, tubes: 26, jump: 2, price: 80_000,
  },

  // ── capital ────────────────────────────────────────────────────────────────
  //
  // A capital is a **volume**, and the tier is defined by what that costs rather than by what it
  // gives. Your hull is wider than a station. Your attitude is measured in seconds. You cannot
  // change your mind about a fight, and the sector's structures stop being things you fly between.
  //
  // Every one of them still outruns every hostile in the sector. That is not an oversight — see
  // the header. What a capital gives up is the ability to *turn*, which is the axis the entire
  // dogfight model is built on (`classes.ts`), so a fighter that stays outside your arc is a
  // genuine problem for the largest ship in the game. That asymmetry is the tier.

  // The entry capital, and the cheapest hull that is one. Small enough to still be flown rather
  // than steered, which makes it the right place to find out whether the tier suits you.
  castellan: {
    id: 'castellan', label: 'CASTELLAN', tier: 'capital', note: 'the smallest thing that is truly a capital',
    shape: 'bulwark', size: 0.048, agility: 0.30, chaseBack: 3.6, chaseUp: 1.05,
    armour: 9, shields: 8, speed: 0.82, tanks: 2.0, guns: 2.4, tubes: 28, jump: 2, price: 120_000,
  },
  // Guns, and nothing else. The slowest-turning hull short of the top two and the hardest-hitting
  // for its price: a siege ship, pointed at something before it starts and not re-pointed after.
  monitor: {
    id: 'monitor', label: 'MONITOR', tier: 'capital', note: 'a siege hull — aim it before you commit',
    shape: 'bulwark', size: 0.062, agility: 0.25, chaseBack: 3.6, chaseUp: 1.05,
    armour: 12, shields: 9, speed: 0.72, tanks: 1.8, guns: 3.4, tubes: 34, jump: 1, price: 175_000,
  },
  // The mobile capital: shields, fuel and three extra jump charges, at the cost of armour. The
  // hull for somebody who intends to keep leaving.
  vanguard: {
    id: 'vanguard', label: 'VANGUARD', tier: 'capital', note: 'a capital that can still leave',
    shape: 'bulwark', size: 0.080, agility: 0.28, chaseBack: 3.6, chaseUp: 1.05,
    armour: 11, shields: 13, speed: 0.95, tanks: 2.4, guns: 2.8, tubes: 30, jump: 3, price: 260_000,
  },
  // The first of the spinal hulls, and where the tier stops pretending to be a ship. It does not
  // manoeuvre; it arrives.
  colossus: {
    id: 'colossus', label: 'COLOSSUS', tier: 'capital', note: 'it does not manoeuvre, it arrives',
    shape: 'sovereign', size: 0.100, agility: 0.20, chaseBack: 3.6, chaseUp: 1.05,
    armour: 18, shields: 14, speed: 0.68, tanks: 2.2, guns: 3.0, tubes: 36, jump: 2, price: 380_000,
  },
  // Armour and shields in the same hull, which nothing below this can afford. The one that stands
  // in a leviathan's broadside and answers it.
  suzerain: {
    id: 'suzerain', label: 'SUZERAIN', tier: 'capital', note: 'stands in a leviathan’s fire and answers it',
    shape: 'sovereign', size: 0.118, agility: 0.17, chaseBack: 3.6, chaseUp: 1.05,
    armour: 21, shields: 19, speed: 0.66, tanks: 2.6, guns: 3.6, tubes: 40, jump: 3, price: 550_000,
  },
  // The largest hull anyone can own — exactly a hostile dreadnought across, and deliberately no
  // larger. The leviathan and the titan stay bigger than anything you can buy, because a game
  // whose top purchase makes you the apex object in the sector has nothing left to point at.
  dominion: {
    id: 'dominion', label: 'DOMINION', tier: 'capital', note: 'a dreadnought of your own — and still not the biggest thing out here',
    shape: 'sovereign', size: 0.135, agility: 0.13, chaseBack: 3.6, chaseUp: 1.05,
    armour: 26, shields: 24, speed: 0.62, tanks: 3.0, guns: 4.0, tubes: 44, jump: 3, price: 800_000,
  },
}

/** Every hull, light to heavy and cheapest first within a tier. The shipyard's order. */
export const HULL_IDS: HullId[] = [
  'skiff', 'scout', 'corvette', 'lancer', 'marauder',
  'prowler', 'halberd', 'rampart', 'aegis', 'carrack', 'paladin',
  'castellan', 'monitor', 'vanguard', 'colossus', 'suzerain', 'dominion',
]

/** The tiers, in the order the shipyard groups them. */
export const HULL_TIERS: HullTier[] = ['light', 'medium', 'capital']

/** One line per tier, shown as the group heading. Says what the trade is, not what the stats are. */
export const TIER_NOTE: Record<HullTier, string> = {
  light: 'you point, and the ship is pointing. Disengaging is free.',
  medium: 'turns noticeably, cannot be missed, cannot pivot out of a mistake.',
  capital: 'a volume rather than a ship. Attitude in seconds; commit before you arrive.',
}

/** The hulls in a tier, in shipyard order. */
export function hullsOf(tier: HullTier): HullSpec[] {
  return HULL_IDS.map((h) => HULLS[h]).filter((s) => s.tier === tier)
}

/**
 * Whether a hull can be bought given what is already owned.
 *
 * Only "not this one" — there is no tech tree, and adding one for the heavy tiers would be the
 * wrong lesson twice over. A player who has scraped together a hundred and twenty thousand SCEMA
 * for a castellan has already done everything a prerequisite would be asking them to do, and a
 * gate would only stop them spending it. The tier's *cost* is the gate.
 */
export function purchasable(current: HullId, want: HullId): boolean {
  return current !== want
}
