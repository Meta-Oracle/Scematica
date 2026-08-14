// The commitment ladder.
//
// The Escrow Market's thesis is that a token's trustworthiness should be measured by
// what its issuer has irreversibly given up, not by its volume. This file grades that.
//
// The ladder is ordered by COST TO FAKE, which is the only ordering that resists
// gaming. Each rung is something the issuer cannot undo and a stranger can verify
// without trusting us:
//
//   0 NONE      nothing given up
//   1 RENOUNCED mint + freeze authority revoked — supply is fixed, balances unfreezable
//   2 LP_BURNED liquidity provider tokens burned — the pool cannot be pulled
//   3 LOCKED    tokens time-locked in a third-party locker (Jupiter Lock, Streamflow)
//   4 RESERVED  a reserve asset locked in a Scema vault behind the token
//
// Deliberately NOT a 0-100 score. A score invites tuning until the number flatters
// whoever is holding the bag, and it hides which specific thing was verified. A rung is
// a claim you can name, check, and disprove. `pool_scorer.rs` earns its numeric score by
// being a trading heuristic; this is an evidence ladder, and they are different jobs.
//
// Tiers 1 and 2 need no program of ours and cost nothing to verify — they come from data
// already on the board. Tier 3 needs one RPC call against programs that are already
// deployed by other people. Only tier 4 requires the Scema vault to exist.

import type { MarketRow, MarketToken } from './types'

/**
 * What the tier-3 lookup returned for a mint.
 *
 * `undefined`  — not checked (the board did not ask)
 * `total: -1`  — asked, but the RPC failed
 * `total: 0`   — asked, no lock contracts reference this mint
 *
 * All three are distinct and none may be rendered as another. Collapsing "not checked"
 * into "no locks" would print an unverified claim; collapsing "RPC failed" into "no
 * locks" would demote a genuinely locked token.
 */
export interface LockLookup {
  byProgram: Record<string, number>
  total: number
}

export type CommitmentTier = 0 | 1 | 2 | 3 | 4

export const TIER_NAME: Record<CommitmentTier, string> = {
  0: 'NONE',
  1: 'RENOUNCED',
  2: 'LP BURNED',
  3: 'LOCKED',
  4: 'RESERVED',
}

export const TIER_MEANING: Record<CommitmentTier, string> = {
  0: 'nothing verifiable has been given up',
  1: 'mint and freeze authority revoked — supply fixed, balances cannot be frozen',
  2: 'liquidity tokens burned — the pool cannot be withdrawn by the issuer',
  3: 'tokens time-locked in a third-party locker',
  4: 'a reserve asset is locked in a Scema vault behind this token',
}

/** LP burn below this is not a commitment — partial burns are common and mean little. */
export const LP_BURN_THRESHOLD_PCT = 90

/** Dev holdings above this void the RENOUNCED rung: fixed supply in one wallet is not
 *  a commitment, it is a loaded gun. Renouncing mint while holding most of the float is
 *  the single most common way the "renounced" badge is used to mislead. */
export const DEV_BALANCE_VOID_PCT = 50

export interface Commitment {
  tier: CommitmentTier
  /** Every rung actually satisfied, so the UI can show the evidence, not just a badge. */
  evidence: string[]
  /** Rungs that could not be evaluated because a source stayed silent. */
  unknown: string[]
}

/**
 * Grade one row.
 *
 * `null` inputs produce an `unknown` entry rather than a failed check. A token whose
 * audit data is missing must not be graded as if it failed the audit — that would
 * punish tokens for their data source's silence.
 */
export function gradeCommitment(row: MarketRow, locks?: LockLookup): Commitment {
  const t: MarketToken = row.token
  const evidence: string[] = []
  const unknown: string[] = []
  let tier: CommitmentTier = 0

  // Tier 4 — a real reserve. Outranks everything below it.
  if (row.backing) {
    if (row.backing.verdict === 'SHORTFALL') {
      // A vault that is short is not a commitment, it is an alarm. It must never
      // outrank an honest lower rung.
      return {
        tier: 0,
        evidence: [],
        unknown: [],
      }
    }
    evidence.push(`reserve locked in vault, verdict ${row.backing.verdict}`)
    tier = 4
  }

  // Tier 3 — time-locked in a third-party locker (Jupiter Lock / Streamflow).
  if (locks === undefined) {
    unknown.push('lock status not checked')
  } else if (locks.total < 0) {
    unknown.push('lock lookup failed — not the same as no locks')
  } else if (locks.total > 0) {
    // "Contracts referencing this mint", never "tokens locked". The lookup counts
    // accounts; it does not read escrow balances. See lib/market/locks.ts.
    evidence.push(
      `${locks.total} lock contract${locks.total === 1 ? '' : 's'} reference this mint`,
    )
    if (tier < 3) tier = 3
  }

  // Tier 2 — LP burned.
  if (t.lpBurnedPct === null) {
    unknown.push('LP burn not reported by this venue')
  } else if (t.lpBurnedPct >= LP_BURN_THRESHOLD_PCT) {
    evidence.push(`${t.lpBurnedPct.toFixed(1)}% of LP burned`)
    if (tier < 2) tier = 2
  }

  // Tier 1 — authorities revoked, unless the deployer holds most of the supply.
  if (t.mintRenounced === null || t.freezeRevoked === null) {
    unknown.push('mint/freeze authority not reported')
  } else if (t.mintRenounced && t.freezeRevoked) {
    if (t.devBalancePct !== null && t.devBalancePct >= DEV_BALANCE_VOID_PCT) {
      // Named explicitly so the UI can explain the omission rather than just not
      // showing a badge the user expected.
      unknown.push(
        `authorities revoked but deployer holds ${t.devBalancePct.toFixed(1)}% of supply`,
      )
    } else {
      evidence.push('mint and freeze authority revoked')
      if (tier < 1) tier = 1
    }
  }

  return { tier, evidence, unknown }
}

export interface CommitmentBreakdown {
  counts: Record<CommitmentTier, number>
  /** Share of listed tokens at tier 1 or above. */
  anyCommitmentShare: number
  total: number
}

export function summariseCommitments(
  rows: MarketRow[],
  locks?: Record<string, LockLookup>,
): CommitmentBreakdown {
  const counts: Record<CommitmentTier, number> = { 0: 0, 1: 0, 2: 0, 3: 0, 4: 0 }
  for (const r of rows) counts[gradeCommitment(r, locks?.[r.token.mint]).tier] += 1
  const committed = counts[1] + counts[2] + counts[3] + counts[4]
  return {
    counts,
    anyCommitmentShare: rows.length === 0 ? 0 : committed / rows.length,
    total: rows.length,
  }
}
