/**
 * The proposal queue: every draft the agent wants to post, and what happened to it.
 *
 * Event-sourced, append-only JSONL. State is derived by replaying the log
 * rather than stored, which buys three things that matter here:
 *
 *   - **Crash safety.** An append either lands or it does not. There is no
 *     window where a rewrite leaves the queue truncated, which a
 *     read-modify-write JSON file would have.
 *   - **An audit trail.** "Why did it post that?" is answerable months later:
 *     the proposal, its scores, the operator's decision, and any edit are all
 *     still there in order.
 *   - **Replayable training data.** The decision history *is* the cortex's
 *     label set. If the network is ever reset, it can be retrained from this
 *     file alone.
 *
 * The cost is that state must be rebuilt on load. At the volume an agent
 * posting a handful of times a day produces, that is nothing.
 */
import { appendFile, mkdir, readFile } from 'node:fs/promises';
import { dirname } from 'node:path';
import { randomUUID } from 'node:crypto';

import { config } from '../config.js';

export type ProposalStatus =
  | 'pending'
  | 'approved'
  | 'rejected'
  | 'posted'
  | 'failed'
  | 'expired';

export interface ProposalScores {
  salience: number;
  taste: number;
  resonance: number;
  novelty: number;
  priority: number;
}

export interface Proposal {
  id: string;
  createdAt: string;
  /** What the agent wants to say. */
  text: string;
  /** Why it wants to say it -- shown to the operator, never posted. */
  rationale: string;
  /** What it was reacting to. */
  sources: string[];
  topic: string;
  scores: ProposalScores;
  status: ProposalStatus;
  /** Set when the operator rewrites the draft before approving. */
  editedText?: string;
  decidedAt?: string;
  decidedBy?: string;
  postedId?: string;
  postedAt?: string;
  error?: string;
  /** Engagement measured after the fact; feeds the resonance head. */
  engagement?: { likes: number; reposts: number; replies: number; measuredAt: string };
}

type QueueEvent =
  | { type: 'proposed'; at: string; proposal: Proposal }
  | {
      type: 'decided';
      at: string;
      id: string;
      status: Extract<ProposalStatus, 'approved' | 'rejected'>;
      by: string;
      editedText?: string;
    }
  | { type: 'posted'; at: string; id: string; postedId: string }
  | { type: 'failed'; at: string; id: string; error: string }
  | { type: 'expired'; at: string; id: string }
  | {
      type: 'engagement';
      at: string;
      id: string;
      likes: number;
      reposts: number;
      replies: number;
    };

/** The text that actually goes out: the operator's edit if there was one. */
export function finalText(proposal: Proposal): string {
  return proposal.editedText ?? proposal.text;
}

export class ProposalQueue {
  private proposals = new Map<string, Proposal>();
  private loaded = false;

  constructor(private readonly path: string = config.paths.queue) {}

  private async append(event: QueueEvent): Promise<void> {
    await mkdir(dirname(this.path), { recursive: true });
    await appendFile(this.path, `${JSON.stringify(event)}\n`, 'utf8');
  }

  /** Rebuild state from the log. Idempotent. */
  async load(): Promise<void> {
    if (this.loaded) return;
    this.proposals.clear();

    let contents: string;
    try {
      contents = await readFile(this.path, 'utf8');
    } catch {
      this.loaded = true;
      return; // no queue yet
    }

    for (const line of contents.split('\n')) {
      if (!line.trim()) continue;
      let event: QueueEvent;
      try {
        event = JSON.parse(line) as QueueEvent;
      } catch {
        // A torn final line from an interrupted write should not poison the
        // whole history.
        console.warn('[queue] skipping unparseable line');
        continue;
      }
      this.apply(event);
    }
    this.loaded = true;
  }

  private apply(event: QueueEvent): void {
    if (event.type === 'proposed') {
      this.proposals.set(event.proposal.id, { ...event.proposal });
      return;
    }
    const proposal = this.proposals.get(event.id);
    if (!proposal) return; // event for a proposal we never saw; ignore

    switch (event.type) {
      case 'decided':
        proposal.status = event.status;
        proposal.decidedAt = event.at;
        proposal.decidedBy = event.by;
        if (event.editedText) proposal.editedText = event.editedText;
        break;
      case 'posted':
        proposal.status = 'posted';
        proposal.postedId = event.postedId;
        proposal.postedAt = event.at;
        break;
      case 'failed':
        proposal.status = 'failed';
        proposal.error = event.error;
        break;
      case 'expired':
        proposal.status = 'expired';
        break;
      case 'engagement':
        proposal.engagement = {
          likes: event.likes,
          reposts: event.reposts,
          replies: event.replies,
          measuredAt: event.at,
        };
        break;
    }
  }

  async propose(
    input: Omit<Proposal, 'id' | 'createdAt' | 'status'> & { id?: string },
  ): Promise<Proposal> {
    await this.load();
    const proposal: Proposal = {
      ...input,
      id: input.id ?? randomUUID().slice(0, 8),
      createdAt: new Date().toISOString(),
      status: 'pending',
    };
    const event: QueueEvent = { type: 'proposed', at: proposal.createdAt, proposal };
    await this.append(event);
    this.apply(event);
    return proposal;
  }

  async decide(
    id: string,
    status: 'approved' | 'rejected',
    by: string,
    editedText?: string,
  ): Promise<Proposal | null> {
    await this.load();
    const existing = this.proposals.get(id);
    if (!existing) return null;
    // Deciding twice is a UI misfire (two taps on the same button), not a
    // state change. Keep the first decision.
    if (existing.status !== 'pending') return existing;

    const event: QueueEvent = {
      type: 'decided',
      at: new Date().toISOString(),
      id,
      status,
      by,
      ...(editedText ? { editedText } : {}),
    };
    await this.append(event);
    this.apply(event);
    return this.proposals.get(id) ?? null;
  }

  async markPosted(id: string, postedId: string): Promise<void> {
    await this.load();
    const event: QueueEvent = { type: 'posted', at: new Date().toISOString(), id, postedId };
    await this.append(event);
    this.apply(event);
  }

  async markFailed(id: string, error: string): Promise<void> {
    await this.load();
    const event: QueueEvent = { type: 'failed', at: new Date().toISOString(), id, error };
    await this.append(event);
    this.apply(event);
  }

  async recordEngagement(
    id: string,
    likes: number,
    reposts: number,
    replies: number,
  ): Promise<void> {
    await this.load();
    const event: QueueEvent = {
      type: 'engagement',
      at: new Date().toISOString(),
      id,
      likes,
      reposts,
      replies,
    };
    await this.append(event);
    this.apply(event);
  }

  /**
   * Expire drafts nobody decided on. A stale take is worse than no take, and
   * an unbounded pending list makes the cockpit useless.
   */
  async expireOlderThan(hours: number): Promise<Proposal[]> {
    await this.load();
    const cutoff = Date.now() - hours * 3600_000;
    const expired: Proposal[] = [];
    for (const proposal of this.proposals.values()) {
      if (proposal.status !== 'pending') continue;
      if (Date.parse(proposal.createdAt) >= cutoff) continue;
      const event: QueueEvent = { type: 'expired', at: new Date().toISOString(), id: proposal.id };
      await this.append(event);
      this.apply(event);
      expired.push(proposal);
    }
    return expired;
  }

  async get(id: string): Promise<Proposal | null> {
    await this.load();
    return this.proposals.get(id) ?? null;
  }

  async byStatus(status: ProposalStatus): Promise<Proposal[]> {
    await this.load();
    return [...this.proposals.values()]
      .filter((proposal) => proposal.status === status)
      .sort((a, b) => b.scores.priority - a.scores.priority);
  }

  async pending(): Promise<Proposal[]> {
    return this.byStatus('pending');
  }

  async all(): Promise<Proposal[]> {
    await this.load();
    return [...this.proposals.values()].sort((a, b) => a.createdAt.localeCompare(b.createdAt));
  }

  /**
   * Posts awaiting an engagement measurement: posted, old enough for numbers
   * to have settled, not yet measured.
   */
  async awaitingEngagement(minAgeHours = 6): Promise<Proposal[]> {
    await this.load();
    const cutoff = Date.now() - minAgeHours * 3600_000;
    return [...this.proposals.values()].filter(
      (proposal) =>
        proposal.status === 'posted' &&
        !proposal.engagement &&
        proposal.postedAt !== undefined &&
        Date.parse(proposal.postedAt) < cutoff,
    );
  }

  async summary(): Promise<Record<ProposalStatus, number>> {
    await this.load();
    const counts: Record<ProposalStatus, number> = {
      pending: 0,
      approved: 0,
      rejected: 0,
      posted: 0,
      failed: 0,
      expired: 0,
    };
    for (const proposal of this.proposals.values()) counts[proposal.status] += 1;
    return counts;
  }
}

let shared: ProposalQueue | undefined;

export function getQueue(): ProposalQueue {
  if (!shared) shared = new ProposalQueue();
  return shared;
}
