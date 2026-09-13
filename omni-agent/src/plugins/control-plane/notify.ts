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
 * The two cannot share a bot, and an earlier version of this comment claimed
 * they could. They cannot: `getUpdates` is exclusive per token and delivers
 * each update to exactly one caller, so a conversational plugin and this poller
 * on one token split the operator's messages between them at random. Give the
 * conversational side its own bot via TELEGRAM_BOT_TOKEN — `index.ts` refuses
 * to load it otherwise — and see `TelegramConflictError` below for what
 * Telegram says when two pollers do overlap.
 */
import { logger } from '@elizaos/core';

import { config } from '../../config.js';
import { readBotState, renderBotState } from '../../lib/bot-state.js';
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

/**
 * Thrown when Telegram reports that something else is already long-polling this
 * bot — almost always `scema-tgbot`, the Rust sniper control bot, on a shared
 * token.
 *
 * It gets its own class because it is the one `getUpdates` failure that must
 * not be swallowed by the retry loop. Every other failure is transient and
 * retrying is correct; this one is a configuration fact, and retrying turns it
 * into a coin flip over who receives the operator's next command.
 */
export class TelegramConflictError extends Error {
  constructor(description: string) {
    super(
      `another process is already polling this bot token (${description}). ` +
        'Telegram delivers each update exactly once, so the two would split your ' +
        'commands between them at random. Give the cockpit its own bot with ' +
        'SCEMA_AGENT_TG_TOKEN, or stop scema-tgbot first.',
    );
    this.name = 'TelegramConflictError';
  }
}

async function telegram(method: string, body: unknown): Promise<unknown> {
  if (!config.telegram.configured) return null;
  const response = await fetch(`${API}/bot${config.telegram.token}/${method}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
    signal: AbortSignal.timeout(30_000),
  });
  const json = (await response.json()) as {
    ok: boolean;
    error_code?: number;
    description?: string;
    result?: unknown;
  };
  if (!json.ok) {
    if (json.error_code === 409) throw new TelegramConflictError(json.description ?? '409');
    throw new Error(`telegram ${method}: ${json.description ?? 'unknown error'}`);
  }
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
      logger.info('control plane inactive: no SCEMA_AGENT_TG_TOKEN or SCEMA_TG_TOKEN');
      return;
    }
    // Refuse *before* the first poll rather than discovering it as a 409 later,
    // because a 409 only arrives once both pollers are live and by then some
    // updates have already gone to the wrong process.
    if (config.telegram.sharedWithSniper && !config.telegram.pollShared) {
      logger.warn(
        'control plane not polling: this token is scema-tgbot\'s. Telegram delivers each ' +
          'update once, so polling it here would take commands away from the sniper bot at ' +
          'random. Set SCEMA_AGENT_TG_TOKEN to a second bot, or SCEMA_AGENT_TG_POLL=1 while ' +
          'scema-tgbot is stopped. Drafts still queue; review them with `npm run queue`.',
      );
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
        // A conflict is not transient: something else owns this token. Retrying
        // would mean the two processes take turns stealing each other's
        // commands, which is worse than this cockpit being off.
        if (error instanceof TelegramConflictError) {
          logger.error({ error: error.message }, 'control plane stopping');
          this.stopped = true;
          return;
        }
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

    if (text === '/bot') {
      // Deliberately the raw provider block rather than a prettier summary: it
      // is the same text the model is given, so what the operator reads here
      // and what the agent is working from cannot drift apart.
      await telegram('sendMessage', {
        chat_id: chatId,
        text: renderBotState(await readBotState()),
      });
      return;
    }

    if (text === '/help') {
      await telegram('sendMessage', {
        chat_id: chatId,
        text: [
          '/pending  resend drafts awaiting a decision',
          '/status   queue and cortex state',
          '/bot      what this agent can see of the live sniper',
          '',
          'Anything else is a normal conversation with the agent.',
          'Controlling the sniper — pause, dump, re-arm — is scema-tgbot, not this bot.',
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
