import { NextResponse, type NextRequest } from 'next/server'
import { ComputeBudgetProgram, PublicKey, Transaction } from '@solana/web3.js'

import { ESCROW_PROGRAM_ID, decodePosition, decodeVault, POSITION_LEN, VAULT_LEN } from '@/lib/escrow/program'
import { resolveRpc } from '@/lib/escrow/rpc'
import {
  MAX_LOCK_SECS,
  TOKEN_2022_PROGRAM_ID,
  TOKEN_PROGRAM_ID,
  associatedTokenAddress,
  createAtaInstruction,
  extendLockInstruction,
  positionPda,
  vaultPda,
  withdrawInstruction,
} from '@/lib/escrow/instructions'

// Build an unsigned withdraw — or an unsigned lock extension — for a matured position.
//
//   POST /api/escrow/withdraw
//   { owner, tokenMint, backingMint, nonce, action?: 'withdraw' | 'extend', newUnlockUnix? }
//
// Separate from /api/escrow/build rather than a mode on it. That route creates things and
// its whole body is about what must be created and what the rent costs; this one destroys
// a position and creates nothing but a possibly-missing ATA. Folding them together would
// mean one handler where the difference between paying money in and taking it out is a
// string field — which is exactly the distinction that should be hardest to get wrong.
//
// Every rejection below mirrors a `require!` or account constraint in
// programs/scematica-vault/src/lib.rs. The program remains the authority; refusing here
// only saves the user a signature prompt and a fee for a transaction that cannot succeed.

export const dynamic = 'force-dynamic'
export const runtime = 'nodejs'

interface Body {
  owner?: string
  tokenMint?: string
  backingMint?: string
  nonce?: string
  action?: string
  /** Absolute unix seconds, for `extend`. */
  newUnlockUnix?: string
}

const bad = (detail: string, reason = 'bad_request', status = 400) =>
  NextResponse.json({ ok: false, reason, detail }, { status })

export async function POST(request: NextRequest) {
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

  let body: Body
  try {
    body = (await request.json()) as Body
  } catch {
    return bad('body must be JSON')
  }

  const action = body.action ?? 'withdraw'
  if (action !== 'withdraw' && action !== 'extend') {
    return bad("action must be 'withdraw' or 'extend'")
  }

  let owner: PublicKey, tokenMint: PublicKey, backingMint: PublicKey
  try {
    owner = new PublicKey(body.owner ?? '')
    tokenMint = new PublicKey(body.tokenMint ?? '')
    backingMint = new PublicKey(body.backingMint ?? '')
  } catch {
    return bad('owner, tokenMint and backingMint must be base58 addresses')
  }

  let nonce: bigint
  try {
    nonce = BigInt(body.nonce ?? '')
  } catch {
    return bad('nonce must be an integer')
  }

  const { connection, host, authenticated } = resolveRpc()

  try {
    const vault = vaultPda(ESCROW_PROGRAM_ID, tokenMint, backingMint)
    const position = positionPda(ESCROW_PROGRAM_ID, vault, owner, nonce)

    const [vaultInfo, positionInfo, tokenMintInfo, backingMintInfo] =
      await connection.getMultipleAccountsInfo([vault, position, tokenMint, backingMint])

    // Three distinct absences, three distinct claims. "No vault here", "no position of
    // yours in it" and "the RPC would not answer" send a reader to different places, and
    // collapsing them into one message is how somebody concludes their money is gone.
    if (!vaultInfo) return bad(`No vault at ${vault.toBase58()} for that pair.`, 'no_vault', 404)
    if (!positionInfo) {
      return bad(
        `No position at ${position.toBase58()} — nonce ${nonce} is not open for ${owner.toBase58()}.`,
        'no_position',
        404,
      )
    }
    if (!tokenMintInfo || !backingMintInfo) return bad('A mint in this pair does not exist on this cluster.')

    if (vaultInfo.data.length !== VAULT_LEN) {
      return bad(`Account at ${vault.toBase58()} is ${vaultInfo.data.length} bytes, not a Vault.`, 'not_a_vault', 502)
    }
    if (positionInfo.data.length !== POSITION_LEN) {
      return bad(
        `Account at ${position.toBase58()} is ${positionInfo.data.length} bytes, not a Position.`,
        'not_a_position',
        502,
      )
    }

    const v = decodeVault(new Uint8Array(vaultInfo.data))
    const p = decodePosition(new Uint8Array(positionInfo.data))
    if (!v || !p) return bad('Vault or position did not decode.', 'decode_failed', 502)

    // `has_one = depositor` would reject this on chain; saying so here costs no signature.
    if (p.depositor !== owner.toBase58()) {
      return bad('That position belongs to a different wallet (NotDepositor).', 'not_depositor', 403)
    }

    const tokenProgram = tokenMintInfo.owner
    const backingProgram = backingMintInfo.owner
    const known = [TOKEN_PROGRAM_ID, TOKEN_2022_PROGRAM_ID]
    if (!known.some((k) => k.equals(tokenProgram)) || !known.some((k) => k.equals(backingProgram))) {
      return bad('A mint in this pair is not owned by a recognised SPL Token program.')
    }

    const tx = new Transaction()
    tx.add(ComputeBudgetProgram.setComputeUnitPrice({ microLamports: 20_000 }))

    if (action === 'extend') {
      let newUnlockUnix: bigint
      try {
        newUnlockUnix = BigInt(body.newUnlockUnix ?? '')
      } catch {
        return bad('newUnlockUnix must be an integer (absolute unix seconds)')
      }
      // VaultError::LockNotExtended — strictly increasing. A lock may be strengthened and
      // never weakened, so an equal time is refused as firmly as an earlier one.
      if (newUnlockUnix <= BigInt(p.unlockUnix)) {
        return bad(
          `New unlock must be later than the current one (${p.unlockUnix}) — LockNotExtended.`,
          'lock_not_extended',
        )
      }
      const now = BigInt(Math.floor(Date.now() / 1000))
      if (newUnlockUnix > now + BigInt(MAX_LOCK_SECS)) {
        return bad(`New unlock is more than ${MAX_LOCK_SECS} seconds out — LockOutOfRange.`, 'lock_out_of_range')
      }
      tx.add(
        extendLockInstruction({
          programId: ESCROW_PROGRAM_ID,
          depositor: owner,
          tokenMint,
          backingMint,
          nonce,
          newUnlockUnix,
        }),
      )
    } else {
      // VaultError::StillLocked. Compared against the CHAIN's clock, not the browser's:
      // the program reads `Clock::get()`, and a client whose clock runs a minute fast
      // would be offered a button that costs a signature and then fails.
      const slot = await connection.getSlot('confirmed')
      const chainNow = (await connection.getBlockTime(slot)) ?? Math.floor(Date.now() / 1000)
      if (BigInt(chainNow) < BigInt(p.unlockUnix)) {
        return bad(
          `Still locked until ${p.unlockUnix}; the chain clock reads ${chainNow} — StillLocked.`,
          'still_locked',
        )
      }

      // The depositor's receiving accounts must exist before the program transfers into
      // them. Normally they do — they were used to fund the deposit — but an ATA can be
      // closed at zero balance, and a withdraw into a closed account fails on chain with
      // an error naming neither the account nor the fix.
      const ownerToken = associatedTokenAddress(tokenMint, owner, tokenProgram)
      const ownerBacking = associatedTokenAddress(backingMint, owner, backingProgram)
      const [tInfo, bInfo] = await connection.getMultipleAccountsInfo([ownerToken, ownerBacking])
      if (!tInfo) tx.add(createAtaInstruction(owner, owner, tokenMint, tokenProgram))
      if (!bInfo) tx.add(createAtaInstruction(owner, owner, backingMint, backingProgram))

      tx.add(
        withdrawInstruction({
          programId: ESCROW_PROGRAM_ID,
          depositor: owner,
          tokenMint,
          backingMint,
          tokenProgram,
          backingProgram,
          nonce,
        }),
      )
    }

    const { blockhash, lastValidBlockHeight } = await connection.getLatestBlockhash('confirmed')
    tx.recentBlockhash = blockhash
    tx.feePayer = owner

    const decimalsOf = (data: Uint8Array) => (data.length > 44 ? data[44] : 0)

    return NextResponse.json(
      {
        ok: true,
        action,
        transaction: tx.serialize({ requireAllSignatures: false, verifySignatures: false }).toString('base64'),
        blockhash,
        lastValidBlockHeight,
        vault: vault.toBase58(),
        position: position.toBase58(),
        // The amounts the program will move: the ones RECORDED on the position, never the
        // vault's balance. Returned so the page states what is coming back before signing.
        returns: { token: p.tokenAmount, backing: p.backingAmount },
        unlockUnix: p.unlockUnix,
        decimals: {
          token: decimalsOf(new Uint8Array(tokenMintInfo.data)),
          backing: decimalsOf(new Uint8Array(backingMintInfo.data)),
        },
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
