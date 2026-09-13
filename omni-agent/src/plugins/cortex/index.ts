/**
 * The cortex plugin: one memory, and one set of learned opinions, shared by
 * every surface the agent speaks on.
 *
 * Two components carry the "one mind, many surfaces" property:
 *
 *   scemaMemoryProvider  injects relevant recollections into the prompt before
 *                       the agent answers, regardless of which surface asked.
 *                       A Telegram question can surface something learned from
 *                       X, because they write to the same store.
 *
 *   scemaMemoryEvaluator writes each exchange back after the fact, tagged with
 *                       the surface it happened on.
 *
 * ElizaOS has its own memory system, and this does not replace it. ElizaOS
 * memory is conversational scrollback scoped to a room; the cortex is a
 * cross-surface semantic store whose vectors also feed the network that ranks
 * and judges. Keeping both means conversations stay coherent locally while
 * judgement stays coherent globally.
 */
import {
  logger,
  type Evaluator,
  type IAgentRuntime,
  type Memory,
  type Plugin,
  type Provider,
  type ProviderResult,
  type State,
} from '@elizaos/core';

import { getCortexClient } from './client.js';

/** Map an ElizaOS room source onto a cortex surface label. */
function surfaceOf(message: Memory): string {
  const source = String(
    (message.content as Record<string, unknown> | undefined)?.source ?? '',
  ).toLowerCase();
  if (source.includes('telegram')) return 'telegram';
  if (source.includes('twitter') || source.includes('x')) return 'twitter';
  if (source.includes('cli') || source.includes('direct')) return 'cli';
  return 'system';
}

export const scemaMemoryProvider: Provider = {
  name: 'SCEMA_MEMORY',
  description:
    'Cross-surface episodic memory and learned judgement from the Scema Cortex. Supplies what ' +
    'the agent has previously said, seen or decided about the current topic, on any platform.',
  // Runs late, so recalled memory sits near the end of the prompt where it
  // carries the most weight.
  position: 50,

  get: async (_runtime: IAgentRuntime, message: Memory): Promise<ProviderResult> => {
    const query = message.content?.text?.trim();
    if (!query) return { text: '', values: {}, data: {} };

    const cortex = getCortexClient();
    const hits = await cortex.recall(query, { k: 5 });

    if (hits.length === 0) {
      return {
        text: '',
        values: { cortexAvailable: cortex.isAvailable },
        data: { recollections: [] },
      };
    }

    const lines = hits.map((hit) => {
      const age =
        hit.age_hours < 1
          ? 'just now'
          : hit.age_hours < 24
            ? `${Math.round(hit.age_hours)}h ago`
            : `${Math.round(hit.age_hours / 24)}d ago`;
      return `- (${hit.surface}, ${age}) ${hit.text.slice(0, 240)}`;
    });

    return {
      text: `# What you already know about this\n${lines.join('\n')}`,
      values: { recollectionCount: hits.length, cortexAvailable: cortex.isAvailable },
      data: { recollections: hits },
    };
  },
};

export const scemaMemoryEvaluator: Evaluator = {
  name: 'SCEMA_REMEMBER',
  description:
    'Writes each exchange into cross-surface memory so later conversations on any platform can ' +
    'draw on it.',
  // Memory should accumulate from ordinary conversation, not only when some
  // heuristic decides a turn was important.
  alwaysRun: true,

  validate: async (_runtime: IAgentRuntime, message: Memory): Promise<boolean> => {
    const text = message.content?.text?.trim() ?? '';
    // Sub-20-character turns ("ok", "thanks", "lol") are noise that would
    // crowd out real recollections and drag novelty scores down.
    return text.length >= 20;
  },

  handler: async (runtime: IAgentRuntime, message: Memory): Promise<void> => {
    const text = message.content?.text?.trim();
    if (!text) return;

    const isAgent = message.entityId === runtime.agentId;
    const result = await getCortexClient().remember(
      text.slice(0, 2000),
      surfaceOf(message),
      isAgent ? 'decision' : 'observation',
      { entityId: message.entityId, roomId: message.roomId },
    );

    if (result && result.novelty < 0.1) {
      logger.debug(`stored a near-duplicate memory (novelty ${result.novelty.toFixed(3)})`);
    }
  },

  examples: [
    {
      prompt: 'The operator explains which benchmarks they care about.',
      messages: [
        {
          name: '{{user}}',
          content: { text: 'I only trust numbers with the hardware and the batch size stated.' },
        },
      ],
      outcome: 'Stored as a cross-surface memory, retrievable when composing posts about claims.',
    },
  ],
};

export const cortexPlugin: Plugin = {
  name: 'cortex',
  description:
    'Bridges the agent to the Scema Cortex: shared episodic memory plus the learned salience, ' +
    'taste and resonance heads that rank what is worth saying.',

  providers: [scemaMemoryProvider],
  evaluators: [scemaMemoryEvaluator],

  async init(): Promise<void> {
    const cortex = getCortexClient();
    const health = await cortex.health();
    if (!health) {
      logger.warn(
        'cortex sidecar unreachable -- the agent will run with neutral judgement and no memory. ' +
          'Start it with: npm run cortex',
      );
      return;
    }
    // Anything buffered while it was down is training signal owed to the net.
    const replayed = await cortex.replayPendingFeedback();
    logger.info(
      `cortex online: ${health.memories} memories, ${health.train_steps} training steps` +
        (replayed ? `, replayed ${replayed} buffered label(s)` : ''),
    );
  },
};

export default cortexPlugin;
