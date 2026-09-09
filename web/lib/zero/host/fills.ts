// What actually arrived — read from the transaction, not from a quote.
//
// PURE. Given the `meta` block `getTransaction` returns, these functions say how much
// moved. No network, so the interesting cases are reachable in a test.
//
// ── Why the transaction and not a balance delta ──────────────────────────────
//
// The vault program credits a *measured balance delta* (read, transfer, reload, subtract)
// and that is right on chain, where nothing else can touch the account mid-instruction.
// In a browser it is not: Zero may hold several positions, a sell can land while a buy is
// confirming, and a delta taken around one signature would silently absorb the other.
//
// `preTokenBalances` / `postTokenBalances` are scoped to THIS transaction, so they
// attribute the movement to the signature that caused it. That is the difference between
// "the wallet changed by X" and "this trade produced X", and only the second can be an
// entry price.
//
// ── Why null and never zero ──────────────────────────────────────────────────
//
// Every function here returns `null` when it cannot find what it is looking for. A fill
// of zero is a real, awful outcome (a trade that produced nothing); a fill that could not
// be read is a different thing entirely, and `engine.ts` turns the second into a position
// whose entry price is `absent` — which `exits.ts` then refuses to price rather than
// selling on a percentage nobody computed.

/** One entry in `pre/postTokenBalances`. */
export interface TokenBalance {
  accountIndex: number
  mint: string
  owner?: string
  uiTokenAmount: { amount: string; decimals: number }
}

/** The `meta` block, narrowed to what a fill needs. */
export interface TxMeta {
  fee: number
  preBalances: number[]
  postBalances: number[]
  preTokenBalances?: TokenBalance[]
  postTokenBalances?: TokenBalance[]
  err?: unknown | null
}

/** `accountKeys` is strings under `json` encoding and objects under `jsonParsed`. */
export type AccountKey = string | { pubkey: string }

export const keyString = (k: AccountKey): string => (typeof k === 'string' ? k : k.pubkey)

function tokenAmount(rows: TokenBalance[] | undefined, owner: string, mint: string): bigint | null {
  const row = rows?.find(b => b.mint === mint && b.owner === owner)
  if (!row) return null
  try {
    return BigInt(row.uiTokenAmount.amount)
  } catch {
    return null
  }
}

/**
 * Tokens received by `owner` for `mint`.
 *
 * A missing PRE entry means the account did not exist before the trade, which is the
 * ordinary case for a first buy — that is a genuine zero, not an unknown, so it is
 * treated as 0 and only a missing POST entry yields `null`. Getting this backwards would
 * make every first purchase unpriceable.
 */
export function tokensReceived(meta: TxMeta, owner: string, mint: string): bigint | null {
  const post = tokenAmount(meta.postTokenBalances, owner, mint)
  if (post === null) return null
  const pre = tokenAmount(meta.preTokenBalances, owner, mint) ?? 0n
  const delta = post - pre
  return delta >= 0n ? delta : null
}

/**
 * Lamports received by `owner`, net of the transaction fee.
 *
 * Net, not gross, and deliberately: the fee is money that left the wallet on this trade,
 * so adding it back would report proceeds nobody received. The buy side pays a fee too,
 * and reporting both sides net is the only way the two percentages compose.
 *
 * `null` when the owner is not among the account keys — a transaction that did not
 * involve them cannot describe their fill.
 */
export function lamportsReceived(meta: TxMeta, keys: AccountKey[], owner: string): number | null {
  const i = keys.findIndex(k => keyString(k) === owner)
  if (i < 0) return null
  if (meta.preBalances.length <= i || meta.postBalances.length <= i) return null
  return meta.postBalances[i] - meta.preBalances[i]
}

/**
 * How much a swap produced, in the unit the caller cares about.
 *
 * A buy is measured in the token's base units; a sell in lamports. Returning `null` is a
 * first-class answer: it becomes `Position.tokensOut = absent(...)`, and from there an
 * unpriceable position that Zero surfaces rather than trades.
 */
export function fillAmount(
  meta: TxMeta,
  keys: AccountKey[],
  owner: string,
  mint: string,
  side: 'buy' | 'sell',
): number | null {
  // A reverted transaction moved nothing. Reading its balances would report the fee as a
  // fill — a small negative number that is not a trade.
  if (meta.err !== null && meta.err !== undefined) return null

  if (side === 'buy') {
    const tokens = tokensReceived(meta, owner, mint)
    if (tokens === null) return null
    // Base units past 2^53 lose precision as a JS number. Token supplies routinely exceed
    // it (a 9-decimal mint with a billion supply is 1e18), so this is refused rather than
    // rounded: a wrong `tokensOut` becomes a wrong entry price and every exit after it.
    if (tokens > BigInt(Number.MAX_SAFE_INTEGER)) return null
    return Number(tokens)
  }

  const lamports = lamportsReceived(meta, keys, owner)
  if (lamports === null) return null
  // A sell that nets negative is a trade that cost more in fees than it returned. Real,
  // and reported as it happened rather than clamped to zero.
  return lamports
}
