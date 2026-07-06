'use client'

import { useEffect, useState } from 'react'
import { isNative, getPairing } from '@/lib/net'
import { initPush } from '@/lib/push'
import { Pairing } from './Pairing'

// Wraps the dashboard. On the web (or an already-paired mobile build) it renders the
// children unchanged. On a native shell with no pairing yet it shows the pairing
// screen instead. Native detection and localStorage are client-only, so we render
// children on the server / first paint and swap in the gate after mount — the web app
// never sees a difference (isNative() is always false there).
export function MobileGate({ children }: { children: React.ReactNode }) {
  const [mounted, setMounted] = useState(false)
  const [paired, setPaired] = useState(true)

  useEffect(() => {
    setMounted(true)
    const ok = !isNative() || getPairing() !== null
    setPaired(ok)
    // Once paired on a device, enrol for trade push notifications.
    if (ok && isNative() && getPairing()) void initPush()
  }, [])

  if (mounted && !paired) return <Pairing onPaired={() => setPaired(true)} />
  return <>{children}</>
}
