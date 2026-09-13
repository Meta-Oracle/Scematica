/**
 * What the sniper is actually doing, read from the files it writes.
 *
 * Scematica's processes talk to each other through JSON files in the bot's
 * working directory and nothing else — no socket, no IPC channel. This module
 * is a fifth reader of that surface, beside the ratatui dashboard, the HTTP
 * API, the web dashboard and `scema-tgbot`. It is strictly read-only: it opens
 * no lock, writes nothing, and starting or stopping the bot is not its job.
 *
 * ## Why this exists at all
 *
 * The agent speaks in public about a live trading system. Without a way to read
 * the bot it has two options when asked how the bot is doing, and both are bad:
 * decline every such question, or produce a plausible number. So it reads.
 *
 * ## The rule the whole file is built around
 *
 * **Absent is not zero, and stale is not fresh.** Three states, never two:
 *
 * - the file is not there → the bot has not run in this directory. There is no
 *   PnL to report, and reporting `0.00 SOL` would be a claim that it broke even.
 * - the file is there and old → these are last-known numbers with an age on
 *   them. `scematica-metrics.json` is rewritten every 5 seconds, so a file an
 *   hour old means the sniper is stopped, not that nothing happened.
 * - the file is there and fresh → a measurement, and the only case where a bare
 *   number may be spoken.
 *
 * This is the same rule as `Provenance::Stale` in `scema-world`, the staleness
 * tolerance in `lib/alchem/`, and the Ψ gate in front of Scylar. It is restated
 * here rather than imported because nothing in this workspace can link to those.
 */
import { readFile, stat } from 'node:fs/promises';
import { join } from 'node:path';

import { config } from '../config.js';

/** Written every 5s by the sniper; past this the numbers are history. */
const METRICS_FRESH_MS = 60_000;
/** The NN stats file is written far less often, so it gets its own budget. */
const NN_FRESH_MS = 10 * 60_000;

export type Freshness = 'fresh' | 'stale' | 'absent';

export interface Reading<T> {
  freshness: Freshness;
  /** Present for `fresh` and `stale`, never for `absent`. */
  value?: T;
  /** How long ago the file was written, in seconds. Absent when unread. */
  ageSecs?: number;
}

/** Mirrors `MetricsSnapshot` in `crates/scematica-core/src/metrics.rs`. */
export interface Metrics {
  trades_attempted: number;
  trades_confirmed: number;
  trades_failed: number;
  arb_opportunities_found: number;
  arb_executed: number;
  total_pnl_lamports: number;
  pools_tracked: number;
  uptime_secs: number;
}

/** Mirrors what the NN agent writes to `scematica-nn-stats.json`. */
export interface NnStats {
  epsilon?: number;
  train_steps?: number;
  replay_size?: number;
  total_reward?: number;
}

export interface BotState {
  /** The directory that was read, or null when none is configured. */
  dir: string | null;
  metrics: Reading<Metrics>;
  nn: Reading<NnStats>;
}

async function readJson<T>(dir: string, name: string, freshMs: number): Promise<Reading<T>> {
  const path = join(dir, name);
  try {
    // stat first: the age is as much a part of the reading as the contents, and
    // a value with no age attached is one nobody can judge.
    const info = await stat(path);
    const ageSecs = Math.max(0, Math.round((Date.now() - info.mtimeMs) / 1000));
    const value = JSON.parse(await readFile(path, 'utf8')) as T;
    return {
      freshness: Date.now() - info.mtimeMs <= freshMs ? 'fresh' : 'stale',
      value,
      ageSecs,
    };
  } catch {
    // Missing, unreadable and unparseable all collapse to `absent` on purpose:
    // each of them means "no measurement", and inventing a fourth state the
    // renderer would have to interpret buys nothing a reader can act on.
    return { freshness: 'absent' };
  }
}

/**
 * Read the bot's current state. Never throws: a missing bot is an ordinary,
 * expected answer, not an error condition.
 *
 * `dir` is a parameter rather than read from `config` at the point of use so
 * the three states can be exercised against real files on disk. `config.ts`
 * reads the environment once at module load, so a test that set an env var
 * would be testing whatever the developer's `.env` happened to say.
 */
export async function readBotState(dir = config.paths.botState): Promise<BotState> {
  if (!dir) {
    return { dir: null, metrics: { freshness: 'absent' }, nn: { freshness: 'absent' } };
  }
  const [metrics, nn] = await Promise.all([
    readJson<Metrics>(dir, 'scematica-metrics.json', METRICS_FRESH_MS),
    readJson<NnStats>(dir, 'scematica-nn-stats.json', NN_FRESH_MS),
  ]);
  return { dir, metrics, nn };
}

/**
 * What `scema-tgbot` announces about the bot it is polling.
 *
 * Mirrors `Presence` in `crates/scematica-tgbot/src/presence.rs`. The `bot_id` is the
 * numeric part of a Telegram token, never the secret.
 */
export interface TgPresence {
  bot_id: number;
  username: string;
  pid: number;
  started_at: string;
}

/** The bot id carried by a token. `null` for anything not shaped `<digits>:<secret>`. */
export function botIdOf(token: string): number | null {
  const colon = token.indexOf(':');
  if (colon <= 0) return null;
  const head = token.slice(0, colon).trim();
  if (!/^\d+$/.test(head)) return null;
  return Number.parseInt(head, 10);
}

/** Is that process still running? */
function alive(pid: number): boolean {
  try {
    // Signal 0 performs the permission and existence checks without delivering anything.
    // It works on Windows too, where it is the documented way to ask this question.
    process.kill(pid, 0);
    return true;
  } catch (error) {
    // EPERM means the process exists and belongs to somebody else — still a conflict.
    // ESRCH means it is gone, and a leftover file from a crash must not look like one.
    return (error as NodeJS.ErrnoException).code === 'EPERM';
  }
}

/**
 * Is the sniper's Telegram bot currently polling the bot this token names?
 *
 * This is the guard that the environment-variable comparison in `config.ts` **cannot**
 * make. That one asks whether `SCEMA_AGENT_TG_TOKEN` equals `SCEMA_TG_TOKEN` in this
 * process — and the two live in separate `.env` files that are never loaded together, so
 * it is structurally unable to fire in the deployment it was written for. It did not fire,
 * against two processes pointed at the same bot.
 *
 * The processes share no language and no config, but they share a directory. So the Rust
 * side publishes which bot it holds, and this reads it — the File-Based IPC convention the
 * rest of the system already runs on, rather than a new mechanism.
 *
 * `null` means "nothing to go on", which is different from "no conflict": no bot directory
 * configured, no announcement, or an announcement from a process that has since exited.
 */
export async function pollingConflict(
  token: string,
  dir = config.paths.botState,
): Promise<TgPresence | null> {
  if (!dir || !token) return null;
  const mine = botIdOf(token);
  if (mine === null) return null;

  let presence: TgPresence;
  try {
    presence = JSON.parse(
      await readFile(join(dir, 'scematica-tgbot-presence.json'), 'utf8'),
    ) as TgPresence;
  } catch {
    return null;
  }

  if (typeof presence.bot_id !== 'number' || presence.bot_id !== mine) return null;
  // A stale file left by a crash names a pid nobody is running. Refusing to poll on the
  // strength of that would disable this cockpit permanently after one hard kill.
  if (typeof presence.pid !== 'number' || !alive(presence.pid)) return null;
  return presence;
}

const LAMPORTS_PER_SOL = 1_000_000_000;

function age(reading: Reading<unknown>): string {
  if (reading.ageSecs === undefined) return '';
  const secs = reading.ageSecs;
  if (secs < 90) return `${secs}s ago`;
  if (secs < 5400) return `${Math.round(secs / 60)}m ago`;
  return `${Math.round(secs / 3600)}h ago`;
}

/**
 * The block that goes into the model's context.
 *
 * Every line carries its own provenance. The header states the verdict in
 * words the model is instructed to repeat, because a header saying STALE and a
 * number underneath it is the shape a model will quote the number out of — so
 * the age is on the same line as the figures too.
 */
export function renderBotState(state: BotState): string {
  if (!state.dir) {
    return [
      '# Scematica bot state',
      'NOT WIRED. No bot directory is configured, so you have no figures about the live bot.',
      'If asked how the bot is doing, say you cannot see it and point at the sniper bot\'s /status.',
      'Do not estimate. Do not reason from past conversation to a current number.',
    ].join('\n');
  }

  const lines: string[] = ['# Scematica bot state'];

  const m = state.metrics;
  if (m.freshness === 'absent') {
    lines.push(
      'METRICS ABSENT. The sniper has not written its metrics file in this directory.',
      'This is not a bot that traded nothing — it is a bot you cannot see. Say that.',
    );
  } else {
    const v = m.value!;
    const pnl = (v.total_pnl_lamports / LAMPORTS_PER_SOL).toFixed(4);
    const winRate =
      v.trades_attempted > 0
        ? `${((v.trades_confirmed / v.trades_attempted) * 100).toFixed(1)}%`
        : '—';
    lines.push(
      m.freshness === 'stale'
        ? `METRICS STALE (written ${age(m)}). These are last-known numbers from a bot that is ` +
            'probably stopped. Quote them only with the age attached.'
        : `METRICS measured ${age(m)}:`,
      `  pnl ${pnl} SOL · trades ${v.trades_confirmed}/${v.trades_attempted} confirmed · ` +
        `win rate ${winRate} · failed ${v.trades_failed}`,
      `  arb ${v.arb_executed}/${v.arb_opportunities_found} executed · pools ${v.pools_tracked} · ` +
        `uptime ${Math.round(v.uptime_secs / 60)}m`,
    );
  }

  const n = state.nn;
  if (n.freshness === 'absent') {
    lines.push('DQ* AGENT: no stats file. You know nothing about the net; do not characterise it.');
  } else {
    const v = n.value!;
    const num = (x: number | undefined, digits: number): string =>
      // The em dash is the whole point: a field the writer omitted is not a
      // field whose value is zero, and every renderer in this project that
      // forgot that has had to be fixed afterwards.
      x === undefined ? '—' : x.toFixed(digits);
    lines.push(
      `DQ* AGENT${n.freshness === 'stale' ? ` (STALE, ${age(n)})` : ` (${age(n)})`}: ` +
        `epsilon ${num(v.epsilon, 3)} · steps ${v.train_steps ?? '—'} · ` +
        `replay ${v.replay_size ?? '—'} · reward ${num(v.total_reward, 1)}`,
    );
  }

  lines.push(
    'Rule: state these figures as read, with their age. Any figure not listed above is one you ' +
      'do not have — say so rather than estimating it.',
  );
  return lines.join('\n');
}
