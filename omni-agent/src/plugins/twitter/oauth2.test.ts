/**
 * OAuth 2.0 token storage and expiry logic.
 *
 * The refresh path is the part worth testing hardest: X rotates the refresh
 * token on every refresh, so failing to persist the new one strands the agent
 * and forces the operator back through a browser flow. That failure would only
 * appear hours after deploy, which is exactly the kind that needs a test.
 */
import assert from 'node:assert/strict';
import { readFile, rm } from 'node:fs/promises';
import { describe, it, beforeEach } from 'node:test';

import { config } from '../../config.js';
import { loadTokens, saveTokens, hasOAuth2Tokens, REQUIRED_SCOPES } from './oauth2.js';

const TOKEN_PATH = `${config.paths.data}/x-oauth2.json`;
const HOUR = 3600_000;

async function clearTokens(): Promise<void> {
  await rm(TOKEN_PATH, { force: true });
}

describe('OAuth 2.0 scopes', () => {
  it('requests offline.access, without which no refresh token is issued', () => {
    assert.ok(
      REQUIRED_SCOPES.includes('offline.access'),
      'dropping offline.access silently limits the agent to a 2-hour lifetime',
    );
  });

  it('requests tweet.write, without which it cannot post', () => {
    assert.ok(REQUIRED_SCOPES.includes('tweet.write'));
    assert.ok(REQUIRED_SCOPES.includes('tweet.read'));
    assert.ok(REQUIRED_SCOPES.includes('users.read'));
  });
});

describe('token storage', () => {
  beforeEach(clearTokens);

  it('returns null rather than throwing when nothing is stored', async () => {
    assert.equal(await loadTokens(), null);
    assert.equal(await hasOAuth2Tokens(), false);
  });

  it('round-trips a full token record', async () => {
    const tokens = {
      accessToken: 'access-abc',
      refreshToken: 'refresh-xyz',
      expiresAt: Date.now() + 2 * HOUR,
      screenName: 'omni',
      userId: '123',
      obtainedAt: Date.now(),
    };
    await saveTokens(tokens);

    const loaded = await loadTokens();
    assert.deepEqual(loaded, tokens);
    assert.equal(await hasOAuth2Tokens(), true);
  });

  it('overwrites cleanly, leaving no stale token behind', async () => {
    await saveTokens({
      accessToken: 'first',
      refreshToken: 'r1',
      expiresAt: Date.now() + HOUR,
      obtainedAt: Date.now(),
    });
    await saveTokens({
      accessToken: 'second',
      refreshToken: 'r2',
      expiresAt: Date.now() + HOUR,
      obtainedAt: Date.now(),
    });

    const loaded = await loadTokens();
    assert.equal(loaded?.accessToken, 'second');
    assert.equal(loaded?.refreshToken, 'r2');

    // The rotated refresh token must not survive anywhere in the file.
    const raw = await readFile(TOKEN_PATH, 'utf8');
    assert.doesNotMatch(raw, /"r1"/, 'the superseded refresh token is still on disk');
    assert.doesNotMatch(raw, /first/);
  });

  it('leaves no temp file after a write', async () => {
    await saveTokens({
      accessToken: 'a',
      expiresAt: Date.now() + HOUR,
      obtainedAt: Date.now(),
    });
    // The atomic write renames its temp file; a leftover means the rename
    // did not happen and a crash could strand a half-written token.
    await assert.rejects(() => readFile(`${TOKEN_PATH}.tmp`, 'utf8'));
  });

  it('survives a corrupt token file instead of crashing the agent', async () => {
    const { writeFile, mkdir } = await import('node:fs/promises');
    const { dirname } = await import('node:path');
    await mkdir(dirname(TOKEN_PATH), { recursive: true });
    await writeFile(TOKEN_PATH, '{ this is not json', 'utf8');

    assert.equal(await loadTokens(), null, 'a corrupt file must read as "not authorised"');
  });

  it('stores an expiry far enough ahead to be usable', async () => {
    const tokens = {
      accessToken: 'a',
      refreshToken: 'r',
      expiresAt: Date.now() + 2 * HOUR,
      obtainedAt: Date.now(),
    };
    await saveTokens(tokens);
    const loaded = await loadTokens();

    assert.ok(loaded!.expiresAt > Date.now(), 'token must not be born expired');
    // The client refreshes 5 minutes early; an expiry inside that margin would
    // mean every single call triggers a refresh.
    assert.ok(loaded!.expiresAt - Date.now() > 5 * 60_000);
  });
});
