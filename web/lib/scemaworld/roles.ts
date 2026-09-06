/**
 * What you are out here to do.
 *
 * Four roles, chosen once when a world is opened. A role is not a class or a stat block — every
 * role flies the same hulls with the same components — it is **who shoots at you and what you are
 * paid for**, which turns out to be the whole of what makes a sector feel different.
 *
 * ## The rule this had to be built around
 *
 * `classes.ts` gave the marshal war classes a bounty of **zero**, with a reason worth quoting:
 * "a payout for killing the good guys would be the game paying for the sector to be less policed."
 * That reasoning was right, and a pirate role appears to contradict it.
 *
 * It does not, because the objection was never to the killing — it was to being paid for it *by
 * a system that also wants the patrol to exist*. The resolution is that **you are paid only for
 * what your role hunts, and nothing for anything else**. A bounty hunter still earns nothing for
 * a marshal; that has not moved a single unit. A pirate earns nothing for a raider. What each
 * role is paid for is exactly what it declared itself to be when the world was opened, so the
 * payout is never an incentive to do the thing the game would rather you did not — it is the
 * definition of the thing you said you were.
 *
 * The rule that has not moved at all: **no quantity in the record may translate into a reward.**
 * A role changes who pays, never how much a world is worth. Every world pays every role
 * identically, and `check:scemaworld` asserts this file reads no record field.
 *
 * ## Why hostility is a function and not a flag
 *
 * `hostileTo` used to be `f === 'raider'`, read in six places — the enemy AI, the jump inhibitor,
 * the sensor board, the threat readout, the fire gate and the collision handler. Every one of
 * those is a place where "is this thing my enemy" has to give the same answer, and a pirate makes
 * the answer depend on who is asking. So it takes the role, and there is still exactly one
 * implementation.
 */

import type { Faction } from './factions.ts'

export type RoleId = 'bounty-hunter' | 'trader' | 'smuggler' | 'pirate'

export interface Role {
  id: RoleId
  label: string
  /** One line, shown on the selection screen. */
  blurb: string
  /** The sentence that says what you actually do, shown under the blurb. */
  brief: string
  /** Factions this role is paid to destroy. Bounties are zero for everything else. */
  hunts: Faction[]
  /** Factions that open fire on sight. */
  huntedBy: Faction[]
  /**
   * What a faction station will sell you, and what it refuses.
   *
   * A pirate is not welcome at a marshal citadel, which is the whole texture of the role: the
   * services you can reach are part of what you chose.
   */
  welcome: Faction[]
}

export const ROLES: Record<RoleId, Role> = {
  'bounty-hunter': {
    id: 'bounty-hunter',
    label: 'BOUNTY HUNTER',
    blurb: 'Paid per raider hull. The patrol tolerates you.',
    brief:
      'You hunt the orange. Raider wings and the raider garrison carry bounties; the marshals ' +
      'leave you alone and their citadels will rearm you.',
    hunts: ['raider'],
    huntedBy: ['raider'],
    welcome: ['marshal'],
  },
  trader: {
    id: 'trader',
    label: 'TRADER',
    blurb: 'Paid for cargo delivered. Everyone else is a hazard.',
    brief:
      'You run contracts between stations. Nothing pays you for a kill — raiders will still ' +
      'come for your hold, and the patrol will not mind you at all.',
    hunts: [],
    huntedBy: ['raider'],
    welcome: ['marshal'],
  },
  smuggler: {
    id: 'smuggler',
    label: 'SMUGGLER',
    blurb: 'Paid far better for cargo nobody should be carrying. Shot at by both sides.',
    brief:
      'The trader contracts, at several times the rate, for cargo the patrol scans for. Both ' +
      'sides will shoot at you — raiders want your hold, marshals want your manifest — but the ' +
      'freeholds still take your business, which is the only reason the job is possible.',
    hunts: [],
    huntedBy: ['raider', 'marshal'],
    // **A freehold still takes your business.** `welcome: []` was the first draft and it left the
    // smuggler with nowhere at all to accept a contract — a role with no board is not a hard role,
    // it is an unplayable one, and nothing in the type system said so. Being *hunted* by the
    // raiders and being *served* by their stations is the texture of the job rather than a
    // contradiction: the freehold wants the cargo moved, its wings still want your hold.
    welcome: ['raider'],
  },
  pirate: {
    id: 'pirate',
    label: 'PIRATE',
    blurb: 'Paid per marshal hull. The raiders count you as one of theirs.',
    brief:
      'You hunt the yellow. The patrol and its war classes carry the bounties; raider wings ' +
      'ignore you, and no marshal citadel will open its rings to you.',
    hunts: ['marshal'],
    huntedBy: ['marshal'],
    welcome: ['raider'],
  },
}

export const ROLE_IDS: RoleId[] = ['bounty-hunter', 'trader', 'smuggler', 'pirate']

/** The role a world opens with when nobody has chosen. Never inferred from the record. */
export const DEFAULT_ROLE: RoleId = 'bounty-hunter'

export function roleOf(id: RoleId | null | undefined): Role {
  return ROLES[id ?? DEFAULT_ROLE] ?? ROLES[DEFAULT_ROLE]
}

/**
 * Whether `faction` will shoot at a player flying `role`.
 *
 * The single implementation, for the same reason `lib/mesh/view.ts::toneFor` is the only thing
 * that picks a colour: six call sites have to agree about who the enemy is, and a seventh that
 * disagreed would be a craft that steers at you and never fires.
 */
export function hostileToPlayer(faction: Faction, role: RoleId): boolean {
  return roleOf(role).huntedBy.includes(faction)
}

/**
 * What destroying a craft of `faction` pays a player flying `role`, as a multiple of its bounty.
 *
 * Zero for anything the role does not hunt — see the module note. This is deliberately a
 * multiplier on the class's own bounty rather than a table of its own: a dreadnought is worth
 * what a dreadnought is worth, and the role decides only whether you are the one being paid.
 */
export function bountyScale(faction: Faction, role: RoleId): number {
  return roleOf(role).hunts.includes(faction) ? 1 : 0
}

/**
 * Whether a faction's stations will serve this role.
 *
 * Keyed on `welcome` and **not** on `huntedBy`, which is the distinction the smuggler exists to
 * make: who shoots at you in open space and who takes your money at a dock are different
 * questions. Deriving one from the other collapsed the outlaw roles into unplayable ones.
 */
export function servedBy(faction: Faction, role: RoleId): boolean {
  return roleOf(role).welcome.includes(faction)
}
