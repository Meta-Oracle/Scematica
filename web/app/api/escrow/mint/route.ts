import { NextResponse, type NextRequest } from 'next/server'
import { PublicKey } from '@solana/web3.js'

import { resolveRpc } from '@/lib/escrow/rpc'
import {
  decodeMint,
  isMintAccount,
  looksLikeAddress,
  looksLikeTokenAccount,
  programKind,
  type MintLabel,
} from '@/lib/escrow/mintinfo'

// Resolve an arbitrary mint address against the chain.
//
//   GET /api/escrow/mint?address=<base58>
//
// This is what lets the vault builder accept a pasted contract address instead of only
// tokens the market board happened to list. The board covers a hundred-odd tokens; the
// vault program covers every SPL mint that exists, and the UI should not be narrower than
// the program it drives.
//
// Server-side for the same reason /api/escrow/build is: the browser's connection falls
// back to the public cluster endpoint (NEXT_PUBLIC_RPC_ENDPOINT is deliberately unset,
// since anything there is published to every visitor) and is rate-limited to uselessness.
//
// **No simulation branch, no defaults.** Same rule as the rest of /escrow. `decimals` in
// particular is never guessed: it multiplies every amount the user types, so an assumed 6
// against a real 8 locks a hundredth of the intended reserve. Either the chain answered
// or this route reports why it could not.
//
// The four failure reasons are distinct on purpose, because they are four different
// things to do about it:
//
//   bad_address  — that is not a base58 pubkey. Fix the paste.
//   not_found    — nothing exists at that address on this cluster. Wrong chain, or a
//                  mint that was never created. NOT "a mint with no supply".
//   not_a_mint   — something is there, but it is not a mint. Usually a token account
//                  (a wallet's holding of the token) pasted instead of the mint itself,
//                  which is the single most common paste mistake, so it is named.
//   rpc_failed   — we could not read. Says nothing about the mint either way.

export const dynamic = 'force-dynamic'
export const runtime = 'nodejs'

const JUP_SEARCH = 'https://lite-api.jup.ag/tokens/v2/search?query='
const LABEL_TIMEOUT_MS = 6_000

const fail = (reason: string, detail: string, status: number) =>
  NextResponse.json({ ok: false, reason, detail }, { status, headers: { 'cache-control': 'no-store' } })

const NO_LABEL: MintLabel = { symbol: null, name: null, source: null }

/**
 * Ask a third party what this token is called.
 *
 * Cosmetic and best-effort by design: it NEVER throws and never blocks the resolution.
 * A mint with no listing anywhere is a completely valid thing to build a vault for — a
 * token launched ten seconds ago has no listing yet, and those are exactly the ones this
 * page is about. An unknown symbol renders as the truncated mint, not as a guess.
 */
async function fetchLabel(mint: string): Promise<MintLabel> {
  try {
    const res = await fetch(`${JUP_SEARCH}${encodeURIComponent(mint)}`, {
      signal: AbortSignal.timeout(LABEL_TIMEOUT_MS),
      headers: { accept: 'application/json' },
      cache: 'no-store',
    })
    if (!res.ok) return NO_LABEL
    const arr = (await res.json()) as { id?: string; symbol?: string; name?: string }[]
    if (!Array.isArray(arr)) return NO_LABEL
    // Exact id match only. `search` is a fuzzy endpoint and will happily return a
    // near-miss for an unknown mint; taking arr[0] would put a different token's name on
    // the address the user is about to lock money behind.
    const hit = arr.find(t => t?.id === mint)
    if (!hit) return NO_LABEL
    return {
      symbol: typeof hit.symbol === 'string' && hit.symbol.trim() ? hit.symbol.trim() : null,
      name: typeof hit.name === 'string' && hit.name.trim() ? hit.name.trim() : null,
      source: 'jupiter tokens/v2/search',
    }
  } catch {
    return NO_LABEL
  }
}

export async function GET(request: NextRequest) {
  const raw = request.nextUrl.searchParams.get('address')?.trim() ?? ''
  if (!raw) return fail('bad_address', 'An address query parameter is required.', 400)
  if (!looksLikeAddress(raw)) {
    return fail(
      'bad_address',
      'That is not a Solana address — expected 32-44 base58 characters (no 0, O, I or l).',
      400,
    )
  }

  let key: PublicKey
  try {
    key = new PublicKey(raw)
  } catch {
    return fail('bad_address', 'That string is the right shape but is not a valid pubkey.', 400)
  }

  const { connection, host, authenticated } = resolveRpc()

  let info: Awaited<ReturnType<typeof connection.getAccountInfo>>
  let slot: number
  try {
    ;[info, slot] = await Promise.all([
      connection.getAccountInfo(key, 'confirmed'),
      connection.getSlot('confirmed'),
    ])
  } catch (error) {
    return NextResponse.json(
      {
        ok: false,
        reason: 'rpc_failed',
        detail: `Could not read ${host}: ${error instanceof Error ? error.message : String(error)}. This says nothing about the mint — it means the read failed.`,
        rpc: { host, authenticated },
      },
      { status: 502, headers: { 'cache-control': 'no-store' } },
    )
  }

  if (!info) {
    return fail(
      'not_found',
      `No account exists at this address on the cluster ${host} is pointed at. That is different from a mint with no supply — there is nothing there at all.`,
      404,
    )
  }

  const owner = info.owner.toBase58()
  const kind = programKind(owner)
  if (!kind) {
    return fail(
      'not_a_mint',
      `That account is owned by ${owner.slice(0, 8)}…, not by a token program, so it is not a mint.`,
      422,
    )
  }

  const data = new Uint8Array(info.data)
  if (!isMintAccount(data, kind)) {
    const detail = looksLikeTokenAccount(data, kind)
      ? 'That is a token account — someone’s holding of a token — not the mint itself. Paste the mint address (the "CA"), not the account address.'
      : `That account is owned by the token program but is ${data.length} bytes, which is not a mint layout.`
    return fail('not_a_mint', detail, 422)
  }

  const decoded = decodeMint(data)
  if (!decoded) return fail('not_a_mint', 'The account is too short to decode as a mint.', 422)
  if (!decoded.initialized) {
    return fail('not_a_mint', 'That mint account exists but was never initialised.', 422)
  }

  const label = await fetchLabel(raw)

  return NextResponse.json(
    {
      ok: true,
      facts: {
        mint: raw,
        program: kind,
        programId: owner,
        hasExtensions: data.length > 82,
        slot,
        ...decoded,
      },
      label,
      rpc: { host, authenticated },
    },
    { headers: { 'cache-control': 'no-store' } },
  )
}
