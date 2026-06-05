import { type NextRequest, NextResponse } from 'next/server'

import { ScemaDexRelay, createPaidRelay } from '@/lib/scemadex'

// End-to-end reference: read a (possibly x402-gated) ScemaDEX signal for a mint,
// paying transparently with the Dexter x402 SDK when a server-side key is set.
//
//   GET /api/scemadex/signal/<mint>?kind=reputation|pool_score|advice
//
// Env:
//   SCEMADEX_RELAY_URL   the relay host (default http://localhost:8080)
//   SOLANA_PRIVATE_KEY   server-only key used to pay gated /signal/* (optional;
//                        omit to read an ungated relay)

export const dynamic = 'force-dynamic'

const RELAY_URL = process.env.SCEMADEX_RELAY_URL || 'http://localhost:8080'

export async function GET(
  request: NextRequest,
  { params }: { params: { mint: string } },
) {
  const kind = request.nextUrl.searchParams.get('kind') ?? 'reputation'
  const mint = params.mint
  const solanaPrivateKey = process.env.SOLANA_PRIVATE_KEY

  try {
    let relay: ScemaDexRelay
    if (solanaPrivateKey) {
      // Real paid path: inject the Dexter client module (resolved at runtime / in
      // the app build; the core lib stays dependency-free).
      // @ts-ignore optional dependency
      const x402 = await import('@dexterai/x402/client')
      relay = await createPaidRelay(RELAY_URL, x402, { solanaPrivateKey })
    } else {
      relay = new ScemaDexRelay(RELAY_URL)
    }

    const data =
      kind === 'pool_score'
        ? await relay.poolScore(mint)
        : kind === 'advice'
          ? await relay.advice(mint)
          : await relay.reputation(mint)

    return NextResponse.json({ ok: true, kind, mint, paid: Boolean(solanaPrivateKey), data })
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err)
    return NextResponse.json({ ok: false, kind, mint, error: message }, { status: 502 })
  }
}
