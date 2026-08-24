// Read-only tools Scylar can call against the operator's bot.
//
// The briefing in `context.ts` is a fixed five-endpoint block: enough to answer "how is
// it doing", useless for "why did we skip that mint". These are the rest of the API,
// exposed as tools the model asks for by name when a question actually needs them —
// which is also cheaper, because a question about Kelly sizing no longer drags a pool
// radar through the prompt.
//
// The security model is one sentence: **the model picks a tool name, never a URL.** Each
// entry hard-codes its path, so there is no argument a model (or a user talking to it)
// can set that reaches an endpoint not on this list. That rules out the whole class of
// "make the server fetch this for me" attacks in a way that validating a caller-supplied
// path never quite does — the same reasoning as `lib/alchem/endpoint.ts` refusing an
// `--rpc-url` equivalent.
//
// Everything here is a GET or a compute-and-return POST, with exactly one exception:
// `omni_seal`, which writes a decision record. No control routes, and nothing that writes a
// file the sniper reads. She can explain what the bot did and she can record what she
// concluded; she cannot make the bot do anything.
//
// That exception is narrow on purpose. A sealed record is the only write in this system
// whose output can be checked *without trusting the writer* — six SHA-256 digests and a
// root that anybody can re-derive offline. If she seals something wrong, the wrongness is
// inspectable. A control route has no such property: it has a side effect on money and the
// operator's only recourse is the transcript. `omni_seal` is also not advertised at all
// unless the deployment enables it, because a listed tool that always fails teaches a model
// to retry it.

import { codexMap, lookup } from './codex.ts'

/** Hard cap on a serialised tool result. Beyond this the model just gets less useful. */
const MAX_RESULT_CHARS = 6_000

/** Rows returned to the model, regardless of what the caller asks for. */
const MAX_ROWS = 40

/**
 * Which subsystem a tool reaches, and therefore what has to be up for it to work.
 *
 * `bot` tools read the sniper's state files through this site's API. `omni` tools run the
 * reasoning loop against a source tree via the omni daemon. Keeping them apart matters
 * because their availability is genuinely independent: the omni loop does not care whether
 * the sniper is running, and the Ψ gate's HOLD is a statement about stale *bot* state that
 * says nothing about a repository.
 *
 * Before this split, every tool was gated on the bot being reachable, which would have
 * switched off reasoning about the codebase whenever the trading process was stopped — the
 * exact moment an operator is most likely to be asking what to do about it.
 */
export type ToolGroup = 'bot' | 'omni' | 'codex'

export interface ToolSpec {
  name: string
  group: ToolGroup
  description: string
  /** OpenAI-style JSON schema for the arguments. */
  parameters: Record<string, unknown>
  /** Fixed path under `/api`. Never derived from model output. Absent for a local tool. */
  path?: string
  /**
   * HTTP method. `POST` is allowed only for endpoints that compute and return —
   * `/api/replay` reads two files and writes nothing. No tool may reach a control route;
   * `check-scylar.mjs` asserts that, because "read-only" is a property of the *list*,
   * not of the verb.
   */
  method?: 'GET' | 'POST'
  /** Query string built from validated arguments. */
  query?: (args: Record<string, unknown>) => Record<string, string>
  /** JSON body built from validated arguments, for POST tools. */
  body?: (args: Record<string, unknown>) => Record<string, unknown>
  /** Trim a raw payload down to the fields worth spending tokens on. */
  shape?: (data: unknown) => unknown
  /**
   * Answer in-process instead of over HTTP.
   *
   * Only the codex uses this. Its data is static and already in the bundle, so routing it
   * through `fetch(origin + path)` would spend a network round trip re-reading a constant —
   * and would give the codex a URL, which is the one thing this file is built to keep away
   * from model output. A local tool has no `path` at all, so there is nothing to guess.
   */
  local?: (args: Record<string, unknown>) => string
}

/** A finite number, or `undefined` so the field is omitted rather than sent as null. */
function numOrUndef(v: unknown): number | undefined {
  const n = Number(v)
  return Number.isFinite(n) ? n : undefined
}

/** Clamp a model-supplied count into range. Models routinely ask for 500. */
function rows(args: Record<string, unknown>, fallback: number): number {
  const n = Number(args?.limit ?? args?.lines ?? fallback)
  if (!Number.isFinite(n)) return fallback
  return Math.max(1, Math.min(MAX_ROWS, Math.floor(n)))
}

const limitParam = (desc: string) => ({
  type: 'object',
  properties: { limit: { type: 'integer', description: desc } },
})

function pick<T extends object>(row: unknown, keys: (keyof T | string)[]): unknown {
  if (!row || typeof row !== 'object') return row
  const src = row as Record<string, unknown>
  const out: Record<string, unknown> = {}
  for (const k of keys) if (src[k as string] !== undefined) out[k as string] = src[k as string]
  return out
}

const listOf = (key: string, keys: string[]) => (data: unknown) => {
  const arr = Array.isArray(data) ? data : (data as Record<string, unknown>)?.[key]
  if (!Array.isArray(arr)) return data
  return arr.slice(-MAX_ROWS).map((r) => pick(r, keys))
}

// ── the codex tools ──────────────────────────────────────────────────────────────
//
// These reach nothing. `lib/scylar/codex.ts` is static data compiled into the bundle, so
// the tool answers in-process — no path, no fetch, no URL for a model to aim at. They are
// available on every deployment, including one with no bot and no daemon, which is the
// point: "what is the coherence breaker" is a question about the repository, and it does
// not stop being answerable because the trading process is stopped.
const CODEX_TOOLS: ToolSpec[] = [
  {
    name: 'explain_project',
    group: 'codex',
    description:
      'Look up a part of the Scematica project — any crate, web product, on-chain program, ' +
      'contract or cross-cutting subsystem, including all of Scematica Omni. Returns the ' +
      'summary, the invariants that are easy to get wrong, the commands that drive it, and ' +
      'the ids of neighbouring areas. ' +
      'Call this BEFORE answering any question about how a part of this project works, ' +
      'rather than describing it from the name. The entries were written from the ' +
      'repository and every path in them is checked to exist. ' +
      'If it reports no entry, say the codex does not cover that — do not fill the gap.',
    parameters: {
      type: 'object',
      properties: {
        topic: {
          type: 'string',
          description:
            'An area id (e.g. "scema-daemon", "coherence-breaker", "omni-verify") or a ' +
            'free-text topic (e.g. "how does verification work").',
        },
      },
      required: ['topic'],
    },
    local: (a) => {
      const topic = typeof a.topic === 'string' ? a.topic.trim() : ''
      if (!topic) return `A topic is needed. Areas that exist:
${codexMap()}`
      // Bounded so a model asking for a whole kind cannot pull the entire codex into one
      // result; three entries is enough to answer a comparison and short enough to leave
      // room for the answer itself.
      return lookup(topic, 3)
    },
  },
  {
    name: 'list_project_areas',
    group: 'codex',
    description:
      'List every area of the Scematica project the codex covers, grouped by kind. Use when ' +
      'the operator asks what the project contains, or when you are not sure which id to ' +
      'pass to explain_project.',
    parameters: { type: 'object', properties: {} },
    local: () => codexMap(),
  },
]

export const TOOLS: ToolSpec[] = [
  {
    name: 'get_pool_decisions',
    group: 'bot',
    description:
      'Per-pool accept/reject decisions from the filter pipeline, newest last. Use this ' +
      'to explain why a specific pool was skipped or taken — it carries the rejecting ' +
      'stage, the reason, the pool score and what the Deep Q* agent advised.',
    parameters: limitParam('How many recent decisions to read (max 40).'),
    path: 'decisions',
    query: (a) => ({ limit: String(rows(a, 25)) }),
    shape: listOf('decisions', [
      'timestamp', 'mint', 'decision', 'stage', 'reason', 'pool_score',
      'effective_min_score', 'pool_size_sol', 'pool_age_secs', 'buy_pressure_ratio',
      'dq_action', 'dq_confidence',
    ]),
  },
  {
    name: 'get_recent_trades',
    group: 'bot',
    description:
      'Executed buys, sells and arbs with realised PnL. Use for "what did we lose money ' +
      'on", "what was the last trade", or to reason about hold times.',
    parameters: limitParam('How many recent trades to read (max 40).'),
    path: 'trades',
    query: (a) => ({ limit: String(rows(a, 20)) }),
    shape: listOf('trades', [
      'timestamp', 'kind', 'symbol', 'mint', 'amount', 'pnl', 'pnl_pct',
      'status', 'dex', 'position_age_secs',
    ]),
  },
  {
    name: 'get_pool_radar',
    group: 'bot',
    description:
      'Pools currently being tracked, with score, size and age. Use for "what is it ' +
      'looking at right now".',
    parameters: limitParam('How many pools to read (max 40).'),
    path: 'pools',
    query: (a) => ({ limit: String(rows(a, 20)) }),
    shape: listOf('pools', ['mint', 'score', 'size_sol', 'age_secs', 'passed_filters']),
  },
  {
    name: 'get_tx_telemetry',
    group: 'bot',
    description:
      'Per-transaction execution quality: attempts, compute-unit price, elapsed time, ' +
      'and the timeout / rate-limit / slippage / blockhash error counters. Use this for ' +
      '"why are trades failing" or "why is execution slow" — not for PnL.',
    parameters: limitParam('How many transactions to read (max 40).'),
    path: 'tx-telemetry',
    query: (a) => ({ limit: String(rows(a, 20)) }),
    shape: listOf('telemetry', [
      'timestamp', 'executor', 'tx_kind', 'confirmed', 'error', 'attempts',
      'elapsed_ms', 'compute_unit_price', 'timeout_count', 'rate_limit_count',
      'slippage_error_count', 'blockhash_error_count',
    ]),
  },
  {
    name: 'get_logs',
    group: 'bot',
    description:
      'Raw tail of the sniper log. Use as a last resort when the structured endpoints ' +
      'do not explain something — it is verbose and costs the most tokens.',
    parameters: {
      type: 'object',
      properties: { lines: { type: 'integer', description: 'How many lines (max 40).' } },
    },
    path: 'logs',
    query: (a) => ({ lines: String(rows(a, 30)) }),
    shape: (data) => {
      const lines = (data as { lines?: unknown })?.lines
      return Array.isArray(lines) ? lines.slice(-MAX_ROWS) : data
    },
  },
  {
    name: 'get_nn_advice',
    group: 'bot',
    description:
      'The Deep Q* agent\'s current action, per-action Q values, top reason and ' +
      'confidence. Use for "what does the agent think right now".',
    parameters: { type: 'object', properties: {} },
    path: 'nn-advice',
  },
  {
    name: 'get_tournament',
    group: 'bot',
    description:
      'The three DQ* variants (conservative/balanced/aggressive), their total rewards, ' +
      'epsilons, and which is promoted to primary.',
    parameters: { type: 'object', properties: {} },
    path: 'tournament',
  },
  {
    name: 'run_counterfactual',
    group: 'bot',
    description:
      'Re-apply different filter thresholds to the pools the bot actually measured, and ' +
      'report what would have changed. Use for "should I loosen/tighten X", "was that ' +
      'threshold worth it", or any tuning question. ' +
      'CRITICAL when reporting the result: tightening removes pools that were really ' +
      'traded, so excluded_realised_pnl_sol is EXACT. Loosening admits pools nobody ' +
      'bought, so there is NO outcome for them and you must not estimate one — compare ' +
      'admitted_avg_* against winner_avg_* and say the return is unknown.',
    parameters: {
      type: 'object',
      properties: {
        min_pool_score: { type: 'number', description: 'Proposed minimum pool score (0-100).' },
        min_pool_size_sol: { type: 'number', description: 'Proposed minimum pool size in SOL.' },
        max_pool_age_secs: { type: 'number', description: 'Proposed maximum pool age in seconds.' },
        min_buy_pressure_ratio: { type: 'number', description: 'Proposed minimum buy-pressure ratio.' },
      },
    },
    path: 'replay',
    method: 'POST',
    body: (a) => ({
      min_pool_score: numOrUndef(a.min_pool_score),
      min_pool_size_sol: numOrUndef(a.min_pool_size_sol),
      max_pool_age_secs: numOrUndef(a.max_pool_age_secs),
      min_buy_pressure_ratio: numOrUndef(a.min_buy_pressure_ratio),
    }),
  },
  {
    name: 'get_calibration',
    group: 'bot',
    description:
      'Your own track record: how often your past calls about specific mints turned out ' +
      'right, scored against realised trade PnL. Use when asked how reliable you are, ' +
      'or before making a confident call. ' +
      'CRITICAL: bullish_accuracy covers only calls that resolved. Bearish calls are ' +
      'almost all unresolved — the bot avoided those pools, so nothing confirms you were ' +
      'right. Never present unresolved claims as a clean record.',
    parameters: { type: 'object', properties: {} },
    path: 'calibration',
  },
  {
    name: 'get_controls',
    group: 'bot',
    description:
      'Current control state: rate mode, TP/SL, multiplier, sell/dump mode, high-speed ' +
      'and moon-chase flags. Read-only — you cannot change any of these.',
    parameters: { type: 'object', properties: {} },
    path: 'controls',
  },

  // ── Scematica Omni ────────────────────────────────────────────────────────────
  //
  // The reasoning loop, as tools. These differ from everything above in kind: the tools
  // above report what the bot measured, these rank what could be done about it and produce
  // an artefact somebody else can check.
  //
  // Each description carries the rule the model gets wrong without it. That is not padding.
  // The last layer of omni's whole design is prose written by a model, and a summary that
  // reports an unmeasured term as a zero has undone the type system underneath it in one
  // sentence, with nothing downstream able to tell.
  {
    name: 'omni_simulate',
    group: 'omni',
    description:
      'Rank competing courses of action against a goal using the Scematica Omni loop, and ' +
      'return the ranking as text. Writes nothing. Use when asked what should be done, ' +
      'what to prioritise, or whether something is worth doing. ' +
      'CRITICAL when reporting the result: quote the rendered matrix and verdict as given. ' +
      'An em dash means the term was NOT MEASURED — it is not zero and not "no gain", and ' +
      'writing 0.00 in its place is the one failure this whole system exists to prevent. ' +
      'Always report the coverage (MEASURED) beside any utility you mention. ' +
      'If it abstained, say so and say WHICH of the five reasons — that is the actionable ' +
      'part. Abstention is an answer, not a failure. ' +
      'Grounding is never inferred: if next_steps suggests --ground ids, relay them as a ' +
      'suggestion for the operator to confirm, and never claim a goal is grounded yourself.',
    parameters: {
      type: 'object',
      properties: {
        goal: {
          type: 'string',
          description: 'What the operator wants, in their own words.',
        },
        ground: {
          type: 'array',
          items: { type: 'string' },
          description:
            'Signal ids the OPERATOR has said this goal addresses. Only ever pass ids the ' +
            'operator named. Never infer these from wording.',
        },
      },
      required: ['goal'],
    },
    path: 'scylar/omni/simulate',
    method: 'POST',
    body: (a) => ({
      goal: typeof a.goal === 'string' ? a.goal : '',
      ground: Array.isArray(a.ground) ? a.ground.filter((x) => typeof x === 'string') : [],
    }),
  },
  {
    name: 'omni_records',
    group: 'omni',
    description:
      'List the sealed decision records, newest first, or fetch one by id. Use when asked ' +
      'what has been decided, or to look up a specific decision.',
    parameters: {
      type: 'object',
      properties: {
        id: { type: 'string', description: 'A record id (hex). Omit to list.' },
      },
    },
    path: 'scylar/omni/records',
    query: (a): Record<string, string> =>
      typeof a.id === 'string' && a.id.trim() ? { id: a.id.trim() } : {},
  },
  {
    name: 'omni_verify',
    group: 'omni',
    description:
      'Recompute a decision record\'s commitment and report whether it still matches. ' +
      'CRITICAL when reporting: this proves the record was not edited after sealing, and ' +
      'nothing else. It does NOT prove the world was as described (provenance carries ' +
      'that), and it does NOT prove this is the original record — it is tamper-evident, ' +
      'not tamper-proof. State all three; a reader who thinks it proves more than it does ' +
      'is worse off than one who never checked.',
    parameters: {
      type: 'object',
      properties: { id: { type: 'string', description: 'The record id to verify.' } },
      required: ['id'],
    },
    path: 'scylar/omni/records',
    query: (a) => ({
      id: typeof a.id === 'string' ? a.id.trim() : '',
      verify: '1',
    }),
  },
  ...CODEX_TOOLS,
]

/**
 * The tool that *proposes* a write, kept out of `TOOLS` so it is absent rather than
 * listed-and-failing when the deployment has not enabled it.
 *
 * Same reasoning as `omni_decide` being missing from the Claude Code plugin's manifest: a
 * model that finds a tool it is allowed to see but never allowed to use learns to retry it,
 * and then to route around the refusal.
 *
 * ## Calling it does not seal anything
 *
 * This is the part worth being precise about, because the first version of it was wrong.
 * The seal route requires `confirm: true`, and the tool body set `confirm: true`
 * unconditionally — so the confirmation was satisfied by the thing asking for it, which is
 * no confirmation at all. A comment claiming "the chat layer only reaches this once the
 * operator has said yes" does not become true by being written down.
 *
 * So `runTool` intercepts this name and never calls the route. It records a *proposal* and
 * tells the model so. The write happens only when the operator activates the confirmation
 * the UI renders, which posts to `/api/scylar/omni/seal` directly. The model can ask; only
 * a human can cause.
 *
 * Same shape as the console, where `enter` simulates and `D` decides behind a confirmation:
 * the two paths compute exactly the same thing, and the only thing keeping a counterfactual
 * from becoming a decision is that they are not the same gesture.
 */
export const SEAL_TOOL: ToolSpec = {
  name: 'omni_seal',
  group: 'omni',
  description:
    'Seal a decision record: run the loop and WRITE the result to disk, permanently. ' +
    'This computes exactly what omni_simulate computes — the only difference is that it ' +
    'leaves a trace. ' +
    'Never call this unless the operator has explicitly asked you to record or seal a ' +
    'decision in this turn. Simulating first and showing them the ranking is always the ' +
    'right order. After sealing, give them the record id and tell them they can check it ' +
    'with `scema verify <id>` or at /omni, without trusting you.',
  parameters: {
    type: 'object',
    properties: {
      goal: { type: 'string', description: 'The goal to decide on.' },
      ground: {
        type: 'array',
        items: { type: 'string' },
        description: 'Signal ids the operator named. Never inferred.',
      },
    },
    required: ['goal'],
  },
  // Recorded for completeness and asserted by `check:scylar`, but never fetched from here:
  // `runTool` intercepts this tool by name. The path is where the *operator's* confirmation
  // goes.
  path: 'scylar/omni/seal',
  method: 'POST',
  body: (a) => ({
    goal: typeof a.goal === 'string' ? a.goal : '',
    ground: Array.isArray(a.ground) ? a.ground.filter((x) => typeof x === 'string') : [],
  }),
}

/** A seal the model asked for and the operator has not yet allowed. */
export interface SealProposal {
  goal: string
  ground: string[]
}

/**
 * What this deployment can currently reach.
 *
 * Every field comes from the server's own configuration or from a live probe — never from
 * the request body. A caller that could ask for the writing tool to be included would be
 * the whole gate.
 */
export interface ToolAvailability {
  /** The sniper's API answered, and the Ψ gate is not holding. */
  bot: boolean
  /** An omni daemon is configured. */
  omni: boolean
  /** Sealing is enabled here *and* on the daemon. */
  seal: boolean
  /**
   * The project codex is offered.
   *
   * Effectively always true — it is static data with no external dependency. It is a field
   * rather than a constant so that a deployment which wants a bare persona can turn it off,
   * and so `availableTools` has one shape of answer for every group rather than a special
   * case that reads as an oversight.
   */
  codex: boolean
}

/** Every tool available for this request. */
export function availableTools(a: ToolAvailability): ToolSpec[] {
  const on: Record<ToolGroup, boolean> = { bot: a.bot, omni: a.omni, codex: a.codex }
  const out = TOOLS.filter((t) => on[t.group])
  if (a.omni && a.seal) out.push(SEAL_TOOL)
  return out
}

/** OpenAI `tools` payload. */
export function toolDefinitions(a: ToolAvailability) {
  return availableTools(a).map((t) => ({
    type: 'function' as const,
    function: { name: t.name, description: t.description, parameters: t.parameters },
  }))
}

export interface ToolOutcome {
  name: string
  /** Serialised result handed back to the model. */
  content: string
  ok: boolean
  /**
   * Set when the model asked to seal a record. Nothing has been written; the UI renders a
   * confirmation and the operator decides. Carried on the outcome rather than inferred from
   * the tool name later, so the goal the model actually proposed is the one confirmed.
   */
  proposal?: SealProposal
}

/**
 * Run one tool call.
 *
 * Failures come back as a message to the model rather than as a thrown error: a tool
 * that cannot be reached is information ("the bot is not answering"), and a model told
 * that will say so, where a 500 would just abort a turn that was otherwise fine.
 */
export async function runTool(
  origin: string,
  name: string,
  rawArgs: string,
  availability: ToolAvailability,
): Promise<ToolOutcome> {
  const available = availableTools(availability)
  const spec = available.find((t) => t.name === name)
  if (!spec) {
    // Models occasionally invent a plausible-sounding tool. Naming the real ones back at
    // them recovers the turn far more often than a bare error does. A model that guessed
    // `omni_seal` while sealing is off lands here too, and is told the truth: it is not on
    // the list. Nothing hints that it exists elsewhere.
    return {
      name,
      ok: false,
      content: `No such tool. Available: ${available.map((t) => t.name).join(', ')}.`,
    }
  }

  let args: Record<string, unknown> = {}
  try {
    args = rawArgs ? (JSON.parse(rawArgs) as Record<string, unknown>) : {}
  } catch {
    args = {}
  }

  // The write path stops here. See the note on `SEAL_TOOL`: a confirmation the caller can
  // satisfy on its own behalf is not a confirmation, so this records what was asked for and
  // returns. The operator's click is what reaches the route.
  if (spec.name === SEAL_TOOL.name) {
    const goal = typeof args.goal === 'string' ? args.goal.trim() : ''
    if (!goal) {
      return { name, ok: false, content: 'A seal needs a goal. Nothing was recorded.' }
    }
    const ground = Array.isArray(args.ground)
      ? args.ground.filter((x): x is string => typeof x === 'string')
      : []
    return {
      name,
      ok: true,
      proposal: { goal: goal.slice(0, 400), ground },
      content:
        'Nothing has been sealed. A confirmation has been put in front of the operator ' +
        `for the goal "${goal}". Tell them what you are proposing to record and why, and ` +
        'that it is theirs to confirm. Do not describe the record as written, and do not ' +
        'invent an id — there is no record until they say so.',
    }
  }

  // A local tool answers from data already in this process. Handled before anything
  // network-shaped is built, so there is no path, no query string and no timeout in play —
  // and no way for a future edit to accidentally give the codex a URL.
  if (spec.local) {
    const content = spec.local(args)
    return {
      name,
      ok: true,
      content:
        content.length > MAX_RESULT_CHARS
          ? content.slice(0, MAX_RESULT_CHARS) + ' …[truncated]'
          : content,
    }
  }

  if (!spec.path) {
    // Unreachable with the current table, and asserted by `check:scylar`. Kept because the
    // alternative is a `!` that turns a future mistake into a runtime crash mid-stream.
    return { name, ok: false, content: `Tool ${name} has no path and no local handler.` }
  }
  const path = spec.path

  const qs = spec.query ? '?' + new URLSearchParams(spec.query(args)).toString() : ''
  const method = spec.method ?? 'GET'

  try {
    const res = await fetch(`${origin}/api/${path}${qs}`, {
      method,
      headers:
        method === 'POST'
          ? { Accept: 'application/json', 'Content-Type': 'application/json' }
          : { Accept: 'application/json' },
      // The body is built here from validated arguments — the model never supplies raw
      // JSON that reaches an endpoint.
      body: method === 'POST' ? JSON.stringify(spec.body ? spec.body(args) : {}) : undefined,
      cache: 'no-store',
      // Replay reads thousands of decision rows; the read tools answer from one file.
      // Omni observes a source tree before it ranks anything, which is a different order
      // of work from reading a state file.
      signal: AbortSignal.timeout(
        path.startsWith('scylar/omni/') ? 20_000 : method === 'POST' ? 8_000 : 4_000,
      ),
    })
    if (!res.ok) {
      return { name, ok: false, content: `The bot API returned ${res.status} for ${path}.` }
    }

    const raw: unknown = await res.json()
    const shaped = spec.shape ? spec.shape(raw) : raw
    const json = JSON.stringify(shaped)

    return {
      name,
      ok: true,
      content:
        json.length > MAX_RESULT_CHARS
          ? json.slice(0, MAX_RESULT_CHARS) + ' …[truncated]'
          : json,
    }
  } catch {
    return {
      name,
      ok: false,
      content: `Could not reach the bot API for ${path} — it may be stopped.`,
    }
  }
}
