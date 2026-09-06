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
import { HULLS, type HullId } from './hulls.ts'
import { HITBOX } from './hitbox.ts'
import { EXTENT } from './scale.ts'

export type Component =
  | 'engine' | 'hull' | 'sensors' | 'laser' | 'missiles' | 'tanks' | 'shields' | 'drive'

export interface Ship {
  /**
   * The hull being flown. Scales what every component gives.
   *
   * A multiplier rather than a replacement, so an upgrade is never wasted by a later purchase:
   * buying a marauder does not make level-four shields irrelevant, it makes them worth more.
   * Flat per-hull stats would evaporate everything you own the moment you changed ship, which
   * teaches people not to change.
   */
  frame: HullId
  /**
   * SCEMA. **A real balance now** — see `economy.ts`, and `claim.ts` for what bounds it.
   *
   * This said "placeholder currency … a number in a tab, not a token", which stopped being true
   * when the withdrawal path landed and stayed on the field for a while afterwards. It is a
   * number in a tab *and* redeemable for $SCEMA from a fixed treasury, and it now survives the
   * tab closing (`wallet.ts`) — which is the whole reason the withdrawal path is reachable at
   * all, since the minimum claim is more than most single sessions used to bank.
   */
  scema: number
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
  // **Yield, not magazine.** The magazine is the hull's tube count and nothing scales it — see
  // `hulls.ts::tubes` and `weapons.ts::photonMagazine`. Levelling this makes each of the rounds
  // you already carry hit harder, which is the only way "six rounds on a marauder, one on a
  // scout" can stay literally true at every level of progression.
  missiles: { label: 'PHOTON', effect: 'warhead yield', base: 180 },
  drive: { label: 'JUMP DRIVE', effect: 'jump charges and spin-up', base: 200 },
}

/** Cost of the next level. Superlinear so late upgrades are a decision, not a formality. */
export function upgradeCost(c: Component, level: number): number | null {
  if (level >= MAX_LEVEL) return null
  return UPGRADES[c].base * (level + 1) * (level + 1)
}

export function newShip(): Ship {
  return {
    frame: 'skiff',
    scema: 0,
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

export function shieldMax(shields: number, frame: HullId = 'skiff'): number {
  return Math.round((40 + shields * 34) * HULLS[frame].shields)
}

/** Shield points per second, once the lull has elapsed. */
export function shieldRegen(shields: number): number {
  return 6 + shields * 4
}

export function jumpCapacity(drive: number, frame: HullId = 'skiff'): number {
  return 3 + drive + HULLS[frame].jump
}

/** Milliseconds to spin the drive up. Faster with a better one, never instant. */
export function jumpCharge(drive: number, base: number): number {
  return Math.round(base * (1 - drive * 0.15))
}

export function fuelCapacity(tanks: number, frame: HullId = 'skiff'): number {
  // **200 at level zero.** The sector grew by a factor of fifty in volume, and a tank sized for
  // the old one turned every crossing into a fuel calculation — which is a fine tension when the
  // map is small enough to know, and simple attrition when it is not. Levels still add 60 each,
  // so the upgrade keeps the same shape and only the floor moved.
  return Math.round((200 + tanks * 60) * HULLS[frame].tanks)
}

export function hullMax(hull: number, frame: HullId = 'skiff'): number {
  return Math.round((100 + hull * 60) * HULLS[frame].armour)
}

/** World units per second at full throttle. */
export function topSpeed(engine: number, frame: HullId = 'skiff'): number {
  return Math.round((SPEED_SHIP + engine * SPEED_SHIP_PER_LEVEL) * HULLS[frame].speed)
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
export function laserCooldown(laser: number, frame: HullId = 'skiff'): number {
  // `guns` above 1 is *faster*, so it divides. A hull that multiplied the cooldown would make
  // the better gunboat shoot more slowly, which is the kind of sign error that survives review.
  return Math.max(26, Math.round((110 - laser * 18) / HULLS[frame].guns))
}

// `photonMagazine` used to live here and took a component level. It takes a *hull* now and lives
// in `weapons.ts`, beside the weapon it describes.
//
// It is deliberately **not** re-exported from here. `weapons.ts` already imports `laserCooldown`
// from this module, so a re-export would close a cycle between the two — which happens to work
// for hoisted function declarations and stops working the first time either file grows a
// module-level constant that the other reads during evaluation. A cycle that only breaks later,
// under an edit that looks unrelated, is worse than making two call sites name the right module.

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
  const max = shieldMax(ship.levels.shields, ship.frame)
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

/**
 * A ship's current maxima, with its hull applied.
 *
 * One place, because every one of these was previously a bare level lookup and adding a frame
 * parameter to each created five chances to pass the wrong thing. Callers ask the ship.
 */
export function limits(ship: Ship) {
  return {
    fuel: fuelCapacity(ship.levels.tanks, ship.frame),
    hull: hullMax(ship.levels.hull, ship.frame),
    shield: shieldMax(ship.levels.shields, ship.frame),
    jump: jumpCapacity(ship.levels.drive, ship.frame),
    speed: topSpeed(ship.levels.engine, ship.frame),
    cooldown: laserCooldown(ship.levels.laser, ship.frame),
  }
}

/**
 * Move to a different hull, carrying every component across and topping everything up.
 *
 * A new ship arrives fuelled and whole. Delivering one empty would mean the first thing a player
 * does after the largest purchase in the game is limp to a depot, which is a strange lesson to
 * attach to a reward.
 */
export function refit(ship: Ship, frame: HullId): Ship {
  const next = { ...ship, frame }
  return {
    ...next,
    fuel: fuelCapacity(next.levels.tanks, frame),
    hull: hullMax(next.levels.hull, frame),
    shield: shieldMax(next.levels.shields, frame),
    jumpFuel: jumpCapacity(next.levels.drive, frame),
  }
}

/**
 * How far ahead of the ship's centre its guns are.
 *
 * The hull's own drawn length, so a round leaves the prow of whatever you are flying. Firing from
 * the centre puts the muzzle flash inside the hull in third person, and firing from the *camera*
 * — which is where it started — puts it behind you.
 */
export function noseOffset(frame: HullId): number {
  return Math.round(EXTENT * HULLS[frame].size * HITBOX[HULLS[frame].shape].ahead * 1.15)
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
  const cap = fuelCapacity(ship.levels.tanks, ship.frame)
  const jcap = jumpCapacity(ship.levels.drive, ship.frame)
  const wantsJump = jump && ship.jumpFuel < jcap
  if (ship.fuel >= cap && !wantsJump) return { ship, message: 'tanks already full', ok: false }
  return {
    ship: { ...ship, fuel: cap, jumpFuel: wantsJump ? jcap : ship.jumpFuel },
    message: wantsJump ? 'refuelled — jump drive charged' : 'refuelled',
    ok: true,
  }
}

export function repair(ship: Ship): ServiceResult {
  const max = hullMax(ship.levels.hull, ship.frame)
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
  if (c === 'tanks') upgraded.fuel = fuelCapacity(levels.tanks, ship.frame)
  if (c === 'hull') upgraded.hull = hullMax(levels.hull, ship.frame)
  if (c === 'shields') upgraded.shield = shieldMax(levels.shields, ship.frame)
  if (c === 'drive') upgraded.jumpFuel = jumpCapacity(levels.drive, ship.frame)
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
