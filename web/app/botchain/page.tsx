import type { Metadata } from 'next'

import { BotchainConsole } from '@/components/botchain/BotchainConsole'

// The BOT Chain surface — fourth product on this site, after the sniper dashboard,
// /alchem-link and /scylar-terminal. Outside the wallet gate and MobileGate: it reads a
// public chain and has nothing to gate.

export const metadata: Metadata = {
  title: 'BOT Chain · Scematica',
  description:
    'Live BOT Chain (chain 677) telemetry: head block, gas, DEX venue activity and address reads.',
}

export default function BotchainPage() {
  return <BotchainConsole />
}
