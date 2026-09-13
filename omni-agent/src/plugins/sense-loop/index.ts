/**
 * The live-sense loop: the reason this agent is different.
 *
 * Most posting agents are broadcasters. They generate on a timer from a
 * persona prompt, with no idea what the room is currently saying, and their
 * output reads that way. This one runs a perception cycle instead:
 *
 *   1. PERCEIVE   Grok's server-side x_search reads live X discourse on each
 *                 watched topic. Not a recollection -- what is being said now.
 *   2. JUDGE      The cortex scores every candidate: salience (does this
 *                 matter), taste (would the operator approve), resonance
 *                 (will it land), novelty (have we already said this).
 *   3. COMPOSE    Only the survivors become drafts, written with relevant
 *                 memory in context so the agent stays consistent with itself.
 *   4. DISPATCH   Each draft is queued for approval, or -- once the network
 *                 has earned it -- posted directly.
 *   5. REFLECT    Engagement on earlier posts is fed back as resonance labels,
 *                 closing the loop that makes step 2 better next time.
 *
 * Step 5 is what separates this from a pipeline: the loop's own output
 * becomes its training signal.
 */
import { logger, Service, type IAgentRuntime } from '@elizaos/core';

import { config } from '../../config.js';
import { getQueue, type Proposal, type ProposalScores } from '../../lib/queue.js';
import { getCortexClient, type ScoreResult } from '../cortex/client.js';
import { extractJson } from '../grok/index.js';
import { getGrokClient, type SearchCitation } from '../grok/client.js';

/** One thing worth possibly reacting to, as seen in the live stream. */
export interface Candidate {
  /** What was said. */
  text: string;
  /** Why it might matter -- Grok's read, not ours. */
  why: string;
  /** Where it came from. */
  url?: string;
  author?: string;
  likes?: number;
  reposts?: number;
  replies?: number;
  topic: string;
}

const PERCEPTION_PROMPT = `Search X for current discussion about: {{TOPIC}}

Find the most substantive posts from the last 24 hours. Ignore engagement bait,
giveaways, and pure promotion. Prefer posts that make a claim, report a result,
or disagree with something.

Return ONLY a JSON array, no prose, of at most {{LIMIT}} items:
[
  {
    "text": "the substance of what was said, in at most 300 characters",
    "why": "one sentence on why this is worth engaging with",
    "url": "link to the post if known, else empty string",
    "author": "handle if known, else empty string",
    "likes": 0,
    "reposts": 0,
    "replies": 0
  }
]`;

const DRAFT_PROMPT = `You are composing a post for X.

What is being discussed:
{{CANDIDATE}}

Why it matters:
{{WHY}}

{{MEMORY}}

Write a reply-worthy post that adds something. Rules:
- Under 270 characters. No hashtags. No emoji unless it genuinely earns its place.
- Make one concrete point. Do not summarise the discussion back at it.
- Do not open with "This is" or "Great point" or any variation.
- If you have nothing worth adding, reply with exactly: SKIP

Return ONLY the post text, or SKIP.`;

/** Turn raw candidate metadata into the feature vector the cortex expects. */
function candidateFeatures(candidate: Candidate): Record<string, number> {
  const now = new Date();
  const hour = now.getHours();
  return {
    log_likes: Math.log1p(candidate.likes ?? 0),
    log_reposts: Math.log1p(candidate.reposts ?? 0),
    log_replies: Math.log1p(candidate.replies ?? 0),
    text_len_norm: Math.min(candidate.text.length / 280, 2),
    has_link: candidate.url ? 1 : 0,
    question_mark: candidate.text.includes('?') ? 1 : 0,
    // Circadian context: the same take lands differently at 3am.
    hour_sin: Math.sin((2 * Math.PI * hour) / 24),
    hour_cos: Math.cos((2 * Math.PI * hour) / 24),
  };
}

export class SenseLoopService extends Service {
  static override serviceType = 'scema-sense-loop';

  override capabilityDescription =
    'Perceives live X discourse through Grok, ranks it with the neural cortex, and composes ' +
    'drafts for approval or autonomous posting.';

  private timer: NodeJS.Timeout | undefined;
  private running = false;
  private cycles = 0;

  static override async start(runtime: IAgentRuntime): Promise<Service> {
    const service = new SenseLoopService(runtime);
    if (!config.sense.enabled) {
      logger.info('sense loop disabled (SCEMA_SENSE_ENABLED=false)');
      return service;
    }
    if (!getGrokClient().configured) {
      logger.warn('sense loop cannot start without XAI_API_KEY -- the agent is deaf without it');
      return service;
    }

    const intervalMs = config.sense.intervalMinutes * 60_000;
    // Deliberately not firing immediately: a cold start should let the
    // runtime settle and the operator see the boot report first.
    service.timer = setInterval(() => {
      void service.runCycle().catch((error) => {
        logger.error({ error: (error as Error).message }, 'sense cycle failed');
      });
    }, intervalMs);

    logger.info(
      `sense loop armed: every ${config.sense.intervalMinutes}m across ${config.sense.topics.length} topic(s)`,
    );
    return service;
  }

  override async stop(): Promise<void> {
    if (this.timer) clearInterval(this.timer);
    this.timer = undefined;
  }

  /**
   * One full perception cycle. Public so the CLI can trigger it on demand --
   * waiting 30 minutes to find out whether your config works is miserable.
   */
  async runCycle(): Promise<{ candidates: number; drafted: number; queued: number; posted: number }> {
    if (this.running) {
      logger.warn('sense cycle already in flight; skipping this tick');
      return { candidates: 0, drafted: 0, queued: 0, posted: 0 };
    }
    this.running = true;
    this.cycles += 1;

    try {
      const cortex = getCortexClient();
      const queue = getQueue();

      // Housekeeping first: stale drafts are noise in the cockpit.
      const expired = await queue.expireOlderThan(24);
      if (expired.length) logger.info(`expired ${expired.length} stale draft(s)`);

      // --- 1. PERCEIVE -------------------------------------------------
      const candidates: Candidate[] = [];
      for (const topic of config.sense.topics) {
        try {
          candidates.push(...(await this.perceive(topic)));
        } catch (error) {
          // One dead topic must not kill the cycle.
          logger.warn({ topic, error: (error as Error).message }, 'perception failed for topic');
        }
      }
      if (candidates.length === 0) {
        logger.info('sense cycle found nothing');
        return { candidates: 0, drafted: 0, queued: 0, posted: 0 };
      }

      // --- 2. JUDGE ----------------------------------------------------
      const scored = await cortex.score(
        candidates.map((candidate, index) => ({
          id: String(index),
          text: candidate.text,
          features: candidateFeatures(candidate),
        })),
      );

      const ranked = scored
        .map((score) => ({ score, candidate: candidates[Number(score.id)]! }))
        .filter((entry) => entry.candidate !== undefined)
        .filter((entry) => entry.score.priority >= config.sense.minPriority)
        .slice(0, config.sense.maxDrafts);

      logger.info(
        `sense cycle ${this.cycles}: ${candidates.length} candidates, ${ranked.length} above priority ${config.sense.minPriority}`,
      );

      // --- 3/4. COMPOSE + DISPATCH -------------------------------------
      let drafted = 0;
      let queued = 0;
      let posted = 0;

      for (const { candidate, score } of ranked) {
        const text = await this.compose(candidate);
        if (!text) continue;
        drafted += 1;

        // Score the draft itself -- what matters for posting is whether the
        // *post* is good, not whether the thing it replies to was interesting.
        const [draftScore] = await cortex.score([
          { id: 'draft', text, features: candidateFeatures({ ...candidate, text }) },
        ]);
        const scores = this.toScores(draftScore ?? score);

        const proposal = await queue.propose({
          text,
          rationale: candidate.why,
          sources: candidate.url ? [candidate.url] : [],
          topic: candidate.topic,
          scores,
        });

        if (await this.shouldAutoPost(scores)) {
          await this.autoPost(proposal);
          posted += 1;
        } else {
          queued += 1;
          await this.notifyOperator(proposal);
        }
      }

      // --- 5. REFLECT ---------------------------------------------------
      await this.reflect();

      return { candidates: candidates.length, drafted, queued, posted };
    } finally {
      this.running = false;
    }
  }

  /** Ask Grok what X is actually saying about a topic. */
  private async perceive(topic: string): Promise<Candidate[]> {
    const prompt = PERCEPTION_PROMPT.replace('{{TOPIC}}', topic).replace(
      '{{LIMIT}}',
      String(Math.max(1, Math.floor(config.sense.maxCandidates / config.sense.topics.length))),
    );

    const result = await getGrokClient().liveSearch(prompt, { tools: ['x_search'] });
    let parsed: unknown;
    try {
      parsed = extractJson(result.text);
    } catch (error) {
      logger.warn(
        { topic, error: (error as Error).message },
        'perception returned unparseable output; skipping topic',
      );
      return [];
    }
    if (!Array.isArray(parsed)) return [];

    const fallbackUrls = result.citations.map((citation: SearchCitation) => citation.url);

    return parsed
      .filter(
        (item): item is Record<string, unknown> =>
          typeof item === 'object' && item !== null && typeof (item as any).text === 'string',
      )
      .map((item, index) => {
        const url =
          typeof item.url === 'string' && item.url.trim() ? item.url : fallbackUrls[index];
        const candidate: Candidate = {
          text: String(item.text).slice(0, 500),
          why: typeof item.why === 'string' ? item.why : 'no rationale given',
          topic,
          likes: Number(item.likes) || 0,
          reposts: Number(item.reposts) || 0,
          replies: Number(item.replies) || 0,
        };
        if (url) candidate.url = url;
        if (typeof item.author === 'string' && item.author) candidate.author = item.author;
        return candidate;
      })
      .filter((candidate) => candidate.text.trim().length > 20);
  }

  /** Write a post, with the agent's own relevant history in context. */
  private async compose(candidate: Candidate): Promise<string | null> {
    // Recall is what keeps the agent consistent across surfaces and over time:
    // it should not contradict what it argued in Telegram last week.
    const recalled = await getCortexClient().recall(candidate.text, { k: 4 });
    const memoryBlock = recalled.length
      ? `What you have said or seen before (stay consistent, do not repeat):\n${recalled
          .map((hit) => `- [${hit.surface}] ${hit.text.slice(0, 200)}`)
          .join('\n')}`
      : '';

    const prompt = DRAFT_PROMPT.replace('{{CANDIDATE}}', candidate.text)
      .replace('{{WHY}}', candidate.why)
      .replace('{{MEMORY}}', memoryBlock);

    try {
      const raw = await getGrokClient().chat([{ role: 'user', content: prompt }], {
        temperature: 0.85,
        maxTokens: 300,
      });
      const text = raw.trim().replace(/^["']|["']$/g, '');

      // Taking SKIP seriously is the point of offering it: an agent that
      // always finds something to say is an agent nobody wants to follow.
      if (!text || text === 'SKIP' || text.toUpperCase().startsWith('SKIP')) return null;
      if (text.length > 280) return text.slice(0, 277).trimEnd() + '...';
      return text;
    } catch (error) {
      logger.warn({ error: (error as Error).message }, 'draft composition failed');
      return null;
    }
  }

  private toScores(score: ScoreResult): ProposalScores {
    return {
      salience: score.salience,
      taste: score.taste,
      resonance: score.resonance,
      novelty: score.novelty,
      priority: score.priority,
    };
  }

  /**
   * Autonomy is earned, not configured.
   *
   * Three gates, all of which must pass: the operator opted in by lowering
   * the threshold below 1.0, the network has seen enough real decisions to
   * have an opinion worth acting on, and this specific draft clears the bar.
   */
  private async shouldAutoPost(scores: ProposalScores): Promise<boolean> {
    if (config.sense.autoPostTaste > 1) return false;
    if (scores.taste < config.sense.autoPostTaste) return false;

    const stats = await getCortexClient().stats();
    const events = stats?.training.total_events ?? 0;
    if (events < config.sense.autoPostMinEvents) {
      logger.info(
        `auto-post withheld: cortex has ${events} training events, needs ${config.sense.autoPostMinEvents}`,
      );
      return false;
    }
    return true;
  }

  private async autoPost(proposal: Proposal): Promise<void> {
    const queue = getQueue();
    await queue.decide(proposal.id, 'approved', 'auto');
    // Posting itself lives in the twitter plugin, which owns dry-run policy.
    const { postProposal } = await import('../twitter/post.js');
    await postProposal(proposal.id);
  }

  private async notifyOperator(proposal: Proposal): Promise<void> {
    const { notifyProposal } = await import('../control-plane/notify.js');
    await notifyProposal(proposal);
  }

  /**
   * Feed measured engagement back as resonance labels.
   *
   * Without this the resonance head never learns anything and the loop is
   * open. With it, "what actually lands" becomes a trained signal.
   */
  private async reflect(): Promise<void> {
    const queue = getQueue();
    const cortex = getCortexClient();
    const awaiting = await queue.awaitingEngagement(6);
    if (awaiting.length === 0) return;

    const { measureEngagement } = await import('../twitter/post.js');
    for (const proposal of awaiting) {
      const metrics = await measureEngagement(proposal);
      if (!metrics) continue;

      await queue.recordEngagement(
        proposal.id,
        metrics.likes,
        metrics.reposts,
        metrics.replies,
      );
      const total = metrics.likes + metrics.reposts * 2 + metrics.replies * 3;
      await cortex.feedback({
        text: proposal.editedText ?? proposal.text,
        resonance: total,
        // A post that landed is also evidence the topic was worth noticing.
        salience: total > 0 ? 1 : 0,
        source: 'reflection',
        ref: proposal.id,
      });
      logger.info(`reflected on ${proposal.id}: engagement ${total}`);
    }
  }
}

export default SenseLoopService;
