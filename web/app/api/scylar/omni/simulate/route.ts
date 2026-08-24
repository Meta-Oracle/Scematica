// Run the omni loop for Scylar. Counterfactual by default; sealing is a separate route.
//
// `POST /api/scylar/omni/simulate` — rank branches against a goal, persist nothing.
//
// Under `/api/scylar/` rather than `/api/omni/` on purpose: `/omni` in this app is the
// record verifier and its defining property is that it has no server side at all. Running
// the loop is Scylar's capability; verification stays the reader's.

import { NextResponse } from 'next/server'

import { failureMessage, simulate } from '@/lib/scylar/omni'

export const dynamic = 'force-dynamic'

/** Model output is not trusted into an array of ids. */
function stringList(v: unknown, cap = 8): string[] {
  if (!Array.isArray(v)) return []
  return v
    .filter((x): x is string => typeof x === 'string')
    .map((s) => s.trim())
    .filter(Boolean)
    .slice(0, cap)
}

export async function POST(req: Request) {
  let body: Record<string, unknown> = {}
  try {
    body = (await req.json()) as Record<string, unknown>
  } catch {
    return NextResponse.json({ error: 'bad_json' }, { status: 400 })
  }

  const goal = typeof body.goal === 'string' ? body.goal.trim() : ''
  if (!goal) {
    return NextResponse.json(
      { error: 'no_goal', detail: 'a goal is what the branches are ranked against' },
      { status: 400 },
    )
  }

  const result = await simulate({
    goal: goal.slice(0, 400),
    // The locator is *not* taken from the request. The daemon confines paths through its
    // own `Workspace`, but there is no reason to let a chat turn choose which directory the
    // loop looks at — the deployment decides that once, when it starts the daemon.
    locator: '.',
    ground: stringList(body.ground),
    mustNot: stringList(body.must_not),
  })

  if (!result.ok) {
    // 503 rather than 500: the loop is unavailable, not broken. The distinction matters
    // because "start the daemon" is an action and "file a bug" is not.
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
    dangling_grounds: c.dangling_grounds,
    // Rendered by `scema_policy::render` in Rust — the only implementation permitted to
    // turn a Term into a string. Passed through verbatim so nothing here can decide that
    // an unmeasured term looks like `0.00`.
    rendered: c.rendered,
  })
}
