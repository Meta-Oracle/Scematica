'use client'

import { createContext, useCallback, useContext, useState, type ReactNode } from 'react'
import { isNative } from './net'

export type UiMode = 'beginner' | 'pro'
const KEY = 'scematica.uimode'

// App-wide Beginner ⇄ Pro mode. Beginner is the friendly, slider-first experience and
// also hides the advanced dashboard panels (see AdvancedOnly). Default is beginner on
// mobile, pro on web; the choice persists. This provider lives under the `ssr: false`
// wallet-provider boundary, so reading localStorage in the initializer is safe (no SSR,
// no hydration mismatch, no flash).
const Ctx = createContext<{ mode: UiMode; setMode: (m: UiMode) => void }>({
  mode: 'pro',
  setMode: () => {},
})

export function UiModeProvider({ children }: { children: ReactNode }) {
  const [mode, setModeState] = useState<UiMode>(() => {
    if (typeof window === 'undefined') return 'pro'
    const stored = window.localStorage.getItem(KEY)
    if (stored === 'beginner' || stored === 'pro') return stored
    return isNative() ? 'beginner' : 'pro'
  })

  const setMode = useCallback((m: UiMode) => {
    setModeState(m)
    try {
      window.localStorage.setItem(KEY, m)
    } catch {
      /* storage disabled */
    }
  }, [])

  return <Ctx.Provider value={{ mode, setMode }}>{children}</Ctx.Provider>
}

export const useUiMode = () => useContext(Ctx)
