'use client'

import { useWallet } from '@solana/wallet-adapter-react'
import { WalletMultiButton } from '@solana/wallet-adapter-react-ui'

// Renders children only when a wallet is connected.
// Shows a connect prompt otherwise so controls stay locked without a wallet.
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
            Controls write directly to sniper IPC files — keyboard shortcuts active when unlocked
          </p>
        </div>
      </div>
    )
  }

  return <>{children}</>
}
