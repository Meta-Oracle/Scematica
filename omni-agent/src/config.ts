/**
 * Configuration, validated once at boot.
 *
 * The rule here: the agent must be able to start and be *useful* with almost
 * nothing configured, but it must never quietly pretend to do something it
 * cannot. A missing Telegram token disables the cockpit and says so; a missing
 * X credential forces dry-run and says so. Nothing silently no-ops.
 */
import { config as loadEnv } from 'dotenv';
import { z } from 'zod';

/**
 * dotenv never overwrites a variable that is already in the environment.
 *
 * That is the documented behaviour and usually the right one, but it is a
 * quiet hazard here: a stale `TWITTER_USERNAME` left in a shell profile
 * silently wins over `.env`, and the agent then reports -- and could act as --
 * the wrong account. Capture what the file *said* so the mismatch can be
 * surfaced rather than discovered after a post goes to the wrong place.
 */
const loaded = loadEnv();
const fileValues: Record<string, string> = loaded.parsed ?? {};

/** Variables where .env was overruled by a pre-existing shell variable. */
export const shadowedByShell: Array<{ name: string; shell: string; file: string }> = Object.entries(
  fileValues,
)
  .filter(([name, fileValue]) => {
    const live = process.env[name];
    return live !== undefined && live !== fileValue && fileValue !== '';
  })
  .map(([name, fileValue]) => ({
    name,
    shell: process.env[name] ?? '',
    file: fileValue,
  }));

const bool = (value: string | undefined, fallback: boolean): boolean => {
  if (value === undefined || value.trim() === '') return fallback;
  return ['1', 'true', 'yes', 'on'].includes(value.trim().toLowerCase());
};

const int = (value: string | undefined, fallback: number): number => {
  const parsed = Number.parseInt(value ?? '', 10);
  return Number.isFinite(parsed) ? parsed : fallback;
};

const csv = (value: string | undefined, fallback: string[]): string[] => {
  if (!value || !value.trim()) return fallback;
  return value
    .split(',')
    .map((entry) => entry.trim())
    .filter(Boolean);
};

/** X API credentials. plugin-twitter speaks the official API, not scraping. */
const twitterSchema = z.object({
  apiKey: z.string().min(1),
  apiSecretKey: z.string().min(1),
  accessToken: z.string().min(1),
  accessTokenSecret: z.string().min(1),
});

export type TwitterCredentials = z.infer<typeof twitterSchema>;

const rawTwitter = {
  apiKey: process.env.TWITTER_API_KEY ?? '',
  apiSecretKey: process.env.TWITTER_API_SECRET_KEY ?? process.env.TWITTER_API_SECRET ?? '',
  accessToken: process.env.TWITTER_ACCESS_TOKEN ?? '',
  accessTokenSecret: process.env.TWITTER_ACCESS_TOKEN_SECRET ?? '',
};

const twitterParsed = twitterSchema.safeParse(rawTwitter);

const xaiKey = process.env.XAI_API_KEY ?? process.env.GROK_API_KEY ?? '';

export const config = {
  agentName: process.env.SCEMA_AGENT_NAME ?? 'Omni',

  xai: {
    apiKey: xaiKey,
    configured: xaiKey.length > 0,
    baseUrl: process.env.XAI_BASE_URL ?? 'https://api.x.ai/v1',
    model: process.env.GROK_MODEL ?? 'grok-4.6',
    smallModel: process.env.GROK_SMALL_MODEL ?? process.env.GROK_MODEL ?? 'grok-4.6',
    temperature: Number.parseFloat(process.env.GROK_TEMPERATURE ?? '0.8'),
    maxTokens: int(process.env.GROK_MAX_TOKENS, 2048),
    /** Wall-clock ceiling for one xAI call. Live search legitimately takes a while. */
    timeoutMs: int(process.env.GROK_TIMEOUT_MS, 120_000),
  },

  cortex: {
    url: process.env.SCEMA_CORTEX_URL ?? 'http://127.0.0.1:7077',
    timeoutMs: int(process.env.SCEMA_CORTEX_TIMEOUT_MS, 20_000),
    /** When the cortex is unreachable, fall back to neutral scores rather than stalling. */
    required: bool(process.env.SCEMA_CORTEX_REQUIRED, false),
  },

  telegram: {
    token: process.env.SCEMA_TG_TOKEN ?? '',
    configured: (process.env.SCEMA_TG_TOKEN ?? '').length > 0,
    /**
     * Only this chat may approve, reject or edit drafts. Everyone else gets
     * ordinary conversation. Without it the cockpit stays read-only, because
     * an unrestricted approval channel is an open door to the account.
     */
    operatorChatId: process.env.SCEMA_TG_OPERATOR_CHAT_ID ?? '',
  },

  twitter: {
    credentials: twitterParsed.success ? twitterParsed.data : null,
    configured: twitterParsed.success,
    /**
     * App-only auth. Sufficient for reading public metrics, which is all the
     * reflection loop needs -- but it cannot create posts. X rejects
     * user-context endpoints with "Authenticating with OAuth 2.0
     * Application-Only is forbidden for this endpoint", so posting requires
     * the OAuth 1.0a access token pair above regardless of this being set.
     */
    bearerToken: process.env.TWITTER_BEARER_TOKEN ?? '',

    /**
     * OAuth 2.0 (Authorization Code + PKCE).
     *
     * A parallel mechanism to the OAuth 1.0a keys, not a replacement for the
     * bearer token. The client id/secret here cannot post on their own: they
     * authorise a browser flow that mints a user access token, and only that
     * token can write. Run `npx tsx src/cli.ts x-auth` to perform it.
     *
     * Worth having as well as OAuth 1.0a because it is the path that does not
     * depend on the access token pair the portal keeps invalidating.
     */
    oauth2: {
      clientId: process.env.TWITTER_CLIENT_ID ?? '',
      clientSecret: process.env.TWITTER_CLIENT_SECRET ?? '',
      callback: process.env.TWITTER_OAUTH2_CALLBACK ?? 'http://localhost:3000/callback',
      configured: Boolean(process.env.TWITTER_CLIENT_ID && process.env.TWITTER_CLIENT_SECRET),
    },
    /**
     * Dry-run is the default and stays forced while credentials are missing.
     * Going live is an explicit, deliberate act: set SCEMA_DRY_RUN=false *and*
     * provide four real credentials.
     */
    dryRun: bool(process.env.SCEMA_DRY_RUN, true) || !twitterParsed.success,
    handle: process.env.TWITTER_USERNAME ?? '',
  },

  sense: {
    enabled: bool(process.env.SCEMA_SENSE_ENABLED, true),
    /** Minutes between perception cycles. */
    intervalMinutes: int(process.env.SCEMA_SENSE_INTERVAL_MINUTES, 30),
    /** What the agent watches. These become X Search queries. */
    topics: csv(process.env.SCEMA_SENSE_TOPICS, [
      'AI agents',
      'open source LLM tooling',
      'inference performance',
    ]),
    /** Candidates pulled per cycle before the cortex ranks them. */
    maxCandidates: int(process.env.SCEMA_SENSE_MAX_CANDIDATES, 25),
    /** Drafts proposed per cycle after ranking. Kept small on purpose. */
    maxDrafts: int(process.env.SCEMA_SENSE_MAX_DRAFTS, 3),
    /**
     * A draft must clear this combined cortex priority to reach the queue.
     * Starts permissive: an untrained net has no opinion worth trusting yet,
     * and the operator's early decisions are what teach it.
     */
    minPriority: Number.parseFloat(process.env.SCEMA_SENSE_MIN_PRIORITY ?? '0.35'),
    /**
     * Taste above this can post without asking, once the net has earned it.
     * Default 1.01 means "never" -- autonomy is opt-in, not the starting state.
     */
    autoPostTaste: Number.parseFloat(process.env.SCEMA_SENSE_AUTOPOST_TASTE ?? '1.01'),
    /** Minimum training events before autoPostTaste is honoured at all. */
    autoPostMinEvents: int(process.env.SCEMA_SENSE_AUTOPOST_MIN_EVENTS, 200),
  },

  paths: {
    data: process.env.SCEMA_DATA_DIR ?? 'data',
    queue: process.env.SCEMA_QUEUE_PATH ?? 'data/queue/proposals.jsonl',
  },
} as const;

export type AgentConfig = typeof config;

/** Human-readable boot report. Printed once, so misconfiguration is obvious. */
export function describeConfig(): string[] {
  const lines: string[] = [];
  lines.push(
    config.xai.configured
      ? `  grok          ${config.xai.model} via ${config.xai.baseUrl}`
      : `  grok          NOT CONFIGURED (set XAI_API_KEY) -- chat and sense loop disabled`,
  );
  lines.push(`  cortex        ${config.cortex.url}${config.cortex.required ? ' (required)' : ''}`);
  lines.push(
    config.telegram.configured
      ? `  telegram      enabled${config.telegram.operatorChatId ? ` (operator ${config.telegram.operatorChatId})` : ' -- NO OPERATOR SET, approvals disabled'}`
      : `  telegram      NOT CONFIGURED (set SCEMA_TG_TOKEN)`,
  );
  if (config.twitter.dryRun) {
    const why = config.twitter.configured
      ? 'SCEMA_DRY_RUN is on'
      : 'no X API credentials -- posting is impossible without them';
    lines.push(`  twitter       DRY RUN (${why}); drafts queue to ${config.paths.queue}`);
  } else {
    // Deliberately loud. Everything else in this report is informational;
    // this line means approved drafts leave the machine.
    lines.push(`  twitter       *** LIVE *** approved drafts are posted to X for real`);
    lines.push(`                run with SCEMA_DRY_RUN=true to stop that`);
  }

  // A shell variable quietly beating .env is how an agent ends up acting as
  // the wrong account, so it is reported at the top rather than buried.
  for (const shadow of shadowedByShell) {
    const secretish = /TOKEN|SECRET|KEY|PASSWORD/i.test(shadow.name);
    const show = (value: string): string => (secretish ? `${value.slice(0, 6)}...` : value);
    lines.push(
      `  !! ${shadow.name} comes from your shell (${show(shadow.shell)}), ` +
        `overriding .env (${show(shadow.file)})`,
    );
  }
  lines.push(
    config.sense.enabled
      ? `  sense loop    every ${config.sense.intervalMinutes}m on [${config.sense.topics.join(', ')}]`
      : `  sense loop    disabled`,
  );
  return lines;
}

/**
 * The handle the agent will actually post as, taken from the authorised
 * OAuth 2.0 token rather than from configuration.
 *
 * TWITTER_USERNAME is a label a human typed and can be stale or shadowed; the
 * token is ground truth about which account will receive the post.
 */
export async function resolveActingAccount(): Promise<{
  handle: string | null;
  source: 'oauth2-token' | 'config' | 'unknown';
  mismatch: boolean;
}> {
  try {
    const { loadTokens } = await import('./plugins/twitter/oauth2.js');
    const tokens = await loadTokens();
    if (tokens?.screenName) {
      return {
        handle: tokens.screenName,
        source: 'oauth2-token',
        mismatch: Boolean(
          config.twitter.handle &&
            config.twitter.handle.toLowerCase() !== tokens.screenName.toLowerCase(),
        ),
      };
    }
  } catch {
    // fall through to configuration
  }
  return {
    handle: config.twitter.handle || null,
    source: config.twitter.handle ? 'config' : 'unknown',
    mismatch: false,
  };
}
