// Turning subscription payloads into `ZeroEvent`s. PURE — no socket, no fetch.
//
// Split out from `rpc.ts` so the interesting half is testable without a mainnet
// WebSocket and a newly created pool, neither of which arrives on demand. Everything
// here is a function from a JSON payload to a typed event or `null`.
//
// The rule that shapes every function below: **a field that is missing produces an
// absent Term or a null event, never a zero.** A vault balance that did not decode is
// not a vault with nothing in it, and a price computed from it would be a number Zero
// would then trade on.

import { type ObservedPool, type ZeroEvent, absent, measured } from '../types.ts'

/** Raydium AMM V4. The program whose `initialize2` marks a new pool. */
export const RAYDIUM_AMM_V4 = '675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8'
/** Alias used by the action layer, which addresses it as a program id. */
export const RAYDIUM_AMM_V4_PROGRAM = RAYDIUM_AMM_V4
export const WSOL_MINT = 'So11111111111111111111111111111111111111112'

/** `logsSubscribe` notification, narrowed to what matters. */
export interface LogsNotification {
  signature: string
  err: unknown | null
  logs: string[]
}

/**
 * Is this log batch a new-pool event?
 *
 * Deliberately narrow. A false positive costs an RPC round trip to fetch a transaction
 * that turns out to be a swap, and at Raydium's volume that is a rate limit rather than
 * an inconvenience. `initialize2` is the instruction name Raydium logs on pool creation.
 *
 * A failed transaction is NOT a pool. The `err` check is not decoration: a reverted
 * `initialize2` logs exactly the same line as a successful one.
 */
export function isNewPool(n: LogsNotification): boolean {
  if (n.err !== null && n.err !== undefined) return false
  return n.logs.some(l => l.includes('initialize2'))
}

/** SPL token account layout: mint at 0, owner at 32, amount (u64 LE) at 64. */
export const TOKEN_ACCOUNT_AMOUNT_OFFSET = 64

/**
 * Decode a token account's balance.
 *
 * Returns `null` — never 0 — for anything that does not decode. A 165-byte floor rather
 * than an equality: Token-2022 accounts carry extensions and are longer, and rejecting
 * them would make every Token-2022 position unpriceable.
 */
export function decodeTokenAmount(data: Uint8Array): bigint | null {
  if (data.length < TOKEN_ACCOUNT_AMOUNT_OFFSET + 8) return null
  const view = new DataView(data.buffer, data.byteOffset, data.byteLength)
  return view.getBigUint64(TOKEN_ACCOUNT_AMOUNT_OFFSET, true)
}

/**
 * Price of the base token in SOL, from the two vault balances.
 *
 * `null` when either side is missing or the base vault is empty. An empty base vault
 * makes the price infinite, not enormous — and an infinite price arriving at
 * `evaluateExit` looks like a take-profit.
 */
export function priceFromVaults(quoteLamports: bigint | null, baseUnits: bigint | null): number | null {
  if (quoteLamports === null || baseUnits === null) return null
  if (baseUnits === 0n) return null
  const p = Number(quoteLamports) / Number(baseUnits)
  return Number.isFinite(p) && p > 0 ? p : null
}

/**
 * Build a `vault.changed` event, or nothing.
 *
 * Returning `null` rather than an event with a zero price is the whole contract here:
 * the engine treats every `vault.changed` as a real observation and evaluates exits
 * against it.
 */
export function vaultEvent(
  mint: string,
  quoteData: Uint8Array | null,
  baseData: Uint8Array | null,
  slot: number,
  atUnix: number,
): ZeroEvent | null {
  const price = priceFromVaults(
    quoteData ? decodeTokenAmount(quoteData) : null,
    baseData ? decodeTokenAmount(baseData) : null,
  )
  if (price === null) return null
  return { kind: 'vault.changed', mint, priceSol: price, slot, atUnix }
}

/** What a discovery source can tell Zero about a pool, before any chain read. */
export interface RawPool {
  mint: string
  symbol?: string
  dev?: string
  createdAtUnix?: number
  sizeSol?: number
  ageSecs?: number
  holderCount?: number
  mintRenounced?: boolean
  freezeDisabled?: boolean
  devHoldingPct?: number
  lpBurned?: boolean
  buyPressure?: number
}

/**
 * Lift a raw pool into an `ObservedPool`.
 *
 * Every optional field becomes an absent `Term` carrying WHY, rather than a default.
 * This is the single most important function in the host: it is the boundary where "the
 * feed did not tell us" would otherwise silently become "the value is zero", and the
 * policy cannot tell those apart once they are both a number.
 */
export function observePool(raw: RawPool, source: string): ObservedPool {
  const t = (v: number | undefined, what: string) =>
    v === undefined || !Number.isFinite(v) ? absent(`${what} not provided by ${source}`) : measured(v)
  const b = (v: boolean | undefined, what: string) =>
    v === undefined ? absent(`${what} not provided by ${source}`) : measured(v ? 1 : 0)

  return {
    mint: raw.mint,
    symbol: raw.symbol ?? raw.mint.slice(0, 4),
    dev: raw.dev ?? '',
    createdAtUnix: raw.createdAtUnix ?? 0,
    sizeSol: t(raw.sizeSol, 'pool depth'),
    ageSecs: t(raw.ageSecs, 'pool age'),
    holderCount: t(raw.holderCount, 'holder count'),
    mintRenounced: b(raw.mintRenounced, 'mint authority'),
    freezeDisabled: b(raw.freezeDisabled, 'freeze authority'),
    devHoldingPct: t(raw.devHoldingPct, 'deployer holding'),
    buyPressure: t(raw.buyPressure, 'buy pressure'),
    lpBurned: b(raw.lpBurned, 'LP burn'),
  }
}

// ── Raydium AMM V4 layout ────────────────────────────────────────────────────

const pubkeyAt = (data: Uint8Array, offset: number): Uint8Array => data.slice(offset, offset + 32)

/**
 * Raydium AMM V4 `LIQUIDITY_STATE_LAYOUT_V4` offsets.
 *
 * These are byte positions in a 752-byte account and they are the reason this file has a
 * test: reading `baseVault` from the wrong offset yields a valid-looking pubkey that is
 * some other account entirely, and Zero would then subscribe to it and price every exit
 * against a balance belonging to a different pool. A wrong offset does not throw.
 */
export const AMM_V4 = {
  SIZE: 752,
  BASE_VAULT: 336,
  QUOTE_VAULT: 368,
  BASE_MINT: 400,
  QUOTE_MINT: 432,
} as const

/**
 * Pull the two vaults out of a pool account, oriented so `quoteVault` is the SOL side.
 *
 * Raydium does not guarantee which leg is which — a pool may be created with SOL as base
 * or as quote — so the orientation is READ rather than assumed. Assuming it inverts the
 * price on half of all pools, and an inverted price is not obviously wrong on a chart: it
 * just makes every exit rule fire backwards.
 */
export function vaultsFromPool(
  data: Uint8Array,
  wsolMintBytes: Uint8Array,
): { baseVault: Uint8Array; quoteVault: Uint8Array; tokenMint: Uint8Array } | null {
  if (data.length < AMM_V4.SIZE) return null

  const baseMint = pubkeyAt(data, AMM_V4.BASE_MINT)
  const quoteMint = pubkeyAt(data, AMM_V4.QUOTE_MINT)
  const baseVault = pubkeyAt(data, AMM_V4.BASE_VAULT)
  const quoteVault = pubkeyAt(data, AMM_V4.QUOTE_VAULT)

  const same = (a: Uint8Array, b: Uint8Array) => a.every((v, i) => v === b[i])

  if (same(quoteMint, wsolMintBytes)) {
    return { baseVault, quoteVault, tokenMint: baseMint }
  }
  if (same(baseMint, wsolMintBytes)) {
    // SOL is the base leg, so the roles invert: the SOL vault is `baseVault`.
    return { baseVault: quoteVault, quoteVault: baseVault, tokenMint: quoteMint }
  }
  // Neither leg is SOL. Zero prices everything in SOL, so it cannot watch this pool —
  // and saying so is better than picking a leg and producing a price in an unknown unit.
  return null
}

// ── base58, with no dependency ───────────────────────────────────────────────

const B58 = '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz'

/** Minimal base58 decode — enough for a 32-byte mint, with no dependency. */
export function base58ToBytes(s: string): Uint8Array {
  let num = 0n
  for (const c of s) {
    const i = B58.indexOf(c)
    if (i < 0) throw new Error('not base58')
    num = num * 58n + BigInt(i)
  }
  const bytes: number[] = []
  while (num > 0n) {
    bytes.unshift(Number(num % 256n))
    num /= 256n
  }
  for (const c of s) {
    if (c !== '1') break
    bytes.unshift(0)
  }
  return Uint8Array.from(bytes)
}

/** Minimal base58 encode, the inverse of the above. */
export function bytesToBase58(bytes: Uint8Array): string {
  let num = 0n
  for (const b of bytes) num = num * 256n + BigInt(b)
  let out = ''
  while (num > 0n) {
    out = B58[Number(num % 58n)] + out
    num /= 58n
  }
  for (const b of bytes) {
    if (b !== 0) break
    out = '1' + out
  }
  return out
}
