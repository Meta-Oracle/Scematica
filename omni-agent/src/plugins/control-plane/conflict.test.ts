/**
 * One bot, one poller.
 *
 * Telegram's `getUpdates` hands each update to exactly one caller. Two
 * processes on one token therefore do not both receive the operator's
 * commands — they split them, silently and at random, and one of the two
 * processes here is `scema-tgbot`, which can sell positions. The guard is
 * cheap; the failure is intermittent, invisible from the operator's side, and
 * lands on live money.
 *
 * These cases pin the two halves of the guard: the error is recognised as its
 * own kind, and it carries the fix rather than just the symptom.
 */
import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import { TelegramConflictError } from './notify.js';

describe('shared-token conflict', () => {
  it('is a distinct error type, so the retry loop can single it out', () => {
    const error = new TelegramConflictError('Conflict: terminated by other getUpdates request');
    assert.ok(error instanceof TelegramConflictError);
    assert.ok(error instanceof Error);
    assert.equal(error.name, 'TelegramConflictError');
  });

  it('names the remedy, not just the symptom', () => {
    // The operator's instinct on any Telegram failure is to go and check the
    // token. The token is fine, which is exactly why the message has to say
    // what to do instead.
    const message = new TelegramConflictError('Conflict').message;
    assert.match(message, /SCEMA_AGENT_TG_TOKEN/);
    assert.match(message, /scema-tgbot/);
    assert.match(message, /exactly once/);
  });

  it('preserves what Telegram actually said', () => {
    const message = new TelegramConflictError('terminated by other getUpdates request').message;
    assert.match(message, /terminated by other getUpdates request/);
  });
});
