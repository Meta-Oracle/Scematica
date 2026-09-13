import type { Metadata } from 'next'

import { AgentTerminal } from '@/components/agent/AgentTerminal'

// Scematica Omni-Agent — the field agent's console.
//
// The ninth product on this site, and the second with no server side at all. There is no
// `/api/omni-agent` route and nothing in `app/api/[...slug]/route.ts` for it: the file
// this page reads is the operator's own decision history, which is exactly the kind of
// artefact that must not be uploaded in order to be read. Same argument as /omni, applied
// to a different file.
//
// The page is named for the thing it documents and deliberately sits beside /omni rather
// than inside it. They share a word and share no claims — Omni seals verifiable decision
// records, the agent drafts prose and asks a human. `NotOmni` in the component renders
// that distinction as a table, because a reader skimming will otherwise take the two for
// one product, and that misreading lends an unsealed opinion a sealed record's authority.

export const metadata: Metadata = {
  title: 'SCEMATICA OMNI-AGENT — the field agent',
  description:
    "Scematica's field agent: it reads live X discourse through Grok, judges every candidate with a network trained on your own approve and reject decisions, and asks before it posts. Drop its proposal log in to see what each decision actually taught the net. Nothing leaves the tab.",
  keywords: [
    'autonomous agent',
    'human in the loop',
    'learned preferences',
    'live search',
    'Grok',
    'agent telemetry',
  ],
}

export default function OmniAgentPage() {
  return <AgentTerminal />
}
