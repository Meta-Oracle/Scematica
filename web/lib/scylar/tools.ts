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
// Everything here is a GET. No control routes, no POSTs, nothing that writes a file the
// sniper reads. She can explain what the bot did; she cannot make it do anything.

/** Hard cap on a serialised tool result. Beyond this the model just gets less useful. */
const MAX_RESULT_CHARS = 6_000

/** Rows returned to the model, regardless of what the caller asks for. */
const MAX_ROWS = 40

export interface ToolSpec {
  name: string
  description: string
  /** OpenAI-style JSON schema for the arguments. */
  parameters: Record<string, unknown>
  /** Fixed path under `/api`. Never derived from model output. */
  path: string
  /** Query string built from validated arguments. */
  query?: (args: Record<string, unknown>) => Record<string, string>
  /** Trim a raw payload down to the fields worth spending tokens on. */
  shape?: (data: unknown) => unknown
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

export const TOOLS: ToolSpec[] = [
  {
    name: 'get_pool_decisions',
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
    description:
      'The Deep Q* agent\'s current action, per-action Q values, top reason and ' +
      'confidence. Use for "what does the agent think right now".',
    parameters: { type: 'object', properties: {} },
    path: 'nn-advice',
  },
  {
    name: 'get_tournament',
    description:
      'The three DQ* variants (conservative/balanced/aggressive), their total rewards, ' +
      'epsilons, and which is promoted to primary.',
    parameters: { type: 'object', properties: {} },
    path: 'tournament',
  },
  {
    name: 'get_controls',
    description:
      'Current control state: rate mode, TP/SL, multiplier, sell/dump mode, high-speed ' +
      'and moon-chase flags. Read-only — you cannot change any of these.',
    parameters: { type: 'object', properties: {} },
    path: 'controls',
  },
]

/** OpenAI `tools` payload. */
export function toolDefinitions() {
  return TOOLS.map((t) => ({
    type: 'function' as const,
    function: { name: t.name, description: t.description, parameters: t.parameters },
  }))
}

export interface ToolOutcome {
  name: string
  /** Serialised result handed back to the model. */
  content: string
  ok: boolean
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
): Promise<ToolOutcome> {
  const spec = TOOLS.find((t) => t.name === name)
  if (!spec) {
    // Models occasionally invent a plausible-sounding tool. Naming the real ones back at
    // them recovers the turn far more often than a bare error does.
    return {
      name,
      ok: false,
      content: `No such tool. Available: ${TOOLS.map((t) => t.name).join(', ')}.`,
    }
  }

  let args: Record<string, unknown> = {}
  try {
    args = rawArgs ? (JSON.parse(rawArgs) as Record<string, unknown>) : {}
  } catch {
    args = {}
  }

  const qs = spec.query ? '?' + new URLSearchParams(spec.query(args)).toString() : ''

  try {
    const res = await fetch(`${origin}/api/${spec.path}${qs}`, {
      headers: { Accept: 'application/json' },
      cache: 'no-store',
      signal: AbortSignal.timeout(4_000),
    })
    if (!res.ok) {
      return { name, ok: false, content: `The bot API returned ${res.status} for ${spec.path}.` }
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
      content: `Could not reach the bot API for ${spec.path} — it may be stopped.`,
    }
  }
}
