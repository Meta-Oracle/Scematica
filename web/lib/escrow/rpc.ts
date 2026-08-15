// Escrow Market vault — chain reads. **Server-only.**
//
// Reads `ESCROW_RPC_ENDPOINT` then `RPC_ENDPOINT`, either of which carries a provider API
// key, so this must never be imported from a client component. Same guard and same
// reasoning as `lib/alchem/endpoint.ts`: Next only inlines `NEXT_PUBLIC_*` into client
// bundles, so a stray import would not leak the key — it would silently fall back to a
// public endpoint, which is the more confusing failure. The throw makes it loud.
//
// WHY THE ESCROW-SPECIFIC OVERRIDE EXISTS. `RPC_ENDPOINT` is shared by every server route
// on this site, so without a dedicated variable the escrow page cannot be pointed at a
// cluster where the vault program is deployed without dragging the market board, the
// sniper API and everything else onto that cluster too. The vault program is a distinct
// artifact with its own deploy lifecycle — it lands on devnet before mainnet, on purpose
// (see programs/scematica-vault/DEPLOY.md §3) — so it needs its own endpoint.
//
// THE HAZARD THIS INTRODUCES, and it is a real one: the page can now read a *different
// cluster* than the rest of the site, and a devnet vault holding test tokens minted out of
// nothing renders exactly like a mainnet vault holding real reserve. `host` is returned on
// every response for this reason and is the only thing distinguishing them today. Do not
// demo this publicly without the cluster on screen.
//
// **There is no simulation branch in this file, by design.** Every other data path on
// this site has one; this one must not. A fabricated reserve figure would defeat the
// entire purpose of a proof-of-reserve page — the page exists to answer "is the money
// actually there?", and an invented answer is worse than no page at all. Unreadable
// vaults surface as errors, never as zeroes and never as omissions.

import { Connection, PublicKey } from '@solana/web3.js'

import {
  decodeVault,
  deriveBackingVaultPda,
  deriveTokenVaultPda,
  deriveVaultPda,
  type VaultState,
} from './program'

if (typeof window !== 'undefined') {
  throw new Error(
    'lib/escrow/rpc.ts is server-only — import lib/escrow/program.ts from client components',
  )
}

/** Public mainnet fallback, used only when no keyed endpoint is configured. */
const PUBLIC_FALLBACK = 'https://solana-rpc.publicnode.com'

export interface ResolvedRpc {
  connection: Connection
  /** Host only — never the full URL, which carries the key. */
  host: string
  authenticated: boolean
}

export function resolveRpc(): ResolvedRpc {
  // Escrow-specific first, shared second. Precedence rather than replacement, so an
  // existing deployment that only sets `RPC_ENDPOINT` keeps working untouched.
  const raw = process.env.ESCROW_RPC_ENDPOINT?.trim() || process.env.RPC_ENDPOINT?.trim()
  const url = raw || PUBLIC_FALLBACK
  let host = 'unknown'
  try {
    host = new URL(url).host
  } catch {
    /* keep 'unknown' rather than echoing a malformed string that may contain a key */
  }
  return {
    connection: new Connection(url, 'confirmed'),
    host,
    authenticated: Boolean(raw),
  }
}

export interface VaultReading {
  vault: string
  state: VaultState
  /** Actual on-chain balance of the token vault, raw u64 as a decimal string. */
  tokenBalance: string
  backingBalance: string
  tokenDecimals: number
  backingDecimals: number
  /** Slot the read was answered at — this is what makes the figure checkable. */
  slot: number
  fetchedAt: string
}

/**
 * Read one vault and both of its token accounts.
 *
 * Throws rather than returning a partial reading. A vault whose balances could not be
 * fetched is not a vault with zero balances, and the distinction is the whole point of
 * the page.
 */
export async function readVault(
  connection: Connection,
  programId: PublicKey,
  tokenMint: PublicKey,
  backingMint: PublicKey,
): Promise<VaultReading> {
  const vault = deriveVaultPda(tokenMint, backingMint, programId)
  const tokenVault = deriveTokenVaultPda(vault, programId)
  const backingVault = deriveBackingVaultPda(vault, programId)

  const info = await connection.getAccountInfo(vault, 'confirmed')
  if (!info) throw new Error(`no vault at ${vault.toBase58()}`)
  if (!info.owner.equals(programId)) {
    // An account at the right address owned by someone else is not this program's
    // vault. Decoding it anyway would turn foreign bytes into a reserve figure.
    throw new Error(`account ${vault.toBase58()} is not owned by the escrow program`)
  }

  const state = decodeVault(info.data)
  if (!state) throw new Error(`account ${vault.toBase58()} is not a Vault (unexpected size)`)

  const [tokenBal, backingBal, slot] = await Promise.all([
    connection.getTokenAccountBalance(tokenVault, 'confirmed'),
    connection.getTokenAccountBalance(backingVault, 'confirmed'),
    connection.getSlot('confirmed'),
  ])

  return {
    vault: vault.toBase58(),
    state,
    tokenBalance: tokenBal.value.amount,
    backingBalance: backingBal.value.amount,
    tokenDecimals: tokenBal.value.decimals,
    backingDecimals: backingBal.value.decimals,
    slot,
    fetchedAt: new Date().toISOString(),
  }
}
