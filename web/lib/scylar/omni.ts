// Scylar's bridge to the Scematica Omni daemon. **Server-only.**
//
// Omni is a reasoning loop with a verifiable output: it perceives an environment, ranks
// competing branches under a stated preference, and either decides or refuses to — and
// seals what it did into a record somebody else can re-check. It has six surfaces (CLI,
// console, daemon, MCP, browser extension, the offline verifier at `/omni`) and not one of
// them talks. Scylar is the only conversational face on this site.
//
// That pairing is the point. Scylar's existing discipline is a *promise*: the persona says
// never invent a number, and the Ψ gate stops her answering from stale state. Omni turns
// the promise into something the operator can check without trusting her, because every
// claim she makes through this bridge can be re-derived from a sealed record she does not
// control.
//
// ## Why this is not `/api/omni`
//
// `/omni` in this app is the record verifier, and its defining property is that it has **no
// server side at all** — no route, no fetch, nothing to phone home to. A verifier that had
// to send the record somewhere would be asking the reader to trust a third party in order
// to avoid trusting one. Nothing here changes that: this lives under `/api/scylar/omni/*`,
// because running the loop is Scylar's capability and verification stays the reader's.
//
// ## Guarded like the other credential holders
//
// Same runtime guard as `lib/alchem/endpoint.ts` and `lib/scylar/provider.ts`, and for the
// same reason: Next.js only inlines `NEXT_PUBLIC_*`, so a stray client import would not leak
// the token — it would silently resolve to "no daemon configured", which is the more
// confusing bug. This makes it loud.
if (typeof window !== 'undefined') {
  throw new Error(
    'lib/scylar/omni.ts is server-only — it holds the omnid bearer token. ' +
      'Client components should call /api/scylar/omni/* instead.',
  )
}

/**
 * Where the daemon is, and the token to reach it.
 *
 * The daemon binds loopback and that is deliberately not configurable on its side, so this
 * is a same-machine call by construction. The token is 256 bits, compared in constant time
 * on the daemon, and written to `.scema/omnid.token` when it starts.
 */
export interface OmniConfig {
  baseUrl: string
  token: string
}

export function omniConfig(): OmniConfig | null {
  const baseUrl = (process.env.SCEMA_OMNID_URL ?? '').trim().replace(/\/+$/, '')
  const token = (process.env.SCEMA_OMNID_TOKEN ?? '').trim()
  if (!baseUrl || !token) return null
  return { baseUrl, token }
}

/**
 * Whether sealing is permitted.
 *
 * Mirrors the daemon's own `--allow-decide` rather than second-guessing it, and defaults
 * off. Note the daemon is the real gate: if it was started without the flag it answers 403
 * no matter what this says. This exists so the *tool is not advertised* when sealing is off
 * — a listed tool that always fails teaches a model to retry it, which is the same reasoning
 * behind `omni_decide` being absent from the Claude Code plugin's manifest.
 */
export function sealingAllowed(): boolean {
  return (process.env.SCYLAR_ALLOW_DECIDE ?? '').trim() === '1'
}

/** Why a call did not produce an answer. Distinct arms, because they mean different things. */
export type OmniFailure =
  | { kind: 'not_configured' }
  | { kind: 'unreachable'; detail: string }
  | { kind: 'refused'; status: number; detail: string }
  | { kind: 'malformed'; detail: string }

export type OmniResult<T> = { ok: true; data: T } | { ok: false; error: OmniFailure }

/**
 * A cycle as the daemon reports it.
 *
 * `rendered` is the part that matters for a chat surface, and it is not a convenience. A
 * client that receives `Term { value: 0.0, measured: false }` has to decide how to print
 * it, and the failure mode — paid for twice in this repository — is that it prints `0.00`,
 * at which point an unmeasured term is indistinguishable from a measured zero and nothing
 * downstream can tell. `scema_policy::render` is the only implementation allowed to make
 * that decision, so the text comes from Rust and Scylar quotes it rather than formatting
 * numbers herself.
 */
export interface OmniRendered {
  matrix: string
  verdict: string
  next_steps: string
  evaluators: string
}

export interface OmniCycle {
  record: {
    id: string
    at: number
    runtime: string
    commitment: { root: string }
    decision: {
      chosen: string | null
      abstention: unknown
      coverage: { measured: number; total: number }
    }
  }
  persisted: boolean
  record_path: string | null
  remembered: number
  dangling_grounds: string[]
  rendered: OmniRendered
}

async function call<T>(
  path: string,
  init: { method: 'GET' | 'POST'; body?: unknown; timeoutMs?: number },
): Promise<OmniResult<T>> {
  const cfg = omniConfig()
  if (!cfg) return { ok: false, error: { kind: 'not_configured' } }

  let res: Response
  try {
    res = await fetch(`${cfg.baseUrl}${path}`, {
      method: init.method,
      headers: {
        Authorization: `Bearer ${cfg.token}`,
        Accept: 'application/json',
        ...(init.body ? { 'Content-Type': 'application/json' } : {}),
      },
      body: init.body ? JSON.stringify(init.body) : undefined,
      cache: 'no-store',
      // Observation walks a source tree. Generous, but bounded — a chat turn that hangs is
      // worse than one that says the daemon is slow.
      signal: AbortSignal.timeout(init.timeoutMs ?? 15_000),
    })
  } catch (e) {
    return { ok: false, error: { kind: 'unreachable', detail: String(e) } }
  }

  if (!res.ok) {
    // The daemon's errors are structured and worth passing through: `outside_workspace` and
    // `observe_failed` are different problems with different fixes, and collapsing them
    // into "it didn't work" throws away the operator's next action.
    let detail = `${res.status}`
    try {
      const body = (await res.json()) as { error?: string; detail?: string }
      detail = [body.error, body.detail].filter(Boolean).join(': ') || detail
    } catch {
      /* a non-JSON error body is itself the detail we have */
    }
    return { ok: false, error: { kind: 'refused', status: res.status, detail } }
  }

  try {
    return { ok: true, data: (await res.json()) as T }
  } catch (e) {
    return { ok: false, error: { kind: 'malformed', detail: String(e) } }
  }
}

/** Perceive a path. Resolved through the daemon's `Workspace`, which answers *where*. */
export function observe(locator: string): Promise<OmniResult<unknown>> {
  return call('/observe', { method: 'POST', body: { locator } })
}

export interface CycleArgs {
  goal: string
  locator?: string
  ground?: string[]
  mustNot?: string[]
}

/** Rank branches against a goal. Persists nothing, on the daemon's side as well as here. */
export function simulate(args: CycleArgs): Promise<OmniResult<OmniCycle>> {
  return call('/simulate', {
    method: 'POST',
    body: {
      locator: args.locator ?? '.',
      goal: args.goal,
      ground: args.ground ?? [],
      must_not: args.mustNot ?? [],
    },
  })
}

/**
 * Rank branches and **seal a record**.
 *
 * The only call in this file that writes anything. It computes exactly what `simulate`
 * computes — the difference is entirely that it leaves a trace, which is why it is a
 * separate function behind a separate flag rather than a boolean argument. The same
 * reasoning puts `enter` and `D` on different keys in the console.
 */
export function decide(args: CycleArgs): Promise<OmniResult<OmniCycle>> {
  return call('/decide', {
    method: 'POST',
    body: {
      locator: args.locator ?? '.',
      goal: args.goal,
      ground: args.ground ?? [],
      must_not: args.mustNot ?? [],
    },
  })
}

/** Sealed records, newest first. */
export function decisions(): Promise<OmniResult<unknown>> {
  return call('/decisions', { method: 'GET', timeoutMs: 5_000 })
}

/** One record, by id. The id is validated by the caller before it reaches a path. */
export function decision(id: string): Promise<OmniResult<unknown>> {
  return call(`/decisions/${encodeURIComponent(id)}`, { method: 'GET', timeoutMs: 5_000 })
}

/** Recompute a record's commitment and report what moved. */
export function verifyRecord(id: string): Promise<OmniResult<unknown>> {
  return call(`/decisions/${encodeURIComponent(id)}/verify`, {
    method: 'GET',
    timeoutMs: 5_000,
  })
}

/** The λ weights, the gates, the registered observers and specialists. */
export function policy(): Promise<OmniResult<unknown>> {
  return call('/policy', { method: 'GET', timeoutMs: 5_000 })
}

/**
 * A record id, or null.
 *
 * Validated against a pattern **before** it is built into a path, exactly as the browser
 * extension does for `GET /decisions/{id}` — so a `../` never reaches the daemon's router.
 * Doing it after the path is assembled is how that check gets quietly bypassed later.
 */
export function validRecordId(raw: unknown): string | null {
  if (typeof raw !== 'string') return null
  const id = raw.trim().toLowerCase()
  return /^[0-9a-f]{4,64}$/.test(id) ? id : null
}

/** A human-readable reason, for the model and for the operator behind it. */
export function failureMessage(e: OmniFailure): string {
  switch (e.kind) {
    case 'not_configured':
      return (
        'The omni daemon is not configured for this deployment, so there is no loop to run. ' +
        'It needs SCEMA_OMNID_URL and SCEMA_OMNID_TOKEN, from `scema daemon --allow <path>`.'
      )
    case 'unreachable':
      return 'The omni daemon is not answering — it may not be running. Start it with `scema daemon --allow <path>`.'
    case 'refused':
      return `The omni daemon refused that (${e.status}): ${e.detail}`
    case 'malformed':
      return `The omni daemon returned something unreadable: ${e.detail}`
  }
}

/**
 * What the model is told about the loop, and about how to report what it says.
 *
 * This is the last layer of omni's design, and the only one with no type system under it.
 * Every mechanism below it exists to stop ignorance being laundered into a number —
 * `Provenance` before value, `Term` before score, `Applicability` before opinion — and all
 * of it is undone by one sentence of prose that reports an unmeasured term as a zero. No
 * config file can prevent that. Only this can, and only if it is phrased as a rule rather
 * than a description: the Scylar context badge was ignored entirely until it was written as
 * a required output token instead of an explanation.
 *
 * Five rules, each a failure this repository has paid for at least once.
 */
export function OMNI_INSTRUCTION(names: string[], sealing: boolean): string {
  const lines = [
    'You have the Scematica Omni reasoning loop available: ' + names.join(', ') + '.',
    '',
    'Omni observes an environment, ranks competing courses of action under stated weights,',
    'and either decides or refuses to — then seals what it did into a record anyone can',
    're-check without trusting you. Use it when the operator asks what to do, what to',
    'prioritise, or whether something is worth doing. It reasons about the codebase, not',
    'the live market, and it works whether or not the sniper is running.',
    '',
    'When you report anything it returns, five rules hold, and they are not stylistic:',
    '',
    '1. AN EM DASH IS NOT A ZERO. "—" in a rendered matrix means the term was not measured.',
    '   It does not mean zero, "no gain", or "we checked and found nothing". Never write',
    '   0.00 in its place, and never describe an unmeasured term as if it had a value.',
    '   A measured 0.00 is a real observation and is printed as 0.00 — the difference',
    '   between those two is the entire point of the system.',
    '',
    '2. COVERAGE NEVER LEAVES THE SCORE IT QUALIFIES. If you quote a utility, quote how many',
    '   terms were measured alongside it. A utility of 0.91 over two terms out of nine is a',
    '   statement about ignorance and has to read like one.',
    '',
    '3. ABSTENTION IS AN ANSWER, AND WHICH ONE IS THE ACTIONABLE PART. If it declined, say',
    '   so plainly and say why — no candidates, all forbidden, no positive utility, too',
    '   little measured, or contested by a specialist. Those are five different situations',
    '   with five different next steps. "It could not decide" throws that away. Relay the',
    '   next_steps text; it names the command that would help.',
    '',
    '4. GROUNDING IS ASSERTED, NEVER INFERRED. A goal is only grounded in a signal if the',
    '   OPERATOR said it is. Never pass a --ground id they did not name, and never tell them',
    '   a goal is supported by evidence when it is not. If the loop suggests ids, offer them',
    '   as a question. An instruction is not evidence — that rule is why the ranking can be',
    '   trusted at all, and you are the layer most likely to break it out of helpfulness.',
    '',
    '5. A VERIFIED RECORD PROVES ONE THING. It proves the record was not edited after it was',
    '   sealed. It does NOT prove the world was as described, and it does NOT prove this is',
    '   the original record. Say all three when it comes up. A reader who believes it proves',
    '   more than it does is worse off than one who never checked.',
    '',
    'Quote the rendered text rather than rebuilding the numbers yourself. It was formatted',
    'by the one piece of code allowed to decide how a term is displayed.',
  ]

  if (sealing) {
    lines.push(
      '',
      'You can also seal a record with omni_seal, which WRITES to disk permanently. Only do',
      'that when the operator asks you to in this turn. Simulate first and show them the',
      'ranking; sealing is not the natural end of a conversation about options. After',
      'sealing, give them the record id and tell them they can check it themselves.',
    )
  } else {
    lines.push(
      '',
      'You cannot seal records on this deployment. If asked, say so — do not describe a',
      'simulation as though it had been recorded.',
    )
  }

  return lines.join('\n')
}
