'use client'

// Moving SOL in and out of the session key.
//
// The arithmetic lives in `lib/zero/funding.ts` and is tested there; this file is only
// the transfer. That split is the same one `transferPlan` / `settle` uses in the
// scema-world treasury, and for the same reason: doing it for real needs a funded key, so
// otherwise the arithmetic bounding every movement would only ever be exercised by moving
// money.
//
// ── The destination is not an input ──────────────────────────────────────────
//
// A sweep returns funds to `fundedBy`, read from the session record. It does not take an
// address. A sweep that took one would be a one-click drain of the hot key to anywhere,
// reachable by anything that can run in the page — which is the same threat model the key
// itself lives under, so it would hand an attacker the whole balance rather than the
// bounded slice the caps are there to expose.

import { Keypair, PublicKey, SystemProgram, Transaction } from '@solana/web3.js'

import { fundingPlan, sweepPlan, type FundingPlan } from '../funding.ts'
import { ZeroRpc } from './rpc.ts'
import { type Signer } from './signer.ts'

/** Who funded this key, so a sweep has somewhere to go that nobody chose at sweep time. */
const FUNDER_STORAGE = 'scematica-zero-funder'

export function loadFunder(): string | null {
  try {
    return localStorage.getItem(FUNDER_STORAGE)
  } catch {
    return null
  }
}

export function saveFunder(address: string): void {
  try {
    localStorage.setItem(FUNDER_STORAGE, address)
  } catch {
    /* storage disabled: the sweep will have to be told a destination by re-funding */
  }
}

export function clearFunder(): void {
  try {
    localStorage.removeItem(FUNDER_STORAGE)
  } catch {
    /* nothing to clear */
  }
}

export interface Movement {
  ok: boolean
  signature?: string
  detail: string
}

/**
 * Fund the session key from the connected wallet.
 *
 * The wallet signs — this is an ordinary transfer the user approves in their own wallet,
 * and it is the one moment in Zero's life where a human explicitly authorises the size of
 * what the bot can lose. The cap is checked against the RESULTING balance, so repeated
 * top-ups cannot walk past it one increment at a time.
 */
export async function fundSession(
  rpc: ZeroRpc,
  walletSigner: Signer,
  sessionAddress: string,
  requestedLamports: number,
  maxBalanceLamports: number,
): Promise<Movement> {
  const [sessionBalance, funderBalance] = await Promise.all([
    rpc.lamports(sessionAddress),
    rpc.lamports(walletSigner.publicKey),
  ])
  // A balance that could not be read is NOT zero. Funding against an unknown current
  // balance is exactly how a cap gets walked past — the resulting figure would be wrong.
  if (sessionBalance === null) {
    return { ok: false, detail: 'could not read the session key balance — refusing to fund against an unknown total' }
  }

  const plan = fundingPlan(sessionBalance, requestedLamports, maxBalanceLamports, funderBalance)
  if (!plan.ok) return { ok: false, detail: plan.detail }

  const tx = new Transaction().add(
    SystemProgram.transfer({
      fromPubkey: new PublicKey(walletSigner.publicKey),
      toPubkey: new PublicKey(sessionAddress),
      lamports: plan.lamports,
    }),
  )
  tx.feePayer = new PublicKey(walletSigner.publicKey)
  tx.recentBlockhash = await rpc.blockhash()

  const signed = (await walletSigner.sign(tx)) as Transaction
  const signature = await rpc.sendRaw(signed.serialize().toString('base64'))

  saveFunder(walletSigner.publicKey)
  return { ok: true, signature, detail: plan.note }
}

/**
 * Return everything to the wallet that supplied it.
 *
 * Signed by the SESSION key, because it is the sender. The destination comes from the
 * stored funder and nowhere else.
 *
 * The reserve left behind is not politeness: a sweep that under-reserves fails, and a
 * failed sweep leaves the whole balance in a key the operator has already decided to stop
 * trusting.
 */
export async function sweepSession(
  rpc: ZeroRpc,
  session: Keypair,
  funder: string | null,
): Promise<Movement> {
  if (!funder) {
    return {
      ok: false,
      detail:
        'no funding wallet on record, so there is nowhere this may safely go. Fund the key once from the wallet you want it swept back to.',
    }
  }

  const balance = await rpc.lamports(session.publicKey.toBase58())
  if (balance === null) {
    return { ok: false, detail: 'could not read the session key balance' }
  }

  const plan: FundingPlan = sweepPlan(balance)
  if (!plan.ok) return { ok: false, detail: plan.detail }

  const tx = new Transaction().add(
    SystemProgram.transfer({
      fromPubkey: session.publicKey,
      toPubkey: new PublicKey(funder),
      lamports: plan.lamports,
    }),
  )
  tx.feePayer = session.publicKey
  tx.recentBlockhash = await rpc.blockhash()
  tx.sign(session)

  const signature = await rpc.sendRaw(tx.serialize().toString('base64'))
  return { ok: true, signature, detail: plan.note }
}
