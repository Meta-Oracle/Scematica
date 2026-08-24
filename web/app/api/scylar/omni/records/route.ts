// Sealed decision records, and their commitments.
//
// `GET /api/scylar/omni/records`            — the list, newest first
// `GET /api/scylar/omni/records?id=<hex>`   — one record
// `GET /api/scylar/omni/records?id=<hex>&verify=1` — recompute its commitment
//
// Read-only. Sealing lives in `../seal`, behind its own flag, because a route that both
// reads and writes depending on a parameter is one flag away from writing by accident.

import { NextResponse } from 'next/server'

import { decision, decisions, failureMessage, validRecordId, verifyRecord } from '@/lib/scylar/omni'

export const dynamic = 'force-dynamic'

export async function GET(req: Request) {
  const url = new URL(req.url)
  const rawId = url.searchParams.get('id')
  const wantsVerify = url.searchParams.get('verify') === '1'

  // No id: the list.
  if (!rawId) {
    const result = await decisions()
    if (!result.ok) {
      const status = result.error.kind === 'refused' ? result.error.status : 503
      return NextResponse.json(
        { error: result.error.kind, detail: failureMessage(result.error) },
        { status },
      )
    }
    return NextResponse.json({ records: result.data })
  }

  // Validated against a pattern *before* it is built into a path, so a `../` never reaches
  // the daemon's router. The extension does the same thing for the same route.
  const id = validRecordId(rawId)
  if (!id) {
    return NextResponse.json(
      {
        error: 'bad_id',
        detail: 'a record id is hex, 4–64 characters — the short form `scema explain` prints',
      },
      { status: 400 },
    )
  }

  const result = wantsVerify ? await verifyRecord(id) : await decision(id)
  if (!result.ok) {
    const status = result.error.kind === 'refused' ? result.error.status : 503
    return NextResponse.json(
      { error: result.error.kind, detail: failureMessage(result.error) },
      { status },
    )
  }

  return NextResponse.json(
    wantsVerify
      ? {
          id,
          verification: result.data,
          // Said on every verification, because the single most common way to misread one
          // is to think it proves more than it does. The model is told the same thing in
          // its system prompt; this is the copy that travels with the data.
          proves:
            'The record was not edited after sealing, and names the field that moved if it was.',
          does_not_prove: [
            'that the world was really as described — provenance carries that, not the digest',
            'that this is the original record — tamper-evident, not tamper-proof, until the root is anchored somewhere the author does not control',
          ],
        }
      : { id, record: result.data },
  )
}
