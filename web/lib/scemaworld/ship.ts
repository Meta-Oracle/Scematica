/**
 * The ship: fuel, hull, and what you can spend salvage on.
 *
 * Pure. No clock, no input, no GL — `check:scemaworld` pins it.
 *
 * ## There is an economy now, and the rule it must not break has been sharpened
 *
 * The first version of this game said "no economy" and a test enforced it. That rule was
 * aimed at a real failure and was stated too broadly. The failure is this: **no quantity in
 * the record may translate into a reward.** Attach a payout to `blind_spots` and you have paid
 * somebody to hide them; attach one to signal magnitude and you have paid them to understate
 * it. A producer with an incentive to misreport is the one thing this project cannot absorb.
 *
 * Salvage is earned from **what you do** — destroying hostiles, scavenging derelicts you flew
 * out to — never from what a record *says*. The record shapes the challenge; it never sets the
 * payout. A world with more blind spots is not worth more, it is worth the same and is harder
 * to survive. `check:scemaworld` asserts the reward function reads no record field.
 *
 * And it stays a single-player progression. No transfer, no price, no token: the moment
 * salvage is worth something outside the game, the paragraph above stops holding.
 */

import { SPEED_SHIP, SPEED_SHIP_PER_LEVEL } from './scale.ts'

export type Component = 'engine' | 'hull' | 'sensors' | 'laser' | 'missiles' | 'tanks'

export interface Ship {
  /** Litres, abstract. Runs down with thrust; refuelled at a depot or dock. */
  fuel: number
  hull: number
  salvage: number
  /** Level 0..MAX_LEVEL per component. */
  levels: Record<Component, number>
  /** Node ids already scavenged, so a derelict pays once. */
  scavenged: number[]
  /** Node id currently in docking range, if any. */
  docked: number | null
}

export const MAX_LEVEL = 4

/** What a component does at each level. Flat tables, so a change is visible in a diff. */
export const UPGRADES: Record<Component, { label: string; effect: string; base: number }> = {
  engine: { label: 'ENGINE', effect: 'top speed', base: 120 },
  tanks: { label: 'TANKS', effect: 'fuel capacity', base: 100 },
  hull: { label: 'HULL', effect: 'integrity', base: 140 },
  sensors: { label: 'SENSORS', effect: 'draw distance and lock cone', base: 110 },
  laser: { label: 'LASER', effect: 'rate of fire', base: 160 },
  missiles: { label: 'PHOTON', effect: 'magazine', base: 180 },
}

/** Cost of the next level. Superlinear so late upgrades are a decision, not a formality. */
export function upgradeCost(c: Component, level: number): number | null {
  if (level >= MAX_LEVEL) return null
  return UPGRADES[c].base * (level + 1) * (level + 1)
}

export function newShip(): Ship {
  return {
    fuel: fuelCapacity(0),
    hull: hullMax(0),
    salvage: 0,
    levels: { engine: 0, hull: 0, sensors: 0, laser: 0, missiles: 0, tanks: 0 },
    scavenged: [],
    docked: null,
  }
}

export function fuelCapacity(tanks: number): number {
  return 100 + tanks * 45
}

export function hullMax(hull: number): number {
  return 100 + hull * 60
}

/** World units per second at full throttle. */
export function topSpeed(engine: number): number {
  return SPEED_SHIP + engine * SPEED_SHIP_PER_LEVEL
}

/** Multiplier on the sensor draw distance. */
export function sensorGain(sensors: number): number {
  return 1 + sensors * 0.35
}

/** Seconds between laser shots. */
export function laserCooldown(laser: number): number {
  return Math.max(40, 110 - laser * 18)
}

export function photonMagazine(missiles: number): number {
  return 12 + missiles * 6
}

/**
 * Fuel burned per second at a given throttle.
 *
 * Superlinear in throttle, so cruising is cheap and running flat out across a sector is a
 * choice with a cost. This is what turns a big empty volume into a decision — without it,
 * distance is only time.
 */
export function burnRate(throttle: number, engine: number): number {
  const t = Math.max(0, Math.min(1, throttle))
  return (0.6 + engine * 0.12) * (t * t * 3.2)
}

/** Spend fuel. Returns the ship unchanged when the tank is dry — thrust simply stops. */
export function burn(ship: Ship, seconds: number, throttle: number): Ship {
  const used = burnRate(throttle, ship.levels.engine) * seconds
  return { ...ship, fuel: Math.max(0, ship.fuel - used) }
}

/** True when the drive will actually push. A dry ship coasts; it does not stall in place. */
export function hasFuel(ship: Ship): boolean {
  return ship.fuel > 0
}

export function damage(ship: Ship, amount: number): Ship {
  return { ...ship, hull: Math.max(0, ship.hull - amount) }
}

export function destroyed(ship: Ship): boolean {
  return ship.hull <= 0
}

// ── services ──────────────────────────────────────────────────────────────────

export type ServiceResult = { ship: Ship; message: string; ok: boolean }

export function refuel(ship: Ship): ServiceResult {
  const cap = fuelCapacity(ship.levels.tanks)
  if (ship.fuel >= cap) return { ship, message: 'tanks already full', ok: false }
  return { ship: { ...ship, fuel: cap }, message: 'refuelled', ok: true }
}

export function repair(ship: Ship): ServiceResult {
  const max = hullMax(ship.levels.hull)
  if (ship.hull >= max) return { ship, message: 'hull intact', ok: false }
  // Repair costs salvage, and being unable to afford it is a real state — a player stranded
  // with a broken hull and no salvage has to fly carefully rather than being rescued.
  const cost = Math.ceil((max - ship.hull) * 0.8)
  if (ship.salvage < cost) {
    return { ship, message: `repair costs ${cost} salvage; you have ${ship.salvage}`, ok: false }
  }
  return {
    ship: { ...ship, hull: max, salvage: ship.salvage - cost },
    message: `hull restored for ${cost} salvage`,
    ok: true,
  }
}

/**
 * Strip a derelict. Pays once per node, ever.
 *
 * The payout is flat and comes from the *act*, not from anything the record reported about
 * that object — see the module note. A derelict that had a bigger `attrs` map is not worth
 * more, or the number in the record would be worth inflating.
 */
export function scavenge(ship: Ship, nodeId: number): ServiceResult {
  if (ship.scavenged.includes(nodeId)) {
    return { ship, message: 'already stripped', ok: false }
  }
  return {
    ship: { ...ship, salvage: ship.salvage + 40, scavenged: [...ship.scavenged, nodeId] },
    message: '+40 salvage',
    ok: true,
  }
}

export function buy(ship: Ship, c: Component): ServiceResult {
  const level = ship.levels[c]
  const cost = upgradeCost(c, level)
  if (cost === null) return { ship, message: `${UPGRADES[c].label} is at maximum`, ok: false }
  if (ship.salvage < cost) {
    return { ship, message: `${cost} salvage needed; you have ${ship.salvage}`, ok: false }
  }
  const levels = { ...ship.levels, [c]: level + 1 }
  const upgraded: Ship = { ...ship, levels, salvage: ship.salvage - cost }
  // A tank or hull upgrade is worthless if it does not also carry the fuel or integrity it
  // just paid for, and a player who bought one and saw no change would reasonably think it
  // did nothing.
  if (c === 'tanks') upgraded.fuel = fuelCapacity(levels.tanks)
  if (c === 'hull') upgraded.hull = hullMax(levels.hull)
  return { ship: upgraded, message: `${UPGRADES[c].label} → level ${level + 1}`, ok: true }
}

/** Salvage for destroying a hostile. Flat: see the module note on what may not set a payout. */
export const BOUNTY = 25

export function bounty(ship: Ship): Ship {
  return { ...ship, salvage: ship.salvage + BOUNTY }
}
