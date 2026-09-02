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
import { SHIELD_DELAY_MS } from './classes.ts'

export type Component =
  | 'engine' | 'hull' | 'sensors' | 'laser' | 'missiles' | 'tanks' | 'shields' | 'drive'

export interface Ship {
  /** Litres, abstract. Runs down with thrust; refuelled at a depot or dock. */
  fuel: number
  /**
   * Hull. **The primary health, and the one that does not come back.**
   *
   * Shields absorb first and regenerate; hull is repaired only at a dock, for salvage. That
   * asymmetry is the whole rhythm of a fight — break contact, let shields recover, re-engage —
   * and inverting it (a regenerating primary bar) would make every engagement a war of
   * attrition against a clock rather than a decision about whether to commit.
   */
  hull: number
  /** Shields. Absorb before hull, and come back after a lull. */
  shield: number
  /** Milliseconds of the last hit taken, so regeneration knows when the lull started. */
  lastHitMs: number
  /** Charges for the jump drive. Scarce, and refilled only at a dock. */
  jumpFuel: number
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
  shields: { label: 'SHIELDS', effect: 'buffer and recharge', base: 150 },
  sensors: { label: 'SENSORS', effect: 'contact range and lock cone', base: 110 },
  laser: { label: 'LASER', effect: 'rate of fire', base: 160 },
  missiles: { label: 'PHOTON', effect: 'magazine', base: 180 },
  drive: { label: 'JUMP DRIVE', effect: 'jump charges and spin-up', base: 200 },
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
    shield: shieldMax(0),
    lastHitMs: -1e9,
    jumpFuel: jumpCapacity(0),
    salvage: 0,
    levels: {
      engine: 0, hull: 0, shields: 0, sensors: 0, laser: 0, missiles: 0, tanks: 0, drive: 0,
    },
    scavenged: [],
    docked: null,
  }
}

export function shieldMax(shields: number): number {
  return 40 + shields * 34
}

/** Shield points per second, once the lull has elapsed. */
export function shieldRegen(shields: number): number {
  return 6 + shields * 4
}

export function jumpCapacity(drive: number): number {
  return 3 + drive
}

/** Milliseconds to spin the drive up. Faster with a better one, never instant. */
export function jumpCharge(drive: number, base: number): number {
  return Math.round(base * (1 - drive * 0.15))
}

export function fuelCapacity(tanks: number): number {
  return 120 + tanks * 60
}

export function hullMax(hull: number): number {
  return 100 + hull * 60
}

/** World units per second at full throttle. */
export function topSpeed(engine: number): number {
  return SPEED_SHIP + engine * SPEED_SHIP_PER_LEVEL
}

/**
 * Multiplier on contact range — how far out the sensor panel resolves a hostile.
 *
 * No longer a multiplier on *draw* distance: the far plane covers the whole sector now, so a
 * SENSORS upgrade buys information rather than visibility. Those were conflated while draw
 * distance was gated by legibility, and the conflation was what put a wall of fog around a
 * volume the entire design is about the size of.
 */
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
  // Roughly a quarter of what it was. A full tank used to be fifty seconds of open throttle,
  // which on a sector this size is not a fuel economy, it is a countdown — you spent the whole
  // of it reaching the first thing you saw. It is now a few minutes of hard flying, so running
  // dry is a consequence of a decision rather than of leaving the hangar.
  return (0.16 + engine * 0.03) * (t * t * 3.2)
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

/**
 * Take a hit: shields first, then hull.
 *
 * Overflow carries through in the same hit rather than being absorbed by a shield that had one
 * point left — a shot that "breaks through" has to actually break through, or the last point of
 * shield is worth a whole volley and players learn to fight at 1%.
 */
export function damage(ship: Ship, amount: number, nowMs: number): Ship {
  const absorbed = Math.min(ship.shield, amount)
  return {
    ...ship,
    shield: ship.shield - absorbed,
    hull: Math.max(0, ship.hull - (amount - absorbed)),
    lastHitMs: nowMs,
  }
}

/** Regenerate shields, but only after a lull. Hull never regenerates — see the `Ship` note. */
export function recharge(ship: Ship, seconds: number, nowMs: number): Ship {
  if (nowMs - ship.lastHitMs < SHIELD_DELAY_MS) return ship
  const max = shieldMax(ship.levels.shields)
  if (ship.shield >= max) return ship
  return { ...ship, shield: Math.min(max, ship.shield + shieldRegen(ship.levels.shields) * seconds) }
}

/** True while shields are down, which is what the HUD turns red about. */
export function exposed(ship: Ship): boolean {
  return ship.shield <= 0
}

export function destroyed(ship: Ship): boolean {
  return ship.hull <= 0
}

// ── services ──────────────────────────────────────────────────────────────────

export type ServiceResult = { ship: Ship; message: string; ok: boolean }

/**
 * Refuel. Fills the main tanks and, at a dock, the jump drive too.
 *
 * A depot does thrust fuel only. Jump charges are the scarce resource and a sector has six
 * times as many depots as docks, so where you can jump *from* is a real constraint on a route
 * rather than a formality.
 */
export function refuel(ship: Ship, jump = false): ServiceResult {
  const cap = fuelCapacity(ship.levels.tanks)
  const jcap = jumpCapacity(ship.levels.drive)
  const wantsJump = jump && ship.jumpFuel < jcap
  if (ship.fuel >= cap && !wantsJump) return { ship, message: 'tanks already full', ok: false }
  return {
    ship: { ...ship, fuel: cap, jumpFuel: wantsJump ? jcap : ship.jumpFuel },
    message: wantsJump ? 'refuelled — jump drive charged' : 'refuelled',
    ok: true,
  }
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
  if (c === 'shields') upgraded.shield = shieldMax(levels.shields)
  if (c === 'drive') upgraded.jumpFuel = jumpCapacity(levels.drive)
  return { ship: upgraded, message: `${UPGRADES[c].label} → level ${level + 1}`, ok: true }
}

/**
 * Salvage for destroying a hostile.
 *
 * The amount comes from the victim's **class** (`classes.ts`), never from anything the record
 * reported about it — see the module note. `BOUNTY` remains as the default for a kill whose
 * class is unknown, and as the unit the tests count in.
 */
export const BOUNTY = 25

export function bounty(ship: Ship, amount: number = BOUNTY): Ship {
  return { ...ship, salvage: ship.salvage + amount }
}
