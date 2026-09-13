/**
 * Posting to X, and measuring what happened afterwards.
 *
 * Dry-run is the default and is enforced here rather than at the call site,
 * so there is exactly one place in the codebase where a real tweet can be
 * emitted. Every path that wants to post goes through `postProposal`.
 *
 * The dry-run path is not a stub: it performs the same queue transitions,
 * writes the composed text to a reviewable file, and records a synthetic id.
 * That means the loop -- propose, decide, post, reflect -- is fully
 * exercisable before a single credential exists, and switching to live
 * changes one thing: whether the bytes leave the machine.
 *
 * Honest limitation of dry-run: there is no engagement to measure, so the
 * resonance head receives no labels until posting is live. Salience and taste
 * still train from operator decisions.
 */
import { appendFile, mkdir } from 'node:fs/promises';
import { dirname } from 'node:path';
import { randomUUID } from 'node:crypto';

import { logger } from '@elizaos/core';
import { TwitterApi } from 'twitter-api-v2';

import { config } from '../../config.js';
import { classifyXFailure } from '../../lib/credentials.js';
import { finalText, getQueue, type Proposal } from '../../lib/queue.js';
import { getOAuth2Client } from './oauth2.js';

const DRY_RUN_LOG = `${config.paths.data}/queue/dry-run-posts.md`;

export interface EngagementMetrics {
  likes: number;
  reposts: number;
  replies: number;
}

let writeClient: TwitterApi | undefined;
let readClient: TwitterApi | undefined;

/**
 * User-context client. The only thing that can create a post.
 *
 * Lazily built so a missing credential never breaks module load.
 */
function getClient(): TwitterApi | null {
  if (!config.twitter.credentials) return null;
  if (!writeClient) {
    writeClient = new TwitterApi({
      appKey: config.twitter.credentials.apiKey,
      appSecret: config.twitter.credentials.apiSecretKey,
      accessToken: config.twitter.credentials.accessToken,
      accessSecret: config.twitter.credentials.accessTokenSecret,
    });
  }
  return writeClient;
}

/**
 * Read-only client, preferring app-only bearer auth.
 *
 * Reading public metrics does not need user context, so the reflection loop
 * can measure engagement with only a bearer token. That matters because the
 * bearer is typically available well before the OAuth 1.0a access token pair
 * is generated.
 */
function getReadClient(): TwitterApi | null {
  if (config.twitter.bearerToken) {
    if (!readClient) readClient = new TwitterApi(config.twitter.bearerToken);
    return readClient;
  }
  return getClient();
}

async function recordDryRun(proposal: Proposal, text: string): Promise<string> {
  const syntheticId = `dry-${randomUUID().slice(0, 8)}`;
  // Build the optional line separately. Filtering falsy entries out of the
  // whole array (the obvious shortcut) also eats the intentional blank-line
  // separators, which runs consecutive entries together as `---## 2026-...`
  // and leaves the review log unreadable.
  const lines = [
    `## ${new Date().toISOString()}  (${proposal.id} -> ${syntheticId})`,
    '',
    text,
    '',
    `- topic: ${proposal.topic}`,
    `- rationale: ${proposal.rationale}`,
    `- scores: taste ${proposal.scores.taste.toFixed(3)}, salience ${proposal.scores.salience.toFixed(3)}, novelty ${proposal.scores.novelty.toFixed(3)}`,
  ];
  if (proposal.sources.length) lines.push(`- reacting to: ${proposal.sources.join(', ')}`);
  lines.push('', '---', '');

  const entry = `${lines.join('\n')}\n`;

  await mkdir(dirname(DRY_RUN_LOG), { recursive: true });
  await appendFile(DRY_RUN_LOG, entry, 'utf8');
  return syntheticId;
}

/**
 * Post an approved proposal.
 *
 * Refuses anything not approved: the approval gate is a safety property, not
 * a UI convention, and bypassing it should require editing this function.
 */
export async function postProposal(
  proposalId: string,
): Promise<{ posted: boolean; id?: string; dryRun: boolean; error?: string }> {
  const queue = getQueue();
  const proposal = await queue.get(proposalId);

  if (!proposal) {
    return { posted: false, dryRun: config.twitter.dryRun, error: 'no such proposal' };
  }
  if (proposal.status !== 'approved') {
    return {
      posted: false,
      dryRun: config.twitter.dryRun,
      error: `proposal is ${proposal.status}, not approved`,
    };
  }

  const text = finalText(proposal);

  if (config.twitter.dryRun) {
    const syntheticId = await recordDryRun(proposal, text);
    await queue.markPosted(proposal.id, syntheticId);
    logger.info(`[dry-run] would post (${proposal.id}): ${text}`);
    return { posted: true, id: syntheticId, dryRun: true };
  }

  // Prefer OAuth 2.0 when the browser flow has been run: its tokens are
  // refreshable and survive the app-permission changes that silently
  // invalidate OAuth 1.0a access tokens. Fall back to OAuth 1.0a otherwise.
  let api: TwitterApi | null = null;
  let authMode = 'oauth1';
  try {
    const oauth2 = await getOAuth2Client();
    if (oauth2) {
      api = oauth2.client;
      authMode = 'oauth2';
    }
  } catch (error) {
    logger.warn(
      { error: (error as Error).message },
      'OAuth 2.0 token refresh failed; falling back to OAuth 1.0a',
    );
  }
  if (!api) api = getClient();

  if (!api) {
    const error =
      'live posting requested but no usable X write credentials: run `npx tsx src/cli.ts x-auth`';
    await queue.markFailed(proposal.id, error);
    return { posted: false, dryRun: false, error };
  }

  try {
    const result = await api.v2.tweet(text);
    logger.debug(`posted via ${authMode}`);
    await queue.markPosted(proposal.id, result.data.id);
    logger.info(`posted ${proposal.id} as ${result.data.id}`);
    return { posted: true, id: result.data.id, dryRun: false };
  } catch (error) {
    // "Request failed with code 402" tells the operator nothing and is what
    // the queue recorded before this. X puts the real reason in the body --
    // credits depleted, invalid token, duplicate content -- so classify it
    // and store something that names the actual problem and its remedy.
    const err = error as { code?: number; data?: unknown; message?: string };
    const body = typeof err.data === 'string' ? err.data : JSON.stringify(err.data ?? {});
    const verdict = classifyXFailure(err.code, body);
    const reason = verdict.remedy
      ? `${verdict.detail} -- ${verdict.remedy}`
      : (verdict.detail ?? err.message ?? 'unknown failure');

    await queue.markFailed(proposal.id, reason);
    logger.error({ error: reason, code: err.code }, `failed to post ${proposal.id}`);
    return { posted: false, dryRun: false, error: reason };
  }
}

/**
 * Read back how a post actually performed.
 *
 * Returns null when there is nothing real to measure -- a dry-run post, a
 * missing credential, or an API failure -- because a fabricated zero would
 * train the resonance head on fiction.
 */
export async function measureEngagement(proposal: Proposal): Promise<EngagementMetrics | null> {
  if (!proposal.postedId || proposal.postedId.startsWith('dry-')) return null;

  // An OAuth 2.0 user client reads fine too; prefer whatever is actually
  // authorised over whatever happens to be configured.
  let api: TwitterApi | null = null;
  try {
    const oauth2 = await getOAuth2Client();
    if (oauth2) api = oauth2.client;
  } catch {
    // fall through to app-only
  }
  if (!api) api = getReadClient();
  if (!api) return null;

  try {
    const response = await api.v2.singleTweet(proposal.postedId, {
      'tweet.fields': ['public_metrics'],
    });
    const metrics = response.data?.public_metrics;
    if (!metrics) return null;
    return {
      likes: metrics.like_count ?? 0,
      reposts: metrics.retweet_count ?? 0,
      replies: metrics.reply_count ?? 0,
    };
  } catch (error) {
    logger.warn(
      { error: (error as Error).message, id: proposal.postedId },
      'could not read engagement',
    );
    return null;
  }
}

/** Where dry-run output lands, for the boot report and the CLI. */
export const dryRunLogPath = DRY_RUN_LOG;
