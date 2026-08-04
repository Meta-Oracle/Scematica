// Non-custodial swap execution through Jupiter.
//
// This is the honest standalone execution path: the route is built by Jupiter, the
// transaction is signed by the user's own wallet in their own browser, and no key ever
// leaves the device. No backend, no custody, no pairing.
//
// ⚠️  THIS IS NOT SNIPING. A wallet prompt puts a human in the loop, which costs
// seconds; the bot's edge on a new pool is sub-second. Treat this as manual execution
// on a token the discovery panels surfaced — for actual sniping, pair a self-hosted
// instance where the sniper signs locally with no prompt.
//
// Both endpoints are keyless and send `Access-Control-Allow-Origin: *`, so they are
// callable straight from the page.

import { VersionedTransaction } from '@solana/web3.js'
import type { Connection, PublicKey } from '@solana/web3.js'

const QUOTE_URL = 'https://lite-api.jup.ag/swap/v1/quote'
const SWAP_URL  = 'https://lite-api.jup.ag/swap/v1/swap'

export const WSOL_MINT = 'So11111111111111111111111111111111111111112'
export const LAMPORTS_PER_SOL = 1_000_000_000

/** Decode base64 without `Buffer` — Next does not polyfill it into the browser bundle. */
function base64ToBytes(b64: string): Uint8Array {
  const bin = atob(b64)
  const out = new Uint8Array(bin.length)
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i)
  return out
}

/** Jupiter's quote payload. Passed back to the swap endpoint verbatim. */
export interface Quote {
  inputMint: string
  outputMint: string
  inAmount: string
  outAmount: string
  otherAmountThreshold: string
  slippageBps: number
  priceImpactPct: string
  swapUsdValue?: string
  routePlan?: unknown[]
}

export interface QuoteRequest {
  inputMint: string
  outputMint: string
  /** Input amount in the input mint's base units (lamports for SOL). */
  amount: number
  slippageBps: number
}

export class SwapError extends Error {
  constructor(message: string, public readonly stage: 'quote' | 'build' | 'sign' | 'confirm') {
    super(message)
    this.name = 'SwapError'
  }
}

/** Fetch a route. Throws SwapError('quote') when no route exists. */
export async function getQuote(req: QuoteRequest): Promise<Quote> {
  const qs = new URLSearchParams({
    inputMint: req.inputMint,
    outputMint: req.outputMint,
    amount: String(Math.floor(req.amount)),
    slippageBps: String(req.slippageBps),
  })
  let res: Response
  try {
    res = await fetch(`${QUOTE_URL}?${qs}`, { signal: AbortSignal.timeout(10_000) })
  } catch {
    throw new SwapError('Could not reach the quote service', 'quote')
  }
  if (!res.ok) {
    throw new SwapError(`No route available (HTTP ${res.status})`, 'quote')
  }
  const quote = (await res.json()) as Quote
  if (!quote?.outAmount) throw new SwapError('No route available for this pair', 'quote')
  return quote
}

/** Ask Jupiter to build the signed-transaction payload for a quote. */
async function buildSwapTx(quote: Quote, userPublicKey: string): Promise<string> {
  let res: Response
  try {
    res = await fetch(SWAP_URL, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        quoteResponse: quote,
        userPublicKey,
        // Lets the user pay in native SOL without pre-creating a WSOL account.
        wrapAndUnwrapSol: true,
        dynamicComputeUnitLimit: true,
      }),
      signal: AbortSignal.timeout(20_000),
    })
  } catch {
    throw new SwapError('Could not reach the swap builder', 'build')
  }
  if (!res.ok) {
    const detail = await res.text().catch(() => '')
    throw new SwapError(`Swap build failed (HTTP ${res.status})${detail ? `: ${detail.slice(0, 160)}` : ''}`, 'build')
  }
  const json = (await res.json()) as { swapTransaction?: string; simulationError?: unknown }
  if (json.simulationError) {
    throw new SwapError('Jupiter simulated this swap and it failed — the route may be stale', 'build')
  }
  if (!json.swapTransaction) throw new SwapError('Swap builder returned no transaction', 'build')
  return json.swapTransaction
}

export interface SwapResult {
  signature: string
  inAmount: string
  outAmount: string
}

/**
 * Quote → build → wallet-sign → confirm. `sendTransaction` comes from the wallet
 * adapter, so the signature prompt and the key both stay in the user's wallet.
 */
export async function executeSwap(
  quote: Quote,
  publicKey: PublicKey,
  connection: Connection,
  sendTransaction: (tx: VersionedTransaction, connection: Connection) => Promise<string>,
): Promise<SwapResult> {
  const b64 = await buildSwapTx(quote, publicKey.toBase58())

  let tx: VersionedTransaction
  try {
    tx = VersionedTransaction.deserialize(base64ToBytes(b64))
  } catch {
    throw new SwapError('Could not decode the swap transaction', 'build')
  }

  let signature: string
  try {
    signature = await sendTransaction(tx, connection)
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err)
    // A user closing the wallet popup is a normal outcome, not a failure to report loudly.
    throw new SwapError(
      /reject|denied|cancel/i.test(msg) ? 'Rejected in wallet' : msg,
      'sign',
    )
  }

  try {
    const bh = await connection.getLatestBlockhash()
    await connection.confirmTransaction(
      { signature, blockhash: bh.blockhash, lastValidBlockHeight: bh.lastValidBlockHeight },
      'confirmed',
    )
  } catch {
    // The transaction may still land — surface the signature so the user can check.
    throw new SwapError(`Sent but not confirmed in time — check ${signature.slice(0, 12)}…`, 'confirm')
  }

  return { signature, inAmount: quote.inAmount, outAmount: quote.outAmount }
}

/** Human-readable price impact, e.g. "0.42%". */
export function fmtPriceImpact(quote: Quote): string {
  const pct = Number(quote.priceImpactPct ?? 0) * 100
  return `${pct.toFixed(2)}%`
}
