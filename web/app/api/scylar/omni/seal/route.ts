// `POST /api/scylar/omni/seal` — run the loop and **seal a decision record**.
//
// The only route on this site through which Scylar writes anything, and the narrowest write
// available on purpose. It creates a JSON file under `.scema/decisions/` and appends
// memory. It moves no money, touches no position, and changes nothing the sniper reads.
//
// ## Why sealing is the right first action, and control routes are not
//
// Scylar has been strictly read-only: ten GETs, no control routes, "she can explain what
// the bot did; she cannot make it do anything." Relaxing that needs a reason better than
// capability, and sealing has one — it is the only write in this system whose output is
// *checkable without trusting the writer*. A record carries six SHA-256 digests and a root;
// anybody can re-derive them, offline, in a browser, with `/omni`. If she seals something
// wrong, the wrongness is inspectable rather than merely regrettable.
//
// A control route has none of that. It has a side effect on money, and the operator's only
// recourse is the transcript. That is why the omni workspace keeps `execute` unimplemented
// behind an approval model rather than shipping it, and this route does not become that
// route by adding paths to it.
//
// ## Three gates, and the daemon is the real one
//
// 1. `SCYLAR_ALLOW_DECIDE=1` here, off by default. Controls whether the tool is *advertised*
//    at all — a listed tool that always fails teaches a model to retry it.
// 2. The daemon's own `--allow-decide`. If it was started without it, this answers 403 no
//    matter what the env says. That is the gate that actually holds.
// 3. `confirm: true` in the body, which the chat layer sets only after the operator has
//    said yes. `simulate` and `decide` compute exactly the same thing and differ only in
//    whether they leave a trace, so the only thing keeping a counterfactual from becoming a
//    decision is that they are not the same gesture.

import { NextResponse } from 'next/server'

import { decide, failureMessage, sealingAllowed } from '@/lib/scylar/omni'

export const dynamic = 'force-dynamic'

function stringList(v: unknown, cap = 8): string[] {
  if (!Array.isArray(v)) return []
  return v
    .filter((x): x is string => typeof x === 'string')
    .map((s) => s.trim())
    .filter(Boolean)
    .slice(0, cap)
}

export async function POST(req: Request) {
  if (!sealingAllowed()) {
    return NextResponse.json(
      {
        error: 'sealing_disabled',
        detail:
          'Sealing is off for this deployment. It needs SCYLAR_ALLOW_DECIDE=1 here and ' +
          '`scema daemon --allow-decide` on the daemon. Simulation is unaffected.',
      },
      { status: 403 },
    )
  }

  let body: Record<string, unknown> = {}
  try {
    body = (await req.json()) as Record<string, unknown>
  } catch {
    return NextResponse.json({ error: 'bad_json' }, { status: 400 })
  }

  if (body.confirm !== true) {
    return NextResponse.json(
      {
        error: 'not_confirmed',
        detail:
          'Sealing writes a record. The operator has to confirm it, and the confirmation ' +
          'cannot come from the model that wants to seal.',
      },
      { status: 428 },
    )
  }

  const goal = typeof body.goal === 'string' ? body.goal.trim() : ''
  if (!goal) {
    return NextResponse.json({ error: 'no_goal' }, { status: 400 })
  }

  const result = await decide({
    goal: goal.slice(0, 400),
    locator: '.',
    ground: stringList(body.ground),
    mustNot: stringList(body.must_not),
  })

  if (!result.ok) {
    const status = result.error.kind === 'refused' ? result.error.status : 503
    return NextResponse.json(
      { error: result.error.kind, detail: failureMessage(result.error) },
      { status },
    )
  }

  const c = result.data
  return NextResponse.json({
    id: c.record.id,
    persisted: c.persisted,
    root: c.record.commitment.root,
    chosen: c.record.decision.chosen,
    coverage: c.record.decision.coverage,
    remembered: c.remembered,
    dangling_grounds: c.dangling_grounds,
    rendered: c.rendered,
    // The operator's recourse, handed over at the moment the write happens rather than
    // buried in documentation they would have to already know to look for.
    verify: `scema verify ${c.record.id}`,
  })
}
