/**
 * Pushing proposals to the operator, and receiving decisions back.
 *
 * This talks to the Telegram Bot API directly rather than going through
 * @elizaos/plugin-telegram. That plugin owns *conversation* -- it turns
 * messages into agent turns. The cockpit is not conversation: it is a control
 * surface with inline keyboards, callback queries and an authorisation rule,
 * and routing that through a conversational pipeline would mean an LLM sits
 * between the operator pressing "reject" and the rejection being recorded.
 *
 * Both run against the same bot token, which Telegram permits: this module
 * uses the callback-query side, the plugin uses the message side.
 */
import { logger } from '@elizaos/core';

import { config } from '../../config.js';
import { finalText, getQueue, type Proposal } from '../../lib/queue.js';
import { getCortexClient } from '../cortex/client.js';
import { postProposal } from '../twitter/post.js';

const API = 'https://api.telegram.org';

interface TelegramUpdate {
  update_id: number;
  callback_query?: {
    id: string;
    from: { id: number; username?: string };
    message?: { chat: { id: number }; message_id: number };
    data?: string;
  };
  message?: {
    chat: { id: number };
    from?: { id: number; username?: string };
    text?: string;
    reply_to_message?: { text?: string };
  };
}

async function telegram(method: string, body: unknown): Promise<unknown> {
  if (!config.telegram.configured) return null;
  const response = await fetch(`${API}/bot${config.telegram.token}/${method}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
    signal: AbortSignal.timeout(30_000),
  });
  const json = (await response.json()) as { ok: boolean; description?: string; result?: unknown };
  if (!json.ok) throw new Error(`telegram ${method}: ${json.description ?? 'unknown error'}`);
  return json.result;
}

function renderProposal(proposal: Proposal): string {
  const scores = proposal.scores;
  const bar = (value: number): string => {
    const filled = Math.round(Math.max(0, Math.min(1, value)) * 10);
    return '█'.repeat(filled) + '░'.repeat(10 - filled);
  };
  return [
    `*Draft* \`${proposal.id}\``,
    '',
    finalText(proposal),
    '',
    `_${proposal.rationale}_`,
    '',
    `taste    ${bar(scores.taste)} ${scores.taste.toFixed(2)}`,
    `salience ${bar(scores.salience)} ${scores.salience.toFixed(2)}`,
    `novelty  ${bar(scores.novelty)} ${scores.novelty.toFixed(2)}`,
    proposal.sources.length ? `\n[source](${proposal.sources[0]})` : '',
  ]
    .filter(Boolean)
    .join('\n');
}

/** Send a draft to the operator with approve/reject buttons. */
export async function notifyProposal(proposal: Proposal): Promise<boolean> {
  if (!config.telegram.configured || !config.telegram.operatorChatId) {
    logger.info(
      `proposal ${proposal.id} queued (no Telegram operator configured; review with: npm run queue)`,
    );
    return false;
  }

  try {
    await telegram('sendMessage', {
      chat_id: config.telegram.operatorChatId,
      text: renderProposal(proposal),
      parse_mode: 'Markdown',
      link_preview_options: { is_disabled: true },
      reply_markup: {
        inline_keyboard: [
          [
            { text: '✅ Post', callback_data: `ok:${proposal.id}` },
            { text: '❌ Reject', callback_data: `no:${proposal.id}` },
          ],
          [{ text: '✏️ Edit (reply to this message)', callback_data: `edit:${proposal.id}` }],
        ],
      },
    });
    return true;
  } catch (error) {
    logger.warn({ error: (error as Error).message }, 'could not notify operator');
    return false;
  }
}

/**
 * Record a decision and teach the cortex.
 *
 * The feedback call is the entire reason the cockpit exists: every button
 * press is one labelled training example for the taste head. An approval and
 * a rejection are equally valuable, which is why rejections are recorded with
 * the same care rather than just dropped.
 */
export async function applyDecision(
  proposalId: string,
  decision: 'approved' | 'rejected',
  by: string,
  editedText?: string,
): Promise<{ ok: boolean; message: string }> {
  const queue = getQueue();
  const proposal = await queue.decide(proposalId, decision, by, editedText);
  if (!proposal) return { ok: false, message: `no draft ${proposalId}` };

  const cortex = getCortexClient();

  // An edit is a richer signal than a bare approval: the original was not
  // good enough (negative), the rewrite was (positive). Teaching both is what
  // lets the net learn the difference rather than just the direction.
  if (decision === 'approved' && editedText && editedText !== proposal.text) {
    await cortex.feedback({
      text: proposal.text,
      taste: 0,
      source: 'telegram-edit-original',
      ref: proposal.id,
    });
    await cortex.feedback({
      text: editedText,
      taste: 1,
      salience: 1,
      source: 'telegram-edit-final',
      ref: proposal.id,
    });
  } else {
    await cortex.feedback({
      text: finalText(proposal),
      taste: decision === 'approved' ? 1 : 0,
      ...(decision === 'approved' ? { salience: 1 } : {}),
      source: 'telegram',
      ref: proposal.id,
    });
  }

  if (decision === 'rejected') {
    return { ok: true, message: `Rejected ${proposalId}. The cortex learned from it.` };
  }

  const result = await postProposal(proposalId);
  if (!result.posted) return { ok: false, message: `Approved but posting failed: ${result.error}` };
  return {
    ok: true,
    message: result.dryRun
      ? `Approved. DRY RUN -- written to the review log as ${result.id}, not sent to X.`
      : `Posted as ${result.id}.`,
  };
}

/**
 * Long-poll Telegram for button presses and edit replies.
 *
 * Only the configured operator chat can decide anything. Without that check a
 * stranger who finds the bot could post from the account, so an unset
 * operator id disables control entirely rather than defaulting to open.
 */
export class ControlPlanePoller {
  private offset = 0;
  private stopped = false;
  /** Proposals the operator has signalled an intent to edit, by chat. */
  private awaitingEdit = new Map<number, string>();

  async start(): Promise<void> {
    if (!config.telegram.configured) {
      logger.info('control plane inactive: no SCEMA_TG_TOKEN');
      return;
    }
    if (!config.telegram.operatorChatId) {
      logger.warn(
        'control plane inactive: SCEMA_TG_OPERATOR_CHAT_ID is unset, so approvals are disabled. ' +
          'Message your bot and check the logs to find your chat id.',
      );
      return;
    }
    logger.info(`control plane listening (operator ${config.telegram.operatorChatId})`);
    void this.loop();
  }

  stop(): void {
    this.stopped = true;
  }

  private async loop(): Promise<void> {
    while (!this.stopped) {
      try {
        const updates = (await telegram('getUpdates', {
          offset: this.offset,
          timeout: 25,
          allowed_updates: ['callback_query', 'message'],
        })) as TelegramUpdate[] | null;

        for (const update of updates ?? []) {
          this.offset = update.update_id + 1;
          await this.handle(update).catch((error) =>
            logger.warn({ error: (error as Error).message }, 'control plane update failed'),
          );
        }
      } catch (error) {
        // Telegram long-polling drops connections routinely; back off and
        // continue rather than tearing down the cockpit.
        logger.debug({ error: (error as Error).message }, 'getUpdates failed; retrying');
        await new Promise((resolve) => setTimeout(resolve, 5000));
      }
    }
  }

  private isOperator(chatId: number | undefined): boolean {
    return String(chatId) === String(config.telegram.operatorChatId);
  }

  private async handle(update: TelegramUpdate): Promise<void> {
    const query = update.callback_query;
    if (query?.data) {
      const chatId = query.message?.chat.id;
      if (!this.isOperator(chatId)) {
        await telegram('answerCallbackQuery', {
          callback_query_id: query.id,
          text: 'Not authorised.',
        });
        return;
      }

      const [verb, proposalId] = query.data.split(':');
      if (!proposalId) return;

      if (verb === 'edit') {
        this.awaitingEdit.set(chatId!, proposalId);
        await telegram('answerCallbackQuery', {
          callback_query_id: query.id,
          text: 'Reply to this message with your rewrite.',
        });
        return;
      }

      const decision = verb === 'ok' ? 'approved' : 'rejected';
      const result = await applyDecision(
        proposalId,
        decision,
        query.from.username ?? String(query.from.id),
      );
      await telegram('answerCallbackQuery', {
        callback_query_id: query.id,
        text: result.message.slice(0, 200),
      });
      await telegram('sendMessage', { chat_id: chatId, text: result.message });
      return;
    }

    const message = update.message;
    if (!message?.text || !this.isOperator(message.chat.id)) return;

    // An edit in flight takes precedence over command parsing.
    const pendingEdit = this.awaitingEdit.get(message.chat.id);
    if (pendingEdit && !message.text.startsWith('/')) {
      this.awaitingEdit.delete(message.chat.id);
      const result = await applyDecision(
        pendingEdit,
        'approved',
        message.from?.username ?? 'operator',
        message.text.trim(),
      );
      await telegram('sendMessage', { chat_id: message.chat.id, text: result.message });
      return;
    }

    await this.handleCommand(message.chat.id, message.text.trim());
  }

  private async handleCommand(chatId: number, text: string): Promise<void> {
    const queue = getQueue();

    if (text === '/pending') {
      const pending = await queue.pending();
      if (pending.length === 0) {
        await telegram('sendMessage', { chat_id: chatId, text: 'Nothing waiting.' });
        return;
      }
      for (const proposal of pending.slice(0, 5)) await notifyProposal(proposal);
      return;
    }

    if (text === '/status') {
      const [summary, stats] = await Promise.all([queue.summary(), getCortexClient().stats()]);
      const lines = [
        `*Queue*  pending ${summary.pending} · posted ${summary.posted} · rejected ${summary.rejected} · expired ${summary.expired}`,
        stats
          ? `*Cortex*  ${stats.training.total_events} labels · ${stats.training.total_steps} steps · ${stats.memory.count} memories · ${stats.kernel}/${stats.device}`
          : '*Cortex*  unreachable',
        config.twitter.dryRun ? '*Mode*  DRY RUN (nothing reaches X)' : '*Mode*  LIVE',
      ];
      await telegram('sendMessage', {
        chat_id: chatId,
        text: lines.join('\n'),
        parse_mode: 'Markdown',
      });
      return;
    }

    if (text === '/help') {
      await telegram('sendMessage', {
        chat_id: chatId,
        text: [
          '/pending  resend drafts awaiting a decision',
          '/status   queue and cortex state',
          'Anything else is a normal conversation with the agent.',
        ].join('\n'),
      });
    }
  }
}

/** Log the chat id of whoever messages the bot, to make setup discoverable. */
export async function discoverOperatorChatId(): Promise<string | null> {
  if (!config.telegram.configured) return null;
  try {
    const updates = (await telegram('getUpdates', { timeout: 0 })) as TelegramUpdate[] | null;
    const chatId = updates?.find((update) => update.message?.chat.id)?.message?.chat.id;
    return chatId ? String(chatId) : null;
  } catch {
    return null;
  }
}
