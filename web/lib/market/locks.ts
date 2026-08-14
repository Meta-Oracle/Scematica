// Tier 3 of the commitment ladder: tokens time-locked in a third-party locker.
//
// SERVER ONLY — takes a Connection from lib/escrow/rpc.ts.
//
// WHY THIS IS A PER-MINT LOOKUP AND NOT A SWEEP. The Scema vault is enumerated whole
// (lib/escrow/vaults.ts) because it will hold a few hundred accounts at most. These
// programs are a different scale entirely — measured live:
//
//     Jupiter Lock   812,786 accounts
//     Streamflow      65,646 accounts
//
// Fetching either is not an option, so the question is inverted: instead of "list every
// lock", ask "does a lock reference THIS mint", which an indexed `memcmp` answers in
// 65-250ms. That is fast enough on demand and far too slow for 200 rows on a 30s poll,
// which is why this is a separate endpoint the client calls for the rows it is showing
// rather than part of the board sweep.
//
// MINT OFFSETS ARE MEASURED, NOT ASSUMED. Both were found by probing candidate offsets
// against known mints and keeping the one that returned hits:
//
//     Jupiter Lock  offset  40   JUP=8946  PUMP=21  WIF=40
//     Streamflow    offset 177   JUP=12    USDC=135
//
// Jupiter Lock is Anchor (8-byte discriminator, then `recipient`, then `token_mint`).
// Streamflow is not Anchor — it has a magic/version header and a run of pubkeys, which
// is why its mint sits so much deeper. If a lookup ever starts returning zero for a
// mint you know is locked, re-probe the offset before trusting the result: a program
// upgrade that reorders fields would silently turn every answer into "no lock".

import { Connection, PublicKey } from '@solana/web3.js'

export interface LockProgram {
  key: string
  label: string
  programId: string
  /** Byte offset of the token mint within the program's account. Measured — see above. */
  mintOffset: number
  url: string
}

export const LOCK_PROGRAMS: LockProgram[] = [
  {
    key: 'jupiterLock',
    label: 'Jupiter Lock',
    programId: 'LocpQgucEQHbqNABEYvBvwoxCPsSbG91A1QaQhQQqjn',
    mintOffset: 40,
    url: 'https://lock.jup.ag',
  },
  {
    key: 'streamflow',
    label: 'Streamflow',
    programId: 'strmRqUCoQUgGUan5YhzUZa6KqdzwX5L6FpUxfmKg5m',
    mintOffset: 177,
    url: 'https://app.streamflow.finance',
  },
]

export interface LockInfo {
  /** Contracts per program, keyed by `LockProgram.key`. */
  byProgram: Record<string, number>
  total: number
}

/**
 * Count lock contracts referencing `mint`.
 *
 * WHAT THIS DOES AND DOES NOT CLAIM. It returns how many lock CONTRACTS reference the
 * mint. It does NOT return how many tokens are locked, and the UI must not imply that it
 * does. Reading amounts would mean fetching every matching contract's escrow token
 * account — for JUP that is 8,946 accounts on one query, which is exactly the sweep this
 * design exists to avoid. A contract count is a real, checkable signal; an invented
 * amount would be the kind of number this whole product refuses to print.
 *
 * A contract that has been fully claimed still exists on-chain, so the count is an upper
 * bound on live locks. That limitation is stated in the UI rather than hidden.
 */
export async function lookupLocks(connection: Connection, mint: string): Promise<LockInfo> {
  let key: PublicKey
  try {
    key = new PublicKey(mint)
  } catch {
    return { byProgram: {}, total: 0 }
  }

  const results = await Promise.all(
    LOCK_PROGRAMS.map(async p => {
      try {
        const accounts = await connection.getProgramAccounts(new PublicKey(p.programId), {
          commitment: 'confirmed',
          // Fetch no data — only the count matters, and pulling account bodies for a
          // mint with thousands of locks would be pointlessly expensive.
          dataSlice: { offset: 0, length: 0 },
          filters: [{ memcmp: { offset: p.mintOffset, bytes: key.toBase58() } }],
        })
        return [p.key, accounts.length] as const
      } catch {
        // -1 marks "this program could not be asked", distinct from 0 meaning "asked,
        // no locks". The caller collapses unknowns rather than reporting a false zero.
        return [p.key, -1] as const
      }
    }),
  )

  const byProgram: Record<string, number> = {}
  let total = 0
  let anyKnown = false
  for (const [k, n] of results) {
    byProgram[k] = n
    if (n >= 0) {
      anyKnown = true
      total += n
    }
  }
  return { byProgram, total: anyKnown ? total : -1 }
}

/** Look up several mints with bounded concurrency. */
export async function lookupLocksBatch(
  connection: Connection,
  mints: string[],
  concurrency = 4,
): Promise<Record<string, LockInfo>> {
  const out: Record<string, LockInfo> = {}
  const queue = [...mints]
  const workers = Array.from({ length: Math.min(concurrency, queue.length) }, async () => {
    for (;;) {
      const mint = queue.shift()
      if (!mint) return
      out[mint] = await lookupLocks(connection, mint)
    }
  })
  await Promise.all(workers)
  return out
}
