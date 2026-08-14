import { NextResponse, type NextRequest } from 'next/server'
import { ComputeBudgetProgram, PublicKey, Transaction } from '@solana/web3.js'

import { ESCROW_PROGRAM_ID } from '@/lib/escrow/program'
import { resolveRpc } from '@/lib/escrow/rpc'
import {
  MAX_LOCK_SECS,
  MIN_LOCK_SECS,
  POSITION_ACCOUNT_LEN,
  TOKEN_2022_PROGRAM_ID,
  TOKEN_ACCOUNT_LEN,
  TOKEN_PROGRAM_ID,
  VAULT_ACCOUNT_LEN,
  associatedTokenAddress,
  createAtaInstruction,
  depositInstruction,
  initializeVaultInstruction,
  vaultPda,
} from '@/lib/escrow/instructions'

// Build an unsigned vault deposit transaction, with a full cost breakdown.
//
//   POST /api/escrow/build
//   { owner, tokenMint, backingMint, tokenAmount, backingAmount, lockSecs, nonce }
//
// Built server-side rather than in the browser for one practical reason: the client's
// connection falls back to the public cluster endpoint (NEXT_PUBLIC_RPC_ENDPOINT is
// intentionally unset, since anything there is published to every visitor), which is
// rate-limited to the point of being unusable. The wallet only signs; /api/escrow/send
// submits through the keyed RPC.
//
// Every rejection below mirrors a `require!` or account constraint in
// programs/scematica-vault/src/lib.rs. Validating here is a courtesy — the program is
// still the authority — but a transaction that will certainly fail should not cost the
// user a signature prompt and a fee.

export const dynamic = 'force-dynamic'
export const runtime = 'nodejs'

interface BuildBody {
  owner?: string
  tokenMint?: string
  backingMint?: string
  /** Base units, as strings — a u64 does not survive JSON's number type. */
  tokenAmount?: string
  backingAmount?: string
  lockSecs?: number
  nonce?: string
}

const bad = (detail: string, reason = 'bad_request', status = 400) =>
  NextResponse.json({ ok: false, reason, detail }, { status })

function parseAmount(raw: string | undefined, label: string): bigint | { error: string } {
  if (raw === undefined || raw === '') return { error: `${label} is required` }
  try {
    const v = BigInt(raw)
    if (v < 0n) return { error: `${label} cannot be negative` }
    return v
  } catch {
    return { error: `${label} must be an integer in base units` }
  }
}

export async function POST(request: NextRequest) {
  if (!ESCROW_PROGRAM_ID) {
    // The honest blocker. Building a transaction against a placeholder would produce
    // something that cannot land, and charging a signature for it would be worse than
    // saying plainly that the program does not exist yet.
    return NextResponse.json(
      {
        ok: false,
        reason: 'not_configured',
        detail:
          'NEXT_PUBLIC_ESCROW_PROGRAM_ID is unset — the vault program is not deployed, so no vault can be created yet.',
      },
      { status: 503 },
    )
  }

  let body: BuildBody
  try {
    body = (await request.json()) as BuildBody
  } catch {
    return bad('body must be JSON')
  }

  let owner: PublicKey, tokenMint: PublicKey, backingMint: PublicKey
  try {
    owner = new PublicKey(body.owner ?? '')
    tokenMint = new PublicKey(body.tokenMint ?? '')
    backingMint = new PublicKey(body.backingMint ?? '')
  } catch {
    return bad('owner, tokenMint and backingMint must be base58 addresses')
  }

  // VaultError::SameMint — "backing a mint with itself is not a product".
  if (tokenMint.equals(backingMint)) {
    return bad('A token cannot be backed by itself (SameMint).')
  }

  const tokenAmount = parseAmount(body.tokenAmount, 'tokenAmount')
  if (typeof tokenAmount === 'object') return bad(tokenAmount.error)
  const backingAmount = parseAmount(body.backingAmount, 'backingAmount')
  if (typeof backingAmount === 'object') return bad(backingAmount.error)

  // VaultError::ZeroBacking — token_amount may be 0, backing_amount may not.
  if (backingAmount === 0n) {
    return bad('Backing amount must be greater than zero (ZeroBacking).')
  }

  const lockSecs = Number(body.lockSecs ?? 0)
  if (!Number.isFinite(lockSecs) || lockSecs < MIN_LOCK_SECS || lockSecs > MAX_LOCK_SECS) {
    return bad(
      `Lock must be between ${MIN_LOCK_SECS} and ${MAX_LOCK_SECS} seconds (7 days to 10 years) — LockOutOfRange.`,
    )
  }

  let nonce: bigint
  try {
    nonce = BigInt(body.nonce ?? '0')
  } catch {
    return bad('nonce must be an integer')
  }

  const { connection, host, authenticated } = resolveRpc()

  try {
    const vault = vaultPda(ESCROW_PROGRAM_ID, tokenMint, backingMint)

    const [tokenMintInfo, backingMintInfo, vaultInfo] = await connection.getMultipleAccountsInfo([
      tokenMint,
      backingMint,
      vault,
    ])
    if (!tokenMintInfo) return bad('Token mint does not exist on this cluster.')
    if (!backingMintInfo) return bad('Backing mint does not exist on this cluster.')

    const tokenProgram = tokenMintInfo.owner
    const backingProgram = backingMintInfo.owner
    const known = [TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID]
    if (!known.some(p => p.equals(tokenProgram))) {
      return bad('Token mint is not owned by a recognised SPL Token program.')
    }

    // `InitializeVault` carries ONE `token_program` account and applies it to BOTH token
    // vaults, so a Token-2022 token cannot be backed by a legacy SPL reserve. The program
    // would reject it; catching it here explains why instead of failing opaquely.
    if (!tokenProgram.equals(backingProgram)) {
      return bad(
        `Both mints must live on the same token program. Token is on ${tokenProgram.toBase58().slice(0, 8)}… and backing is on ${backingProgram.toBase58().slice(0, 8)}… — the vault has a single token_program account for both legs.`,
      )
    }

    const decimalsOf = (data: Uint8Array) => (data.length > 44 ? data[44] : 0)
    const tokenDecimals = decimalsOf(new Uint8Array(tokenMintInfo.data))
    const backingDecimals = decimalsOf(new Uint8Array(backingMintInfo.data))

    // ── what must be created, and what that costs ───────────────────────────
    const needsVault = vaultInfo === null
    const ownerToken = associatedTokenAddress(tokenMint, owner, tokenProgram)
    const ownerBacking = associatedTokenAddress(backingMint, owner, tokenProgram)

    const [ownerTokenInfo, ownerBackingInfo] = await connection.getMultipleAccountsInfo([
      ownerToken,
      ownerBacking,
    ])

    const [rentVault, rentTokenAcct, rentPosition, lamports] = await Promise.all([
      connection.getMinimumBalanceForRentExemption(VAULT_ACCOUNT_LEN),
      connection.getMinimumBalanceForRentExemption(TOKEN_ACCOUNT_LEN),
      connection.getMinimumBalanceForRentExemption(POSITION_ACCOUNT_LEN),
      connection.getBalance(owner),
    ])

    const lineItems: { label: string; lamports: number }[] = []
    if (needsVault) {
      lineItems.push({ label: 'Vault state account (one-time, per token pair)', lamports: rentVault })
      lineItems.push({ label: 'Token vault (PDA-owned)', lamports: rentTokenAcct })
      lineItems.push({ label: 'Backing vault (PDA-owned)', lamports: rentTokenAcct })
    }
    if (!ownerTokenInfo) lineItems.push({ label: 'Your token account', lamports: rentTokenAcct })
    if (!ownerBackingInfo) lineItems.push({ label: 'Your backing account', lamports: rentTokenAcct })
    lineItems.push({ label: 'Position account (refunded on withdraw)', lamports: rentPosition })

    const BASE_FEE = 5_000
    const PRIORITY_FEE = 10_000
    lineItems.push({ label: 'Network fee', lamports: BASE_FEE + PRIORITY_FEE })

    const totalLamports = lineItems.reduce((s, l) => s + l.lamports, 0)

    // ── balances the program will check ─────────────────────────────────────
    const readAmount = (info: { data: Buffer } | null) =>
      info && info.data.length >= 72
        ? new DataView(info.data.buffer, info.data.byteOffset, info.data.byteLength)
            .getBigUint64(64, true)
            .toString()
        : '0'
    const tokenBalance = readAmount(ownerTokenInfo)
    const backingBalance = readAmount(ownerBackingInfo)

    const shortfalls: string[] = []
    if (BigInt(backingBalance) < backingAmount) {
      shortfalls.push(`backing: you hold ${backingBalance} base units, need ${backingAmount}`)
    }
    if (BigInt(tokenBalance) < tokenAmount) {
      shortfalls.push(`token: you hold ${tokenBalance} base units, need ${tokenAmount}`)
    }
    if (lamports < totalLamports) {
      shortfalls.push(`SOL: you hold ${lamports} lamports, need ${totalLamports}`)
    }

    // ── assemble ────────────────────────────────────────────────────────────
    const tx = new Transaction()
    tx.add(ComputeBudgetProgram.setComputeUnitPrice({ microLamports: 20_000 }))
    if (needsVault) {
      tx.add(
        initializeVaultInstruction({
          programId: ESCROW_PROGRAM_ID,
          payer: owner,
          tokenMint,
          backingMint,
          tokenProgram,
        }),
      )
    }
    if (!ownerTokenInfo) tx.add(createAtaInstruction(owner, owner, tokenMint, tokenProgram))
    if (!ownerBackingInfo) tx.add(createAtaInstruction(owner, owner, backingMint, tokenProgram))
    tx.add(
      depositInstruction({
        programId: ESCROW_PROGRAM_ID,
        depositor: owner,
        tokenMint,
        backingMint,
        tokenProgram,
        nonce,
        tokenAmount,
        backingAmount,
        lockSecs: BigInt(Math.floor(lockSecs)),
      }),
    )

    const { blockhash, lastValidBlockHeight } = await connection.getLatestBlockhash('confirmed')
    tx.recentBlockhash = blockhash
    tx.feePayer = owner

    return NextResponse.json(
      {
        ok: true,
        transaction: tx.serialize({ requireAllSignatures: false, verifySignatures: false }).toString('base64'),
        blockhash,
        lastValidBlockHeight,
        vault: vault.toBase58(),
        needsVault,
        tokenProgram: tokenProgram.toBase58(),
        decimals: { token: tokenDecimals, backing: backingDecimals },
        balances: { token: tokenBalance, backing: backingBalance, lamports },
        costs: {
          lineItems,
          totalLamports,
          // Token-2022 mints carrying extensions need a larger token account than the
          // 165-byte base, so the figure can under-read for those. Flagged rather than
          // quietly wrong — the wallet's own preview is the authority before signing.
          estimated: tokenProgram.equals(TOKEN_2022_PROGRAM_ID),
        },
        shortfalls,
        rpc: { host, authenticated },
      },
      { headers: { 'cache-control': 'no-store' } },
    )
  } catch (error) {
    return NextResponse.json(
      {
        ok: false,
        reason: 'build_failed',
        detail: error instanceof Error ? error.message : String(error),
        rpc: { host, authenticated },
      },
      { status: 502 },
    )
  }
}
