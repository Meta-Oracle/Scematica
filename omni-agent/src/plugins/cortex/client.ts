/**
 * Client for the Python cortex sidecar.
 *
 * Failure policy is the interesting part. The cortex is a separate process
 * that can be down, restarting, or mid-checkpoint. The agent must keep
 * working when it is -- degraded, and honest about being degraded.
 *
 *   scoring     -> neutral scores (0.5), so nothing is ranked confidently
 *   recall      -> empty, so prompts lose memory but stay valid
 *   feedback    -> buffered on disk and replayed, because a lost approve/reject
 *                  is lost *training signal*, which is the scarcest thing here
 *
 * That last one is why this class is more than a fetch wrapper: operator
 * decisions are the product. Dropping them silently would quietly hollow out
 * the entire premise.
 */
import { appendFile, mkdir, readFile, writeFile } from 'node:fs/promises';
import { dirname } from 'node:path';

import { config } from '../../config.js';

export interface ScoreItem {
  id?: string;
  text: string;
  features?: Record<string, number>;
}

export interface ScoreResult {
  id: string | null;
  salience: number;
  taste: number;
  resonance: number;
  novelty: number;
  priority: number;
}

export interface RecallHit {
  id: string;
  text: string;
  surface: string;
  kind: string;
  age_hours: number;
  score: number;
  weighted: number;
  meta: Record<string, unknown>;
}

export interface FeedbackPayload {
  text: string;
  salience?: number;
  taste?: number;
  resonance?: number;
  features?: Record<string, number>;
  source?: string;
  ref?: string;
}

export interface CortexStats {
  embedder: { backend: string; dim: number };
  model: { params: number };
  kernel: string;
  device: string;
  training: {
    total_steps: number;
    total_events: number;
    buffer_size: number;
    last_loss: number | null;
    labels: Record<string, number>;
  };
  memory: { count: number };
}

const PENDING_FEEDBACK_PATH = `${config.paths.data}/cortex-pending-feedback.jsonl`;

export class CortexClient {
  private available = true;
  private lastFailureAt = 0;
  private replayInFlight = false;

  constructor(
    private readonly baseUrl: string = config.cortex.url,
    private readonly fetchImpl: typeof fetch = fetch,
  ) {}

  get isAvailable(): boolean {
    return this.available;
  }

  private async call<T>(path: string, body?: unknown, method = 'POST'): Promise<T> {
    const response = await this.fetchImpl(`${this.baseUrl}${path}`, {
      method,
      headers: body ? { 'Content-Type': 'application/json' } : undefined,
      body: body ? JSON.stringify(body) : undefined,
      signal: AbortSignal.timeout(config.cortex.timeoutMs),
    });
    if (!response.ok) {
      const detail = await response.text().catch(() => '');
      throw new Error(`cortex ${path}: ${response.status} ${detail.slice(0, 200)}`);
    }
    const json = (await response.json()) as T;
    if (!this.available) {
      this.available = true;
      // Coming back up is the moment to flush whatever we buffered.
      void this.replayPendingFeedback();
    }
    return json;
  }

  private noteFailure(path: string, error: unknown): void {
    const wasAvailable = this.available;
    this.available = false;
    this.lastFailureAt = Date.now();
    if (wasAvailable) {
      console.warn(
        `[cortex] unreachable at ${this.baseUrl} (${(error as Error).message}). ` +
          `Running with neutral scores; start it with: npm run cortex`,
      );
    }
    void path;
  }

  async health(): Promise<{ ok: boolean; memories: number; train_steps: number } | null> {
    try {
      return await this.call('/health', undefined, 'GET');
    } catch (error) {
      this.noteFailure('/health', error);
      return null;
    }
  }

  async stats(): Promise<CortexStats | null> {
    try {
      return await this.call<CortexStats>('/stats', undefined, 'GET');
    } catch (error) {
      this.noteFailure('/stats', error);
      return null;
    }
  }

  /**
   * Rank candidates. On failure returns neutral scores in the original order,
   * so callers get a usable list and no false confidence.
   */
  async score(items: ScoreItem[], weights?: Record<string, number>): Promise<ScoreResult[]> {
    if (items.length === 0) return [];
    try {
      const json = await this.call<{ scored: ScoreResult[] }>('/score', { items, weights });
      return json.scored;
    } catch (error) {
      this.noteFailure('/score', error);
      return items.map((item) => ({
        id: item.id ?? null,
        salience: 0.5,
        taste: 0.5,
        resonance: 0,
        novelty: 0.5,
        priority: 0.5,
      }));
    }
  }

  /**
   * Teach the network. Never throws, never drops: an unreachable cortex means
   * the label goes to disk for replay.
   */
  async feedback(payload: FeedbackPayload): Promise<{ delivered: boolean; buffered: boolean }> {
    try {
      await this.call('/feedback', payload);
      return { delivered: true, buffered: false };
    } catch (error) {
      this.noteFailure('/feedback', error);
      await this.bufferFeedback(payload);
      return { delivered: false, buffered: true };
    }
  }

  private async bufferFeedback(payload: FeedbackPayload): Promise<void> {
    try {
      await mkdir(dirname(PENDING_FEEDBACK_PATH), { recursive: true });
      await appendFile(PENDING_FEEDBACK_PATH, `${JSON.stringify(payload)}\n`, 'utf8');
    } catch (error) {
      // If even this fails, say so loudly -- a silently lost label is worse
      // than a noisy log.
      console.error('[cortex] FAILED to buffer feedback; this label is lost:', error);
    }
  }

  /** Drain the on-disk buffer. Safe to call repeatedly; self-serialising. */
  async replayPendingFeedback(): Promise<number> {
    if (this.replayInFlight) return 0;
    this.replayInFlight = true;
    try {
      let contents: string;
      try {
        contents = await readFile(PENDING_FEEDBACK_PATH, 'utf8');
      } catch {
        return 0; // nothing buffered
      }

      const lines = contents.split('\n').filter((line) => line.trim());
      if (lines.length === 0) return 0;

      const unsent: string[] = [];
      let delivered = 0;
      for (const [index, line] of lines.entries()) {
        let payload: FeedbackPayload;
        try {
          payload = JSON.parse(line) as FeedbackPayload;
        } catch {
          continue; // a corrupt line should not block the rest
        }
        try {
          await this.call('/feedback', payload);
          delivered += 1;
        } catch {
          // Still down. Keep this line and everything after it.
          unsent.push(...lines.slice(index));
          break;
        }
      }

      await writeFile(
        PENDING_FEEDBACK_PATH,
        unsent.length ? `${unsent.join('\n')}\n` : '',
        'utf8',
      );
      if (delivered > 0) {
        console.log(`[cortex] replayed ${delivered} buffered feedback event(s)`);
      }
      return delivered;
    } finally {
      this.replayInFlight = false;
    }
  }

  /**
   * Embed text in the cortex's own vector space.
   *
   * Deliberately routed through the cortex rather than a second embedding
   * provider: ElizaOS memory and cortex memory must live in the same space,
   * or "what do I remember about this" gives different answers depending on
   * which subsystem asks.
   *
   * On failure this returns zero vectors, not hashed stand-ins. A zero vector
   * is cosine-neutral to everything, so retrieval degrades to "no opinion".
   * A fabricated vector would instead be confidently wrong and would poison
   * the stored index permanently.
   */
  async embed(texts: string[]): Promise<number[][]> {
    if (texts.length === 0) return [];
    try {
      const json = await this.call<{ dim: number; vectors: number[][] }>('/embed', { texts });
      if (json.dim > 0) this.embedDim = json.dim;
      return json.vectors;
    } catch (error) {
      this.noteFailure('/embed', error);
      const dim = this.embedDim ?? 384;
      console.warn(`[cortex] embedding unavailable; returning ${dim}-d zero vectors`);
      return texts.map(() => new Array<number>(dim).fill(0));
    }
  }

  /** Last known embedding width, used to shape the degraded fallback. */
  private embedDim: number | undefined;

  async remember(
    text: string,
    surface: string,
    kind = 'observation',
    meta: Record<string, unknown> = {},
  ): Promise<{ id: string; novelty: number } | null> {
    try {
      return await this.call('/remember', { text, surface, kind, meta });
    } catch (error) {
      this.noteFailure('/remember', error);
      return null;
    }
  }

  async recall(
    query: string,
    options: { k?: number; surfaces?: string[]; kinds?: string[] } = {},
  ): Promise<RecallHit[]> {
    try {
      const json = await this.call<{ hits: RecallHit[] }>('/recall', {
        query,
        k: options.k ?? 6,
        surfaces: options.surfaces ?? null,
        kinds: options.kinds ?? null,
      });
      return json.hits;
    } catch (error) {
      this.noteFailure('/recall', error);
      return [];
    }
  }

  async save(): Promise<boolean> {
    try {
      await this.call('/save', {});
      return true;
    } catch (error) {
      this.noteFailure('/save', error);
      return false;
    }
  }
}

let shared: CortexClient | undefined;

export function getCortexClient(): CortexClient {
  if (!shared) shared = new CortexClient();
  return shared;
}
