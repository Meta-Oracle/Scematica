import type { Metadata } from 'next'

import { OmniTerminal } from '@/components/omni/OmniTerminal'

// Scematica Omni — the decision-record console.
//
// The seventh product on this site, and the only one with no server side at all. There is
// no `/api/omni` route and nothing in `app/api/[...slug]/route.ts` for it: a decision record
// is a self-contained artefact, and the whole point of the page is that a reader can check
// one without trusting anything except their own browser's SHA-256.
//
// That also makes the "no simulation branch" rule of /mesh and /escrow trivially true here.
// There is nothing to simulate — the page renders a file the reader supplied — and nothing
// to phone home to.
//
// The commitment arithmetic in `lib/omni/canonical.ts` is a PORT of
// `scematica-omni/crates/scema-verify/src/canonical.rs`, and Rust is authoritative.
// `npm run check:omni` re-derives the digests of a real `scema decide` record and compares
// them against the ones Rust wrote into it, so drift fails a check rather than surfacing as
// an untampered record reported INVALID.

export const metadata: Metadata = {
  title: 'SCEMATICA OMNI — verify a decision record',
  description:
    'Check a Scematica Omni decision record in your browser: what the agent perceived, every branch it considered with the terms nobody measured left blank, the preferences in force, and whether the file has been edited since it was sealed. Nothing leaves the tab.',
  keywords: [
    'agent runtime',
    'proof-carrying decisions',
    'counterfactual simulation',
    'verifiable AI',
    'decision record',
    'world model',
  ],
}

export default function OmniPage() {
  return <OmniTerminal />
}
