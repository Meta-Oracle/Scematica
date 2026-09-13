/**
 * Terminal control surface.
 *
 *   npm run chat            talk to the agent (memory + live search)
 *   npm run sense           run one perception cycle now
 *   npm run queue           review and decide on pending drafts
 *   tsx src/cli.ts doctor   check every dependency and say what is broken
 *
 * This exists so the agent is fully operable before Telegram or X credentials
 * are in place. Every decision made here goes through the same code path the
 * Telegram cockpit uses, so it trains the cortex identically.
 */
import { createInterface } from 'node:readline/promises';
import { stdin, stdout } from 'node:process';

import { config, describeConfig, resolveActingAccount } from './config.js';
import { pollingConflict, readBotState, renderBotState } from './lib/bot-state.js';
import { classifyFailure, classifyXFailure, type CredentialCheck } from './lib/credentials.js';
import { finalText, getQueue } from './lib/queue.js';
import { applyDecision, discoverOperatorChatId } from './plugins/control-plane/notify.js';
import { getCortexClient } from './plugins/cortex/client.js';
import { getGrokClient, type GrokError, type GrokMessage } from './plugins/grok/client.js';
import { dryRunLogPath, wrongAccount } from './plugins/twitter/post.js';

const DIM = '\x1b[2m';
const BOLD = '\x1b[1m';
const RESET = '\x1b[0m';
const GREEN = '\x1b[32m';
const RED = '\x1b[31m';
const YELLOW = '\x1b[33m';

function heading(text: string): void {
  console.log(`\n${BOLD}${text}${RESET}`);
}

/**
 * Test the X app credentials and the user credentials independently.
 *
 * A 401 on a posting call cannot tell you which of four credentials to
 * replace. Verifying the consumer pair on its own (via app-only login) and
 * then the full user context (via v1.1 verify_credentials, which returns
 * numeric error codes) isolates the broken half.
 *
 * Read-only throughout: verifying a write credential by writing would put a
 * real post on a real account.
 */
async function checkXWriteCredentials(
  report: (label: string, result: CredentialCheck) => void,
  check: (ok: boolean, label: string, detail: string) => void,
): Promise<void> {
  const creds = config.twitter.credentials;
  if (!creds) return;

  const { TwitterApi } = await import('twitter-api-v2');

  let appPairValid = false;
  try {
    await new TwitterApi({ appKey: creds.apiKey, appSecret: creds.apiSecretKey }).appLogin();
    appPairValid = true;
  } catch {
    appPairValid = false;
  }

  try {
    const me = await new TwitterApi({
      appKey: creds.apiKey,
      appSecret: creds.apiSecretKey,
      accessToken: creds.accessToken,
      accessSecret: creds.accessTokenSecret,
    }).v1.verifyCredentials();

    // A working credential for the WRONG account is not an `ok`. It is the most
    // dangerous state this check can find: everything downstream looks healthy, and the
    // posts land somewhere the operator is not watching. `postProposal` refuses on it, so
    // reporting it green here would contradict the thing that actually happens.
    if (wrongAccount(config.twitter.handle, me.screen_name)) {
      report('x post', {
        verdict: 'wrong-account',
        detail: `credentials are @${me.screen_name}, but TWITTER_USERNAME is @${config.twitter.handle} -- posting is REFUSED`,
        remedy:
          'sign in to X as the account you want, then run `npm run x-auth` (OAuth 2.0 ' +
          'follows whoever approves it). Note plugin-twitter ignores that token and uses the ' +
          'OAuth 1.0a pair, so its autonomous replies need a pair for the right account.',
      });
    } else {
      report('x post', {
        verdict: 'ok',
        detail: `@${me.screen_name} authenticated${config.twitter.dryRun ? ' (SCEMA_DRY_RUN is on)' : ' -- LIVE'}`,
      });
    }
  } catch (error) {
    const err = error as { code?: number; data?: unknown; message?: string };
    const body = typeof err.data === 'string' ? err.data : JSON.stringify(err.data ?? {});
    const verdict = classifyXFailure(err.code, body);
    report('x post', verdict);
    check(
      appPairValid,
      'x app pair',
      appPairValid
        ? 'API key + secret are valid, so only the Access Token pair needs replacing'
        : 'API key + secret are also rejected -- check they come from the same app',
    );
  }
}

/** --------------------------------------------------------------- doctor */

async function doctor(): Promise<number> {
  heading('Scematica Omni-Agent :: diagnostics');
  for (const line of describeConfig()) console.log(line);

  let failures = 0;
  const check = (ok: boolean, label: string, detail: string): void => {
    const mark = ok ? `${GREEN}ok  ${RESET}` : `${RED}FAIL${RESET}`;
    if (!ok) failures += 1;
    console.log(`  ${mark} ${label.padEnd(14)} ${DIM}${detail}${RESET}`);
  };

  /**
   * Report a classified credential result.
   *
   * An exhausted balance is shown as BILL rather than FAIL: the credential is
   * correct and regenerating it would waste your time. The distinction is the
   * whole point of running diagnostics.
   */
  const report = (label: string, result: CredentialCheck): void => {
    if (result.verdict === 'ok') {
      check(true, label, result.detail);
      return;
    }
    failures += 1;
    // Three markers, because three of these send the operator somewhere different.
    // BILL: the credential is right, the account is not funded. WRNG: the credential is
    // right and belongs to somebody else — regenerating it would waste an afternoon.
    const mark =
      result.verdict === 'out-of-credit'
        ? `${YELLOW}BILL${RESET}`
        : result.verdict === 'wrong-account'
          ? `${RED}WRNG${RESET}`
          : `${RED}FAIL${RESET}`;
    console.log(`  ${mark} ${label.padEnd(14)} ${DIM}${result.detail}${RESET}`);
    if (result.remedy) console.log(`       ${' '.repeat(14)} ${DIM}-> ${result.remedy}${RESET}`);
  };

  heading('dependencies');

  const cortex = getCortexClient();
  const health = await cortex.health();
  check(
    health !== null,
    'cortex',
    health
      ? `${health.memories} memories, ${health.train_steps} steps`
      : `unreachable at ${config.cortex.url} -- run: npm run cortex`,
  );

  const grok = getGrokClient();
  if (!grok.configured) {
    check(false, 'grok', 'XAI_API_KEY is not set');
  } else {
    try {
      const reply = await grok.chat([{ role: 'user', content: 'Reply with exactly: OK' }], {
        maxTokens: 2000,
      });
      check(true, 'grok', `${config.xai.model} responded (${reply.trim().slice(0, 20)})`);
    } catch (error) {
      const grokError = error as GrokError;
      const verdict = classifyFailure(grokError.status, grokError.body ?? grokError.message);
      report('grok', verdict);
    }
  }

  if (config.telegram.configured) {
    if (config.telegram.operatorChatId) {
      check(true, 'telegram', `operator ${config.telegram.operatorChatId}`);
    } else {
      const discovered = await discoverOperatorChatId();
      check(
        false,
        'telegram',
        discovered
          ? `token works but SCEMA_TG_OPERATOR_CHAT_ID is unset. Yours looks like: ${discovered}`
          : 'token set, but no operator id and nobody has messaged the bot yet',
      );
    }
  } else {
    check(false, 'telegram', 'no SCEMA_AGENT_TG_TOKEN or SCEMA_TG_TOKEN (cockpit disabled)');
  }

  // A shared bot is reported as its own line rather than folded into the one above,
  // because the token is *valid* — the problem is that two processes want it, and a FAIL
  // on "telegram" would send somebody to check the token.
  //
  // Asked of the running bot rather than of the environment. The two tokens live in
  // separate `.env` files, so `sharedWithSniper` compares a variable against one that is
  // not defined in this process and reports no conflict while two pollers fight.
  const conflict = await pollingConflict(config.telegram.token);
  if (conflict) {
    check(
      config.telegram.pollShared,
      'tg cockpit',
      config.telegram.pollShared
        ? `polling @${conflict.username} anyway — scema-tgbot (pid ${conflict.pid}) is on it too`
        : `off: scema-tgbot (pid ${conflict.pid}) is polling @${conflict.username}. ` +
          'Set SCEMA_AGENT_TG_TOKEN to a second bot from @BotFather',
    );
  } else if (config.telegram.sharedWithSniper && !config.telegram.pollShared) {
    check(
      false,
      'tg cockpit',
      "off: this is scema-tgbot's bot. Set SCEMA_AGENT_TG_TOKEN to a second bot, " +
        'or SCEMA_AGENT_TG_POLL=1 while scema-tgbot is stopped',
    );
  }

  // The bot is optional, so an unwired one is not a failure — it is a fact the
  // operator should see, since it decides whether the agent can answer "how is
  // the bot doing?" at all.
  const bot = await readBotState();
  if (!bot.dir) {
    console.log(
      `  ${DIM}--${RESET} ${'bot state'.padEnd(14)} ${DIM}not wired (SCEMA_BOT_DIR unset)${RESET}`,
    );
  } else {
    check(
      bot.metrics.freshness === 'fresh',
      'bot state',
      bot.metrics.freshness === 'fresh'
        ? `live, metrics ${bot.metrics.ageSecs}s old`
        : bot.metrics.freshness === 'stale'
          ? `metrics ${bot.metrics.ageSecs}s old -- sniper looks stopped`
          : `no metrics file in ${bot.dir} -- the sniper has not run there`,
    );
  }

  // Actually call X rather than just checking which variables are non-empty.
  // A present-but-unusable credential is the case worth catching.
  if (config.twitter.bearerToken) {
    try {
      const response = await fetch('https://api.twitter.com/2/tweets/20', {
        headers: { Authorization: `Bearer ${config.twitter.bearerToken}` },
        signal: AbortSignal.timeout(15_000),
      });
      if (response.ok) {
        report('x read', { verdict: 'ok', detail: 'bearer token works (app-only reads)' });
      } else {
        report('x read', classifyFailure(response.status, await response.text().catch(() => '')));
      }
    } catch (error) {
      report('x read', { verdict: 'unknown', detail: (error as Error).message.slice(0, 120) });
    }
  } else {
    check(false, 'x read', 'TWITTER_BEARER_TOKEN is not set (engagement cannot be measured)');
  }

  const missingWrite = [
    ['TWITTER_API_KEY', process.env.TWITTER_API_KEY],
    ['TWITTER_API_SECRET_KEY', process.env.TWITTER_API_SECRET_KEY ?? process.env.TWITTER_API_SECRET],
    ['TWITTER_ACCESS_TOKEN', process.env.TWITTER_ACCESS_TOKEN],
    ['TWITTER_ACCESS_TOKEN_SECRET', process.env.TWITTER_ACCESS_TOKEN_SECRET],
  ]
    .filter(([, value]) => !value)
    .map(([name]) => name as string);

  if (missingWrite.length > 0) {
    check(false, 'x post', `missing ${missingWrite.join(', ')}`);
    console.log(
      `       ${' '.repeat(14)} ${DIM}-> a bearer token cannot post; generate the Access Token ` +
        `pair in the X developer portal (set the app to Read and write first)${RESET}`,
    );
  } else {
    // All four are present, which says nothing about whether they work. Test
    // the two halves separately: knowing *which* pair is bad is the entire
    // difference between a two-minute fix and an afternoon.
    await checkXWriteCredentials(report, check);
  }

  // OAuth 2.0 is an independent route to a write credential, so report it
  // separately rather than folding it into the OAuth 1.0a verdict.
  // Which account will actually receive a post, from the token not the config.
  const acting = await resolveActingAccount();
  if (acting.handle && acting.source === 'oauth2-token') {
    check(true, 'posting as', `@${acting.handle} (from the authorised token)`);
    if (acting.mismatch) {
      console.log(
        `       ${' '.repeat(14)} ${YELLOW}TWITTER_USERNAME says @${config.twitter.handle}, ` +
          `but posts go to @${acting.handle}${RESET}`,
      );
    }
  }

  const { loadTokens } = await import('./plugins/twitter/oauth2.js');
  const oauth2Tokens = await loadTokens();
  if (oauth2Tokens) {
    const expired = Date.now() >= oauth2Tokens.expiresAt;
    const who = oauth2Tokens.screenName ? `@${oauth2Tokens.screenName}` : 'authorised';
    check(
      !expired || Boolean(oauth2Tokens.refreshToken),
      'x oauth2',
      expired
        ? oauth2Tokens.refreshToken
          ? `${who}, access expired but will auto-refresh`
          : `${who}, expired with no refresh token -- re-run: npm run x-auth`
        : `${who}, valid until ${new Date(oauth2Tokens.expiresAt).toLocaleTimeString()}`,
    );
  } else if (config.twitter.oauth2.configured) {
    check(false, 'x oauth2', 'client id/secret set but not yet authorised');
    console.log(
      `       ${' '.repeat(14)} ${DIM}-> run: npm run x-auth  ` +
        `(this is the write path that does not depend on the OAuth 1.0a tokens)${RESET}`,
    );
  }

  heading('queue');
  const summary = await getQueue().summary();
  console.log(
    `  pending ${summary.pending} · posted ${summary.posted} · rejected ${summary.rejected} · ` +
      `failed ${summary.failed} · expired ${summary.expired}`,
  );
  if (config.twitter.dryRun) console.log(`  ${DIM}dry-run output: ${dryRunLogPath}${RESET}`);

  console.log(
    failures === 0
      ? `\n${GREEN}everything needed is available${RESET}\n`
      : `\n${YELLOW}${failures} thing(s) unavailable -- the agent still runs, degraded${RESET}\n`,
  );
  return failures === 0 ? 0 : 1;
}

/** ----------------------------------------------------------------- chat */

async function chat(): Promise<number> {
  const grok = getGrokClient();
  if (!grok.configured) {
    console.error(`${RED}XAI_API_KEY is not set, so there is nothing to talk to.${RESET}`);
    return 1;
  }

  const cortex = getCortexClient();
  heading(`Omni :: chat  ${DIM}(/search <q>, /recall <q>, /quit)${RESET}`);

  const rl = createInterface({ input: stdin, output: stdout });
  const history: GrokMessage[] = [];

  try {
    for (;;) {
      const input = (await rl.question(`${BOLD}you ${RESET}> `)).trim();
      if (!input) continue;
      if (input === '/quit' || input === '/exit') break;

      if (input.startsWith('/search ')) {
        const query = input.slice(8);
        process.stdout.write(`${DIM}searching X...${RESET}\n`);
        try {
          const result = await grok.liveSearch(query);
          console.log(`\n${result.text}\n`);
          for (const citation of result.citations.slice(0, 5)) {
            console.log(`${DIM}  - ${citation.title ?? citation.url}${RESET}`);
          }
          console.log();
          await cortex.remember(result.text.slice(0, 2000), 'cli', 'observation', { query });
        } catch (error) {
          console.error(`${RED}${(error as Error).message}${RESET}`);
        }
        continue;
      }

      if (input.startsWith('/recall ')) {
        const hits = await cortex.recall(input.slice(8), { k: 8 });
        if (hits.length === 0) console.log(`${DIM}  nothing remembered${RESET}`);
        for (const hit of hits) {
          console.log(
            `${DIM}  [${hit.surface}] ${hit.weighted.toFixed(3)} ${hit.text.slice(0, 150)}${RESET}`,
          );
        }
        console.log();
        continue;
      }

      // Same memory the other surfaces read from, so the CLI is not a
      // second-class citizen with its own private context.
      const recalled = await cortex.recall(input, { k: 5 });
      const memoryBlock = recalled.length
        ? `Relevant things you already know:\n${recalled
            .map((hit) => `- [${hit.surface}] ${hit.text.slice(0, 200)}`)
            .join('\n')}\n\n`
        : '';

      history.push({ role: 'user', content: input });
      const messages: GrokMessage[] = [
        { role: 'system', content: `${(await import('./character.js')).character.system}` },
        ...(memoryBlock ? [{ role: 'system' as const, content: memoryBlock }] : []),
        ...history.slice(-12),
      ];

      try {
        const reply = await grok.chat(messages);
        history.push({ role: 'assistant', content: reply });
        console.log(`\n${reply}\n`);
        await cortex.remember(input, 'cli', 'observation');
        await cortex.remember(reply, 'cli', 'decision');
      } catch (error) {
        console.error(`${RED}${(error as Error).message}${RESET}\n`);
        history.pop();
      }
    }
  } finally {
    rl.close();
    await cortex.save();
  }
  return 0;
}

/** ---------------------------------------------------------------- sense */

async function sense(): Promise<number> {
  heading('Omni :: perception cycle');
  if (!getGrokClient().configured) {
    console.error(`${RED}XAI_API_KEY is not set; the agent cannot perceive anything.${RESET}`);
    return 1;
  }

  // Constructed directly rather than through the runtime: a one-shot cycle
  // should not require booting a database and every platform connector.
  const { SenseLoopService } = await import('./plugins/sense-loop/index.js');
  const service = new SenseLoopService(undefined as never);

  console.log(`${DIM}watching: ${config.sense.topics.join(', ')}${RESET}\n`);
  const result = await service.runCycle();

  console.log(
    `\ncandidates ${result.candidates} · drafted ${result.drafted} · ` +
      `queued ${result.queued} · posted ${result.posted}`,
  );
  if (result.queued > 0) console.log(`${DIM}review them with: npm run queue${RESET}`);
  await getCortexClient().save();
  return 0;
}

/** ---------------------------------------------------------------- queue */

async function reviewQueue(): Promise<number> {
  const queue = getQueue();
  const pending = await queue.pending();

  if (pending.length === 0) {
    heading('Omni :: queue');
    console.log(`${DIM}  nothing pending${RESET}\n`);
    const summary = await queue.summary();
    console.log(
      `  posted ${summary.posted} · rejected ${summary.rejected} · expired ${summary.expired}\n`,
    );
    return 0;
  }

  heading(`Omni :: queue  ${DIM}(${pending.length} awaiting decision)${RESET}`);
  const rl = createInterface({ input: stdin, output: stdout });

  try {
    for (const proposal of pending) {
      const scores = proposal.scores;
      console.log(`\n${BOLD}[${proposal.id}]${RESET} ${DIM}${proposal.topic}${RESET}`);
      console.log(`\n  ${finalText(proposal)}\n`);
      console.log(`${DIM}  why: ${proposal.rationale}${RESET}`);
      console.log(
        `${DIM}  taste ${scores.taste.toFixed(2)} · salience ${scores.salience.toFixed(2)} · ` +
          `novelty ${scores.novelty.toFixed(2)} · priority ${scores.priority.toFixed(2)}${RESET}`,
      );
      if (proposal.sources[0]) console.log(`${DIM}  re: ${proposal.sources[0]}${RESET}`);

      const answer = (
        await rl.question(`\n  ${GREEN}[a]${RESET}pprove  ${RED}[r]${RESET}eject  [e]dit  [s]kip  [q]uit > `)
      )
        .trim()
        .toLowerCase();

      if (answer === 'q') break;
      if (answer === 's' || answer === '') continue;

      if (answer === 'e') {
        const edited = (await rl.question('  rewrite > ')).trim();
        if (!edited) {
          console.log(`${DIM}  empty edit, skipping${RESET}`);
          continue;
        }
        const result = await applyDecision(proposal.id, 'approved', 'cli', edited);
        console.log(`  ${result.ok ? GREEN : RED}${result.message}${RESET}`);
        continue;
      }

      if (answer === 'a' || answer === 'r') {
        const decision = answer === 'a' ? 'approved' : 'rejected';
        const result = await applyDecision(proposal.id, decision, 'cli');
        console.log(`  ${result.ok ? GREEN : RED}${result.message}${RESET}`);
      }
    }
  } finally {
    rl.close();
    await getCortexClient().save();
  }

  console.log();
  return 0;
}

/** -------------------------------------------------------------- x-auth */

/**
 * Run the X OAuth 2.0 browser flow.
 *
 * This is the route to a working write credential that does not depend on the
 * OAuth 1.0a access token pair, which the X portal invalidates whenever app
 * permissions change.
 */
async function xAuth(): Promise<number> {
  heading('Omni :: authorise X (OAuth 2.0)');

  if (!config.twitter.oauth2.configured) {
    console.error(
      `${RED}TWITTER_CLIENT_ID and TWITTER_CLIENT_SECRET must be set in .env.${RESET}\n` +
        `${DIM}Find them in the X developer portal under your app -> "Keys and tokens" ->\n` +
        `"OAuth 2.0 Client ID and Client Secret".${RESET}`,
    );
    return 1;
  }

  const { runAuthFlow, loadTokens, REQUIRED_SCOPES } = await import('./plugins/twitter/oauth2.js');

  const existing = await loadTokens();
  if (existing) {
    console.log(
      `${DIM}  already authorised${existing.screenName ? ` as @${existing.screenName}` : ''}; ` +
        `re-running replaces the stored tokens${RESET}`,
    );
  }

  console.log(`${DIM}  callback: ${config.twitter.oauth2.callback}${RESET}`);
  console.log(`${DIM}  scopes:   ${REQUIRED_SCOPES.join(', ')}${RESET}`);
  console.log(
    `\n${YELLOW}  This callback URL must be registered on the app in the X portal,\n` +
      `  under "User authentication settings" -> Callback URI. An exact match,\n` +
      `  including the path and the absence of a trailing slash.${RESET}\n`,
  );

  try {
    const tokens = await runAuthFlow((url) => {
      console.log(`${BOLD}  Open this to authorise:${RESET}\n\n  ${url}\n`);
      console.log(`${DIM}  waiting for the callback...${RESET}`);
    });

    console.log(
      `\n${GREEN}  authorised${tokens.screenName ? ` as @${tokens.screenName}` : ''}${RESET}`,
    );
    console.log(`${DIM}  access token expires ${new Date(tokens.expiresAt).toLocaleString()}${RESET}`);
    console.log(
      tokens.refreshToken
        ? `${DIM}  refresh token stored -- the agent renews access automatically${RESET}`
        : `${YELLOW}  no refresh token issued; access will expire in ~2h and not renew.\n` +
            `  The offline.access scope was probably not granted.${RESET}`,
    );
    if (tokens.screenName) {
      console.log(`\n${DIM}  consider setting TWITTER_USERNAME=${tokens.screenName} in .env${RESET}`);
    }
    console.log(
      `\n${DIM}  posting still respects SCEMA_DRY_RUN. Set it to false when you want\n` +
        `  drafts to actually reach X.${RESET}\n`,
    );
    return 0;
  } catch (error) {
    console.error(`\n${RED}  ${(error as Error).message}${RESET}`);
    console.error(
      `${DIM}  Common causes: the callback URL is not registered on the app, the app has\n` +
        `  no "User authentication settings" configured yet, or the client secret is wrong.${RESET}\n`,
    );
    return 1;
  }
}

/** ----------------------------------------------------------------- main */

/**
 * Print exactly the block the model is given about the live bot.
 *
 * Not a prettier summary of it: the value of this command is that what the
 * operator reads and what the agent is reasoning from are the same characters.
 * A second renderer would drift, and the drift would be invisible until the
 * agent said something about the bot that the terminal disagreed with.
 */
async function botState(): Promise<number> {
  heading('Omni :: what I can see of the sniper');
  console.log(renderBotState(await readBotState()));
  console.log();
  return 0;
}

const COMMANDS: Record<string, () => Promise<number>> = {
  chat,
  sense,
  queue: reviewQueue,
  bot: botState,
  doctor,
  status: doctor,
  'x-auth': xAuth,
};

async function main(): Promise<void> {
  const command = process.argv[2] ?? 'chat';
  const handler = COMMANDS[command];

  if (!handler) {
    console.error(`unknown command: ${command}`);
    console.error(`available: ${Object.keys(COMMANDS).join(', ')}`);
    process.exit(2);
  }

  process.exit(await handler());
}

main().catch((error) => {
  console.error(`${RED}${(error as Error).stack ?? error}${RESET}`);
  process.exit(1);
});
