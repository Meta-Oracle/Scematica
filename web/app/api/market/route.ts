import { NextResponse } from 'next/server'

import { ESCROW_PROGRAM_ID } from '@/lib/escrow/program'
import { listVaults, type VaultIndex } from '@/lib/escrow/vaults'
import { resolveRpc } from '@/lib/escrow/rpc'
import { buildSnapshot, computeStats } from '@/lib/market/aggregate'
import { summariseCommitments } from '@/lib/market/commitment'
import { fetchLaunches, fetchRaydium, fetchTopTokens } from '@/lib/market/sources'
import type { BackingClaim, SourceStatus } from '@/lib/market/types'

// The Escrow Market board.
//
//   GET /api/market
//
// Joins live market data (Raydium, Jupiter, and pump.fun via Jupiter's launchpad tag)
// against every vault the escrow program owns, and returns one row per token.
//
// Three rules inherited from /api/escrow/vault, none of them relaxed because this route
// is bigger:
//
//  1. **No simulation branch.** Sources that fail are REPORTED as failures in `sources`,
//     never replaced with invented rows. A market board that quietly substitutes fake
//     liquidity next to a real reserve figure lends the fake number the real one's
//     credibility, which is the specific harm this whole product exists to prevent.
//  2. **Market data and custody claims never merge.** `row.token` is a third-party
//     observation; `row.backing` is read from the chain at a named slot. They are
//     separate objects in the payload so a client cannot accidentally render a price as
//     though the program vouched for it.
//  3. **`backing: null` is the normal case, not an error.** Most tokens are backed by
//     nothing. That is the market's actual condition and the reason to build this.
//
// Vault reads are all-or-nothing on purpose: if the chain cannot be read, every row
// reports `backingAvailable: false` rather than `backing: null`, because "nothing backs
// this" and "we could not check" are different claims and only one is an accusation.

export const dynamic = 'force-dynamic'
export const runtime = 'nodejs'

export async function GET() {
  const [raydium, launches, top] = await Promise.all([
    fetchRaydium(),
    fetchLaunches(),
    fetchTopTokens(),
  ])

  const sources: SourceStatus[] = [raydium.status, launches.status, top.status]
  const tokens = [...raydium.tokens, ...launches.tokens, ...top.tokens]

  // ── custody side ──────────────────────────────────────────────────────────
  let index: VaultIndex | null = null
  let backingAvailable = false
  let backingError: string | null = null
  let measuredAtSlot: number | null = null
  let rpcHost: string | null = null
  let rpcAuthenticated = false

  if (!ESCROW_PROGRAM_ID) {
    backingError =
      'NEXT_PUBLIC_ESCROW_PROGRAM_ID is unset — the escrow program has not been deployed, so no vault can exist yet.'
  } else {
    const { connection, host, authenticated } = resolveRpc()
    rpcHost = host
    rpcAuthenticated = authenticated
    try {
      index = await listVaults(connection, ESCROW_PROGRAM_ID)
      backingAvailable = true
      measuredAtSlot = index.slot
    } catch (error) {
      backingError = error instanceof Error ? error.message : String(error)
    }
  }

  const backingFor = (mint: string): BackingClaim | null => {
    if (!index) return null
    const listings = index.byTokenMint.get(mint)
    if (!listings || listings.length === 0) return null
    // A token may carry several reserves. Surface the alarming one if any exists —
    // showing the healthy vault while a sibling is short would be actively misleading.
    const v = listings.find(l => l.backingVerdict === 'SHORTFALL') ?? listings[0]
    return {
      backingMint: v.state.backingMint,
      backingSymbol: null,
      recordedBacking: v.state.totalBackingLocked,
      actualBacking: v.backingBalance,
      backingDecimals: v.backingDecimals,
      recordedToken: v.state.totalTokenLocked,
      actualToken: v.tokenBalance,
      tokenDecimals: v.tokenDecimals,
      verdict: v.backingVerdict,
      measuredAtSlot: index.slot,
      positionsOpen: Number(v.state.positionsOpen),
    }
  }

  const snapshot = buildSnapshot(tokens, sources, backingFor)
  const stats = computeStats(snapshot.rows)
  // Graded server-side so the ladder has exactly one definition. Recomputing it in the
  // client would be a second implementation to drift.
  const commitments = summariseCommitments(snapshot.rows)

  return NextResponse.json(
    {
      ok: true,
      rows: snapshot.rows,
      sources: snapshot.sources,
      stats,
      commitments,
      custody: {
        available: backingAvailable,
        error: backingError,
        programId: ESCROW_PROGRAM_ID?.toBase58() ?? null,
        vaultCount: index?.all.length ?? 0,
        measuredAtSlot,
        rpc: { host: rpcHost, authenticated: rpcAuthenticated },
      },
      fetchedAt: snapshot.fetchedAt,
    },
    { headers: { 'cache-control': 'no-store' } },
  )
}
