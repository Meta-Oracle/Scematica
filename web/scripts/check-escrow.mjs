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
import {
  decodePosition,
  decodeVault,
  derivePositionPda,
  formatAmount,
  isUnlocked,
  POSITION_LEN,
  solvency,
  VAULT_LEN,
} from '../lib/escrow/program.ts'
import {
  associatedTokenAddress,
  backingVaultPda,
  depositInstruction,
  extendLockInstruction,
  initializeVaultInstruction,
  positionPda,
  tokenVaultPda,
  vaultPda,
  withdrawInstruction,
} from '../lib/escrow/instructions.ts'
import { PublicKey, SystemProgram, SYSVAR_RENT_PUBKEY } from '@solana/web3.js'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'

let failed = 0
const check = (name, ok) => {
  if (!ok) failed++
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}`)
}

// A stand-in program id. Only its consistency matters here: every PDA below is derived
// from it, so a builder using a different one would fail every order pin.
const PROG = new PublicKey('A7h6khtKFJEu46By7C4hREdMQKkgvnuBCbVyusZRu4YW')

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
// InitializeVault carries one token program PER LEG, so a Token-2022 token backed by a
// legacy-SPL reserve is legal — and it is the product's central case, since new mints are
// routinely Token-2022 while wBTC/wETH/wSOL are all legacy SPL. This assertion is inverted
// from what it used to be; a single shared token_program account made SCEMA/wBTC, SCEMA/
// wETH and SCEMA/SOL all unconstructible.
check('a Token-2022 token can be backed by an SPL reserve', pairingProblem(scemaF, usdcF) === null)
check('and the reverse pairing too', pairingProblem(usdcF, scemaF) === null)
const otherT22 = facts('Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS', 'token-2022', 9)
check('same-program pairs are of course still fine', pairingProblem(scemaF, otherT22) === null)

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

console.log('\n-- position decoding -----------------------------------')

// A Position laid out exactly as programs/scematica-vault/src/lib.rs declares it: the
// pure-reserve deposit the lifecycle script leaves at nonce 1 (token=0, backing=2000000),
// with its lock extended. Built field by field so a layout change here is deliberate.
const u64le = (v) => { const b = Buffer.alloc(8); b.writeBigUInt64LE(v); return b }
const i64le = (v) => { const b = Buffer.alloc(8); b.writeBigInt64LE(v); return b }
const POSITION_DATA = Buffer.concat([
  Buffer.from([0xaa, 0xbb, 0xcc, 0xdd, 0xee, 0xff, 0x00, 0x11]),
  new PublicKey('48QFnbCwqvKwPTsraheS6qLqDpqGvJqLKPBLTMEqBGXt').toBuffer(),
  new PublicKey('BFnj2t3vUdBiccnk8URSecc88HkypE5tt9S5DMVRLuZ7').toBuffer(),
  u64le(0n),
  u64le(2000000n),
  i64le(1788872193n),
  i64le(1791464193n),
  u64le(1n),
  Buffer.from([254]),
])

check('Position::LEN matches the Rust field list', POSITION_LEN === 113 && POSITION_DATA.length === POSITION_LEN)
const pos = decodePosition(POSITION_DATA)
check('a position decodes to its recorded amounts', pos?.backingAmount === '2000000')
// A pure-reserve deposit is legal, so its token leg is a MEASURED zero and must decode as
// '0' rather than as anything absent. This is the amount somebody gets back.
check('a zero token leg is a real amount, not a missing one', pos?.tokenAmount === '0')
check('a position decodes its unlock instant', pos?.unlockUnix === '1791464193' && pos?.nonce === '1')
// i64, not u64. Read unsigned, any negative timestamp becomes ~1.8e19 and would compare
// as locked until the heat death of the universe.
const negative = Buffer.from(POSITION_DATA)
negative.writeBigInt64LE(-86400n, 8 + 32 + 32 + 8 + 8)
check('timestamps are read as signed', decodePosition(negative)?.createdUnix === '-86400')
check('a position decode against an unexpected size is refused', decodePosition(new Uint8Array(POSITION_LEN - 1)) === null)
check('the unlock comparison matches the program: now >= unlock', isUnlocked(pos, 1791464193) && !isUnlocked(pos, 1791464192))
check('the position PDA agrees with the instruction builders',
  derivePositionPda(new PublicKey(pos.vault), new PublicKey(pos.depositor), 1n, PROG)
    .equals(positionPda(PROG, new PublicKey(pos.vault), new PublicKey(pos.depositor), 1n)))

console.log('\n-- account order vs the Rust structs -------------------')

// The strongest pin in this file. Anchor matches accounts POSITIONALLY, so a field added
// or moved in lib.rs silently repoints every account after it -- which is exactly what
// happened once: token_token_program / backing_token_program were split per leg and a
// builder kept passing one, so the System program landed in a token-program slot and
// every instruction failed with 3008 InvalidProgramId against a healthy program.
//
// The expected order is therefore READ FROM lib.rs rather than restated here. A Rust
// change fails this check instead of surfacing later as a confusing constraint error --
// or, worse, as the right account in the wrong slot.
const RUST = readFileSync(
  join(dirname(fileURLToPath(import.meta.url)), '..', '..', 'programs', 'scematica-vault', 'src', 'lib.rs'),
  'utf8',
)

function rustAccountFields(struct) {
  const m = RUST.match(new RegExp('pub struct ' + struct + "<'info> \\{([\\s\\S]*?)\\n\\}"))
  if (!m) return null
  return [...m[1].matchAll(/^ {4}pub (\w+):/gm)].map((x) => x[1])
}

// Distinct, recognisable inputs, so a swapped pair cannot pass by coincidence.
const TOKEN_MINT = new PublicKey('HcsHqEJ9suf4oHJ8mb52M7AVKjhYhnTaeHgTmde7pump')
const BACKING_MINT = new PublicKey('EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v')
const WHO = new PublicKey('BFnj2t3vUdBiccnk8URSecc88HkypE5tt9S5DMVRLuZ7')
const T22 = new PublicKey('TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb')
const SPL = new PublicKey('TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA')
const V = vaultPda(PROG, TOKEN_MINT, BACKING_MINT)

// A DELIBERATELY MIXED pair -- Token-2022 token, legacy-SPL reserve. That is the product's
// central case, and the only shape in which one shared token program is distinguishable
// from two per-leg ones. Passing the same program twice would make this check vacuous.
const legs = { tokenProgram: T22, backingProgram: SPL }
const expected = {
  payer: WHO,
  depositor: WHO,
  token_mint: TOKEN_MINT,
  backing_mint: BACKING_MINT,
  vault: V,
  token_vault: tokenVaultPda(PROG, V),
  backing_vault: backingVaultPda(PROG, V),
  position: positionPda(PROG, V, WHO, 7n),
  depositor_token: associatedTokenAddress(TOKEN_MINT, WHO, T22),
  depositor_backing: associatedTokenAddress(BACKING_MINT, WHO, SPL),
  token_token_program: T22,
  backing_token_program: SPL,
  system_program: SystemProgram.programId,
  rent: SYSVAR_RENT_PUBKEY,
}

function pinOrder(label, struct, ix) {
  const fields = rustAccountFields(struct)
  if (!fields) {
    check(label + ': the Rust struct ' + struct + ' was found', false)
    return
  }
  check(label + ': key count matches ' + struct + "'s " + fields.length + ' fields', ix.keys.length === fields.length)
  const wrong = fields
    .map((f, i) => {
      const want = expected[f]
      if (!want) return f + ' (no expectation declared -- a new Rust field)'
      if (!ix.keys[i]) return f + ' (missing at slot ' + i + ')'
      return want.equals(ix.keys[i].pubkey) ? null : 'slot ' + i + ' should be ' + f
    })
    .filter(Boolean)
  check(label + ': every slot holds the account ' + struct + ' names' + (wrong.length ? ' -- ' + wrong.join(', ') : ''), wrong.length === 0)
}

const ixInit = initializeVaultInstruction({ programId: PROG, payer: WHO, tokenMint: TOKEN_MINT, backingMint: BACKING_MINT, ...legs })
const ixDep = depositInstruction({ programId: PROG, depositor: WHO, tokenMint: TOKEN_MINT, backingMint: BACKING_MINT, ...legs, nonce: 7n, tokenAmount: 1n, backingAmount: 1n, lockSecs: 604800n })
const ixWit = withdrawInstruction({ programId: PROG, depositor: WHO, tokenMint: TOKEN_MINT, backingMint: BACKING_MINT, ...legs, nonce: 7n })
const ixExt = extendLockInstruction({ programId: PROG, depositor: WHO, tokenMint: TOKEN_MINT, backingMint: BACKING_MINT, nonce: 7n, newUnlockUnix: 1n })

pinOrder('initialize_vault', 'InitializeVault', ixInit)
pinOrder('deposit', 'Deposit', ixDep)
pinOrder('withdraw', 'Withdraw', ixWit)
pinOrder('extend_lock', 'ExtendLock', ixExt)

// The per-leg split, stated as its own claim rather than left implicit in the order pin.
check('a mixed pair carries two DIFFERENT token programs', !ixDep.keys[9].pubkey.equals(ixDep.keys[10].pubkey))
// The ATA seeds include the token program, so deriving a leg with the other leg's program
// yields a valid address the depositor does not own and has no balance at.
check('each leg ATA is derived with its own program',
  ixDep.keys[7].pubkey.equals(associatedTokenAddress(TOKEN_MINT, WHO, T22))
  && ixDep.keys[8].pubkey.equals(associatedTokenAddress(BACKING_MINT, WHO, SPL))
  && !ixDep.keys[8].pubkey.equals(associatedTokenAddress(BACKING_MINT, WHO, T22)))
// Withdraw creates nothing, so it carries neither the system program nor the rent sysvar.
// Copying Deposit's tail would leave two accounts past the end of the list.
check('withdraw carries no system program and no rent sysvar',
  !ixWit.keys.some((k) => k.pubkey.equals(SystemProgram.programId) || k.pubkey.equals(SYSVAR_RENT_PUBKEY)))
// The signer pays no rent and receives nothing here, so it is not writable.
check('extend_lock signs but writes only the position', ixExt.keys[0].isSigner && !ixExt.keys[0].isWritable && ixExt.keys[1].isWritable)
// Discriminators are computed from the instruction name, so a rename in Rust cannot leave
// a stale constant here quietly calling a different handler.
check('the four instructions have four distinct discriminators',
  new Set([ixInit, ixDep, ixWit, ixExt].map((i) => i.data.subarray(0, 8).toString('hex'))).size === 4)
// Withdraw pays position.depositor and nobody else. The builder must never be able to
// aim a payout at a third party -- the receiving ATAs are derived from the signer.
check('withdraw pays the signer, not an arbitrary account',
  ixWit.keys[7].pubkey.equals(associatedTokenAddress(TOKEN_MINT, WHO, T22))
  && ixWit.keys[8].pubkey.equals(associatedTokenAddress(BACKING_MINT, WHO, SPL)))

console.log(`\n${failed === 0 ? 'ALL PASS' : `${failed} FAILED`}`)
process.exit(failed === 0 ? 0 : 1)
