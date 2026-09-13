/**
 * xAI client: two endpoints, two jobs.
 *
 *   /v1/chat/completions  - ordinary generation. OpenAI-compatible.
 *   /v1/responses         - generation *with server-side tools*, including
 *                           x_search. This is the agent's sense organ: xAI
 *                           runs the X search on their side and Grok reads
 *                           the results before answering.
 *
 * The second endpoint is the whole reason this agent uses Grok rather than
 * any other model. Nothing else can read live X discourse as part of
 * inference; without it the agent would be posting into a room it cannot hear.
 *
 * Response parsing is deliberately forgiving. The Responses API returns a
 * heterogeneous `output` array whose exact citation placement is not pinned
 * down in public docs, and an agent that throws on an unfamiliar item shape
 * is worse than one that extracts what it recognises. `parseResponsesOutput`
 * is therefore tested against several plausible shapes rather than one.
 */
import { config } from '../../config.js';

export interface GrokMessage {
  role: 'system' | 'user' | 'assistant';
  content: string;
}

export interface ChatOptions {
  model?: string;
  temperature?: number;
  maxTokens?: number;
  stopSequences?: string[];
  signal?: AbortSignal;
}

export interface LiveSearchOptions extends ChatOptions {
  /** Server-side tools to enable. x_search is the one that matters here. */
  tools?: Array<'x_search' | 'web_search' | 'code_interpreter'>;
}

/** One thing the agent noticed in the live stream. */
export interface SearchCitation {
  url: string;
  title?: string;
  snippet?: string;
}

export interface LiveSearchResult {
  text: string;
  citations: SearchCitation[];
  /** Raw payload, kept so callers can mine fields this parser does not know. */
  raw: unknown;
}

export class GrokError extends Error {
  constructor(
    message: string,
    readonly status?: number,
    readonly body?: string,
  ) {
    super(message);
    this.name = 'GrokError';
  }

  /** Retrying a 401 or a 400 just burns time; a 429 or 5xx is worth another go. */
  get retryable(): boolean {
    if (this.status === undefined) return true; // network-level failure
    return this.status === 429 || this.status >= 500;
  }
}

const RETRYABLE_ATTEMPTS = 3;

async function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

export class GrokClient {
  constructor(
    private readonly apiKey: string = config.xai.apiKey,
    private readonly baseUrl: string = config.xai.baseUrl,
    private readonly fetchImpl: typeof fetch = fetch,
  ) {}

  get configured(): boolean {
    return this.apiKey.length > 0;
  }

  private async request(path: string, body: unknown, signal?: AbortSignal): Promise<unknown> {
    if (!this.configured) {
      throw new GrokError('xAI API key is not configured (set XAI_API_KEY)');
    }

    let lastError: GrokError | undefined;
    for (let attempt = 1; attempt <= RETRYABLE_ATTEMPTS; attempt += 1) {
      // Per-attempt timeout, combined with any caller-supplied cancellation.
      const timeout = AbortSignal.timeout(config.xai.timeoutMs);
      const composite = signal ? AbortSignal.any([signal, timeout]) : timeout;

      try {
        const response = await this.fetchImpl(`${this.baseUrl}${path}`, {
          method: 'POST',
          headers: {
            'Content-Type': 'application/json',
            Authorization: `Bearer ${this.apiKey}`,
          },
          body: JSON.stringify(body),
          signal: composite,
        });

        if (!response.ok) {
          const text = await response.text().catch(() => '');
          throw new GrokError(
            `xAI ${path} failed: ${response.status} ${response.statusText}`,
            response.status,
            text.slice(0, 500),
          );
        }
        return await response.json();
      } catch (error) {
        const grokError =
          error instanceof GrokError
            ? error
            : new GrokError(
                `xAI ${path} request failed: ${(error as Error).message}`,
                undefined,
                undefined,
              );

        // A caller-initiated abort is not a failure to retry around.
        if (signal?.aborted) throw grokError;
        if (!grokError.retryable || attempt === RETRYABLE_ATTEMPTS) throw grokError;

        lastError = grokError;
        await sleep(2 ** (attempt - 1) * 1000);
      }
    }
    throw lastError ?? new GrokError(`xAI ${path} failed`);
  }

  /** Plain text generation. */
  async chat(messages: GrokMessage[], options: ChatOptions = {}): Promise<string> {
    const payload = {
      model: options.model ?? config.xai.model,
      messages,
      temperature: options.temperature ?? config.xai.temperature,
      max_tokens: options.maxTokens ?? config.xai.maxTokens,
      ...(options.stopSequences?.length ? { stop: options.stopSequences } : {}),
    };
    const json = (await this.request('/chat/completions', payload, options.signal)) as {
      choices?: Array<{ message?: { content?: string } }>;
    };
    const content = json.choices?.[0]?.message?.content;
    if (typeof content !== 'string') {
      throw new GrokError('xAI returned no message content');
    }
    return content;
  }

  /**
   * Generation with live X/web search running server-side.
   *
   * This is what lets the agent answer "what is actually being said about X
   * right now" instead of "what did my training data contain".
   */
  async liveSearch(prompt: string, options: LiveSearchOptions = {}): Promise<LiveSearchResult> {
    const tools = (options.tools ?? ['x_search']).map((type) => ({ type }));
    const payload = {
      model: options.model ?? config.xai.model,
      input: [{ role: 'user', content: prompt }],
      tools,
      // Streaming would complicate parsing for no benefit: the sense loop
      // needs the whole answer before it can rank anything.
      stream: false,
      ...(options.maxTokens ? { max_output_tokens: options.maxTokens } : {}),
    };
    const raw = await this.request('/responses', payload, options.signal);
    const { text, citations } = parseResponsesOutput(raw);
    return { text, citations, raw };
  }
}

/**
 * Extract text and citations from a Responses API payload.
 *
 * Exported for testing. Tolerant by design: it walks the structure looking for
 * recognisable shapes rather than asserting one, because getting a slightly
 * unfamiliar payload should cost us citations, not the whole cycle.
 */
export function parseResponsesOutput(raw: unknown): { text: string; citations: SearchCitation[] } {
  const texts: string[] = [];
  const citations: SearchCitation[] = [];
  const seenUrls = new Set<string>();

  const addCitation = (value: unknown): void => {
    if (typeof value === 'string') {
      if (value.startsWith('http') && !seenUrls.has(value)) {
        seenUrls.add(value);
        citations.push({ url: value });
      }
      return;
    }
    if (!value || typeof value !== 'object') return;
    const record = value as Record<string, unknown>;
    const url = record.url ?? record.link ?? record.source;
    if (typeof url !== 'string' || seenUrls.has(url)) return;
    seenUrls.add(url);
    const citation: SearchCitation = { url };
    if (typeof record.title === 'string') citation.title = record.title;
    const snippet = record.snippet ?? record.text ?? record.description;
    if (typeof snippet === 'string') citation.snippet = snippet;
    citations.push(citation);
  };

  const visit = (node: unknown, depth = 0): void => {
    if (depth > 8 || node === null || node === undefined) return;

    if (Array.isArray(node)) {
      for (const item of node) visit(item, depth + 1);
      return;
    }
    if (typeof node !== 'object') return;

    const record = node as Record<string, unknown>;

    // Citations appear under several plausible keys depending on the shape.
    for (const key of ['citations', 'sources', 'search_results', 'references']) {
      const value = record[key];
      if (Array.isArray(value)) for (const entry of value) addCitation(entry);
    }

    // Assistant text: either an output_text item, or content blocks.
    if (record.type === 'output_text' && typeof record.text === 'string') {
      texts.push(record.text);
    } else if (typeof record.text === 'string' && record.type === undefined) {
      texts.push(record.text);
    }

    // Chat-completions-shaped fallback.
    const message = record.message as Record<string, unknown> | undefined;
    if (message && typeof message.content === 'string') texts.push(message.content);

    for (const value of Object.values(record)) visit(value, depth + 1);
  };

  // Prefer the convenience field when the API provides it.
  if (raw && typeof raw === 'object') {
    const top = raw as Record<string, unknown>;
    if (typeof top.output_text === 'string' && top.output_text.trim()) {
      texts.push(top.output_text);
    }
  }
  visit(raw);

  // De-duplicate while preserving order: the walk can reach the same text
  // through both a convenience field and the structure it summarises.
  const unique: string[] = [];
  for (const text of texts) {
    const trimmed = text.trim();
    if (trimmed && !unique.includes(trimmed)) unique.push(trimmed);
  }

  return { text: unique.join('\n\n'), citations };
}

let shared: GrokClient | undefined;

export function getGrokClient(): GrokClient {
  if (!shared) shared = new GrokClient();
  return shared;
}
