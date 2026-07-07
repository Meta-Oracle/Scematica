'use client'

import { useUiMode } from '@/lib/UiModeContext'

// Renders its children only in Pro mode. Wrap advanced dashboard sections with this so
// Beginner mode shows a clean, focused view. Server-rendered children are fine — this is
// a thin client gate around them.
export function AdvancedOnly({ children }: { children: React.ReactNode }) {
  const { mode } = useUiMode()
  return mode === 'pro' ? <>{children}</> : null
}
