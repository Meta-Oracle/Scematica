// Escrow Market vault — addresses and account decoding.
//
// Pure and client-safe: PDA derivation and byte-layout decoding only, no RPC and no
// secrets. The server-only counterpart is `lib/escrow/rpc.ts`, split for the same
// reason `lib/alchem/endpoint.ts` is split from `networks.ts` — this module gets
// imported by client components, so nothing that reads an env secret may live here.
//
// The layout below mirrors `programs/scemadex-vault/src/lib.rs` exactly. The Rust
// program is authoritative; if `Vault` gains a field there, it must be added here in
// the same order or every number this file produces is silently wrong. `VAULT_LEN` is
// the tripwire — a decode against an account of unexpected size is rejected rather
// than guessed at.

import { PublicKey } from '@solana/web3.js'

/**
 * The deployed program. `null` until one exists.
 *
 * Deliberately not defaulted to the `declare_id!` placeholder in the Rust source: a
 * placeholder that derives real-looking PDAs would produce a page full of confident
 * "vault not found" rows for addresses that could never hold anything. Absent
 * configuration must read as absent, not as empty.
 */
export const ESCROW_PROGRAM_ID: PublicKey | null = (() => {
  const raw = process.env.NEXT_PUBLIC_ESCROW_PROGRAM_ID?.trim()
  if (!raw) return null
  try {
    return new PublicKey(raw)
  } catch {
    return null
  }
})()

/** `Vault` account size: 8 discriminator + 4 pubkeys + 4 u64 + bump. */
export const VAULT_LEN = 8 + 32 * 4 + 8 * 4 + 1

export interface VaultState {
  tokenMint: string
  backingMint: string
  tokenVault: string
  backingVault: string
  /** u64 — carried as a decimal string. See the note on `readU64`. */
  totalTokenLocked: string
  totalBackingLocked: string
  positionsOpen: string
  positionsLifetime: string
  bump: number
}

export function deriveVaultPda(
  tokenMint: PublicKey,
  backingMint: PublicKey,
  programId: PublicKey,
): PublicKey {
  return PublicKey.findProgramAddressSync(
    [Buffer.from('vault'), tokenMint.toBuffer(), backingMint.toBuffer()],
    programId,
  )[0]
}

export function deriveTokenVaultPda(vault: PublicKey, programId: PublicKey): PublicKey {
  return PublicKey.findProgramAddressSync(
    [Buffer.from('token_vault'), vault.toBuffer()],
    programId,
  )[0]
}

export function deriveBackingVaultPda(vault: PublicKey, programId: PublicKey): PublicKey {
  return PublicKey.findProgramAddressSync(
    [Buffer.from('backing_vault'), vault.toBuffer()],
    programId,
  )[0]
}

/**
 * Read a little-endian u64 as a decimal string.
 *
 * Not a `number`. A u64 runs to ~1.8e19 and `Number.MAX_SAFE_INTEGER` is ~9e15, so a
 * balance of wBTC in satoshis or a token with 9 decimals silently loses precision the
 * moment it is coerced. Every amount on this page is a claim about how much money is
 * locked somewhere; rounding one is not an option.
 */
function readU64(view: DataView, offset: number): string {
  return view.getBigUint64(offset, true).toString()
}

/**
 * Decode a `Vault` account. Returns `null` for anything that is not one.
 *
 * The length check is not paranoia: `getAccountInfo` on a derived address returns
 * whatever happens to live there, and decoding a differently-shaped account would
 * produce plausible-looking totals out of unrelated bytes.
 */
export function decodeVault(data: Uint8Array): VaultState | null {
  if (data.length !== VAULT_LEN) return null

  const view = new DataView(data.buffer, data.byteOffset, data.byteLength)
  const pk = (offset: number) => new PublicKey(data.slice(offset, offset + 32)).toBase58()

  let o = 8 // skip the Anchor discriminator
  const tokenMint = pk(o); o += 32
  const backingMint = pk(o); o += 32
  const tokenVault = pk(o); o += 32
  const backingVault = pk(o); o += 32
  const totalTokenLocked = readU64(view, o); o += 8
  const totalBackingLocked = readU64(view, o); o += 8
  const positionsOpen = readU64(view, o); o += 8
  const positionsLifetime = readU64(view, o); o += 8
  const bump = data[o]

  return {
    tokenMint,
    backingMint,
    tokenVault,
    backingVault,
    totalTokenLocked,
    totalBackingLocked,
    positionsOpen,
    positionsLifetime,
    bump,
  }
}

/** Render a raw u64 amount at `decimals`, without going through a float. */
export function formatAmount(raw: string, decimals: number): string {
  if (decimals === 0) return raw
  const padded = raw.padStart(decimals + 1, '0')
  const whole = padded.slice(0, padded.length - decimals)
  const frac = padded.slice(padded.length - decimals).replace(/0+$/, '')
  return frac ? `${whole}.${frac}` : whole
}

/**
 * Does the on-chain balance back the recorded total?
 *
 * `balance >= recorded` is the healthy relation, not `==`. Anyone may transfer SPL
 * tokens into any account, so a stranger's donation pushes the balance above the sum of
 * live positions — those funds are permanently stuck, because the program can only move
 * amounts recorded on a position, and a sweeper would mean a privileged role.
 *
 * `balance < recorded` is the alarming direction: the accounting and the tokens
 * disagree, and some depositor's withdraw will fail.
 */
export type SolvencyVerdict = 'backed' | 'donated' | 'SHORTFALL'

export function solvency(recorded: string, balance: string): SolvencyVerdict {
  const r = BigInt(recorded)
  const b = BigInt(balance)
  if (b < r) return 'SHORTFALL'
  if (b > r) return 'donated'
  return 'backed'
}
