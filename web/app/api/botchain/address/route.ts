import { NextResponse, type NextRequest } from 'next/server'

import { getNetwork, isAddress } from '@/lib/botchain/networks'
import { decodeUint, encodeBalanceOf, ethCall, rpc, verifiedChainId } from '@/lib/botchain/rpc'

// GET /api/botchain/address?address=0x…&network=mainnet
//
// Native BOT balance, nonce, and ERC-20 balances for the known tokens.
//
// Read-only by construction: the only RPC methods reached from here are `eth_getBalance`,
// `eth_getTransactionCount` and `eth_call`. There is no signing path in this codebase yet
// and no route that could broadcast one.

export const dynamic = 'force-dynamic'
export const runtime = 'nodejs'

export async function GET(request: NextRequest) {
  const key = request.nextUrl.searchParams.get('network') || 'mainnet'
  const address = (request.nextUrl.searchParams.get('address') || '').trim()
  const network = getNetwork(key)

  // Validated here rather than passed through: an unchecked value would be forwarded
  // verbatim into an RPC call, and garbage in an `eth_call` target is how a clear
  // "that isn't an address" becomes an opaque node error.
  if (!isAddress(address)) {
    return NextResponse.json(
      { ok: false, error: 'Not a valid 0x-prefixed 20-byte address.' },
      { status: 400 },
    )
  }

  try {
    const verified = await verifiedChainId(key)

    const [balHex, nonceHex] = await Promise.all([
      rpc<string>(key, 'eth_getBalance', [address, 'latest']),
      rpc<string>(key, 'eth_getTransactionCount', [address, 'latest']),
    ])

    // Settled, not all-or-nothing: one token contract reverting must not blank the whole
    // lookup. A failed read is rendered as a failure row, never dropped — the same rule
    // alchem-link follows for unreadable feeds.
    const tokenResults = await Promise.allSettled(
      network.tokens.map(async (t) => {
        const raw = await ethCall(key, t.address, encodeBalanceOf(address))
        const value = decodeUint(raw)
        if (value === null) throw new Error('contract returned no data')
        return { ...t, balance: value.toString() }
      }),
    )

    const tokens = tokenResults.map((r, i) =>
      r.status === 'fulfilled'
        ? { ...r.value, ok: true as const }
        : {
            ...network.tokens[i],
            ok: false as const,
            balance: null,
            error: r.reason instanceof Error ? r.reason.message : String(r.reason),
          },
    )

    return NextResponse.json(
      {
        ok: true,
        address,
        network: { key: network.key, name: network.name, chainId: verified.result },
        source: { endpoint: verified.endpoint, kind: verified.kind },
        // Strings, not numbers: a wei balance exceeds Number.MAX_SAFE_INTEGER and JSON
        // has no bigint, so serialising as a number silently rounds someone's balance.
        nativeWei: BigInt(balHex.result).toString(),
        symbol: network.symbol,
        decimals: network.decimals,
        nonce: Number.parseInt(nonceHex.result, 16),
        tokens,
      },
      { headers: { 'Cache-Control': 'no-store' } },
    )
  } catch (err) {
    return NextResponse.json(
      { ok: false, error: err instanceof Error ? err.message : String(err) },
      { status: 502, headers: { 'Cache-Control': 'no-store' } },
    )
  }
}
