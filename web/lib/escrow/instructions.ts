// Instruction builders for the Escrow Market vault.
//
// Mirrors `programs/scematica-vault/src/lib.rs`. THE RUST IS AUTHORITATIVE — account
// order here must match the field order of each `#[derive(Accounts)]` struct exactly.
// Anchor matches accounts positionally, so a reordered field produces a transaction that
// either fails with a confusing constraint error or, far worse, passes the wrong account
// into the right slot.
//
// Discriminators are computed (`sha256("global:<snake_name>")[..8]`) rather than stored,
// so renaming an instruction in Rust cannot leave a stale constant here silently calling
// the wrong handler.
//
// Server-side: uses node crypto. Not imported by client components.

import { createHash } from 'node:crypto'
import {
  PublicKey,
  SystemProgram,
  SYSVAR_RENT_PUBKEY,
  TransactionInstruction,
} from '@solana/web3.js'

/** Vault::LEN — 8 discriminator + 4 pubkeys + 4 u64 + bump. */
export const VAULT_ACCOUNT_LEN = 8 + 32 * 4 + 8 * 4 + 1
/** Position::LEN — 8 discriminator + 2 pubkeys + 3 u64 + 2 i64 + bump. */
export const POSITION_ACCOUNT_LEN = 8 + 32 * 2 + 8 * 3 + 8 * 2 + 1
/** Base SPL / Token-2022 token account. Token-2022 mints with extensions are larger. */
export const TOKEN_ACCOUNT_LEN = 165

/** Enforced by the program: `require!((MIN..=MAX).contains(&lock_secs))`. */
export const MIN_LOCK_SECS = 7 * 24 * 60 * 60
export const MAX_LOCK_SECS = 10 * 365 * 24 * 60 * 60

export const TOKEN_PROGRAM_ID = new PublicKey('TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA')
export const TOKEN_2022_PROGRAM_ID = new PublicKey('TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb')
export const ASSOCIATED_TOKEN_PROGRAM_ID = new PublicKey(
  'ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL',
)

const disc = (name: string) =>
  createHash('sha256').update(`global:${name}`).digest().subarray(0, 8)

function u64(n: bigint): Buffer {
  const b = Buffer.alloc(8)
  b.writeBigUInt64LE(n)
  return b
}
function i64(n: bigint): Buffer {
  const b = Buffer.alloc(8)
  b.writeBigInt64LE(n)
  return b
}

const meta = (pubkey: PublicKey, isSigner: boolean, isWritable: boolean) => ({
  pubkey,
  isSigner,
  isWritable,
})

// ── PDAs — seeds must match lib.rs ───────────────────────────────────────────

export const vaultPda = (programId: PublicKey, tokenMint: PublicKey, backingMint: PublicKey) =>
  PublicKey.findProgramAddressSync(
    [Buffer.from('vault'), tokenMint.toBuffer(), backingMint.toBuffer()],
    programId,
  )[0]

export const tokenVaultPda = (programId: PublicKey, vault: PublicKey) =>
  PublicKey.findProgramAddressSync([Buffer.from('token_vault'), vault.toBuffer()], programId)[0]

export const backingVaultPda = (programId: PublicKey, vault: PublicKey) =>
  PublicKey.findProgramAddressSync([Buffer.from('backing_vault'), vault.toBuffer()], programId)[0]

export const positionPda = (
  programId: PublicKey,
  vault: PublicKey,
  depositor: PublicKey,
  nonce: bigint,
) =>
  PublicKey.findProgramAddressSync(
    [Buffer.from('position'), vault.toBuffer(), depositor.toBuffer(), u64(nonce)],
    programId,
  )[0]

/** ATA derivation, parameterised by token program so Token-2022 mints resolve correctly. */
export function associatedTokenAddress(
  mint: PublicKey,
  owner: PublicKey,
  tokenProgram: PublicKey,
): PublicKey {
  return PublicKey.findProgramAddressSync(
    [owner.toBuffer(), tokenProgram.toBuffer(), mint.toBuffer()],
    ASSOCIATED_TOKEN_PROGRAM_ID,
  )[0]
}

export function createAtaInstruction(
  payer: PublicKey,
  owner: PublicKey,
  mint: PublicKey,
  tokenProgram: PublicKey,
): TransactionInstruction {
  const ata = associatedTokenAddress(mint, owner, tokenProgram)
  return new TransactionInstruction({
    programId: ASSOCIATED_TOKEN_PROGRAM_ID,
    // Idempotent create (discriminator 1): a plain create fails if the account already
    // exists, which would abort the whole deposit for a user who simply already holds
    // the token.
    data: Buffer.from([1]),
    keys: [
      meta(payer, true, true),
      meta(ata, false, true),
      meta(owner, false, false),
      meta(mint, false, false),
      meta(SystemProgram.programId, false, false),
      meta(tokenProgram, false, false),
    ],
  })
}

// ── instructions ─────────────────────────────────────────────────────────────

export interface InitializeVaultArgs {
  programId: PublicKey
  payer: PublicKey
  tokenMint: PublicKey
  backingMint: PublicKey
  tokenProgram: PublicKey
}

/** Account order mirrors `InitializeVault`. Note: NO rent sysvar — it was removed from
 *  that struct to fit the 4096-byte SBF stack frame. Adding one back here would shift
 *  every subsequent account by one slot. */
export function initializeVaultInstruction(a: InitializeVaultArgs): TransactionInstruction {
  const vault = vaultPda(a.programId, a.tokenMint, a.backingMint)
  return new TransactionInstruction({
    programId: a.programId,
    data: Buffer.from(disc('initialize_vault')),
    keys: [
      meta(a.payer, true, true),
      meta(a.tokenMint, false, false),
      meta(a.backingMint, false, false),
      meta(vault, false, true),
      meta(tokenVaultPda(a.programId, vault), false, true),
      meta(backingVaultPda(a.programId, vault), false, true),
      meta(a.tokenProgram, false, false),
      meta(SystemProgram.programId, false, false),
    ],
  })
}

export interface DepositArgs {
  programId: PublicKey
  depositor: PublicKey
  tokenMint: PublicKey
  backingMint: PublicKey
  tokenProgram: PublicKey
  nonce: bigint
  tokenAmount: bigint
  backingAmount: bigint
  lockSecs: bigint
}

/** Account order mirrors `Deposit`, which DOES still carry the rent sysvar. */
export function depositInstruction(a: DepositArgs): TransactionInstruction {
  const vault = vaultPda(a.programId, a.tokenMint, a.backingMint)
  return new TransactionInstruction({
    programId: a.programId,
    data: Buffer.concat([
      disc('deposit'),
      u64(a.nonce),
      u64(a.tokenAmount),
      u64(a.backingAmount),
      i64(a.lockSecs),
    ]),
    keys: [
      meta(a.depositor, true, true),
      meta(vault, false, true),
      meta(positionPda(a.programId, vault, a.depositor, a.nonce), false, true),
      meta(tokenVaultPda(a.programId, vault), false, true),
      meta(backingVaultPda(a.programId, vault), false, true),
      meta(a.tokenMint, false, false),
      meta(a.backingMint, false, false),
      meta(associatedTokenAddress(a.tokenMint, a.depositor, a.tokenProgram), false, true),
      meta(associatedTokenAddress(a.backingMint, a.depositor, a.tokenProgram), false, true),
      meta(a.tokenProgram, false, false),
      meta(SystemProgram.programId, false, false),
      meta(SYSVAR_RENT_PUBKEY, false, false),
    ],
  })
}
