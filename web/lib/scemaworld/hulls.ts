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
 * currency you have to *convert* — deliberately at a loss — makes it a decision rather than an
 * inevitability. See `economy.ts`, and note the placeholder warning there.
 */

export type HullId = 'skiff' | 'scout' | 'corvette' | 'lancer' | 'marauder'

export interface HullSpec {
  id: HullId
  label: string
  /**
   * The silhouette flown in third person.
   *
   * Player hulls get their own shapes rather than borrowing an enemy's, because in third person
   * you look at yours for the whole session — and a ship indistinguishable from the thing
   * shooting at you is a bad thing to identify with.
   */
  shape: 'interceptor' | 'corvette' | 'gunship' | 'marauder'
  /** Drawn size, as a fraction of `EXTENT`. Also the chase camera's distance scale. */
  size: number
  /** One line, shown in the shipyard. */
  note: string
  /** Multiplier on `hullMax`. */
  armour: number
  /** Multiplier on `shieldMax`. */
  shields: number
  /** Multiplier on `topSpeed`. */
  speed: number
  /** Multiplier on `fuelCapacity`. */
  tanks: number
  /** Multiplier on laser rate of fire — above 1 is *faster*. */
  guns: number
  /**
   * Photon tubes: the whole magazine, and **a flat count rather than a multiplier**.
   *
   * The one stat on a hull that is not scaled by a component, and deliberately so. A photon is
   * now a single decisive round rather than one of a dozen pecks (`weapons.ts`), and what
   * decides how many you carry is how much ship there is to carry them — six on a marauder,
   * one on a scout. Making this a multiplier over a component level would put the number back
   * in a formula, and the number is the point: a pilot has to be able to answer "how many
   * missiles do I have left" without arithmetic, because that count is the decision.
   *
   * The MISSILES component therefore buys **yield**, not capacity. See `weapons.ts::photonDamage`.
   */
  tubes: number
  /** Extra jump charges over the drive's own. */
  jump: number
  /** Price in SCEMA. Zero for the starter. */
  price: number
}

export const HULLS: Record<HullId, HullSpec> = {
  // What you start in. Deliberately unremarkable: a starter that is good at something teaches
  // that the shipyard is optional.
  skiff: {
    id: 'skiff', label: 'SKIFF', note: 'the hull you arrived in',
    shape: 'interceptor', size: 0.0022,
    armour: 1, shields: 1, speed: 1, tanks: 1, guns: 1, tubes: 8, jump: 0, price: 0,
  },
  // Fast and fragile. The exploration hull: it will not survive a wing, and it will reach the
  // far side of the sector on one tank and be gone before anything closes.
  scout: {
    id: 'scout', label: 'SCOUT', note: 'fast, long-legged, and made of paper',
    shape: 'interceptor', size: 0.0019,
    armour: 0.7, shields: 0.9, speed: 1.5, tanks: 1.8, guns: 0.9, tubes: 8, jump: 2, price: 400,
  },
  // The all-rounder, and the one most people should buy first.
  corvette: {
    id: 'corvette', label: 'CORVETTE', note: 'no weakness and no speciality',
    shape: 'corvette', size: 0.0032,
    armour: 1.6, shields: 1.5, speed: 1.15, tanks: 1.2, guns: 1.2, tubes: 12, jump: 1, price: 900,
  },
  // The gunboat. Slower than a raider, which means committing: you cannot disengage from a fight
  // in this, so you pick the ones you intend to finish.
  lancer: {
    id: 'lancer', label: 'LANCER', note: 'guns and shields, at the cost of running away',
    shape: 'gunship', size: 0.0046,
    armour: 2.2, shields: 2.6, speed: 0.92, tanks: 1, guns: 1.7, tubes: 16, jump: 1, price: 2200,
  },
  // What a titan is fought in. Enormous, ponderous, and the only hull that survives a war-class
  // broadside long enough to answer it.
  marauder: {
    id: 'marauder', label: 'MARAUDER', note: 'built to stand in front of a capital',
    shape: 'marauder', size: 0.0078,
    armour: 4.5, shields: 4.2, speed: 0.8, tanks: 1.6, guns: 2.1, tubes: 24, jump: 2, price: 6500,
  },
}

export const HULL_IDS: HullId[] = ['skiff', 'scout', 'corvette', 'lancer', 'marauder']

/**
 * Whether a hull can be bought given what is already owned.
 *
 * Only "not this one" — there is no tech tree. A player who has scraped together six and a half
 * thousand SCEMA for a marauder has already done the work a gate would be asking them to do, and
 * a prerequisite would only stop them spending it.
 */
export function purchasable(current: HullId, want: HullId): boolean {
  return current !== want
}
