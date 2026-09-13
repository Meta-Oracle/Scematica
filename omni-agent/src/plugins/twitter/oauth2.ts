/**
 * X OAuth 2.0 (Authorization Code + PKCE).
 *
 * Why this exists alongside the OAuth 1.0a path: the client id and secret
 * cannot post. They authorise a browser flow that mints a *user* access
 * token, and only that token can write. It is the route that does not depend
 * on the OAuth 1.0a access token pair, which the X portal silently
 * invalidates whenever app permissions change.
 *
 * Token lifetimes are the part that bites:
 *
 *   access token   expires after ~2 hours
 *   refresh token  durable, but **rotates on every refresh** -- the old one
 *                  stops working the moment a new one is issued
 *
 * That rotation is why tokens live in a JSON file rather than .env. A
 * long-running agent refreshes many times a day, and an env var that must be
 * rewritten on every refresh would either go stale or require the process to
 * edit its own configuration. The file is written atomically, because losing
 * the refresh token mid-write means going back through the browser flow.
 */
import { mkdir, readFile, rename, writeFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { dirname } from 'node:path';

import { TwitterApi } from 'twitter-api-v2';

import { config } from '../../config.js';

/** Scopes: read and write posts, identify the user, and keep a refresh token. */
export const REQUIRED_SCOPES = [
  'tweet.read',
  'tweet.write',
  'users.read',
  'offline.access', // without this, no refresh token is issued at all
] as const;

const TOKEN_PATH = `${config.paths.data}/x-oauth2.json`;

// Refresh this long before actual expiry, so a request never races the clock.
const REFRESH_MARGIN_MS = 5 * 60_000;

export interface StoredTokens {
  accessToken: string;
  refreshToken?: string;
  /** Epoch milliseconds. */
  expiresAt: number;
  screenName?: string;
  userId?: string;
  obtainedAt: number;
}

export async function loadTokens(): Promise<StoredTokens | null> {
  try {
    return JSON.parse(await readFile(TOKEN_PATH, 'utf8')) as StoredTokens;
  } catch {
    return null;
  }
}

export async function saveTokens(tokens: StoredTokens): Promise<void> {
  await mkdir(dirname(TOKEN_PATH), { recursive: true });
  // Write-then-rename: a crash mid-write must not leave a truncated file where
  // the refresh token used to be.
  const tmp = `${TOKEN_PATH}.tmp`;
  await writeFile(tmp, JSON.stringify(tokens, null, 2), 'utf8');
  await rename(tmp, TOKEN_PATH);
}

function appClient(): TwitterApi {
  return new TwitterApi({
    clientId: config.twitter.oauth2.clientId,
    clientSecret: config.twitter.oauth2.clientSecret,
  });
}

export interface AuthLink {
  url: string;
  codeVerifier: string;
  state: string;
}

export function generateAuthLink(): AuthLink {
  const { url, codeVerifier, state } = appClient().generateOAuth2AuthLink(
    config.twitter.oauth2.callback,
    { scope: [...REQUIRED_SCOPES] },
  );
  return { url, codeVerifier, state };
}

export async function completeAuth(code: string, codeVerifier: string): Promise<StoredTokens> {
  const result = await appClient().loginWithOAuth2({
    code,
    codeVerifier,
    redirectUri: config.twitter.oauth2.callback,
  });

  const tokens: StoredTokens = {
    accessToken: result.accessToken,
    expiresAt: Date.now() + (result.expiresIn ?? 7200) * 1000,
    obtainedAt: Date.now(),
  };
  if (result.refreshToken) tokens.refreshToken = result.refreshToken;

  // Record who authorised, so `doctor` can show it and a wrong-account
  // authorisation is caught immediately rather than at first post.
  try {
    const me = await result.client.v2.me();
    tokens.screenName = me.data.username;
    tokens.userId = me.data.id;
  } catch {
    // Identity is a convenience; a billing-blocked project still authorises.
  }

  await saveTokens(tokens);
  return tokens;
}

/**
 * A client authorised as the user, refreshing the token when it is close to
 * expiry. Returns null when the flow has not been run.
 */
export async function getOAuth2Client(): Promise<{
  client: TwitterApi;
  tokens: StoredTokens;
} | null> {
  const tokens = await loadTokens();
  if (!tokens) return null;

  if (Date.now() < tokens.expiresAt - REFRESH_MARGIN_MS) {
    return { client: new TwitterApi(tokens.accessToken), tokens };
  }

  if (!tokens.refreshToken) {
    // Expired with no way back: the flow was run without offline.access.
    return null;
  }

  const refreshed = await appClient().refreshOAuth2Token(tokens.refreshToken);
  const updated: StoredTokens = {
    ...tokens,
    accessToken: refreshed.accessToken,
    expiresAt: Date.now() + (refreshed.expiresIn ?? 7200) * 1000,
    obtainedAt: Date.now(),
  };
  // The refresh token rotates; persist the new one or the next refresh fails.
  if (refreshed.refreshToken) updated.refreshToken = refreshed.refreshToken;

  await saveTokens(updated);
  return { client: new TwitterApi(updated.accessToken), tokens: updated };
}

export async function hasOAuth2Tokens(): Promise<boolean> {
  return (await loadTokens()) !== null;
}

/**
 * Run the browser flow, capturing the callback on a temporary local server.
 *
 * Capturing the redirect beats asking the operator to paste a code out of the
 * address bar: the code is single-use and expires in seconds, and a paste that
 * grabs the wrong query parameter fails in a way that looks like a bad client
 * secret.
 */
export async function runAuthFlow(
  onUrl: (url: string) => void,
  timeoutMs = 300_000,
): Promise<StoredTokens> {
  const { url, codeVerifier, state } = generateAuthLink();

  let callbackUrl: URL;
  try {
    callbackUrl = new URL(config.twitter.oauth2.callback);
  } catch {
    throw new Error(
      `TWITTER_OAUTH2_CALLBACK is not a valid URL: ${config.twitter.oauth2.callback}`,
    );
  }
  const port = Number(callbackUrl.port || 80);

  return new Promise<StoredTokens>((resolve, reject) => {
    const server = createServer((req, res) => {
      const incoming = new URL(req.url ?? '/', `http://localhost:${port}`);
      if (incoming.pathname !== callbackUrl.pathname) {
        res.writeHead(404).end('not the callback path');
        return;
      }

      const code = incoming.searchParams.get('code');
      const returnedState = incoming.searchParams.get('state');
      const error = incoming.searchParams.get('error');

      const finish = (status: number, message: string): void => {
        res.writeHead(status, { 'Content-Type': 'text/html; charset=utf-8' });
        res.end(
          `<!doctype html><meta charset="utf-8"><title>Scematica Omni-Agent</title>` +
            `<body style="font:16px system-ui;padding:3rem;max-width:34rem">` +
            `<h1 style="font-size:1.2rem">${message}</h1>` +
            `<p style="color:#666">You can close this tab and return to the terminal.</p>`,
        );
      };

      if (error) {
        finish(400, `Authorisation denied: ${error}`);
        server.close();
        reject(new Error(`authorisation denied: ${error}`));
        return;
      }
      // State is the CSRF guard; a mismatch means this callback is not ours.
      if (!code || returnedState !== state) {
        finish(400, 'Invalid callback (state mismatch).');
        server.close();
        reject(new Error('callback state did not match; aborting'));
        return;
      }

      completeAuth(code, codeVerifier)
        .then((tokens) => {
          finish(200, `Authorised${tokens.screenName ? ` as @${tokens.screenName}` : ''}.`);
          server.close();
          resolve(tokens);
        })
        .catch((exchangeError: Error) => {
          finish(500, `Token exchange failed: ${exchangeError.message}`);
          server.close();
          reject(exchangeError);
        });
    });

    server.on('error', (serverError: NodeJS.ErrnoException) => {
      reject(
        serverError.code === 'EADDRINUSE'
          ? new Error(`port ${port} is already in use; free it or change TWITTER_OAUTH2_CALLBACK`)
          : serverError,
      );
    });

    const timer = setTimeout(() => {
      server.close();
      reject(new Error('timed out waiting for authorisation'));
    }, timeoutMs);
    timer.unref();

    server.listen(port, () => onUrl(url));
  });
}
