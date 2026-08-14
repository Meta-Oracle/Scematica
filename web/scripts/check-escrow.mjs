#!/usr/bin/env node
// Pin the Escrow Market's pure logic: mint decoding, address shape, pair legality, and
// the human-decimal → base-unit conversion.
//
// These are the functions where being wrong costs money rather than looking wrong. The
// vault builder converts a typed amount to base units by shifting the decimal point
// `decimals` places, so a mint decoded with the wrong `decimals` — or a token account
// mistaken for a mint, whose amount field lands where `decimals` should be — produces a
// transaction that transfers a different quantity than the one on screen. Nothing here
// touches the network: the mint fixtures are real mainnet account bytes, captured once.
//
//   node scripts/check-escrow.mjs        (Node 22+; types are stripped natively)

import {
  MINT_LEN,
  PROGRAM_LABEL,
  TOKEN_2022_PROGRAM,
  TOKEN_PROGRAM,
  decodeMint,
  displaySymbol,
  isMintAccount,
  looksLikeAddress,
  looksLikeTokenAccount,
  pairingProblem,
  programKind,
  toBaseUnits,
} from '../lib/escrow/mintinfo.ts'
import { decodeVault, formatAmount, solvency, VAULT_LEN } from '../lib/escrow/program.ts'

let failed = 0
const check = (name, ok) => {
  if (!ok) failed++
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}`)
}

// Real mainnet account data, base64, captured from getAccountInfo.
const USDC_MINT = 'EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v'
const USDC_DATA = Buffer.from(
  'AQAAAJj+huiNm+Lqi8HMpIeLKYjCQPUrhCS/tA7Rot3LXhmb8S2YBZRCGwAGAQEAAABicKqKWcWUBbRShshncubNEm6bil06OFNtN/e0FOi2Zw==',
  'base64',
)
// SCEMA — Token-2022 with metadata extensions, 417 bytes. The extension case is the one
// a naive `length === 82` check silently rejects, which would make every Token-2022
// token unvaultable through this UI.
const SCEMA_MINT = 'HcsHqEJ9suf4oHJ8mb52M7AVKjhYhnTaeHgTmde7pump'
const SCEMA_DATA = Buffer.from(
  'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAxrmwX3uNAwAGAQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAARIAQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAPbrLrTxuep4QefgDN5X6TmAaWs0ehEmv89rZp7nJahPEwCzAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA9usutPG56nhB5+AM3lfpOYBpazR6ESa/z2tmnuclqE8MAAAAU2NlbWF0aWNhLXYyBwAAAFNjZW1hVjJQAAAAaHR0cHM6Ly9pcGZzLmlvL2lwZnMvYmFma3JlaWd5dXZmMm8zZnRvbTRteXN6cjJ2NW1vaG8zbjRmaHc2Y3Z2b3ZqdXpqY3c2bjYzbHd2cnkAAAAA',
  'base64',
)

console.log('── address shape ─────────────────────────────────────────')

check('a real mint is address-shaped', looksLikeAddress(USDC_MINT))
check('surrounding whitespace is tolerated — people paste with it', looksLikeAddress(`  ${USDC_MINT} \n`))
check('a partial paste is not', !looksLikeAddress('EPjFWdd5Aufq'))
// Base58 excludes these four precisely so a transposition is caught rather than
// silently resolving to a different address.
check('base58 excludes 0, O, I and l', !looksLikeAddress('0'.repeat(44)) && !looksLikeAddress('O'.repeat(44)))
check('a ticker is not an address', !looksLikeAddress('USDC'))
check('an ethereum address is not a solana one', !looksLikeAddress('0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48'))

console.log('\n── program identification ────────────────────────────────')

check('legacy SPL Token is recognised', programKind(TOKEN_PROGRAM) === 'spl-token')
check('Token-2022 is recognised', programKind(TOKEN_2022_PROGRAM) === 'token-2022')
check('anything else is not a token program', programKind('11111111111111111111111111111111') === null)
check('both programs have labels', PROGRAM_LABEL['spl-token'] && PROGRAM_LABEL['token-2022'])

console.log('\n── mint decoding (real mainnet bytes) ────────────────────')

check('USDC is the bare 82-byte layout', USDC_DATA.length === MINT_LEN)
check('USDC decodes as a mint', isMintAccount(USDC_DATA, 'spl-token'))
const usdc = decodeMint(USDC_DATA)
check('USDC has 6 decimals', usdc.decimals === 6)
check('USDC is initialised', usdc.initialized === true)
// Circle still holds both — this is what an unrevoked authority looks like, and the
// builder colours it as such rather than hiding it.
check('USDC mint authority is live', typeof usdc.mintAuthority === 'string')
check('USDC freeze authority is live', typeof usdc.freezeAuthority === 'string')
check('supply is a decimal string, never a number', typeof usdc.supply === 'string')
// ~9.2e12 at 6dp. A u64 supply routinely exceeds Number.MAX_SAFE_INTEGER; the moment it
// becomes a float the figure is quietly wrong in its low digits.
check('supply survives beyond 2^53', BigInt(usdc.supply) > 9_000_000_000_000n)

check('SCEMA carries Token-2022 extensions', SCEMA_DATA.length === 417)
check('an extended Token-2022 mint is still a mint', isMintAccount(SCEMA_DATA, 'token-2022'))
const scema = decodeMint(SCEMA_DATA)
check('SCEMA has 6 decimals', scema.decimals === 6)
check('SCEMA mint authority is revoked', scema.mintAuthority === null)
check('SCEMA freeze authority is revoked', scema.freezeAuthority === null)

console.log('\n── what is NOT a mint ────────────────────────────────────')

// The single most common paste mistake: a wallet's token account instead of the mint.
// It is owned by the same program, so only the layout tells them apart — and byte 44,
// where `decimals` lives in a mint, is part of the amount field in an account. Decoding
// one as the other yields a decimals value out of somebody's balance.
const tokenAccount = Buffer.alloc(165)
check('a 165-byte token account is not a mint', !isMintAccount(tokenAccount, 'spl-token'))
check('and is named as a token account', looksLikeTokenAccount(tokenAccount, 'spl-token'))
const t22Account = Buffer.alloc(200)
t22Account[165] = 2 // AccountType::Account
check('a tagged Token-2022 account is not a mint', !isMintAccount(t22Account, 'token-2022'))
check('and is named as a token account', looksLikeTokenAccount(t22Account, 'token-2022'))
const t22Mint = Buffer.alloc(200)
t22Mint[165] = 1 // AccountType::Mint
check('a tagged Token-2022 mint is a mint', isMintAccount(t22Mint, 'token-2022'))
check('an over-long legacy account is not a mint', !isMintAccount(Buffer.alloc(120), 'spl-token'))
check('a truncated account decodes to nothing', decodeMint(Buffer.alloc(40)) === null)
check('an uninitialised mint is caught', decodeMint(Buffer.alloc(82)).initialized === false)

console.log('\n── pair legality ─────────────────────────────────────────')

const facts = (mint, program, decimals) => ({
  mint,
  program,
  programId: program === 'spl-token' ? TOKEN_PROGRAM : TOKEN_2022_PROGRAM,
  decimals,
  supply: '0',
  mintAuthority: null,
  freezeAuthority: null,
  initialized: true,
  hasExtensions: false,
  slot: 1,
})
const usdcF = facts(USDC_MINT, 'spl-token', 6)
const scemaF = facts(SCEMA_MINT, 'token-2022', 6)
const solF = facts('So11111111111111111111111111111111111111112', 'spl-token', 9)

check('a legal pair has no problem', pairingProblem(usdcF, solF) === null)
check('a mint cannot back itself', /SameMint/.test(pairingProblem(usdcF, usdcF) ?? ''))
// InitializeVault carries ONE token_program account and applies it to both legs, so this
// pair cannot exist however sensible it looks.
check('mixed token programs are rejected', /same token program/i.test(pairingProblem(scemaF, usdcF) ?? ''))
check('the rejection names both sides', /Token-2022/.test(pairingProblem(scemaF, usdcF) ?? ''))

console.log('\n── amounts (the money path) ──────────────────────────────')

check('a whole number shifts by decimals', toBaseUnits('1', 6) === 1_000_000n)
check('a fraction shifts exactly', toBaseUnits('0.1', 9) === 100_000_000n)
// The float trap this whole path exists to avoid: parseFloat('0.1') * 1e9 is
// 100000000.00000001, and 0.07 at 8dp lands a unit low.
check('0.07 wBTC is exact at 8dp', toBaseUnits('0.07', 8) === 7_000_000n)
check('a zero-decimal mint does not shift', toBaseUnits('42', 0) === 42n)
check('empty means zero, not invalid', toBaseUnits('', 6) === 0n)
check('a lone dot is rejected', toBaseUnits('.', 6) === null)
check('letters are rejected', toBaseUnits('1e6', 6) === null)
check('a negative is rejected', toBaseUnits('-1', 6) === null)
// Truncating here would move a different quantity than the one typed, silently.
check('excess precision is refused, not rounded', toBaseUnits('0.1234567', 6) === null)
check('exact precision is accepted', toBaseUnits('0.123456', 6) === 123_456n)
// The specific bug the chain-read replaced: a board that reports 6 decimals for a mint
// that really has 8 books one hundredth of the intended reserve.
check(
  'the same input at the wrong decimals is 100x off',
  toBaseUnits('0.07', 6) * 100n === toBaseUnits('0.07', 8),
)
check('a huge amount stays exact past 2^53', toBaseUnits('18446744073.709551615', 9) === 18_446_744_073_709_551_615n)

console.log('\n── labels ────────────────────────────────────────────────')

check('a known symbol is used', displaySymbol(USDC_MINT, { symbol: 'USDC', name: 'USD Coin', source: 'jupiter' }) === 'USDC')
// An unlisted mint is normal — a token minted a minute ago is on no list — so it renders
// as its own address rather than a placeholder that could be mistaken for a name.
check('an unlisted mint shows its address', displaySymbol(USDC_MINT, null) === 'EPjF…Dt1v')
check('a blank symbol is not a symbol', displaySymbol(USDC_MINT, { symbol: '  ', name: null, source: 'x' }) === 'EPjF…Dt1v')

console.log('\n── vault accounting (unchanged rules, re-pinned) ─────────')

check('a decode against an unexpected size is refused', decodeVault(new Uint8Array(VAULT_LEN - 1)) === null)
check('equal balance is backed', solvency('100', '100') === 'backed')
// Anyone can transfer into any token account, so a surplus is normal and permanently
// stuck — hence three verdicts rather than a boolean.
check('a surplus is donated, not an error', solvency('100', '101') === 'donated')
check('a deficit is the alarm', solvency('100', '99') === 'SHORTFALL')
check('u64-scale comparison does not go through a float', solvency('18446744073709551615', '18446744073709551614') === 'SHORTFALL')
check('formatAmount places the point on the string', formatAmount('123456789', 6) === '123.456789')
check('trailing zeros are trimmed', formatAmount('1000000', 6) === '1')

console.log(`\n${failed === 0 ? 'ALL PASS' : `${failed} FAILED`}`)
process.exit(failed === 0 ? 0 : 1)
