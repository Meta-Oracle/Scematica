'use client'

import { useEffect, useState } from 'react'
import { isNative, getPairing, needsPairing } from '@/lib/net'
import { initPush } from '@/lib/push'
import { Pairing } from './Pairing'

// Wraps the dashboard. On the server-backed web build (or an already-paired mobile
// build) it renders the children unchanged. When the build has no `/api/*` proxy to
// fall back on and nothing is paired yet — a native shell, or the static export opened
// in a plain browser — it shows the pairing screen instead, so the dashboard never
// mounts twenty pollers against an endpoint that cannot exist. Native detection and
// localStorage are client-only, so we render children on the server / first paint and
// swap in the gate after mount — the server-backed web app never sees a difference
// (needsPairing() is always false there).
export function MobileGate({ children }: { children: React.ReactNode }) {
  const [mounted, setMounted] = useState(false)
  const [paired, setPaired] = useState(true)

  useEffect(() => {
    setMounted(true)
    const ok = !needsPairing()
    setPaired(ok)
    // Once paired on a device, enrol for trade push notifications.
    if (ok && isNative() && getPairing()) void initPush()
  }, [])

  if (mounted && !paired) return <Pairing onPaired={() => setPaired(true)} />
  return <>{children}</>
}
