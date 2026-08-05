// Minimal ABI decode for the three aggregator calls this page makes.
//
// A port of `alchem-link/src/alchem_link/abi.py`, kept deliberately literal so the two
// can be diffed by eye. Python stays authoritative: `tests/test_abi.py` pins the decoded
// fixtures against a live mainnet aggregator, and `lib/alchem/__tests__` mirrors those
// same fixtures here.
//
// No ethers/viem. Reading a Chainlink feed needs four function selectors and the ability
// to decode five static words plus one dynamic string — that is this file, versus a
// megabyte of wallet plumbing for the same result. The bundle stays server-side anyway.

export const WORD = 32

const UINT256_CEILING = 1n << 256n
const INT256_SIGN_BIT = 1n << 255n

// keccak256(signature)[:4] — verified live against 0x5f4eC3Df…19 (ETH/USD, mainnet).
// Stored as constants rather than computed: keccak-256 is not SHA3-256 (the padding
// differs), so deriving these would mean shipping a hash implementation to save nothing.
export const SELECTOR_LATEST_ROUND_DATA = '0xfeaf968c' // latestRoundData()
export const SELECTOR_DECIMALS = '0x313ce567' // decimals()
export const SELECTOR_DESCRIPTION = '0x7284e416' // description()
export const SELECTOR_VERSION = '0x54fd4d50' // version()

export class AbiError extends Error {
  constructor(message: string) {
    super(message)
    this.name = 'AbiError'
  }
}

export function strip0x(value: string): string {
  return value.startsWith('0x') || value.startsWith('0X') ? value.slice(2) : value
}

export function toBytes(hex: string): Uint8Array {
  const raw = strip0x(hex)
  if (raw.length % 2) throw new AbiError(`odd-length hex payload (${raw.length} chars)`)
  if (raw.length && !/^[0-9a-fA-F]+$/.test(raw)) {
    throw new AbiError('payload is not valid hex')
  }
  const out = new Uint8Array(raw.length / 2)
  for (let i = 0; i < out.length; i++) {
    out[i] = parseInt(raw.slice(i * 2, i * 2 + 2), 16)
  }
  return out
}

/** Split an ABI payload into 32-byte words. */
export function words(hex: string): Uint8Array[] {
  const raw = toBytes(hex)
  if (raw.length % WORD) {
    throw new AbiError(`payload is not a whole number of 32-byte words (${raw.length} bytes)`)
  }
  const out: Uint8Array[] = []
  for (let i = 0; i < raw.length; i += WORD) out.push(raw.slice(i, i + WORD))
  return out
}

export function toUint(word: Uint8Array): bigint {
  let value = 0n
  for (const byte of word) value = (value << 8n) | BigInt(byte)
  return value
}

/** Decode a two's-complement int256. Chainlink answers are signed and can go negative. */
export function toInt(word: Uint8Array): bigint {
  const value = toUint(word)
  return value >= INT256_SIGN_BIT ? value - UINT256_CEILING : value
}

/** Decode a single dynamic `string` return value (offset, length, utf-8 bytes). */
export function decodeString(hex: string): string {
  const raw = toBytes(hex)
  if (raw.length < WORD) throw new AbiError('string payload too short to hold an offset')

  const offset = Number(toUint(raw.slice(0, WORD)))
  if (offset + WORD > raw.length) {
    throw new AbiError('string offset points past the end of the payload')
  }
  const length = Number(toUint(raw.slice(offset, offset + WORD)))
  const start = offset + WORD
  if (start + length > raw.length) {
    throw new AbiError('string length runs past the end of the payload')
  }
  return new TextDecoder('utf-8').decode(raw.slice(start, start + length))
}

/**
 * Convert a raw feed answer into a human number.
 *
 * Matches Python's `answer / 10 ** decimals`: both widen an arbitrary-precision integer
 * to a double, so both lose the same low bits on absurdly large answers. Feed answers at
 * 8 decimals sit many orders of magnitude inside double precision, so this is exact in
 * practice — but it is a *port* of the Python behaviour, not an improvement on it.
 */
export function scale(answer: bigint, decimals: number): number {
  if (decimals < 0) throw new AbiError(`negative decimals: ${decimals}`)
  return Number(answer) / 10 ** decimals
}
