/**
 * Control-plane plugin: the operator's authority over the agent, as a service.
 *
 * Runs the Telegram long-poll loop that receives approve/reject/edit, and
 * exposes the queue to the agent itself so the operator can ask about it in
 * plain language ("what's waiting?") instead of only pressing buttons.
 */
import {
  logger,
  Service,
  type Action,
  type ActionResult,
  type HandlerCallback,
  type IAgentRuntime,
  type Memory,
  type Plugin,
  type Provider,
  type ProviderResult,
  type State,
} from '@elizaos/core';

import { config } from '../../config.js';
import { readBotState, renderBotState } from '../../lib/bot-state.js';
import { getQueue } from '../../lib/queue.js';
import { getCortexClient } from '../cortex/client.js';
import { ControlPlanePoller } from './notify.js';

export class ControlPlaneService extends Service {
  static override serviceType = 'scema-control-plane';

  override capabilityDescription =
    'Receives operator approvals, rejections and edits from Telegram, and turns each decision ' +
    'into a training label for the cortex.';

  private poller: ControlPlanePoller | undefined;

  static override async start(runtime: IAgentRuntime): Promise<Service> {
    const service = new ControlPlaneService(runtime);
    service.poller = new ControlPlanePoller();
    await service.poller.start();
    return service;
  }

  override async stop(): Promise<void> {
    this.poller?.stop();
    this.poller = undefined;
  }
}

/** Lets the operator ask about the queue conversationally. */
export const queueStatusAction: Action = {
  name: 'QUEUE_STATUS',
  similes: ['PENDING_DRAFTS', 'WHATS_QUEUED', 'SHOW_QUEUE', 'AGENT_STATUS'],
  description:
    'Report what drafts are awaiting approval and how the learned cortex is doing. Use when ' +
    'asked about the queue, pending posts, or the agent own state.',

  validate: async (): Promise<boolean> => true,

  handler: async (
    _runtime: IAgentRuntime,
    _message: Memory,
    _state?: State,
    _options?: unknown,
    callback?: HandlerCallback,
  ): Promise<ActionResult> => {
    const queue = getQueue();
    const [summary, pending, stats] = await Promise.all([
      queue.summary(),
      queue.pending(),
      getCortexClient().stats(),
    ]);

    const lines = [
      `Queue: ${summary.pending} pending, ${summary.posted} posted, ${summary.rejected} rejected, ${summary.expired} expired.`,
    ];
    if (pending.length) {
      lines.push('Waiting on you:');
      for (const proposal of pending.slice(0, 3)) {
        lines.push(`  [${proposal.id}] (taste ${proposal.scores.taste.toFixed(2)}) ${proposal.text}`);
      }
    }
    lines.push(
      stats
        ? `Cortex: ${stats.training.total_events} labels, ${stats.training.total_steps} steps, ${stats.memory.count} memories.`
        : 'Cortex: unreachable.',
    );
    lines.push(config.twitter.dryRun ? 'Mode: DRY RUN, nothing reaches X.' : 'Mode: LIVE.');

    const text = lines.join('\n');
    await callback?.({ text, actions: ['QUEUE_STATUS'] });
    return { success: true, text, data: { summary, pendingCount: pending.length } };
  },

  examples: [
    [
      { name: '{{user}}', content: { text: 'anything waiting for me to approve?' } },
      { name: '{{agent}}', content: { text: 'Checking the queue.', actions: ['QUEUE_STATUS'] } },
    ],
  ],
};

/** Keeps the agent aware of its own operational state in every conversation. */
export const operationalStateProvider: Provider = {
  name: 'SCEMA_STATE',
  description: 'The agent current posting mode and pending-approval count.',
  position: 10,

  get: async (): Promise<ProviderResult> => {
    const summary = await getQueue().summary();
    const mode = config.twitter.dryRun ? 'dry-run (nothing is actually posted)' : 'live';
    return {
      text: `# Your operational state\nPosting mode: ${mode}. Drafts awaiting operator approval: ${summary.pending}.`,
      values: { dryRun: config.twitter.dryRun, pendingDrafts: summary.pending },
      data: { summary },
    };
  },
};

/**
 * Puts the live bot's own numbers in front of the model, or says plainly that
 * there are none.
 *
 * This is deliberately a *provider* rather than an action. An action fires when
 * the model decides to call it, and the failure being designed against is the
 * model not realising it needs to look — it answers "how's the bot doing?" from
 * the conversation, confidently, with a figure nobody measured. A provider runs
 * on every turn, so the refusal to guess is in context before the question is.
 */
export const botStateProvider: Provider = {
  name: 'SCEMA_BOT_STATE',
  description:
    "The live sniper's measured state, with the age of every figure — or an explicit " +
    'statement that it cannot be read.',
  position: 5,

  get: async (): Promise<ProviderResult> => {
    const state = await readBotState();
    return {
      text: renderBotState(state),
      values: {
        botWired: state.dir !== null,
        botMetricsFreshness: state.metrics.freshness,
      },
      data: { state },
    };
  },
};

export const controlPlanePlugin: Plugin = {
  name: 'control-plane',
  description:
    'Human-in-the-loop control: operator approves, edits or rejects every draft, and each ' +
    'decision trains the cortex.',

  services: [ControlPlaneService],
  actions: [queueStatusAction],
  providers: [operationalStateProvider, botStateProvider],

  async init(): Promise<void> {
    if (!config.telegram.configured) {
      logger.info('control plane: no Telegram token, review drafts with `npm run queue`');
    }
  },
};

export default controlPlanePlugin;
