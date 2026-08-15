// Escrow Market — resolving an arbitrary mint address.
//
// Pure and client-safe: byte decoding and address shape only, no RPC and no secrets.
// The server half is `app/api/escrow/mint/route.ts`, split for the same reason
// `lib/escrow/rpc.ts` is split from `lib/escrow/program.ts`.
//
// WHY THIS EXISTS. The vault builder used to offer only tokens the market board had
// listed, plus a hardcoded reserve list. That made the product a menu — and the point of
// a non-custodial vault program with no privileged role is that it takes *any* pair of
// SPL mints. Anything you can paste, you can back.
//
// THE DECIMALS RULE, which is a money rule and not a display one. Human input is
// converted to base units by shifting the decimal point `decimals` places. Get `decimals`
// wrong by one and the deposit is off by 10x; wrong by two (a board's 6 against wBTC's
// real 8) and it is off by 100x, in the direction of locking a hundredth of the reserve
// the depositor believed they were locking. Third-party token lists are wrong about this
// often enough to matter. So the ONLY decimals allowed anywhere near an amount are the
// ones decoded here from the mint account itself, and the builder refuses to convert an
// amount until that read lands. A market row's `decimals` is a label, never an input.
//
// A symbol is the opposite kind of fact: cosmetic, unverifiable, and safe to be missing.
// It comes from a third party, is labelled with who said it, and renders as the truncated
// mint when nobody did. Never blocked on, never invented.

import { PublicKey } from '@solana/web3.js'

/** Base58 (no 0/O/I/l) at pubkey length. A cheap shape test, not a validity claim. */
export const ADDRESS_RE = /^[1-9A-HJ-NP-Za-km-z]{32,44}$/

export function looksLikeAddress(s: string): boolean {
  return ADDRESS_RE.test(s.trim())
}

export const TOKEN_PROGRAM = 'TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA'
export const TOKEN_2022_PROGRAM = 'TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb'

export type TokenProgramKind = 'spl-token' | 'token-2022'

export const PROGRAM_LABEL: Record<TokenProgramKind, string> = {
  'spl-token': 'SPL Token',
  'token-2022': 'Token-2022',
}

export function programKind(owner: string): TokenProgramKind | null {
  if (owner === TOKEN_PROGRAM) return 'spl-token'
  if (owner === TOKEN_2022_PROGRAM) return 'token-2022'
  return null
}

/** Base `Mint`: 82 bytes. Token-2022 mints with extensions are longer. */
export const MINT_LEN = 82
/** A token *account* is this long — and is owned by the same program as its mint. */
export const TOKEN_ACCOUNT_LEN = 165
/** Token-2022 tags extended accounts here: 1 = Mint, 2 = Account. */
const ACCOUNT_TYPE_OFFSET = 165
const ACCOUNT_TYPE_MINT = 1

/**
 * Is this account a mint?
 *
 * The check matters because `getAccountInfo` happily returns whatever lives at an
 * address, and a token account is owned by the very same program. Decoding one as a mint
 * yields a plausible-looking `decimals` read out of an amount field — a wrong number that
 * looks right, which is the failure mode this whole page is built to avoid.
 */
export function isMintAccount(data: Uint8Array, kind: TokenProgramKind): boolean {
  if (data.length < MINT_LEN) return false
  if (kind === 'spl-token') return data.length === MINT_LEN
  // Token-2022: either the bare base layout, or an extended account explicitly tagged.
  if (data.length === MINT_LEN) return true
  return data.length > ACCOUNT_TYPE_OFFSET && data[ACCOUNT_TYPE_OFFSET] === ACCOUNT_TYPE_MINT
}

/** A token account misread as a mint is the likeliest paste mistake — name it exactly. */
export function looksLikeTokenAccount(data: Uint8Array, kind: TokenProgramKind): boolean {
  if (data.length === TOKEN_ACCOUNT_LEN) return true
  return (
    kind === 'token-2022' &&
    data.length > ACCOUNT_TYPE_OFFSET &&
    data[ACCOUNT_TYPE_OFFSET] === 2
  )
}

export interface DecodedMint {
  decimals: number
  /** u64 as a decimal string — supply exceeds Number.MAX_SAFE_INTEGER routinely. */
  supply: string
  /** `null` means revoked: no new supply can ever be minted. */
  mintAuthority: string | null
  /** `null` means revoked: no holder's balance can be frozen. */
  freezeAuthority: string | null
  initialized: boolean
}

/**
 * Decode the 82-byte base `Mint` layout, shared by SPL Token and Token-2022.
 *
 *   0..4   mint authority COption tag (u32 LE, 1 = present)
 *   4..36  mint authority
 *   36..44 supply (u64 LE)
 *   44     decimals
 *   45     is_initialized
 *   46..50 freeze authority COption tag
 *   50..82 freeze authority
 */
export function decodeMint(data: Uint8Array): DecodedMint | null {
  if (data.length < MINT_LEN) return null
  const view = new DataView(data.buffer, data.byteOffset, data.byteLength)
  const pk = (o: number) => new PublicKey(data.slice(o, o + 32)).toBase58()

  const hasMintAuthority = view.getUint32(0, true) === 1
  const supply = view.getBigUint64(36, true).toString()
  const decimals = data[44]
  const initialized = data[45] === 1
  const hasFreezeAuthority = view.getUint32(46, true) === 1

  return {
    decimals,
    supply,
    mintAuthority: hasMintAuthority ? pk(4) : null,
    freezeAuthority: hasFreezeAuthority ? pk(50) : null,
    initialized,
  }
}

/** What the chain says. Authoritative for everything the builder computes with. */
export interface MintFacts extends DecodedMint {
  mint: string
  program: TokenProgramKind
  programId: string
  /** Token-2022 mint carrying extensions — its token accounts exceed the 165-byte base. */
  hasExtensions: boolean
  /** The slot the read was answered at; what makes the reading checkable. */
  slot: number
}

/** What a third party calls it. Cosmetic — `source: null` means nobody answered. */
export interface MintLabel {
  symbol: string | null
  name: string | null
  source: string | null
}

export type MintFailure = 'bad_address' | 'not_found' | 'not_a_mint' | 'rpc_failed'

export type MintResolution =
  | { ok: true; facts: MintFacts; label: MintLabel }
  | { ok: false; reason: MintFailure; detail: string }

/** Display name for a resolved mint — never invented, falls back to the address. */
export function displaySymbol(mint: string, label?: MintLabel | null): string {
  return label?.symbol?.trim() || `${mint.slice(0, 4)}…${mint.slice(-4)}`
}

/**
 * Can these two mints share one vault?
 *
 * Exactly one thing makes a pair illegal: a mint cannot back itself (`VaultError::
 * SameMint`), because the published ratio would be meaningless.
 *
 * MIXED TOKEN PROGRAMS ARE LEGAL, and this is worth stating because it used to be the
 * opposite. `InitializeVault` originally carried a single `token_program` account and
 * applied it to both legs, which made a Token-2022 token backed by a legacy-SPL reserve
 * unconstructible — i.e. it barred the product's central case, since new mints are
 * routinely Token-2022 (SCEMA is) while wBTC, wETH and wSOL are all legacy SPL. The
 * program now takes one token program *per leg*, so the pairing is decided by each mint's
 * own owner and never by the other side's.
 */
export function pairingProblem(token: MintFacts, backing: MintFacts): string | null {
  if (token.mint === backing.mint) {
    return 'A token cannot be backed by itself — the program rejects this as SameMint.'
  }
  return null
}

/**
 * Human decimal string → base units, without a float anywhere in the path.
 *
 * `parseFloat('0.1') * 1e9` is 100000000.00000001. A token amount wrong in the last place
 * is a transfer of the wrong quantity, so the shift happens on the digit string.
 *
 * Returns `null` for anything unparseable, including more fractional digits than the mint
 * has — silently truncating those would move money the user did not intend to move.
 */
export function toBaseUnits(input: string, decimals: number): bigint | null {
  const s = input.trim()
  if (!s) return 0n
  if (!/^\d*\.?\d*$/.test(s)) return null
  const [whole = '', frac = ''] = s.split('.')
  if (whole === '' && frac === '') return null
  if (frac.length > decimals) return null
  const padded = frac.padEnd(decimals, '0')
  try {
    return BigInt((whole || '0') + (decimals > 0 ? padded : ''))
  } catch {
    return null
  }
}
