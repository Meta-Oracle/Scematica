/**
 * Plugin wrapper for the sense loop, plus the action that lets the operator
 * trigger a perception cycle on demand ("go look now") instead of waiting for
 * the timer.
 */
import {
  logger,
  type Action,
  type ActionResult,
  type HandlerCallback,
  type IAgentRuntime,
  type Memory,
  type Plugin,
  type State,
} from '@elizaos/core';

import { config } from '../../config.js';
import { SenseLoopService } from './index.js';

export const senseNowAction: Action = {
  name: 'SENSE_NOW',
  similes: ['RUN_SENSE_CYCLE', 'CHECK_TIMELINE_NOW', 'GO_LOOK', 'SCAN_X'],
  description:
    'Run a full perception cycle immediately: read live X discourse on the watched topics, rank ' +
    'it, and draft anything worth posting. Use when the operator asks the agent to go look now.',

  validate: async (): Promise<boolean> => config.sense.enabled,

  handler: async (
    runtime: IAgentRuntime,
    _message: Memory,
    _state?: State,
    _options?: unknown,
    callback?: HandlerCallback,
  ): Promise<ActionResult> => {
    const service = runtime.getService(SenseLoopService.serviceType) as SenseLoopService | null;
    if (!service) {
      return { success: false, text: 'The sense loop is not running.', error: 'service missing' };
    }

    await callback?.({ text: 'Reading the timeline now. This takes a moment.' });

    try {
      const result = await service.runCycle();
      const text =
        `Cycle done: ${result.candidates} candidates seen, ${result.drafted} drafted, ` +
        `${result.queued} queued for your approval, ${result.posted} posted.`;
      await callback?.({ text, actions: ['SENSE_NOW'] });
      return { success: true, text, data: result };
    } catch (error) {
      const reason = (error as Error).message;
      logger.error({ error: reason }, 'manual sense cycle failed');
      await callback?.({ text: `The cycle failed: ${reason}` });
      return { success: false, text: 'sense cycle failed', error: reason };
    }
  },

  examples: [
    [
      { name: '{{user}}', content: { text: 'go check what is happening on X right now' } },
      { name: '{{agent}}', content: { text: 'Running a cycle.', actions: ['SENSE_NOW'] } },
    ],
  ],
};

export const senseLoopPlugin: Plugin = {
  name: 'sense-loop',
  description:
    'Periodically perceives live X discourse through Grok, ranks it with the cortex, and drafts ' +
    'posts for approval.',

  services: [SenseLoopService],
  actions: [senseNowAction],
};

export default senseLoopPlugin;
