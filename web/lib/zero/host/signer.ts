'use client'

// Signers — the attended wallet, and the capped session key.
//
// ── What is stored, and the sentence that must accompany it ──────────────────
//
// The session key's secret bytes live in `localStorage`. That is not an oversight and it
// is not hidden: `docs/SCEMATICA-ZERO.md` §2.1 states the model plainly — the caps buy
// **bounded loss, not secrecy**. A key that malware or an XSS can reach is bounded by
// what is in it and by `session.ts`'s ledger, and by nothing else.
//
// A passphrase-derived encryption would raise the bar against a casual reader of
// localStorage and would not change the model at all: the key must be usable without a
// prompt (that is the entire point of a session key), so anything that can run in the
// page can use it whether or not it is encrypted at rest. Adding one would look like
// security while buying almost none, which is worse than the honest version.
//
// So the rules are: small defaults, a visible expiry, a visible cap, and a sweep.

import { Keypair, VersionedTransaction, Transaction } from '@solana/web3.js'

const SESSION_STORAGE = 'scematica-zero-session'

export interface StoredSession {
  /** base64 of the 64-byte secret key. */
  secret: string
  createdAtUnix: number
  expiresAtUnix: number
}

export function loadSession(): { keypair: Keypair; stored: StoredSession } | null {
  try {
    const raw = localStorage.getItem(SESSION_STORAGE)
    if (!raw) return null
    const stored = JSON.parse(raw) as StoredSession
    const bytes = Uint8Array.from(atob(stored.secret), c => c.charCodeAt(0))
    return { keypair: Keypair.fromSecretKey(bytes), stored }
  } catch {
    return null
  }
}

export function createSession(expiresAtUnix: number, nowUnix: number): { keypair: Keypair; stored: StoredSession } {
  const keypair = Keypair.generate()
  const stored: StoredSession = {
    secret: btoa(String.fromCharCode(...keypair.secretKey)),
    createdAtUnix: nowUnix,
    expiresAtUnix,
  }
  try {
    localStorage.setItem(SESSION_STORAGE, JSON.stringify(stored))
  } catch {
    /* storage disabled — the key exists for this page load only, which is safe by default */
  }
  return { keypair, stored }
}

/**
 * Destroy the session key.
 *
 * Called by the sweep and by the kill switch. It removes the material rather than a
 * flag, so a key that has been discarded cannot be re-armed by editing state — and so
 * "disarmed" means the same thing to a reader of localStorage as it does to the UI.
 */
export function destroySession(): void {
  try {
    localStorage.removeItem(SESSION_STORAGE)
  } catch {
    /* nothing to do; the key was never persisted */
  }
}

export type SignerKind = 'wallet' | 'session'

export interface Signer {
  kind: SignerKind
  publicKey: string
  sign(tx: VersionedTransaction | Transaction): Promise<VersionedTransaction | Transaction>
}

export function sessionSigner(keypair: Keypair): Signer {
  return {
    kind: 'session',
    publicKey: keypair.publicKey.toBase58(),
    async sign(tx) {
      if (tx instanceof VersionedTransaction) tx.sign([keypair])
      else tx.partialSign(keypair)
      return tx
    },
  }
}

export function walletSigner(
  publicKey: string,
  signTransaction: (tx: VersionedTransaction | Transaction) => Promise<VersionedTransaction | Transaction>,
): Signer {
  return { kind: 'wallet', publicKey, sign: signTransaction }
}
