import { NextResponse, type NextRequest } from 'next/server'

import { getNetwork } from '@/lib/botchain/networks'
import { rpc, verifiedChainId } from '@/lib/botchain/rpc'

// GET /api/botchain/status?network=mainnet
//
// Chain head, gas, and the pool-creation flow across every known venue.
//
// **No simulation branch** — this reads the chain or reports why it could not, the same
// rule the alchem-link routes follow. The pool-creation count in particular must never be
// softened: it is currently ~2 events in 8 days, and that number is the entire argument
// against porting the sniper yet. A dashboard that rounded it up to something encouraging
// would be lying about the one thing the page exists to tell you.

export const dynamic = 'force-dynamic'
export const runtime = 'nodejs'

/** Blocks surveyed for venue activity. ~0.67s blocks, so this is roughly 3.7 hours. */
const WINDOW_BLOCKS = 20_000

/** `eth_getLogs` chunk. Public nodes reject wider ranges. */
const CHUNK = 2_000

/** Measured block time, used to turn a count into a rate. */
const BLOCK_SECONDS = 0.67

export async function GET(request: NextRequest) {
  const key = request.nextUrl.searchParams.get('network') || 'mainnet'
  const network = getNetwork(key)

  try {
    // Verify before reporting anything as BOT Chain data.
    const verified = await verifiedChainId(key)
    const [headHex, gasHex] = await Promise.all([
      rpc<string>(key, 'eth_blockNumber'),
      rpc<string>(key, 'eth_gasPrice'),
    ])

    const head = Number.parseInt(headHex.result, 16)
    const gasWei = BigInt(gasHex.result)
    const from = Math.max(0, head - WINDOW_BLOCKS)

    // Filtered by factory address with **no topic filter**. Filtering on a guessed event
    // signature answers "does this fork emit the event I assumed" and returns zero when
    // it does not — indistinguishable from "nothing happened". These venues are V3-style
    // and emit PoolCreated, not the V2 PairCreated a first pass looked for.
    const venues = await Promise.all(
      network.venues.map(async (v) => {
        let events = 0
        let scanned = 0
        let refused = 0
        for (let start = from; start < head; start += CHUNK) {
          const end = Math.min(start + CHUNK, head)
          try {
            const logs = await rpc<unknown[]>(key, 'eth_getLogs', [
              {
                address: v.factory,
                fromBlock: '0x' + start.toString(16),
                toBlock: '0x' + end.toString(16),
              },
            ])
            events += logs.result.length
            scanned += end - start
          } catch {
            refused += 1
          }
        }
        return {
          ...v,
          events,
          blocksScanned: scanned,
          rangesRefused: refused,
          // Null rather than 0 when nothing was scanned: a rate over blocks never read
          // is not a measurement of anything.
          perDay: scanned > 0 ? (events / scanned) * (86_400 / BLOCK_SECONDS) : null,
        }
      }),
    )

    const scannedTotal = venues.reduce((n, v) => n + v.blocksScanned, 0)
    const eventsTotal = venues.reduce((n, v) => n + v.events, 0)

    return NextResponse.json(
      {
        ok: true,
        network: {
          key: network.key,
          name: network.name,
          chainId: verified.result,
          symbol: network.symbol,
          explorer: network.explorer,
          chainIdWarning: network.chainIdWarning ?? null,
        },
        source: {
          endpoint: verified.endpoint,
          kind: verified.kind,
          elapsedMs: verified.elapsedMs,
          // The UI needs this to decide whether to offer anything that writes.
          canBroadcast: verified.kind === 'node',
        },
        head,
        gasGwei: Number(gasWei) / 1e9,
        blockSeconds: BLOCK_SECONDS,
        venues,
        flow: {
          windowBlocks: WINDOW_BLOCKS,
          blocksScanned: scannedTotal,
          events: eventsTotal,
          perDay: scannedTotal > 0 ? (eventsTotal / scannedTotal) * (86_400 / BLOCK_SECONDS) : null,
        },
      },
      { headers: { 'Cache-Control': 'no-store' } },
    )
  } catch (err) {
    return NextResponse.json(
      {
        ok: false,
        error: err instanceof Error ? err.message : String(err),
        network: { key: network.key, name: network.name, chainId: network.chainId },
      },
      { status: 502, headers: { 'Cache-Control': 'no-store' } },
    )
  }
}
