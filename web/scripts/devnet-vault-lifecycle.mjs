#!/usr/bin/env node
// Devnet lifecycle test for the Escrow Market vault — DEPLOY.md §3.
//
// Covers tests 11 and 12: a Token-2022 token backed by a legacy-SPL reserve. That pairing
// is the product's central case (new mints are routinely Token-2022, every reserve asset
// worth locking is legacy SPL) and it is the one the account constraints cannot be checked
// for anywhere else — the unit tests in lib.rs are pure arithmetic over constants, and
// `initialize_account3` rejecting a mint its program does not own only happens on chain.
//
// Drives the REAL instruction builders from lib/escrow/instructions.ts rather than
// reimplementing them, so an account-order regression between the Rust structs and the
// web client fails here instead of in front of a user holding a signature prompt.
//
//   node scripts/devnet-vault-lifecycle.mjs <TOKEN_MINT> <BACKING_MINT>
//
// Needs a funded devnet keypair at the solana CLI's default path and both mints already
// created with a balance in the signer's ATAs. Never point this at mainnet: it locks real
// funds for MIN_LOCK_SECS (7 days) with no early exit, by design.

import { readFileSync } from 'node:fs'
import { homedir } from 'node:os'
import { join } from 'node:path'
import {
  ComputeBudgetProgram,
  Connection,
  Keypair,
  PublicKey,
  Transaction,
  sendAndConfirmTransaction,
} from '@solana/web3.js'

import {
  MIN_LOCK_SECS,
  TOKEN_2022_PROGRAM_ID,
  TOKEN_PROGRAM_ID,
  associatedTokenAddress,
  backingVaultPda,
  depositInstruction,
  initializeVaultInstruction,
  tokenVaultPda,
  vaultPda,
} from '../lib/escrow/instructions.ts'
import { decodeVault } from '../lib/escrow/program.ts'

const PROGRAM_ID = new PublicKey(
  process.env.ESCROW_PROGRAM_ID ?? 'A7h6khtKFJEu46By7C4hREdMQKkgvnuBCbVyusZRu4YW',
)
const RPC = process.env.RPC_ENDPOINT ?? 'https://api.devnet.solana.com'

const [tokenMintArg, backingMintArg] = process.argv.slice(2)
if (!tokenMintArg || !backingMintArg) {
  console.error('usage: node scripts/devnet-vault-lifecycle.mjs <TOKEN_MINT> <BACKING_MINT>')
  process.exit(2)
}

const tokenMint = new PublicKey(tokenMintArg)
const backingMint = new PublicKey(backingMintArg)

const keypairPath = process.env.SOLANA_KEYPAIR ?? join(homedir(), '.config', 'solana', 'id.json')
const signer = Keypair.fromSecretKey(
  Uint8Array.from(JSON.parse(readFileSync(keypairPath, 'utf8'))),
)

const connection = new Connection(RPC, 'confirmed')

const label = (programId) =>
  programId.equals(TOKEN_2022_PROGRAM_ID)
    ? 'Token-2022'
    : programId.equals(TOKEN_PROGRAM_ID)
      ? 'SPL Token'
      : 'UNKNOWN'

let failed = 0
const check = (name, ok, detail = '') => {
  if (!ok) failed++
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${detail ? ` — ${detail}` : ''}`)
}

console.log(`program  ${PROGRAM_ID.toBase58()}`)
console.log(`signer   ${signer.publicKey.toBase58()}`)
console.log(`rpc      ${RPC.split('?')[0]}\n`)

// ── the per-leg programs, read from the chain rather than assumed ─────────────
const [tokenInfo, backingInfo] = await connection.getMultipleAccountsInfo([tokenMint, backingMint])
if (!tokenInfo || !backingInfo) {
  console.error('one of the mints does not exist on this cluster')
  process.exit(1)
}
const tokenProgram = tokenInfo.owner
const backingProgram = backingInfo.owner

console.log(`token    ${tokenMint.toBase58()}  ${label(tokenProgram)}`)
console.log(`backing  ${backingMint.toBase58()}  ${label(backingProgram)}\n`)

console.log('── the pairing this test exists for ──────────────────────')
check(
  'the two legs are on DIFFERENT token programs',
  !tokenProgram.equals(backingProgram),
  `${label(tokenProgram)} vs ${label(backingProgram)}`,
)
if (tokenProgram.equals(backingProgram)) {
  console.log('\nboth legs share a program — this run proves nothing about the mixed path.')
}

const vault = vaultPda(PROGRAM_ID, tokenMint, backingMint)
const tokenVault = tokenVaultPda(PROGRAM_ID, vault)
const backingVault = backingVaultPda(PROGRAM_ID, vault)

// ── initialize_vault ─────────────────────────────────────────────────────────
console.log('\n── initialize_vault ──────────────────────────────────────')
const existing = await connection.getAccountInfo(vault)
if (existing) {
  console.log(`vault already exists at ${vault.toBase58()} — skipping init`)
} else {
  const tx = new Transaction()
    .add(ComputeBudgetProgram.setComputeUnitLimit({ units: 400_000 }))
    .add(
      initializeVaultInstruction({
        programId: PROGRAM_ID,
        payer: signer.publicKey,
        tokenMint,
        backingMint,
        tokenProgram,
        backingProgram,
      }),
    )
  const sig = await sendAndConfirmTransaction(connection, tx, [signer], {
    commitment: 'confirmed',
  })
  console.log(`  signature ${sig}`)
}

const vaultInfo = await connection.getAccountInfo(vault)
check('the vault account exists', vaultInfo !== null, vault.toBase58())

// The point of the whole change: one PDA token account per leg, each owned by the token
// program that owns its own mint. A single shared token_program made this unreachable.
const [tvInfo, bvInfo] = await connection.getMultipleAccountsInfo([tokenVault, backingVault])
check('the token vault exists', tvInfo !== null)
check('the backing vault exists', bvInfo !== null)
check(
  'the token vault is owned by the TOKEN leg program',
  tvInfo !== null && tvInfo.owner.equals(tokenProgram),
  tvInfo ? label(tvInfo.owner) : 'missing',
)
check(
  'the backing vault is owned by the BACKING leg program',
  bvInfo !== null && bvInfo.owner.equals(backingProgram),
  bvInfo ? label(bvInfo.owner) : 'missing',
)
check(
  'the two PDA vaults are under different programs',
  tvInfo !== null && bvInfo !== null && !tvInfo.owner.equals(bvInfo.owner),
)

// ── deposit ──────────────────────────────────────────────────────────────────
console.log('\n── deposit ───────────────────────────────────────────────')
const tokenAmount = 1_000n * 10n ** 6n // 1,000 of a 6dp token
const backingAmount = 5n * 10n ** 7n // 0.5 of an 8dp reserve
const nonce = BigInt(Date.now())

const before = decodeVault(new Uint8Array(vaultInfo.data))
console.log(`  before: token=${before.totalTokenLocked} backing=${before.totalBackingLocked} positions=${before.positionsOpen}`)

const depositTx = new Transaction()
  .add(ComputeBudgetProgram.setComputeUnitLimit({ units: 400_000 }))
  .add(
    depositInstruction({
      programId: PROGRAM_ID,
      depositor: signer.publicKey,
      tokenMint,
      backingMint,
      tokenProgram,
      backingProgram,
      nonce,
      tokenAmount,
      backingAmount,
      lockSecs: BigInt(MIN_LOCK_SECS),
    }),
  )
const depositSig = await sendAndConfirmTransaction(connection, depositTx, [signer], {
  commitment: 'confirmed',
})
console.log(`  signature ${depositSig}`)

const after = decodeVault(new Uint8Array((await connection.getAccountInfo(vault)).data))
console.log(`  after:  token=${after.totalTokenLocked} backing=${after.totalBackingLocked} positions=${after.positionsOpen}`)

check(
  'the token leg was credited',
  BigInt(after.totalTokenLocked) - BigInt(before.totalTokenLocked) === tokenAmount,
  `+${BigInt(after.totalTokenLocked) - BigInt(before.totalTokenLocked)}`,
)
check(
  'the backing leg was credited',
  BigInt(after.totalBackingLocked) - BigInt(before.totalBackingLocked) === backingAmount,
  `+${BigInt(after.totalBackingLocked) - BigInt(before.totalBackingLocked)}`,
)
check('a position was opened', Number(after.positionsOpen) === Number(before.positionsOpen) + 1)

// Solvency, the question the whole product answers: does the account actually hold what
// the vault claims it holds? Read the token accounts, not the program's own bookkeeping.
const [tvAfter, bvAfter] = await connection.getMultipleAccountsInfo([tokenVault, backingVault])
const amountOf = (info) =>
  new DataView(info.data.buffer, info.data.byteOffset, info.data.byteLength).getBigUint64(64, true)
check(
  'the token vault really holds the recorded amount',
  amountOf(tvAfter) >= BigInt(after.totalTokenLocked),
  `${amountOf(tvAfter)} on chain vs ${after.totalTokenLocked} recorded`,
)
check(
  'the backing vault really holds the recorded amount',
  amountOf(bvAfter) >= BigInt(after.totalBackingLocked),
  `${amountOf(bvAfter)} on chain vs ${after.totalBackingLocked} recorded`,
)

// The depositor's two ATAs are derived with DIFFERENT programs on a mixed pair. Deriving
// both with one leg's program yields an address the depositor does not own — a transaction
// that fails only after the wallet has been asked to sign.
const ownerToken = associatedTokenAddress(tokenMint, signer.publicKey, tokenProgram)
const ownerBacking = associatedTokenAddress(backingMint, signer.publicKey, backingProgram)
const wrongBacking = associatedTokenAddress(backingMint, signer.publicKey, tokenProgram)
check('the two depositor ATAs differ', !ownerToken.equals(ownerBacking))
check(
  'deriving the backing ATA with the wrong program gives a different address',
  !wrongBacking.equals(ownerBacking),
  wrongBacking.toBase58().slice(0, 8) + '… vs ' + ownerBacking.toBase58().slice(0, 8) + '…',
)

console.log(`\nvault    ${vault.toBase58()}`)
console.log(`unlock   position matures in ${MIN_LOCK_SECS / 86400} days — withdraw (DEPLOY.md §3 test 8) cannot run before then`)
console.log(`\n${failed === 0 ? 'ALL PASS' : `${failed} FAILED`}`)
process.exit(failed === 0 ? 0 : 1)
