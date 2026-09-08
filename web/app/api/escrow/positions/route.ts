import { NextResponse, type NextRequest } from 'next/server'
import { PublicKey } from '@solana/web3.js'

import {
  ESCROW_PROGRAM_ID,
  POSITION_DEPOSITOR_OFFSET,
  POSITION_LEN,
  VAULT_LEN,
  decodePosition,
  decodeVault,
} from '@/lib/escrow/program'
import { resolveRpc } from '@/lib/escrow/rpc'

// Every position a wallet holds, across every vault.
//
//   GET /api/escrow/positions?owner=<base58>
//
// This route exists because a lock has no exit without it. `withdraw` is keyed by
// `(token_mint, backing_mint, nonce)` and none of those three are recoverable from a
// wallet — the nonce in particular is chosen at deposit time and stored nowhere the
// depositor can see. Without this listing, getting funds back out means remembering three
// values from a transaction that may be months old.
//
// Server-side for the same reason /api/escrow/build is: `getProgramAccounts` is a heavy
// scan that public endpoints rate-limit or refuse outright.
//
// NO SIMULATION BRANCH, same rule as the rest of /escrow and for the same reason. This
// answers "what is mine and when can I take it back". A fabricated position is an
// invitation to sign a transaction that cannot land; a fabricated absence tells somebody
// their money is gone. Failures render as failures.

export const dynamic = 'force-dynamic'
export const runtime = 'nodejs'

export interface PositionRow {
  address: string
  vault: string
  tokenMint: string
  backingMint: string
  tokenAmount: string
  backingAmount: string
  createdUnix: string
  unlockUnix: string
  nonce: string
  /** `null` when the vault account could not be read — NOT a guess at its mints.
   *  A position whose vault is unreadable is still the depositor's, and dropping it
   *  from the list would report their money as absent. It is listed, and it says so. */
  vaultReadable: boolean
}

export async function GET(request: NextRequest) {
  if (!ESCROW_PROGRAM_ID) {
    return NextResponse.json(
      {
        ok: false,
        reason: 'not_configured',
        detail:
          'NEXT_PUBLIC_ESCROW_PROGRAM_ID is unset — the vault program is not deployed, so no position can exist yet.',
      },
      { status: 503 },
    )
  }

  const raw = request.nextUrl.searchParams.get('owner')?.trim()
  if (!raw) {
    return NextResponse.json(
      { ok: false, reason: 'bad_request', detail: 'owner is required' },
      { status: 400 },
    )
  }

  let owner: PublicKey
  try {
    owner = new PublicKey(raw)
  } catch {
    return NextResponse.json(
      { ok: false, reason: 'bad_address', detail: 'owner is not a base58 address' },
      { status: 400 },
    )
  }

  const { connection, host, authenticated } = resolveRpc()

  try {
    // Filtered by SIZE as well as by depositor. `Position` and `Vault` live under the same
    // program, and a 32-byte window at the same offset matches bytes in a `Vault` too —
    // the size filter is what keeps a vault from decoding as somebody's position.
    const accounts = await connection.getProgramAccounts(ESCROW_PROGRAM_ID, {
      commitment: 'confirmed',
      filters: [
        { dataSize: POSITION_LEN },
        { memcmp: { offset: POSITION_DEPOSITOR_OFFSET, bytes: owner.toBase58() } },
      ],
    })

    const decoded = accounts
      .map((a) => ({ address: a.pubkey.toBase58(), state: decodePosition(new Uint8Array(a.account.data)) }))
      .filter((x): x is { address: string; state: NonNullable<ReturnType<typeof decodePosition>> } => x.state !== null)

    // The position records its vault but not the mints, and `withdraw` needs both. One
    // batched read rather than one per position: a wallet with a dozen positions across
    // three vaults should not cost a dozen round trips.
    const vaultKeys = [...new Set(decoded.map((d) => d.state.vault))]
    const vaultInfos = vaultKeys.length
      ? await connection.getMultipleAccountsInfo(vaultKeys.map((k) => new PublicKey(k)))
      : []
    const vaults = new Map(
      vaultKeys.map((k, i) => {
        const info = vaultInfos[i]
        return [k, info && info.data.length === VAULT_LEN ? decodeVault(new Uint8Array(info.data)) : null]
      }),
    )

    const positions: PositionRow[] = decoded.map((d) => {
      const v = vaults.get(d.state.vault) ?? null
      return {
        address: d.address,
        vault: d.state.vault,
        tokenMint: v?.tokenMint ?? '',
        backingMint: v?.backingMint ?? '',
        tokenAmount: d.state.tokenAmount,
        backingAmount: d.state.backingAmount,
        createdUnix: d.state.createdUnix,
        unlockUnix: d.state.unlockUnix,
        nonce: d.state.nonce,
        vaultReadable: v !== null,
      }
    })

    // Soonest unlock first — the actionable end of the list. Sorted as BigInt: an i64
    // through `Number` loses ordering past 2^53, and these are timestamps the page uses
    // to decide which button is live.
    positions.sort((a, b) => (BigInt(a.unlockUnix) < BigInt(b.unlockUnix) ? -1 : 1))

    return NextResponse.json(
      {
        ok: true,
        owner: owner.toBase58(),
        programId: ESCROW_PROGRAM_ID.toBase58(),
        positions,
        // The reader compares against their own clock otherwise, which drifts from the
        // chain's and would show a matured position as locked (or the reverse).
        measuredAt: { unix: Math.floor(Date.now() / 1000) },
        rpc: { host, authenticated },
      },
      { headers: { 'cache-control': 'no-store' } },
    )
  } catch (error) {
    return NextResponse.json(
      {
        ok: false,
        reason: 'rpc_failed',
        detail: error instanceof Error ? error.message : String(error),
        rpc: { host, authenticated },
      },
      { status: 502 },
    )
  }
}
