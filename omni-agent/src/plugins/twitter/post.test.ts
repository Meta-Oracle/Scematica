/**
 * Dry-run posting behaviour.
 *
 * The formatting test exists because of a real bug: filtering falsy entries
 * out of the line array also removed the blank-line separators, so successive
 * entries ran together as `---## 2026-...` and the review log -- the only
 * output a dry-run operator actually reads -- was unreadable.
 */
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { describe, it } from 'node:test';

import { config } from '../../config.js';
import { getQueue, type ProposalScores } from '../../lib/queue.js';
import { dryRunLogPath, postProposal } from './post.js';

const scores: ProposalScores = {
  salience: 0.6,
  taste: 0.8,
  resonance: 3,
  novelty: 0.9,
  priority: 0.7,
};

function draft(text: string, sources: string[] = []) {
  return { text, rationale: 'testing', sources, topic: 'testing', scores };
}

describe('postProposal', () => {
  it('refuses to post anything that is not approved', async () => {
    const queue = getQueue();
    const proposal = await queue.propose(draft('never approved'));

    const result = await postProposal(proposal.id);
    assert.equal(result.posted, false);
    assert.match(result.error ?? '', /not approved/);
  });

  it('refuses an unknown proposal id', async () => {
    const result = await postProposal('does-not-exist');
    assert.equal(result.posted, false);
    assert.match(result.error ?? '', /no such proposal/);
  });

  it('writes separable entries to the review log in dry-run', async (t) => {
    if (!config.twitter.dryRun) {
      t.skip('only meaningful in dry-run mode');
      return;
    }

    const queue = getQueue();
    const before = await readFile(dryRunLogPath, 'utf8').catch(() => '');
    const beforeCount = (before.match(/^## /gm) ?? []).length;

    const first = await queue.propose(draft('first entry under test', ['https://x.com/a/status/1']));
    await queue.decide(first.id, 'approved', 'test');
    await postProposal(first.id);

    const second = await queue.propose(draft('second entry under test'));
    await queue.decide(second.id, 'approved', 'test');
    await postProposal(second.id);

    const after = await readFile(dryRunLogPath, 'utf8');
    const afterCount = (after.match(/^## /gm) ?? []).length;

    assert.equal(afterCount, beforeCount + 2, 'each dry-run post needs its own heading');
    assert.doesNotMatch(after, /---##/, 'entries must not run together');
    assert.match(after, /first entry under test/);
    assert.match(after, /- reacting to: https:\/\/x\.com\/a\/status\/1/);
  });

  it('marks the proposal posted with a synthetic id that is never mistaken for real', async (t) => {
    if (!config.twitter.dryRun) {
      t.skip('only meaningful in dry-run mode');
      return;
    }
    const queue = getQueue();
    const proposal = await queue.propose(draft('synthetic id check'));
    await queue.decide(proposal.id, 'approved', 'test');

    const result = await postProposal(proposal.id);
    assert.equal(result.posted, true);
    assert.equal(result.dryRun, true);
    assert.match(result.id ?? '', /^dry-/, 'dry-run ids must be identifiable');

    const stored = await queue.get(proposal.id);
    assert.equal(stored?.status, 'posted');
    assert.equal(stored?.postedId, result.id);
  });
});
