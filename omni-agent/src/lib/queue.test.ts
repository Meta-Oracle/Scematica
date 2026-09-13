import assert from 'node:assert/strict';
import { mkdtemp, readFile, writeFile, appendFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { after, describe, it } from 'node:test';

import { ProposalQueue, finalText, type ProposalScores } from './queue.js';

const scores: ProposalScores = {
  salience: 0.8,
  taste: 0.7,
  resonance: 12,
  novelty: 0.9,
  priority: 0.75,
};

async function freshQueue(): Promise<{ queue: ProposalQueue; path: string }> {
  const dir = await mkdtemp(join(tmpdir(), 'scema-queue-'));
  const path = join(dir, 'proposals.jsonl');
  return { queue: new ProposalQueue(path), path };
}

function draft(text: string, overrides: Partial<Parameters<ProposalQueue['propose']>[0]> = {}) {
  return {
    text,
    rationale: 'because it is relevant',
    sources: ['https://x.com/someone/status/1'],
    topic: 'AI agents',
    scores,
    ...overrides,
  };
}

describe('ProposalQueue', () => {
  it('records a proposal as pending and reads it back', async () => {
    const { queue } = await freshQueue();
    const proposal = await queue.propose(draft('a first take'));

    assert.equal(proposal.status, 'pending');
    const pending = await queue.pending();
    assert.equal(pending.length, 1);
    assert.equal(pending[0]!.text, 'a first take');
  });

  it('survives a restart by replaying the log', async () => {
    const { queue, path } = await freshQueue();
    const first = await queue.propose(draft('persisted take'));
    await queue.decide(first.id, 'approved', 'operator');
    await queue.markPosted(first.id, 'tweet-123');

    const revived = new ProposalQueue(path);
    const restored = await revived.get(first.id);
    assert.equal(restored?.status, 'posted');
    assert.equal(restored?.postedId, 'tweet-123');
    assert.equal(restored?.decidedBy, 'operator');
  });

  it('keeps the operator edit as the text that ships', async () => {
    const { queue } = await freshQueue();
    const proposal = await queue.propose(draft('the original phrasing'));
    await queue.decide(proposal.id, 'approved', 'operator', 'the better phrasing');

    const updated = await queue.get(proposal.id);
    assert.equal(updated?.text, 'the original phrasing', 'original must be preserved for audit');
    assert.equal(updated?.editedText, 'the better phrasing');
    assert.equal(finalText(updated!), 'the better phrasing');
  });

  it('ignores a second decision instead of overwriting the first', async () => {
    const { queue } = await freshQueue();
    const proposal = await queue.propose(draft('double tapped'));
    await queue.decide(proposal.id, 'approved', 'operator');
    const second = await queue.decide(proposal.id, 'rejected', 'operator');

    assert.equal(second?.status, 'approved', 'a double tap must not flip the decision');
  });

  it('returns null when deciding on an unknown id', async () => {
    const { queue } = await freshQueue();
    assert.equal(await queue.decide('nope', 'approved', 'operator'), null);
  });

  it('orders pending drafts by priority', async () => {
    const { queue } = await freshQueue();
    await queue.propose(draft('low', { scores: { ...scores, priority: 0.2 } }));
    await queue.propose(draft('high', { scores: { ...scores, priority: 0.95 } }));
    await queue.propose(draft('mid', { scores: { ...scores, priority: 0.6 } }));

    const pending = await queue.pending();
    assert.deepEqual(
      pending.map((p) => p.text),
      ['high', 'mid', 'low'],
    );
  });

  it('expires only stale pending drafts', async () => {
    const { queue, path } = await freshQueue();
    const fresh = await queue.propose(draft('fresh'));
    const stale = await queue.propose(draft('stale'));
    const decided = await queue.propose(draft('already decided'));
    await queue.decide(decided.id, 'approved', 'operator');

    // Backdate two of them in the log, then reload.
    const log = await readFile(path, 'utf8');
    const old = new Date(Date.now() - 48 * 3600_000).toISOString();
    await writeFile(
      path,
      log
        .split('\n')
        .map((line) => {
          if (!line.trim()) return line;
          const event = JSON.parse(line);
          if (event.type === 'proposed' && event.proposal.id !== fresh.id) {
            event.proposal.createdAt = old;
          }
          return JSON.stringify(event);
        })
        .join('\n'),
      'utf8',
    );

    const reloaded = new ProposalQueue(path);
    const expired = await reloaded.expireOlderThan(24);

    assert.equal(expired.length, 1, 'only the stale pending draft should expire');
    assert.equal(expired[0]!.id, stale.id);
    assert.equal((await reloaded.get(fresh.id))?.status, 'pending');
    assert.equal((await reloaded.get(decided.id))?.status, 'approved');
  });

  it('lists posts ready for an engagement measurement', async () => {
    const { queue, path } = await freshQueue();
    const recent = await queue.propose(draft('just posted'));
    const settled = await queue.propose(draft('posted a while ago'));
    const measured = await queue.propose(draft('already measured'));

    for (const proposal of [recent, settled, measured]) {
      await queue.decide(proposal.id, 'approved', 'operator');
      await queue.markPosted(proposal.id, `tweet-${proposal.id}`);
    }
    await queue.recordEngagement(measured.id, 10, 2, 1);

    // Backdate the postedAt of two entries.
    const old = new Date(Date.now() - 12 * 3600_000).toISOString();
    const log = await readFile(path, 'utf8');
    await writeFile(
      path,
      log
        .split('\n')
        .map((line) => {
          if (!line.trim()) return line;
          const event = JSON.parse(line);
          if (event.type === 'posted' && event.id !== recent.id) event.at = old;
          return JSON.stringify(event);
        })
        .join('\n'),
      'utf8',
    );

    const reloaded = new ProposalQueue(path);
    const awaiting = await reloaded.awaitingEngagement(6);
    assert.deepEqual(
      awaiting.map((p) => p.id),
      [settled.id],
    );
  });

  it('skips a torn final line rather than losing the whole history', async () => {
    const { queue, path } = await freshQueue();
    const good = await queue.propose(draft('written cleanly'));
    // Simulate a crash mid-append.
    await appendFile(path, '{"type":"proposed","at":"2026', 'utf8');

    const reloaded = new ProposalQueue(path);
    const all = await reloaded.all();
    assert.equal(all.length, 1);
    assert.equal(all[0]!.id, good.id);
  });

  it('counts every status in the summary', async () => {
    const { queue } = await freshQueue();
    const a = await queue.propose(draft('a'));
    const b = await queue.propose(draft('b'));
    const c = await queue.propose(draft('c'));
    await queue.propose(draft('d'));

    await queue.decide(a.id, 'approved', 'op');
    await queue.markPosted(a.id, 'x1');
    await queue.decide(b.id, 'rejected', 'op');
    await queue.decide(c.id, 'approved', 'op');
    await queue.markFailed(c.id, 'rate limited');

    const summary = await queue.summary();
    assert.equal(summary.posted, 1);
    assert.equal(summary.rejected, 1);
    assert.equal(summary.failed, 1);
    assert.equal(summary.pending, 1);
  });
});
