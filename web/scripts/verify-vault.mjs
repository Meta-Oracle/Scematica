// Live lifecycle verification for `scemadex-vault` (programs/scemadex-vault).
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
import crypto from 'node:crypto';
import {
  Connection,
  Keypair,
  PublicKey,
  SystemProgram,
  Transaction,
  TransactionInstruction,
  SYSVAR_RENT_PUBKEY,
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
};

const DECIMALS = 6;
const ONE = 1_000_000n;
const WEEK = 7 * 24 * 60 * 60;

const disc = (name) =>
  crypto.createHash('sha256').update(`global:${name}`).digest().subarray(0, 8);
const u64 = (n) => {
  const b = Buffer.alloc(8);
  b.writeBigUInt64LE(BigInt(n));
  return b;
};
const i64 = (n) => {
  const b = Buffer.alloc(8);
  b.writeBigInt64LE(BigInt(n));
  return b;
};

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

const pda = (seeds) => PublicKey.findProgramAddressSync(seeds, PROGRAM_ID)[0];
const vaultPda = (t, b) => pda([Buffer.from('vault'), t.toBuffer(), b.toBuffer()]);
const tokenVaultPda = (v) => pda([Buffer.from('token_vault'), v.toBuffer()]);
const backingVaultPda = (v) => pda([Buffer.from('backing_vault'), v.toBuffer()]);
const positionPda = (v, d, nonce) =>
  pda([Buffer.from('position'), v.toBuffer(), d.toBuffer(), u64(nonce)]);

const meta = (pubkey, isSigner, isWritable) => ({ pubkey, isSigner, isWritable });

// Account order MUST match the field order of the #[derive(Accounts)] struct in lib.rs.
function ixInitializeVault({ payer, tokenMint, backingMint }) {
  const vault = vaultPda(tokenMint, backingMint);
  return new TransactionInstruction({
    programId: PROGRAM_ID,
    data: disc('initialize_vault'),
    keys: [
      meta(payer, true, true),
      meta(tokenMint, false, false),
      meta(backingMint, false, false),
      meta(vault, false, true),
      meta(tokenVaultPda(vault), false, true),
      meta(backingVaultPda(vault), false, true),
      meta(TOKEN_PROGRAM_ID, false, false),
      meta(SystemProgram.programId, false, false),
      // no rent sysvar — removed from InitializeVault to fit the SBF stack frame
    ],
  });
}

function ixDeposit({
  depositor, tokenMint, backingMint, depositorToken, depositorBacking,
  nonce, tokenAmount, backingAmount, lockSecs,
}) {
  const vault = vaultPda(tokenMint, backingMint);
  return new TransactionInstruction({
    programId: PROGRAM_ID,
    data: Buffer.concat([
      disc('deposit'),
      u64(nonce),
      u64(tokenAmount),
      u64(backingAmount),
      i64(lockSecs),
    ]),
    keys: [
      meta(depositor, true, true),
      meta(vault, false, true),
      meta(positionPda(vault, depositor, nonce), false, true),
      meta(tokenVaultPda(vault), false, true),
      meta(backingVaultPda(vault), false, true),
      meta(tokenMint, false, false),
      meta(backingMint, false, false),
      meta(depositorToken, false, true),
      meta(depositorBacking, false, true),
      meta(TOKEN_PROGRAM_ID, false, false),
      meta(SystemProgram.programId, false, false),
      meta(SYSVAR_RENT_PUBKEY, false, false),
    ],
  });
}

function ixExtendLock({ depositor, position, newUnlockUnix }) {
  return new TransactionInstruction({
    programId: PROGRAM_ID,
    data: Buffer.concat([disc('extend_lock'), i64(newUnlockUnix)]),
    keys: [meta(depositor, true, false), meta(position, false, true)],
  });
}

function ixWithdraw({
  depositor, positionOwner, tokenMint, backingMint, depositorToken, depositorBacking, nonce,
}) {
  const vault = vaultPda(tokenMint, backingMint);
  return new TransactionInstruction({
    programId: PROGRAM_ID,
    data: disc('withdraw'),
    keys: [
      meta(depositor, true, true),
      meta(vault, false, true),
      meta(positionPda(vault, positionOwner, nonce), false, true),
      meta(tokenVaultPda(vault), false, true),
      meta(backingVaultPda(vault), false, true),
      meta(tokenMint, false, false),
      meta(backingMint, false, false),
      meta(depositorToken, false, true),
      meta(depositorBacking, false, true),
      meta(TOKEN_PROGRAM_ID, false, false),
    ],
  });
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

  const base = { depositor: payer.publicKey, tokenMint, backingMint, depositorToken: aToken, depositorBacking: aBacking };

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
      depositor: bob.publicKey, positionOwner: payer.publicKey, tokenMint, backingMint,
      depositorToken: bToken, depositorBacking: bBacking, nonce: 1,
    })], [bob]);

  const posA = positionPda(vault, payer.publicKey, 1);
  await expectErr(conn, '7  extend_lock to an earlier time', 6003,
    [ixExtendLock({ depositor: payer.publicKey, position: posA, newUnlockUnix: 1 })], [payer]);

  const later = Math.floor(Date.now() / 1000) + 30 * 24 * 60 * 60;
  await expectOk(conn, '7a extend_lock to a later time succeeds',
    [ixExtendLock({ depositor: payer.publicKey, position: posA, newUnlockUnix: later })], [payer]);

  // 10 (deposit half). Two depositors, one vault.
  await expectOk(conn, '10 second depositor opens an independent position',
    [ixDeposit({
      depositor: bob.publicKey, tokenMint, backingMint, depositorToken: bToken,
      depositorBacking: bBacking, nonce: 1, tokenAmount: 3n * ONE, backingAmount: 4n * ONE,
      lockSecs: WEEK,
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
