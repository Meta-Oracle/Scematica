/**
 * Scematica Omni-Agent entry point.
 *
 * Boot order is deliberate: report the configuration first, check the cortex
 * second, and only then start the runtime. A misconfigured agent should be
 * obvious in the first ten lines of output, not discovered twenty minutes
 * later when nothing has posted.
 */
import {
  ElizaOS,
  logger,
  stringToUuid,
  type IAgentRuntime,
  type Plugin,
} from '@elizaos/core';

import { character } from './character.js';
import { config, describeConfig } from './config.js';
import { ensureSchema } from './lib/migrate.js';
import { getQueue } from './lib/queue.js';
import { controlPlanePlugin } from './plugins/control-plane/index.js';
import { cortexPlugin } from './plugins/cortex/index.js';
import { getCortexClient } from './plugins/cortex/client.js';
import { grokPlugin } from './plugins/grok/index.js';
import { senseLoopPlugin } from './plugins/sense-loop/plugin.js';
import { oauth1Handle, wrongAccount } from './plugins/twitter/post.js';

const BANNER = String.raw`
   ___  __  __ _  _ ___     SCEMATICA OMNI-AGENT
  / _ \|  \/  | \| |_ _|    grok · cortex · x · telegram
 | (_) | |\/| | .. || |     perceives and drafts; it does not seal records
  \___/|_|  |_|_|\_|___|    and it does not command the sniper
`;

async function buildPlugins(): Promise<(Plugin | string)[]> {
  // Order matters: sql provides the database adapter, bootstrap provides the
  // default message pipeline, and our plugins layer on top of both.
  const plugins: (Plugin | string)[] = ['@elizaos/plugin-sql'];

  plugins.push(grokPlugin, cortexPlugin, controlPlanePlugin, senseLoopPlugin);

  // Conversational surfaces are opt-in by credential. Loading a platform
  // plugin without its token produces a noisy, confusing failure at runtime,
  // so they are only added when they can actually connect.
  // The conversational Telegram plugin needs its OWN bot, not the cockpit's.
  // Both long-poll `getUpdates`, which Telegram answers for exactly one caller
  // per token, so sharing would mean the operator's messages arriving at
  // whichever of the two won the race that second. A different token is the
  // only configuration where both can run, so it is the only one accepted.
  const conversational = config.telegram.conversationalToken;
  if (conversational && conversational !== config.telegram.token) {
    plugins.push('@elizaos/plugin-telegram');
  } else if (conversational) {
    logger.warn(
      'TELEGRAM_BOT_TOKEN is the same bot as the cockpit; conversational Telegram not loaded. ' +
        'Create a second bot with @BotFather for chat, or leave it unset.',
    );
  }
  // `@elizaos/plugin-twitter` acts on its own — it answers mentions, and can post and
  // take timeline actions — and **none of that passes through the approval queue**. So
  // the account it will act as is worth confirming before it is loaded, not after
  // somebody notices replies from the wrong handle.
  //
  // It reads only the OAuth 1.0a pair. The OAuth 2.0 token this project mints with
  // `x-auth` is invisible to it, so running that flow does not redirect these behaviours
  // and cannot be the answer here. The only fix is an access-token pair for the intended
  // account.
  //
  // A refusal rather than a warning, for the same reason `postProposal` refuses: an
  // unattended reply from the wrong account is not recoverable by deleting it.
  if (config.twitter.configured && !config.twitter.dryRun) {
    const actual = await oauth1Handle();
    if (wrongAccount(config.twitter.handle, actual)) {
      logger.error(
        `NOT loading plugin-twitter: its OAuth 1.0a credentials are @${actual}, but this ` +
          `project posts as @${config.twitter.handle}. That plugin replies and posts ` +
          'unattended, so it would act as the wrong account. `x-auth` will not fix this — ' +
          'the plugin never reads the OAuth 2.0 token. Supply an access-token pair for ' +
          `@${config.twitter.handle}, or set TWITTER_USERNAME to @${actual} if that is ` +
          'genuinely the account you want.',
      );
    } else {
      plugins.push('@elizaos/plugin-twitter');
    }
  }

  plugins.push('@elizaos/plugin-bootstrap');
  return plugins;
}

async function main(): Promise<void> {
  console.log(BANNER);
  console.log('configuration:');
  for (const line of describeConfig()) console.log(line);
  console.log();

  // Surface cortex state before the runtime starts, because "the agent has no
  // judgement and no memory" is something you want to know immediately.
  const cortex = getCortexClient();
  const health = await cortex.health();
  if (health) {
    console.log(
      `  cortex online: ${health.memories} memories, ${health.train_steps} training steps\n`,
    );
  } else if (config.cortex.required) {
    console.error(
      `  cortex REQUIRED but unreachable at ${config.cortex.url}.\n` +
        `  Start it first:  npm run cortex\n`,
    );
    process.exit(1);
  } else {
    console.warn(
      `  cortex offline -- running with neutral judgement and no memory.\n` +
        `  Start it in another terminal:  npm run cortex\n`,
    );
  }

  const queue = getQueue();
  const summary = await queue.summary();
  if (summary.pending > 0) {
    console.log(`  ${summary.pending} draft(s) already waiting for your decision\n`);
  }

  // Create the tables before the runtime queries them. `AgentRuntime.initialize` runs its
  // migrations AFTER its first SELECT against `agents`, so a fresh database cannot boot
  // without this — see `lib/migrate.ts` for the full trace.
  await ensureSchema(stringToUuid(character.name));

  const elizaOS = new ElizaOS();
  const runtimes = await elizaOS.addAgents(
    [{ character, plugins: await buildPlugins() }],
    { autoStart: true, returnRuntimes: true },
  );

  const runtime: IAgentRuntime | undefined = runtimes[0];
  if (!runtime) throw new Error('runtime failed to start');

  logger.info(`${character.name} is running`);

  const shutdown = async (signal: string): Promise<void> => {
    console.log(`\n${signal} received, shutting down...`);
    try {
      // Persist learned state before exiting; an unsaved training session is
      // operator decisions thrown away.
      await cortex.save();
      await elizaOS.stopAgents();
    } catch (error) {
      logger.error({ error: (error as Error).message }, 'error during shutdown');
    }
    process.exit(0);
  };

  process.on('SIGINT', () => void shutdown('SIGINT'));
  process.on('SIGTERM', () => void shutdown('SIGTERM'));
}

main().catch((error) => {
  console.error('failed to start:', error);
  process.exit(1);
});
