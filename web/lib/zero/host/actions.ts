'use client'

// The two impure things Zero has to do: find a pool's vaults, and swap.
//
// Both are supplied to `ZeroRuntime` as hooks rather than imported by it, so the runtime
// stays testable and so the extension shell in phase 3 can substitute its own without
// touching the loop.

import { VersionedTransaction } from '@solana/web3.js'

import { getQuote, WSOL_MINT } from '../../swap.ts'
import { type Effect } from '../types.ts'
import { ZeroRpc, base64ToBytes, redact, type Subscription } from './rpc.ts'
// The layout and base58 helpers live in parse.ts, with the rest of the pure decoding:
// they are the part that must be tested, and this module cannot be imported by a test
// runner because it pulls in the swap path.
import {
  AMM_V4, RAYDIUM_AMM_V4_PROGRAM, base58ToBytes, bytesToBase58, vaultsFromPool,
} from './parse.ts'
export { AMM_V4, RAYDIUM_AMM_V4_PROGRAM, base58ToBytes, bytesToBase58, vaultsFromPool }
import { type Signer } from './signer.ts'

/**
 * Find the vaults for a mint by scanning Raydium's pools.
 *
 * `getProgramAccounts` with a memcmp is a heavy call and providers rate-limit it, which
 * is exactly why a failure here is reported as a coherence miss rather than swallowed:
 * a position Zero cannot watch is a position whose exits are not being evaluated.
 */
export async function resolveVaults(
  rpc: ZeroRpc,
  mint: string,
  encodeBase58: (b: Uint8Array) => string,
): Promise<Subscription | null> {
  const accounts = await rpc.call<Array<{ account: { data: [string, string] } }>>(
    'getProgramAccounts',
    [
      RAYDIUM_AMM_V4_PROGRAM,
      {
        encoding: 'base64',
        commitment: 'confirmed',
        filters: [
          { dataSize: AMM_V4.SIZE },
          { memcmp: { offset: AMM_V4.BASE_MINT, bytes: mint } },
        ],
      },
    ],
  )
  if (accounts.length === 0) return null

  const wsol = base58ToBytes(WSOL_MINT)
  for (const a of accounts) {
    const parsed = vaultsFromPool(base64ToBytes(a.account.data[0]), wsol)
    if (parsed) {
      return {
        mint,
        quoteVault: encodeBase58(parsed.quoteVault),
        baseVault: encodeBase58(parsed.baseVault),
      }
    }
  }
  return null
}

/**
 * Build, sign and submit a swap.
 *
 * Jupiter routes it; the signer signs it; `sendRaw` submits it. **Nothing here waits for
 * a confirmation** — `ZeroRuntime.observe` polls `getSignatureStatuses` instead, because
 * `sendAndConfirmTransaction` waits on a WebSocket subscription that has already, in this
 * repo, failed to settle on a transaction that finalized.
 */
export async function submitSwap(
  rpc: ZeroRpc,
  effect: Extract<Effect, { kind: 'swap' }>,
  signer: Signer,
): Promise<{ signature: string; outAmount: number | null }> {
  const quote = await getQuote({
    inputMint: effect.side === 'buy' ? WSOL_MINT : effect.mint,
    outputMint: effect.side === 'buy' ? effect.mint : WSOL_MINT,
    amount: effect.amount,
    slippageBps: 300,
  })

  const res = await fetch('https://lite-api.jup.ag/swap/v1/swap', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({
      quoteResponse: quote,
      userPublicKey: signer.publicKey,
      wrapAndUnwrapSol: true,
      dynamicComputeUnitLimit: true,
    }),
  })
  if (!res.ok) throw new Error(redact(`jupiter swap → HTTP ${res.status}`))
  const { swapTransaction } = (await res.json()) as { swapTransaction: string }

  const tx = VersionedTransaction.deserialize(base64ToBytes(swapTransaction))
  const signed = (await signer.sign(tx)) as VersionedTransaction
  const signature = await rpc.sendRaw(bytesToBase64(signed.serialize()))

  // The quote's `outAmount` is an ESTIMATE and is not returned as the fill. Realised
  // amounts come from an observed balance, never from a quote — the same rule the Rust
  // exit path learned when a stale target produced a 99% PnL that never existed.
  return { signature, outAmount: null }
}

function bytesToBase64(bytes: Uint8Array): string {
  let bin = ''
  for (const b of bytes) bin += String.fromCharCode(b)
  return btoa(bin)
}

