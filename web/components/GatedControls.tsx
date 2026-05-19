'use client'

import { useWallet } from '@solana/wallet-adapter-react'
import { WalletMultiButton } from '@solana/wallet-adapter-react-ui'

export function GatedControls({ children }: { children: React.ReactNode }) {
  const { connected } = useWallet()

  if (!connected) {
    return (
      <div className="panel flex items-center justify-center gap-4 py-5 px-4">
        <div className="flex flex-col items-center gap-3 text-center">
          <div className="flex items-center gap-2 text-scema-muted text-xs tracking-widest uppercase">
            <span className="w-1.5 h-1.5 rounded-full bg-scema-dim" />
            Wallet required to send control commands
          </div>
          <WalletMultiButton />
          <p className="text-scema-dim text-xs">
            Each control command costs 0.01 SOL via x402 — keyboard shortcuts active when connected
          </p>
        </div>
      </div>
    )
  }

  return <>{children}</>
}
