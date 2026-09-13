'use client'

/**
 * /omni-agent — the field agent's console.
 *
 * Two things live on this page and they are deliberately different in kind.
 *
 * The top half explains what the Omni-Agent is and, more importantly, what it is *not*:
 * it shares a word with Scematica Omni and shares none of its claims. Omni seals
 * verifiable decision records. This agent perceives live discourse and drafts prose it
 * asks a human to approve. A reader who conflates the two would give an unsealed opinion
 * the authority of a sealed record, which is precisely what the other page exists to
 * prevent.
 *
 * The bottom half is a reader for the agent's own audit trail. Drop a
 * `data/queue/proposals.jsonl` in and it replays every draft, every decision, and — the
 * part that is invisible in the raw file — the labelled training examples each decision
 * handed the cortex.
 *
 * ## Constraints, inherited and one new
 *
 * 1. **No network, exactly as on /omni.** No `/api/omni-agent` route, no entry in
 *    `app/api/[...slug]/route.ts`, no fetch. The log is read with `FileReader` in the
 *    reader's own browser. "No simulation branch" is trivially true: there is nothing to
 *    simulate and nowhere to phone home to. The queue is an operator's own decision
 *    history, which is exactly the kind of file that must not be uploaded to read.
 *
 * 2. **An unscored draft is not a draft scored zero.** A proposal written while the cortex
 *    was unreachable carries no scores at all, and prints as an em dash. Same rule as
 *    `lib/omni/view.ts::cell` and `scema_policy::render`; a measured zero still prints
 *    `0.00`, because that is a real observation.
 *
 * 3. **Counts, never invented rates.** There is no approval percentage on this page. Over
 *    the handful of decisions a real log contains, a percentage is a number of the right
 *    shape with nothing behind it.
 */

import { useCallback, useRef, useState } from 'react'

import {
  finalText,
  isScored,
  labelTotals,
  labelsFor,
  replayQueue,
  scoreCell,
  type Proposal,
  type ProposalStatus,
  type Replay,
} from '@/lib/agent/replay'

interface Loaded {
  name: string
  replay: Replay
}

type LoadError = { name: string; message: string }

export function AgentTerminal() {
  const [loaded, setLoaded] = useState<Loaded | null>(null)
  const [error, setError] = useState<LoadError | null>(null)
  const [dragging, setDragging] = useState(false)
  const fileInput = useRef<HTMLInputElement>(null)

  const ingest = useCallback((name: string, text: string) => {
    setError(null)
    const replay = replayQueue(text)
    // A file that parsed into nothing is not an empty queue — it is the wrong file, and
    // rendering "0 pending" for it would be a confident statement about somebody's
    // history that this page never read.
    if (replay.events === 0) {
      setLoaded(null)
      setError({
        name,
        message:
          replay.malformedLines > 0
            ? `no line in this file parsed as JSON (${replay.malformedLines} tried). The queue is JSONL — one event object per line.`
            : 'this file contains no events. Expected data/queue/proposals.jsonl.',
      })
      return
    }
    setLoaded({ name, replay })
  }, [])

  const onFiles = useCallback(
    async (files: FileList | null) => {
      const file = files?.[0]
      if (!file) return
      ingest(file.name, await file.text())
    },
    [ingest]
  )

  return (
    <div className="agent-root font-mono text-[13px] leading-relaxed">
      <div className="mx-auto max-w-6xl px-5 py-10">
        <Header />
        <NotOmni />

        <div
          onDragOver={(e) => {
            e.preventDefault()
            setDragging(true)
          }}
          onDragLeave={() => setDragging(false)}
          onDrop={(e) => {
            e.preventDefault()
            setDragging(false)
            void onFiles(e.dataTransfer.files)
          }}
          className={`mt-8 rounded border border-dashed p-8 text-center transition-colors ${
            dragging ? 'border-agent-accent bg-agent-hi' : 'border-agent-border bg-agent-surface'
          }`}
        >
          <p className="text-agent-muted">
            Drop a proposal log here —{' '}
            <code className="text-agent-text">data/queue/proposals.jsonl</code>
          </p>
          <p className="mt-2 text-agent-dim">
            Nothing leaves this tab. It is your own decision history; it is read in your
            browser and uploaded nowhere.
          </p>
          <button
            type="button"
            onClick={() => fileInput.current?.click()}
            className="mt-4 rounded border border-agent-border-hi px-4 py-1.5 text-agent-text hover:border-agent-accent"
          >
            Choose a file
          </button>
          <input
            ref={fileInput}
            type="file"
            accept=".jsonl,.json,.txt,application/json"
            className="hidden"
            onChange={(e) => void onFiles(e.target.files)}
          />
        </div>

        {error && (
          <section className="mt-6 rounded border border-agent-rejected bg-agent-surface p-4">
            <h2 className="text-agent-rejected">COULD NOT READ {error.name}</h2>
            <p className="mt-1 text-agent-muted">{error.message}</p>
          </section>
        )}

        {loaded && <QueueView loaded={loaded} />}
        {!loaded && !error && <WhatThisIs />}
      </div>
    </div>
  )
}

function Header() {
  return (
    <header className="border-b border-agent-border pb-5">
      <h1 className="text-lg tracking-[0.18em] text-agent-accent">SCEMATICA OMNI-AGENT</h1>
      <p className="mt-1 text-agent-muted">
        The field agent — it reads live X discourse through Grok, judges it with a network
        trained on your own approve and reject decisions, and asks before it posts.
      </p>
    </header>
  )
}

/**
 * The distinction the whole page hangs on.
 *
 * Stated as a table rather than a sentence because the two things genuinely overlap in
 * name and in vocabulary, and a reader skimming will take "omni" as one product. The
 * column that matters is the second row: one of these proves something, the other
 * proves nothing and says so.
 */
function NotOmni() {
  const rows: [string, string, string][] = [
    ['What it does', 'observes a world, projects branches, seals a record', 'perceives discourse, drafts prose, asks you'],
    ['What it proves', 'the record was not edited after sealing', 'nothing — it is not a verifier'],
    ['Its output', 'a decision record, checkable offline on /omni', 'a draft in a queue, and a post if you approve'],
    ['Its judgement', 'additive utility over measured terms, with coverage', 'a trained net, over your real decisions'],
    ['Where it acts', 'scema execute — gated twice, dry-run by default', 'X, Telegram, the terminal'],
  ]
  return (
    <section className="mt-8 rounded border border-agent-border bg-agent-surface p-4">
      <h2 className="text-agent-accent">THIS IS NOT SCEMATICA OMNI</h2>
      <p className="mt-2 text-agent-muted">
        Two things here carry the word. They do different jobs, and only one of them makes
        a claim you can check.
      </p>
      <div className="mt-3 overflow-x-auto">
        <table className="w-full min-w-[640px] border-collapse">
          <thead>
            <tr className="text-left text-agent-dim">
              <th className="py-1 pr-4 font-normal"> </th>
              <th className="py-1 pr-4 font-normal">
                <a
                  href="/omni"
                  className="text-agent-text underline decoration-agent-border hover:decoration-agent-accent"
                >
                  /omni
                </a>{' '}
                — the runtime
              </th>
              <th className="py-1 font-normal text-agent-accent">this — the agent</th>
            </tr>
          </thead>
          <tbody>
            {rows.map(([label, omni, agent]) => (
              <tr key={label} className="border-t border-agent-border align-top">
                <td className="py-1.5 pr-4 text-agent-dim">{label}</td>
                <td className="py-1.5 pr-4 text-agent-muted">{omni}</td>
                <td className="py-1.5 text-agent-text">{agent}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <p className="mt-3 text-agent-dim">
        It is also not <code className="text-agent-text">scema-tgbot</code>, the Telegram
        bot that commands the live sniper. That one can pause, dump and re-arm. This one
        can only read the bot, and says so when it cannot.
      </p>
    </section>
  )
}

function WhatThisIs() {
  return (
    <section className="mt-8 space-y-4 text-agent-muted">
      <TheLoop />
      <div className="rounded border border-agent-border bg-agent-surface p-4">
        <h2 className="text-agent-accent">PRODUCE A LOG</h2>
        <pre className="mt-2 overflow-x-auto text-agent-text">{`cd omni-agent
npm install && cp .env.example .env      # XAI_API_KEY at minimum
npm run cortex                            # the neural sidecar
npm run sense                             # one perception cycle
npm run queue                             # approve / reject / edit
# → data/queue/proposals.jsonl`}</pre>
        <p className="mt-3">
          Every decision you make there is one labelled training example. Drop the file
          above onto this page to see what the cortex was actually taught.
        </p>
      </div>
      <ThreeStates />
      <OneBotOnePoller />
    </section>
  )
}

function TheLoop() {
  return (
    <div className="rounded border border-agent-border bg-agent-surface p-4">
      <h2 className="text-agent-accent">THE LOOP</h2>
      <pre className="mt-2 overflow-x-auto text-agent-muted">{`  PERCEIVE   Grok server-side x_search reads live X discourse
  JUDGE      TasteNet, one trunk, three heads
               salience   does this matter at all
               taste      would the operator approve this
               resonance  how much engagement will it earn
  COMPOSE    draft, with cross-surface memory in context
  DISPATCH   Telegram:  post  ·  reject  ·  edit
  REFLECT    engagement measured, fed back as labels
               └─────────────── trains the cortex ──────────────┘`}</pre>
      <p className="mt-3">
        The loop closes. Your decisions <span className="text-agent-text">are</span> the
        training set; measured engagement <span className="text-agent-text">is</span> the
        resonance label. The network is not decoration — the sense loop cannot rank without
        it.
      </p>
    </div>
  )
}

/**
 * Why the agent can talk about the bot at all without inventing figures.
 *
 * This is the part of the design that ties the agent to the rest of Scematica, and it is
 * the same three-state rule the mesh, the oracle console and the sentience gate each
 * arrived at independently.
 */
function ThreeStates() {
  return (
    <div className="rounded border border-agent-border bg-agent-surface p-4">
      <h2 className="text-agent-accent">WHAT IT MAY SAY ABOUT THE LIVE BOT</h2>
      <p className="mt-2">
        An agent that speaks in public about a trading system has two bad options when
        asked how the bot is doing: decline every such question, or produce a plausible
        number. So it reads the sniper&apos;s own state files, and keeps{' '}
        <span className="text-agent-text">three</span> states apart, never two.
      </p>
      <ul className="mt-3 space-y-2">
        <li>
          <span className="text-agent-unscored">absent</span> — no file. Not a bot that
          broke even. <code className="text-agent-text">0.00 SOL</code> would be a claim
          nobody measured.
        </li>
        <li>
          <span className="text-agent-pending">stale</span> — a file with an age attached.
          Metrics are rewritten every five seconds, so an hour old means the sniper is
          stopped, not that nothing happened.
        </li>
        <li>
          <span className="text-agent-posted">fresh</span> — a measurement, and the only
          case where a bare number may be spoken.
        </li>
      </ul>
      <p className="mt-3 text-agent-dim">
        Read-only throughout: no lock, no write, no command. Nine tests assert what does{' '}
        <span className="text-agent-text">not</span> come out — no zero for an unread PnL,
        no 0.0% win rate over zero trades.
      </p>
    </div>
  )
}

/** The one operational rule somebody running this will otherwise discover the hard way. */
function OneBotOnePoller() {
  return (
    <div className="rounded border border-agent-border bg-agent-surface p-4">
      <h2 className="text-agent-accent">ONE BOT, ONE POLLER</h2>
      <p className="mt-2">
        Telegram hands each update to <span className="text-agent-text">exactly one</span>{' '}
        caller. Two processes polling one token do not both receive your commands — they
        split them between them, at random, with no error anywhere. One of those processes
        is the bot that can sell positions, so the agent refuses to poll a token it can see
        belongs to the sniper, and stops rather than retrying if Telegram reports the
        conflict anyway.
      </p>
    </div>
  )
}

/* ── the replayed log ─────────────────────────────────────────────────────── */

const STATUS_TONE: Record<ProposalStatus, string> = {
  pending: 'text-agent-pending',
  approved: 'text-agent-posted',
  posted: 'text-agent-posted',
  rejected: 'text-agent-rejected',
  failed: 'text-agent-rejected',
  expired: 'text-agent-dim',
}

function QueueView({ loaded }: { loaded: Loaded }) {
  const { replay, name } = loaded
  const totals = labelTotals(replay.proposals)
  const order: ProposalStatus[] = ['pending', 'posted', 'approved', 'rejected', 'failed', 'expired']

  return (
    <section className="mt-8 space-y-6">
      <div className="rounded border border-agent-border bg-agent-surface p-4">
        <h2 className="text-agent-accent">{name}</h2>
        <p className="mt-1 text-agent-dim">
          {replay.events} event{replay.events === 1 ? '' : 's'} replayed ·{' '}
          {replay.proposals.length} proposal{replay.proposals.length === 1 ? '' : 's'}
        </p>

        <div className="mt-3 flex flex-wrap gap-x-5 gap-y-1">
          {order.map((status) => (
            <span key={status} className={replay.counts[status] ? STATUS_TONE[status] : 'text-agent-dim'}>
              {status} {replay.counts[status]}
            </span>
          ))}
        </div>

        {(replay.unknownEvents > 0 || replay.malformedLines > 0) && (
          <p className="mt-3 text-agent-pending">
            {replay.unknownEvents > 0 && (
              <>
                {replay.unknownEvents} event
                {replay.unknownEvents === 1 ? '' : 's'} this page does not understand —
                probably written by a newer agent than this reader. They are counted, not
                applied, so the history above is incomplete rather than wrong.{' '}
              </>
            )}
            {replay.malformedLines > 0 && (
              <>
                {replay.malformedLines} line
                {replay.malformedLines === 1 ? '' : 's'} did not parse as JSON.
              </>
            )}
          </p>
        )}
      </div>

      <div className="rounded border border-agent-border bg-agent-surface p-4">
        <h2 className="text-agent-accent">WHAT THE CORTEX WAS TAUGHT</h2>
        <p className="mt-2 text-agent-muted">
          Every decision is one labelled example. An{' '}
          <span className="text-agent-text">edit</span> is two, with opposite signs — the
          original was not good enough, the rewrite was — which is what lets the net learn
          the difference rather than only the direction.
        </p>
        <div className="mt-3 flex flex-wrap gap-x-6 gap-y-1">
          <span className="text-agent-posted">taste 1 · {totals.positive}</span>
          <span className="text-agent-rejected">taste 0 · {totals.negative}</span>
          <span className="text-agent-muted">from edits · {totals.fromEdits}</span>
        </div>
        <p className="mt-3 text-agent-dim">
          Counts, not a rate. Over this many decisions an approval percentage would be a
          number of the right shape with nothing behind it.
        </p>
      </div>

      <div className="space-y-3">
        {replay.proposals.map((proposal) => (
          <ProposalCard key={proposal.id} proposal={proposal} />
        ))}
      </div>
    </section>
  )
}

function ProposalCard({ proposal }: { proposal: Proposal }) {
  const labels = labelsFor(proposal)
  const edited = proposal.editedText !== undefined && proposal.editedText !== proposal.text

  return (
    <article className="rounded border border-agent-border bg-agent-surface p-4">
      <header className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
        <span className="text-agent-dim">
          <code className="text-agent-muted">{proposal.id}</code>
          {proposal.topic && <> · {proposal.topic}</>}
        </span>
        <span className={STATUS_TONE[proposal.status]}>{proposal.status.toUpperCase()}</span>
      </header>

      <p className="mt-3 text-agent-text">{finalText(proposal)}</p>
      {edited && (
        <p className="mt-2 text-agent-dim">
          you rewrote it. Originally:{' '}
          <span className="line-through">{proposal.text}</span>
        </p>
      )}
      {proposal.rationale && <p className="mt-2 italic text-agent-muted">{proposal.rationale}</p>}

      <div className="mt-3 flex flex-wrap gap-x-5 gap-y-1">
        {(['taste', 'salience', 'novelty', 'resonance', 'priority'] as const).map((key) => (
          <span key={key} className={isScored(proposal) ? 'text-agent-muted' : 'text-agent-unscored'}>
            {key} {scoreCell(proposal, key)}
          </span>
        ))}
      </div>
      {!isScored(proposal) && (
        // The em-dash rule, said out loud. A row of dashes could be misread as five
        // zeroes by somebody skimming, and the difference is the whole point.
        <p className="mt-2 text-agent-dim">
          the cortex never scored this draft — it was not rated zero, it was not rated.
        </p>
      )}

      {proposal.engagement && (
        <p className="mt-3 text-agent-muted">
          measured: {proposal.engagement.likes} likes · {proposal.engagement.reposts} reposts
          · {proposal.engagement.replies} replies
        </p>
      )}
      {proposal.error && <p className="mt-3 text-agent-rejected">failed: {proposal.error}</p>}

      {labels.length > 0 && (
        <p className="mt-3 text-agent-dim">
          taught the cortex{' '}
          {labels.map((label, i) => (
            <span key={label.source}>
              {i > 0 && ', '}
              <span className={label.taste === 1 ? 'text-agent-posted' : 'text-agent-rejected'}>
                taste {label.taste}
              </span>{' '}
              <span className="text-agent-muted">({label.source})</span>
            </span>
          ))}
        </p>
      )}
      {labels.length === 0 && proposal.status === 'pending' && (
        <p className="mt-3 text-agent-dim">
          nothing taught yet — an undecided draft is not a rejected one.
        </p>
      )}
    </article>
  )
}
