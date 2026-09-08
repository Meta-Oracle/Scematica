// Live lifecycle verification for `scematica-vault` (programs/scematica-vault).
//
// Runs the DEPLOY.md section 3 table against a DEPLOYED program, on whatever cluster
// RPC_ENDPOINT points at. It creates two throwaway SPL mints, so it needs no wBTC and no
// pre-existing token; total cost is well under 0.05 SOL in rent and fees.
//
//   cd web ; node scripts/verify-vault.mjs
//   VAULT_PROGRAM_ID=<id> node scripts/verify-vault.mjs
//
// It lives under web/ purely so that `@solana/web3.js` and `@solana/spl-token` resolve
// from web/node_modules; it has nothing to do with the Next app.
//
// WHAT THIS CANNOT PROVE. `MIN_LOCK_SECS` is 7 days and is checked against the chain
// clock, so a *successful* withdraw cannot be exercised here on any cluster. Every
// rejection path can, including `StillLocked`. The summary at the end states this
// rather than quietly reporting a pass — a verification tool that overstates coverage on
// a custody program is worse than none.
//
// Negative tests assert the specific Anchor error NUMBER. Asserting only "it failed"
// would pass on a typo'd account list, which is the exact bug this is meant to catch.

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import {
  Connection,
  Keypair,
  PublicKey,
  SystemProgram,
  Transaction,
  LAMPORTS_PER_SOL,
  sendAndConfirmTransaction,
} from '@solana/web3.js';
import {
  TOKEN_PROGRAM_ID,
  createMint,
  getOrCreateAssociatedTokenAccount,
  mintTo,
  getAccount,
} from '@solana/spl-token';

import {
  backingVaultPda as sharedBackingVaultPda,
  depositInstruction,
  extendLockInstruction,
  initializeVaultInstruction,
  positionPda as sharedPositionPda,
  tokenVaultPda as sharedTokenVaultPda,
  associatedTokenAddress,
  vaultPda as sharedVaultPda,
  createAtaInstruction,
  withdrawInstruction,
} from '../lib/escrow/instructions.ts';
import { decodePosition } from '../lib/escrow/program.ts';

const PROGRAM_ID = new PublicKey(
  process.env.VAULT_PROGRAM_ID ?? 'A7h6khtKFJEu46By7C4hREdMQKkgvnuBCbVyusZRu4YW',
);

const ERR = {
  6000: 'ZeroBacking',
  6001: 'LockOutOfRange',
  6002: 'StillLocked',
  6003: 'LockNotExtended',
  6004: 'NotDepositor',
  6005: 'VaultMismatch',
  6006: 'MintMismatch',
  6007: 'MathOverflow',
  6008: 'AccountingUnderflow',
  6009: 'SameMint',
  // An Anchor framework code, not one of ours — the replay check expects it.
  3012: 'AccountNotInitialized',
};

const DECIMALS = 6;
const ONE = 1_000_000n;
const WEEK = 7 * 24 * 60 * 60;


function readEnvRpc() {
  if (process.env.RPC_ENDPOINT) return process.env.RPC_ENDPOINT;
  const envPath = path.resolve(process.cwd(), '..', '.env');
  if (fs.existsSync(envPath)) {
    for (const line of fs.readFileSync(envPath, 'utf8').split(/\r?\n/)) {
      const m = line.match(/^\s*RPC_ENDPOINT\s*=\s*(.+?)\s*$/);
      if (m) return m[1].replace(/^["']|["']$/g, '');
    }
  }
  return 'https://api.mainnet-beta.solana.com';
}

function loadKeypair() {
  const p =
    process.env.DEPLOYER_KEYPAIR ?? path.join(os.homedir(), '.config', 'solana', 'id.json');
  return Keypair.fromSecretKey(Uint8Array.from(JSON.parse(fs.readFileSync(p, 'utf8'))));
}

// The account lists are NOT rebuilt here. They come from lib/escrow/instructions.ts —
// the same builders the /escrow page hands to a user's wallet — because a verification
// script with its own copy of the account order verifies its own copy and nothing else.
//
// This file learned that the hard way. It carried a private set of builders written
// before `token_token_program` / `backing_token_program` were split per leg, so it kept
// passing ONE token program where the program wants two. Anchor reads accounts
// positionally, so the System program landed in the `backing_token_program` slot and
// every single check failed with 3008 InvalidProgramId — against a program that was
// completely healthy. A drifted verifier does not merely miss bugs, it invents them.
const vaultPda = (t, b) => sharedVaultPda(PROGRAM_ID, t, b);
const tokenVaultPda = (v) => sharedTokenVaultPda(PROGRAM_ID, v);
const backingVaultPda = (v) => sharedBackingVaultPda(PROGRAM_ID, v);
const positionPda = (v, d, nonce) => sharedPositionPda(PROGRAM_ID, v, d, BigInt(nonce));

// These mints are created by `createMint`, which is legacy SPL, so both legs are
// TOKEN_PROGRAM_ID. The mixed Token-2022 pairing is covered by
// scripts/devnet-vault-lifecycle.mjs, which needs two pre-made mints to exercise it.
const LEGS = { tokenProgram: TOKEN_PROGRAM_ID, backingProgram: TOKEN_PROGRAM_ID };

const ixInitializeVault = ({ payer, tokenMint, backingMint }) =>
  initializeVaultInstruction({ programId: PROGRAM_ID, payer, tokenMint, backingMint, ...LEGS });

const ixDeposit = ({
  depositor, tokenMint, backingMint, nonce, tokenAmount, backingAmount, lockSecs,
}) =>
  depositInstruction({
    programId: PROGRAM_ID, depositor, tokenMint, backingMint, ...LEGS,
    nonce: BigInt(nonce),
    tokenAmount: BigInt(tokenAmount),
    backingAmount: BigInt(backingAmount),
    lockSecs: BigInt(lockSecs),
  });

const ixExtendLock = ({ depositor, tokenMint, backingMint, nonce, newUnlockUnix }) =>
  extendLockInstruction({
    programId: PROGRAM_ID, depositor, tokenMint, backingMint,
    nonce: BigInt(nonce), newUnlockUnix: BigInt(newUnlockUnix),
  });

// `positionOwner` exists only for negative test 6, where Bob signs against Alice's
// position. The shared builder derives the position from the signer — correct for every
// real caller, and the one thing that check needs to violate. Overriding the single
// `position` key rather than hand-rolling the list keeps the ORDER authoritative, which
// is the part that drifts; slot 2 is asserted below so a reordering cannot silently make
// this overwrite something else.
function ixWithdraw({ depositor, positionOwner, tokenMint, backingMint, nonce, tokenProgram, backingProgram }) {
  const ix = withdrawInstruction({
    programId: PROGRAM_ID, depositor, tokenMint, backingMint,
    tokenProgram: tokenProgram ?? LEGS.tokenProgram,
    backingProgram: backingProgram ?? LEGS.backingProgram,
    nonce: BigInt(nonce),
  });
  if (positionOwner && !positionOwner.equals(depositor)) {
    const vault = vaultPda(tokenMint, backingMint);
    const mine = positionPda(vault, depositor, nonce);
    if (!ix.keys[2].pubkey.equals(mine)) {
      throw new Error('Withdraw account order changed — slot 2 is no longer `position`.');
    }
    ix.keys[2] = { pubkey: positionPda(vault, positionOwner, nonce), isSigner: false, isWritable: true };
  }
  return ix;
}

function decodeVault(data) {
  let o = 8; // anchor account discriminator
  const key = () => {
    const k = new PublicKey(data.subarray(o, o + 32));
    o += 32;
    return k;
  };
  const n = () => {
    const v = data.readBigUInt64LE(o);
    o += 8;
    return v;
  };
  return {
    tokenMint: key(),
    backingMint: key(),
    tokenVault: key(),
    backingVault: key(),
    totalTokenLocked: n(),
    totalBackingLocked: n(),
    positionsOpen: n(),
    positionsLifetime: n(),
    bump: data[o],
  };
}

const results = [];
function record(name, ok, detail) {
  results.push({ name, ok, detail });
  console.log(`${ok ? '  PASS' : '  FAIL'}  ${name}${detail ? ` — ${detail}` : ''}`);
}

async function errCodeOf(conn, e) {
  let logs = e?.logs;
  if (!logs && typeof e?.getLogs === 'function') {
    try { logs = await e.getLogs(conn); } catch { /* keep undefined */ }
  }
  for (const l of logs ?? []) {
    const m = l.match(/Error Number:\s*(\d+)/);
    if (m) return Number(m[1]);
  }
  const m2 = String(e?.message ?? '').match(/custom program error:\s*0x([0-9a-fA-F]+)/);
  if (m2) return parseInt(m2[1], 16);
  return null;
}

async function send(conn, ixs, signers) {
  const tx = new Transaction().add(...ixs);
  return sendAndConfirmTransaction(conn, tx, signers, {
    commitment: 'confirmed',
    skipPreflight: false,
  });
}

async function expectOk(conn, label, ixs, signers) {
  try {
    const sig = await send(conn, ixs, signers);
    record(label, true, sig.slice(0, 16) + '…');
    return true;
  } catch (e) {
    const code = await errCodeOf(conn, e);
    record(label, false, code ? `unexpected ${code} ${ERR[code] ?? ''}` : String(e?.message).slice(0, 160));
    return false;
  }
}

async function expectErr(conn, label, wantCode, ixs, signers) {
  try {
    await send(conn, ixs, signers);
    record(label, false, `expected ${wantCode} ${ERR[wantCode]}, but it SUCCEEDED`);
    return false;
  } catch (e) {
    const code = await errCodeOf(conn, e);
    const ok = code === wantCode;
    record(
      label,
      ok,
      ok ? `${wantCode} ${ERR[wantCode]}` : `expected ${wantCode} ${ERR[wantCode]}, got ${code ?? 'unknown'} ${ERR[code] ?? ''}`,
    );
    return ok;
  }
}

/**
 * DEPLOY.md #8 and #9 — the two checks the main run cannot reach.
 *
 * `MIN_LOCK_SECS` is 7 days against the chain clock and there is no early exit by
 * design, so a successful withdraw is only observable a week after the deposit that
 * created it. This mode re-attaches to a vault the main run left behind and finishes
 * the table.
 *
 *   node scripts/verify-vault.mjs --withdraw <TOKEN_MINT> <BACKING_MINT>
 *
 * A position that has not matured yet is NOT a failure — it is the program working —
 * so that case prints the remaining time and exits 0. Reporting it as a failed check
 * would train whoever runs this to ignore a red line.
 */
async function withdrawMode(conn, payer, tokenMint, backingMint, nonce = 1) {
  const vault = vaultPda(tokenMint, backingMint);
  const tokenVault = tokenVaultPda(vault);
  const backingVault = backingVaultPda(vault);
  const position = positionPda(vault, payer.publicKey, nonce);

  const pAcc = await conn.getAccountInfo(position);
  if (!pAcc) {
    console.error(`No position at ${position.toBase58()} for ${payer.publicKey.toBase58()}.`);
    console.error(`Run the script with no arguments first; it leaves one behind at nonce 1.`);
    console.error('Or pass the nonce as a fourth argument — /api/escrow/positions lists them.');
    process.exit(2);
  }
  const p = decodePosition(pAcc.data);
  if (!p) {
    console.error(`Account at ${position.toBase58()} is ${pAcc.data.length} bytes, not a Position.`);
    process.exit(2);
  }

  const now = Math.floor(Date.now() / 1000);
  console.log(`position : ${position.toBase58()}`);
  console.log(`  token=${p.tokenAmount} backing=${p.backingAmount} unlock=${p.unlockUnix}`);
  if (BigInt(now) < BigInt(p.unlockUnix)) {
    const left = Number(BigInt(p.unlockUnix) - BigInt(now));
    const d = Math.floor(left / 86400), h = Math.floor((left % 86400) / 3600);
    console.log(`
Still locked for ${d}d ${h}h — that is the program working, not a fault.`);
    console.log('Re-run once it matures to finish DEPLOY.md #8 and #9.');
    process.exit(0);
  }

  // Each leg's token program comes off its MINT, never assumed. The main run creates
  // legacy-SPL mints, but this mode re-attaches to any vault -- including one opened
  // through /escrow against a Token-2022 token -- and the ATA seeds include the token
  // program, so assuming the wrong one derives a valid address nobody controls.
  const [tokenMintInfo, backingMintInfo] =
    await conn.getMultipleAccountsInfo([tokenMint, backingMint]);
  const legs = { tokenProgram: tokenMintInfo.owner, backingProgram: backingMintInfo.owner };

  // Balances before, so #8 can assert the EXACT amounts rather than "it did not throw".
  const vBefore = decodeVault((await conn.getAccountInfo(vault)).data);
  const myToken = associatedTokenAddress(tokenMint, payer.publicKey, legs.tokenProgram);
  const myBacking = associatedTokenAddress(backingMint, payer.publicKey, legs.backingProgram);

  // The receiving accounts must exist before the program transfers into them. Usually
  // they do -- they funded the deposit -- but an ATA can be closed at zero balance, and
  // this run found exactly that: a matured position whose depositor no longer had a
  // token account. Left alone it fails on chain with an error naming neither the account
  // nor the fix, which is why /api/escrow/withdraw prepends the same two instructions.
  const setup = [];
  const [tInfo, bInfo] = await conn.getMultipleAccountsInfo([myToken, myBacking]);
  if (!tInfo) setup.push(createAtaInstruction(payer.publicKey, payer.publicKey, tokenMint, legs.tokenProgram));
  if (!bInfo) setup.push(createAtaInstruction(payer.publicKey, payer.publicKey, backingMint, legs.backingProgram));
  if (setup.length) {
    console.log(`  (creating ${setup.length} missing receiving account(s) first)`);
    await send(conn, setup, [payer]);
  }

  const tBefore = (await getAccount(conn, myToken, 'confirmed', legs.tokenProgram)).amount;
  const bBefore = (await getAccount(conn, myBacking, 'confirmed', legs.backingProgram)).amount;
  const tvBefore = (await getAccount(conn, tokenVault, 'confirmed', legs.tokenProgram)).amount;
  const bvBefore = (await getAccount(conn, backingVault, 'confirmed', legs.backingProgram)).amount;

  console.log('');
  console.log('--- DEPLOY.md section 3, tests 8 and 9 ---');

  const base = { depositor: payer.publicKey, tokenMint, backingMint, ...legs };
  const ok = await expectOk(conn, '8  withdraw after unlock, correct depositor',
    [ixWithdraw({ ...base, nonce })], [payer]);

  if (ok) {
    // The amounts must be the ones RECORDED at deposit, not the vault's balance. That is
    // what stops one position reaching another's funds, so it is asserted exactly.
    const tAfter = (await getAccount(conn, myToken, 'confirmed', legs.tokenProgram)).amount;
    const bAfter = (await getAccount(conn, myBacking, 'confirmed', legs.backingProgram)).amount;
    const exact = tAfter - tBefore === BigInt(p.tokenAmount)
      && bAfter - bBefore === BigInt(p.backingAmount);
    record('8a both legs returned exactly the recorded amounts', exact,
      `token +${tAfter - tBefore}/${p.tokenAmount}, backing +${bAfter - bBefore}/${p.backingAmount}`);

    const closed = (await conn.getAccountInfo(position)) === null;
    record('8b position account closed and rent refunded', closed,
      closed ? 'account gone' : 'position still exists');

    const vAfter = decodeVault((await conn.getAccountInfo(vault)).data);
    const totals = vAfter.totalTokenLocked === vBefore.totalTokenLocked - BigInt(p.tokenAmount)
      && vAfter.totalBackingLocked === vBefore.totalBackingLocked - BigInt(p.backingAmount)
      && vAfter.positionsOpen === vBefore.positionsOpen - 1n;
    record('8c vault totals decremented by exactly this position', totals,
      `token=${vAfter.totalTokenLocked} backing=${vAfter.totalBackingLocked} open=${vAfter.positionsOpen}`);

    // Test 10 from the other side: the second depositor's funds must be untouched.
    const tvAfter = (await getAccount(conn, tokenVault, 'confirmed', legs.tokenProgram)).amount;
    const bvAfter = (await getAccount(conn, backingVault, 'confirmed', legs.backingProgram)).amount;
    const isolated = tvAfter >= vAfter.totalTokenLocked && bvAfter >= vAfter.totalBackingLocked
      && tvBefore - tvAfter === BigInt(p.tokenAmount)
      && bvBefore - bvAfter === BigInt(p.backingAmount);
    record("10c the other depositor's funds were not touched", isolated,
      `vault token ${tvAfter}/${vAfter.totalTokenLocked}, backing ${bvAfter}/${vAfter.totalBackingLocked}`);

    // 9. Replay. The position account no longer exists, so account validation rejects it
    // before any handler runs — an Anchor framework error rather than one of ours.
    await expectErr(conn, '9  replay the same withdraw', 3012,
      [ixWithdraw({ ...base, nonce })], [payer]);
  }

  const passed = results.filter((r) => r.ok).length;
  console.log('');
  console.log(`==== ${passed}/${results.length} checks passed ====`);
  if (passed !== results.length) process.exit(1);
}

async function main() {
  const rpc = readEnvRpc();
  const conn = new Connection(rpc, 'confirmed');
  const payer = loadKeypair();

  console.log(`program : ${PROGRAM_ID.toBase58()}`);
  console.log(`rpc     : ${rpc.split('?')[0]}`);
  console.log(`payer   : ${payer.publicKey.toBase58()}`);

  const info = await conn.getAccountInfo(PROGRAM_ID);
  if (!info) {
    console.error('\nProgram is not deployed at that address. Deploy first.');
    process.exit(2);
  }
  if (!info.executable) {
    console.error('\nAccount exists but is not executable — that is not a program.');
    process.exit(2);
  }
  const bal = await conn.getBalance(payer.publicKey);
  console.log(`balance : ${(bal / LAMPORTS_PER_SOL).toFixed(4)} SOL\n`);
  const argv = process.argv.slice(2);
  if (argv[0] === '--withdraw') {
    if (argv.length < 3) {
      console.error('usage: node scripts/verify-vault.mjs --withdraw <TOKEN_MINT> <BACKING_MINT> [NONCE]');
      process.exit(2);
    }
    // The nonce is optional and defaults to the one the main run leaves behind. A vault
    // opened through /escrow uses a timestamp nonce, which /api/escrow/positions lists.
    return withdrawMode(conn, payer, new PublicKey(argv[1]), new PublicKey(argv[2]), argv[3] ?? 1);
  }

  console.log('--- setup: two throwaway SPL mints ---');
  const tokenMint = await createMint(conn, payer, payer.publicKey, null, DECIMALS);
  const backingMint = await createMint(conn, payer, payer.publicKey, null, DECIMALS);
  console.log(`  token   ${tokenMint.toBase58()}`);
  console.log(`  backing ${backingMint.toBase58()}`);

  const aToken = (await getOrCreateAssociatedTokenAccount(conn, payer, tokenMint, payer.publicKey)).address;
  const aBacking = (await getOrCreateAssociatedTokenAccount(conn, payer, backingMint, payer.publicKey)).address;
  await mintTo(conn, payer, tokenMint, aToken, payer, 100n * ONE);
  await mintTo(conn, payer, backingMint, aBacking, payer, 100n * ONE);

  // Second depositor, for the isolation and NotDepositor checks.
  const bob = Keypair.generate();
  await send(conn, [SystemProgram.transfer({
    fromPubkey: payer.publicKey, toPubkey: bob.publicKey, lamports: 0.03 * LAMPORTS_PER_SOL,
  })], [payer]);
  const bToken = (await getOrCreateAssociatedTokenAccount(conn, payer, tokenMint, bob.publicKey)).address;
  const bBacking = (await getOrCreateAssociatedTokenAccount(conn, payer, backingMint, bob.publicKey)).address;
  await mintTo(conn, payer, tokenMint, bToken, payer, 50n * ONE);
  await mintTo(conn, payer, backingMint, bBacking, payer, 50n * ONE);
  console.log(`  second depositor ${bob.publicKey.toBase58()}\n`);

  const vault = vaultPda(tokenMint, backingMint);
  const tokenVault = tokenVaultPda(vault);
  const backingVault = backingVaultPda(vault);

  console.log('--- DEPLOY.md section 3 ---');

  // 1. initialize_vault. This is the instruction the stack-frame overflow would have broken.
  await expectOk(conn, '1  initialize_vault creates vault + two PDA token accounts',
    [ixInitializeVault({ payer: payer.publicKey, tokenMint, backingMint })], [payer]);

  const vAcc = await conn.getAccountInfo(vault);
  if (vAcc) {
    const v = decodeVault(vAcc.data);
    const wired =
      v.tokenMint.equals(tokenMint) && v.backingMint.equals(backingMint) &&
      v.tokenVault.equals(tokenVault) && v.backingVault.equals(backingVault);
    record('1a vault record wired to the right mints and vaults', wired,
      wired ? `${vAcc.data.length} bytes` : 'field mismatch');
    const tv = await getAccount(conn, tokenVault);
    const bv = await getAccount(conn, backingVault);
    const owned = tv.owner.equals(vault) && bv.owner.equals(vault);
    record('1b both token vaults are owned by the vault PDA, not a wallet', owned,
      owned ? `authority ${vault.toBase58().slice(0, 8)}…` : 'owner mismatch');
  } else {
    record('1a vault record wired to the right mints and vaults', false, 'vault account missing');
    record('1b both token vaults are owned by the vault PDA, not a wallet', false, 'vault account missing');
  }

  // SameMint: a token backed by itself is rejected outright.
  await expectErr(conn, '1c initialize_vault with token == backing', 6009,
    [ixInitializeVault({ payer: payer.publicKey, tokenMint, backingMint: tokenMint })], [payer]);

  const base = { depositor: payer.publicKey, tokenMint, backingMint };

  await expectErr(conn, '2  deposit with backing_amount = 0', 6000,
    [ixDeposit({ ...base, nonce: 1, tokenAmount: ONE, backingAmount: 0n, lockSecs: WEEK })], [payer]);

  await expectErr(conn, '3  deposit with lock_secs = 3600', 6001,
    [ixDeposit({ ...base, nonce: 1, tokenAmount: ONE, backingAmount: ONE, lockSecs: 3600 })], [payer]);

  await expectOk(conn, '4  deposit with token_amount = 0, backing > 0 (pure reserve)',
    [ixDeposit({ ...base, nonce: 1, tokenAmount: 0n, backingAmount: 2n * ONE, lockSecs: WEEK })], [payer]);

  await expectErr(conn, '5  withdraw before unlock_unix', 6002,
    [ixWithdraw({ ...base, positionOwner: payer.publicKey, nonce: 1 })], [payer]);

  // Bob signs against Alice's position. `has_one = depositor` rejects it during account
  // validation, before the handler's StillLocked check ever runs.
  await expectErr(conn, '6  withdraw signed by a different wallet', 6004,
    [ixWithdraw({
      depositor: bob.publicKey, positionOwner: payer.publicKey, tokenMint, backingMint, nonce: 1,
    })], [bob]);

  await expectErr(conn, '7  extend_lock to an earlier time', 6003,
    [ixExtendLock({ depositor: payer.publicKey, ...base, nonce: 1, newUnlockUnix: 1 })], [payer]);

  const later = Math.floor(Date.now() / 1000) + 30 * 24 * 60 * 60;
  await expectOk(conn, '7a extend_lock to a later time succeeds',
    [ixExtendLock({ depositor: payer.publicKey, ...base, nonce: 1, newUnlockUnix: later })], [payer]);

  // 10 (deposit half). Two depositors, one vault.
  await expectOk(conn, '10 second depositor opens an independent position',
    [ixDeposit({
      depositor: bob.publicKey, tokenMint, backingMint, nonce: 1,
      tokenAmount: 3n * ONE, backingAmount: 4n * ONE, lockSecs: WEEK,
    })], [bob]);

  // Accounting: the invariant the /escrow page reports on.
  const vAcc2 = await conn.getAccountInfo(vault);
  if (vAcc2) {
    const v = decodeVault(vAcc2.data);
    const tv = await getAccount(conn, tokenVault);
    const bv = await getAccount(conn, backingVault);
    const okTotals = v.totalTokenLocked === 3n * ONE && v.totalBackingLocked === 6n * ONE;
    record('10a vault totals equal the sum of both positions', okTotals,
      `token=${v.totalTokenLocked} backing=${v.totalBackingLocked} open=${v.positionsOpen}`);
    const okBal = tv.amount >= v.totalTokenLocked && bv.amount >= v.totalBackingLocked;
    record('10b real balances >= recorded totals (the proof-of-reserve invariant)', okBal,
      `token ${tv.amount}/${v.totalTokenLocked}, backing ${bv.amount}/${v.totalBackingLocked}`);
  }

  const passed = results.filter((r) => r.ok).length;
  console.log(`\n==== ${passed}/${results.length} checks passed ====`);
  console.log('NOT COVERED: a successful withdraw, and the replay of one (DEPLOY.md #8/#9).');
  console.log('MIN_LOCK_SECS is 7 days against the chain clock, so neither can run today.');
  console.log(`To finish coverage later:  node scripts/verify-vault.mjs --withdraw ${tokenMint.toBase58()} ${backingMint.toBase58()}`);

  if (passed !== results.length) process.exit(1);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
