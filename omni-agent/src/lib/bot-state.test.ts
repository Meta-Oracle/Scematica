/**
 * What the agent may say about the live bot.
 *
 * Every case here asserts something does *not* happen. The module's whole job
 * is to stop three different kinds of nothing — no directory, no file, an old
 * file — from reaching the model as a number, because the model will state a
 * number it is given and a stated PnL that nobody measured is the most
 * expensive sentence this project can emit.
 */
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, utimesSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';

import {
  botIdOf,
  pollingConflict,
  readBotState,
  renderBotState,
  type BotState,
  type Metrics,
} from './bot-state.js';

const METRICS: Metrics = {
  trades_attempted: 40,
  trades_confirmed: 26,
  trades_failed: 14,
  arb_opportunities_found: 9,
  arb_executed: 2,
  total_pnl_lamports: 1_950_000_000,
  pools_tracked: 812,
  uptime_secs: 7_200,
};

const unwired: BotState = {
  dir: null,
  metrics: { freshness: 'absent' },
  nn: { freshness: 'absent' },
};

test('an unwired bot produces no figures at all', () => {
  const text = renderBotState(unwired);
  assert.match(text, /NOT WIRED/);
  // The exact failure being designed against: a zero that reads as a measurement.
  assert.doesNotMatch(text, /0\.00|0 SOL/);
  assert.match(text, /Do not estimate/);
});

test('an absent metrics file is not a bot that broke even', () => {
  const text = renderBotState({ dir: '/bot', metrics: { freshness: 'absent' }, nn: { freshness: 'absent' } });
  assert.match(text, /METRICS ABSENT/);
  assert.match(text, /not a bot that traded nothing/);
  assert.doesNotMatch(text, /pnl/);
});

test('a stale reading keeps its value and gains an age', () => {
  const text = renderBotState({
    dir: '/bot',
    metrics: { freshness: 'stale', value: METRICS, ageSecs: 4000 },
    nn: { freshness: 'absent' },
  });
  // Stale is not dropped: the numbers are real, they are just old.
  assert.match(text, /1\.9500 SOL/);
  assert.match(text, /METRICS STALE/);
  assert.match(text, /67m ago/);
});

test('a fresh reading states the measurement and its age', () => {
  const text = renderBotState({
    dir: '/bot',
    metrics: { freshness: 'fresh', value: METRICS, ageSecs: 3 },
    nn: { freshness: 'absent' },
  });
  assert.match(text, /METRICS measured 3s ago/);
  assert.match(text, /26\/40 confirmed/);
  assert.match(text, /65\.0%/);
});

test('a win rate over zero attempts is an em dash, never 0%', () => {
  const text = renderBotState({
    dir: '/bot',
    metrics: {
      freshness: 'fresh',
      value: { ...METRICS, trades_attempted: 0, trades_confirmed: 0 },
      ageSecs: 1,
    },
    nn: { freshness: 'absent' },
  });
  assert.match(text, /win rate —/);
  assert.doesNotMatch(text, /win rate 0\.0%/);
});

test('a missing DQ* field is an em dash, not a zero', () => {
  const text = renderBotState({
    dir: '/bot',
    metrics: { freshness: 'absent' },
    // `epsilon` omitted entirely: a writer that did not record it and a net
    // that has fully annealed are different facts.
    nn: { freshness: 'fresh', value: { train_steps: 12_000 }, ageSecs: 10 },
  });
  assert.match(text, /epsilon —/);
  assert.doesNotMatch(text, /epsilon 0\.000/);
});

test('readBotState reports absent, not an error, for a directory with no bot', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'scema-bot-'));
  const state = await readBotState(dir);
  assert.equal(state.metrics.freshness, 'absent');
  assert.equal(state.metrics.value, undefined);
});

test('an old metrics file reads as stale rather than fresh', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'scema-bot-'));
  const path = join(dir, 'scematica-metrics.json');
  writeFileSync(path, JSON.stringify(METRICS));
  // The sniper rewrites this every 5 seconds, so an hour old means stopped.
  const hourAgo = new Date(Date.now() - 3_600_000);
  utimesSync(path, hourAgo, hourAgo);

  const state = await readBotState(dir);
  assert.equal(state.metrics.freshness, 'stale');
  assert.equal(state.metrics.value?.pools_tracked, 812);
  assert.ok((state.metrics.ageSecs ?? 0) > 3000);
});

test('a corrupt metrics file is absent, never a partial reading', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'scema-bot-'));
  // A half-written file is what a reader sees mid-rename on a system that does
  // not honour the atomic-rename convention; it must not become a number.
  writeFileSync(join(dir, 'scematica-metrics.json'), '{"trades_attempted": 4');
  const state = await readBotState(dir);
  assert.equal(state.metrics.freshness, 'absent');
});

/* ── one bot, one poller ──────────────────────────────────────────────────── */

test('a bot id is the part of a token before the colon, never the secret', () => {
  assert.equal(botIdOf('8849814959:AAEg4WurVrJwvq9C'), 8849814959);
  // Not zero and not a guess: two malformed tokens must not collide on a default and be
  // reported as the same bot.
  assert.equal(botIdOf('nonsense'), null);
  assert.equal(botIdOf('abc:def'), null);
  assert.equal(botIdOf(''), null);
});

test('no bot directory means nothing to go on, not "no conflict"', async () => {
  assert.equal(await pollingConflict('8849814959:x', ''), null);
});

test('a live announcement for the same bot is a conflict', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'scema-bot-'));
  // `process.pid` is by definition alive, which is what makes this testable at all.
  writeFileSync(
    join(dir, 'scematica-tgbot-presence.json'),
    JSON.stringify({ bot_id: 8849814959, username: 'Scematicabot', pid: process.pid, started_at: '' }),
  );
  const conflict = await pollingConflict('8849814959:secret', dir);
  assert.equal(conflict?.username, 'Scematicabot');
});

test('an announcement for a DIFFERENT bot is not a conflict', async () => {
  // The whole point of giving the cockpit its own bot. Two pollers, two tokens, no clash.
  const dir = mkdtempSync(join(tmpdir(), 'scema-bot-'));
  writeFileSync(
    join(dir, 'scematica-tgbot-presence.json'),
    JSON.stringify({ bot_id: 111, username: 'Scematicabot', pid: process.pid, started_at: '' }),
  );
  assert.equal(await pollingConflict('222:secret', dir), null);
});

test('a stale announcement from a dead process is not a conflict', async () => {
  // A crash leaves the file behind. Refusing to poll on the strength of it would disable
  // the cockpit permanently after one hard kill, which is why the pid is checked.
  const dir = mkdtempSync(join(tmpdir(), 'scema-bot-'));
  writeFileSync(
    join(dir, 'scematica-tgbot-presence.json'),
    // Above the 32-bit pid ceiling, so it cannot belong to a live process on any platform.
    JSON.stringify({ bot_id: 8849814959, username: 'Scematicabot', pid: 4294967294, started_at: '' }),
  );
  assert.equal(await pollingConflict('8849814959:secret', dir), null);
});

test('a corrupt announcement is not a conflict', async () => {
  const dir = mkdtempSync(join(tmpdir(), 'scema-bot-'));
  writeFileSync(join(dir, 'scematica-tgbot-presence.json'), '{"bot_id":');
  assert.equal(await pollingConflict('8849814959:secret', dir), null);
});
