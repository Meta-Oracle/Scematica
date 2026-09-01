import type { Metadata } from 'next'

import { ScemaWorldTerminal } from '@/components/scemaworld/ScemaWorldTerminal'

// Scema-World — the eighth product on this site, and the second with no server side at all.
//
// Like `/omni` there is no `/api/scema-world` route and no entry in
// `app/api/[...slug]/route.ts`. The record is read with `FileReader` and the space is computed
// in the reader's own tab. That is not a convenience: the claim the game makes is that **the
// record is the map**, and a map fetched from a server would be that server's map instead.
//
// The generator (`lib/scemaworld/generate.ts`) is a pure function of the record's commitment,
// which is itself proven byte-for-byte between Rust and the browser by `check:omni`. So two
// players holding the same record fly the same space without a server and without trusting
// each other — and `npm run check:scemaworld` pins that the generator reads no clock, no
// randomness, and produces only integer coordinates.
//
// There is deliberately no economy here. Anything that priced a world would make a record's
// content worth misreporting, and a producer paid to hide its blind spots is the single
// failure this project cannot absorb. See `scematica-omni/docs/SCEMA-WORLD.md`.

export const metadata: Metadata = {
  title: 'SCEMA-WORLD — fly a decision record',
  description:
    'A space exploration game whose map is a sealed Scematica Omni decision record. Blind spots become rifts you cannot see into, estimated signals become ghost contacts that may not be there, and how well the world was observed is literally how far you can see. No server, no account, no economy.',
  keywords: [
    'browser game',
    'space exploration',
    'procedural generation',
    'verifiable world',
    'decision record',
    'deterministic map',
  ],
}

export default function ScemaWorldPage() {
  return <ScemaWorldTerminal />
}
