import { NextResponse, type NextRequest } from 'next/server'

import { resolveRpc } from '@/lib/escrow/rpc'

// Submit a wallet-signed transaction through the keyed RPC.
//
//   POST /api/escrow/send  { transaction: <base64 signed> }
//
// The browser's connection falls back to the public cluster endpoint, which is
// rate-limited to the point of being unusable for landing a transaction. Relaying
// through the server's keyed RPC is the difference between a deposit that confirms and
// one that silently never lands.
//
// This route CANNOT tamper with what it forwards: the transaction arrives already
// signed, and altering a byte invalidates the signature. It can only submit or refuse.
//
// Preflight stays ON. Skipping it would land failing transactions on-chain and charge
// the user for them; the extra round trip is worth an error the user can read.

export const dynamic = 'force-dynamic'
export const runtime = 'nodejs'

export async function POST(request: NextRequest) {
  let raw: string
  try {
    const body = (await request.json()) as { transaction?: string }
    if (!body.transaction) throw new Error('missing')
    raw = body.transaction
  } catch {
    return NextResponse.json(
      { ok: false, reason: 'bad_request', detail: 'transaction (base64) is required' },
      { status: 400 },
    )
  }

  const { connection, host, authenticated } = resolveRpc()

  let buf: Buffer
  try {
    buf = Buffer.from(raw, 'base64')
  } catch {
    return NextResponse.json(
      { ok: false, reason: 'bad_request', detail: 'transaction is not valid base64' },
      { status: 400 },
    )
  }

  try {
    const signature = await connection.sendRawTransaction(buf, {
      skipPreflight: false,
      preflightCommitment: 'confirmed',
      maxRetries: 3,
    })
    return NextResponse.json({ ok: true, signature, rpc: { host, authenticated } })
  } catch (error) {
    // Surface the simulation logs when the cluster provides them — an Anchor error like
    // `LockOutOfRange` or `ZeroBacking` is only actionable if the user can see it.
    const detail = error instanceof Error ? error.message : String(error)
    const logs =
      typeof error === 'object' && error !== null && 'logs' in error
        ? ((error as { logs?: string[] }).logs ?? null)
        : null
    return NextResponse.json(
      { ok: false, reason: 'send_failed', detail, logs, rpc: { host, authenticated } },
      { status: 502 },
    )
  }
}
