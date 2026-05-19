import type { Metadata } from 'next'
import './globals.css'
import { Providers } from '@/components/Providers'

export const metadata: Metadata = {
  title: 'SCEMATICA — Solana Sniper Protocol',
  description: 'AI-powered Solana memecoin sniper with 3-layer LLM pool selection. Gated by $SCEMA.',
  keywords: ['Solana', 'sniper', 'memecoin', 'DeFi', 'SCEMA', 'pump.fun'],
  openGraph: {
    title: 'SCEMATICA — Solana Sniper Protocol',
    description: 'AI-powered Solana sniper. 250k $SCEMA required.',
    type: 'website',
  },
  icons: { icon: '/favicon.ico' },
}

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en" className="dark">
      <body className="bg-scema-black min-h-screen font-mono antialiased">
        <div className="crt-vignette" />
        <Providers>
          {children}
        </Providers>
      </body>
    </html>
  )
}
