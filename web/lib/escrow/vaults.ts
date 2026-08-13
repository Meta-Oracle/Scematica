// Enumerate every Escrow Market vault, server-side.
//
// SERVER ONLY — imports lib/escrow/rpc.ts, which reads RPC_ENDPOINT. Same split as
// lib/alchem/endpoint.ts vs networks.ts.
//
// Why enumerate rather than look up. A vault PDA is derived from BOTH mints
// (`["vault", token_mint, backing_mint]`), so you cannot find "the vault for token X"
// without already knowing what backs it. The market boards have a token and no idea
// whether anything backs it — that is the entire question they exist to answer.
// `getProgramAccounts` filtered by account size inverts the problem: fetch every vault
// once, key them by token mint, and the join becomes local. One RPC call for the whole
// board instead of a guess per row.
//
// This scales because the interesting property of this dataset is that it is SMALL. A
// market with a hundred thousand tokens and forty backed ones is the honest picture, and
// forty accounts is nothing. If that ever stops being true it is a very good problem.
//
// Same rules as everywhere in /escrow: no simulation, no price, raw u64 amounts as
// strings, and `balance >= recorded` giving three verdicts rather than a boolean.

import { Connection, PublicKey } from '@solana/web3.js'

import { VAULT_LEN, decodeVault, solvency, type SolvencyVerdict, type VaultState } from './program'

/** SPL token account: mint(32) owner(32) amount(u64 @ 64). */
const TOKEN_ACCOUNT_AMOUNT_OFFSET = 64
/** SPL mint: supply(8) @ 36, decimals(u8) @ 44. Same in Token-2022's base layout. */
const MINT_DECIMALS_OFFSET = 44

export interface VaultListing {
  vault: string
  state: VaultState
  tokenBalance: string
  backingBalance: string
  tokenDecimals: number
  backingDecimals: number
  tokenVerdict: SolvencyVerdict
  backingVerdict: SolvencyVerdict
}

export interface VaultIndex {
  /** Keyed by `state.tokenMint`. A token may be backed by more than one reserve. */
  byTokenMint: Map<string, VaultListing[]>
  all: VaultListing[]
  slot: number
}

function readU64LE(data: Uint8Array, offset: number): string {
  if (data.length < offset + 8) return '0'
  const view = new DataView(data.buffer, data.byteOffset, data.byteLength)
  return view.getBigUint64(offset, true).toString()
}

/** getMultipleAccounts caps at 100 addresses per call. */
async function getManyAccounts(connection: Connection, keys: PublicKey[]) {
  const out: (Uint8Array | null)[] = []
  for (let i = 0; i < keys.length; i += 100) {
    const batch = await connection.getMultipleAccountsInfo(keys.slice(i, i + 100))
    for (const acc of batch) out.push(acc ? new Uint8Array(acc.data) : null)
  }
  return out
}

/**
 * Read every vault the program owns.
 *
 * Throws on RPC failure rather than returning an empty index — "there are no vaults" and
 * "we could not ask" are different claims, and only the caller can decide how to render
 * the second. Returning `[]` on error would render an unreadable chain as an unbacked
 * market, which is the single most misleading thing this page could do.
 */
export async function listVaults(
  connection: Connection,
  programId: PublicKey,
): Promise<VaultIndex> {
  const slot = await connection.getSlot('confirmed')

  const accounts = await connection.getProgramAccounts(programId, {
    commitment: 'confirmed',
    filters: [{ dataSize: VAULT_LEN }],
  })

  const decoded: { vault: string; state: VaultState }[] = []
  for (const { pubkey, account } of accounts) {
    const state = decodeVault(new Uint8Array(account.data))
    // decodeVault already length-checks; a null here means the bytes are not a Vault
    // despite matching the size, so it is skipped rather than guessed at.
    if (state) decoded.push({ vault: pubkey.toBase58(), state })
  }
  if (decoded.length === 0) return { byTokenMint: new Map(), all: [], slot }

  const tokenAccountKeys = decoded.flatMap(d => [
    new PublicKey(d.state.tokenVault),
    new PublicKey(d.state.backingVault),
  ])
  const mintKeys = decoded.flatMap(d => [
    new PublicKey(d.state.tokenMint),
    new PublicKey(d.state.backingMint),
  ])

  const [tokenAccounts, mints] = await Promise.all([
    getManyAccounts(connection, tokenAccountKeys),
    getManyAccounts(connection, mintKeys),
  ])

  const all: VaultListing[] = decoded.map((d, i) => {
    const tvData = tokenAccounts[i * 2]
    const bvData = tokenAccounts[i * 2 + 1]
    const tMint = mints[i * 2]
    const bMint = mints[i * 2 + 1]

    const tokenBalance = tvData ? readU64LE(tvData, TOKEN_ACCOUNT_AMOUNT_OFFSET) : '0'
    const backingBalance = bvData ? readU64LE(bvData, TOKEN_ACCOUNT_AMOUNT_OFFSET) : '0'

    return {
      vault: d.vault,
      state: d.state,
      tokenBalance,
      backingBalance,
      tokenDecimals: tMint && tMint.length > MINT_DECIMALS_OFFSET ? tMint[MINT_DECIMALS_OFFSET] : 0,
      backingDecimals: bMint && bMint.length > MINT_DECIMALS_OFFSET ? bMint[MINT_DECIMALS_OFFSET] : 0,
      tokenVerdict: solvency(d.state.totalTokenLocked, tokenBalance),
      backingVerdict: solvency(d.state.totalBackingLocked, backingBalance),
    }
  })

  const byTokenMint = new Map<string, VaultListing[]>()
  for (const v of all) {
    const list = byTokenMint.get(v.state.tokenMint)
    if (list) list.push(v)
    else byTokenMint.set(v.state.tokenMint, [v])
  }

  return { byTokenMint, all, slot }
}
