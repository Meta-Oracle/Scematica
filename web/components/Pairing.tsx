'use client'

import { useState } from 'react'
import { setPairing, probePairing, normalizeBase, type Pairing as PairingT } from '@/lib/net'

// The one-time pairing screen shown in the mobile app before it knows which sniper to
// talk to. The operator runs their own instance (self-hosted), exposes the Rust API,
// and pairs the phone to it with a base URL + the instance's SCEMATICA_API_TOKEN. A
// pairing string `scematica://pair?url=<base>&token=<tok>` can be pasted to fill both
// fields at once (a QR of that string is the frictionless path — see docs/mobile-app.md).

function parsePairString(s: string): Partial<PairingT> | null {
  try {
    const t = s.trim()
    if (t.startsWith('scematica://')) {
      const u = new URL(t)
      const url = u.searchParams.get('url') ?? undefined
      const token = u.searchParams.get('token') ?? undefined
      const label = u.searchParams.get('label') ?? undefined
      return url ? { baseUrl: url, token, label } : null
    }
    // Bare URL pasted.
    if (t.startsWith('http://') || t.startsWith('https://')) return { baseUrl: t }
    return null
  } catch {
    return null
  }
}

export function Pairing({ onPaired }: { onPaired?: () => void }) {
  const [baseUrl, setBaseUrl] = useState('')
  const [token, setToken] = useState('')
  const [label, setLabel] = useState('')
  const [status, setStatus] = useState<'idle' | 'probing' | 'error'>('idle')
  const [error, setError] = useState('')

  function applyPasted(s: string) {
    const p = parsePairString(s)
    if (p?.baseUrl) {
      setBaseUrl(p.baseUrl)
      if (p.token) setToken(p.token)
      if (p.label) setLabel(p.label)
    } else {
      setBaseUrl(s)
    }
  }

  async function pair() {
    setStatus('probing')
    setError('')
    const candidate: PairingT = {
      // Normalised here so the field shows the operator what was actually saved — the
      // repaired `/api` suffix is the whole reason this pairing used to fail silently.
      baseUrl: normalizeBase(baseUrl),
      token: token.trim() || undefined,
      label: label.trim() || undefined,
    }
    if (!/^https?:\/\//.test(candidate.baseUrl)) {
      setStatus('error')
      setError('Base URL must start with http:// or https://')
      return
    }
    if (candidate.baseUrl !== baseUrl.trim()) setBaseUrl(candidate.baseUrl)

    const probe = await probePairing(candidate)
    if (!probe.ok) {
      setStatus('error')
      setError(
        {
          unauthorized:
            'The instance rejected this token. Check SCEMATICA_API_TOKEN on the machine running the API.',
          'not-an-api': `Something answered at ${candidate.baseUrl} but it is not a Scematica API — GET ${candidate.baseUrl}/api/health did not return JSON. Give the API's root URL (the port the API prints on startup), not a page or a sub-path.`,
          'mixed-content': `This dashboard is served over HTTPS and ${candidate.baseUrl} is plain HTTP, so the browser blocks the request before it is sent. Put an HTTPS tunnel (cloudflared, ngrok, Tailscale Funnel) in front of the API, or open the dashboard from http://localhost:3000 on the machine running the bot.`,
          unreachable:
            'Could not reach the instance. Check the URL and port, that the API is running, that CORS/a firewall isn’t blocking it, and that the tunnel is up.',
        }[probe.reason],
      )
      return
    }
    setPairing(candidate)
    onPaired?.()
    // Reload so every panel re-fetches against the paired instance.
    if (typeof window !== 'undefined') window.location.reload()
  }

  return (
    <div className="min-h-screen flex flex-col items-center justify-center bg-scema-black px-6 text-scema-muted">
      <div className="w-full max-w-sm">
        <div className="flex items-center gap-3 mb-8">
          <div className="relative w-8 h-8 flex items-center justify-center">
            <div className="absolute inset-0 border border-scema-red rotate-45 animate-glow-pulse" />
            <span className="text-scema-red-hi text-sm font-bold relative z-10">S</span>
          </div>
          <div>
            <div className="text-scema-muted font-bold tracking-wide">SCEMATICA</div>
            <div className="text-xs text-scema-dim">pair with your sniper</div>
          </div>
        </div>

        <label className="block text-xs text-scema-dim mb-1">Instance URL</label>
        <input
          value={baseUrl}
          onChange={(e) => applyPasted(e.target.value)}
          placeholder="http://192.168.1.50:3001"
          autoCapitalize="none"
          autoCorrect="off"
          spellCheck={false}
          className="w-full mb-4 bg-scema-dim/20 border border-scema-border rounded px-3 py-2 text-sm text-scema-muted outline-none focus:border-scema-red/60"
        />

        <label className="block text-xs text-scema-dim mb-1">API token</label>
        <input
          value={token}
          onChange={(e) => setToken(e.target.value)}
          placeholder="SCEMATICA_API_TOKEN"
          autoCapitalize="none"
          autoCorrect="off"
          spellCheck={false}
          type="password"
          className="w-full mb-4 bg-scema-dim/20 border border-scema-border rounded px-3 py-2 text-sm text-scema-muted outline-none focus:border-scema-red/60"
        />

        <label className="block text-xs text-scema-dim mb-1">Label (optional)</label>
        <input
          value={label}
          onChange={(e) => setLabel(e.target.value)}
          placeholder="my-vps"
          className="w-full mb-5 bg-scema-dim/20 border border-scema-border rounded px-3 py-2 text-sm text-scema-muted outline-none focus:border-scema-red/60"
        />

        {status === 'error' && <div className="text-xs text-scema-red-hi mb-3">{error}</div>}

        <button
          onClick={pair}
          disabled={status === 'probing'}
          className="w-full py-2.5 rounded border border-scema-red/70 text-scema-red-hi bg-scema-red/10 text-sm font-bold tracking-wide disabled:opacity-50"
        >
          {status === 'probing' ? 'PAIRING…' : 'PAIR'}
        </button>

        <p className="text-[11px] text-scema-dim mt-5 leading-relaxed">
          Your keys and capital stay on your own machine. This app is a remote for the
          sniper you run yourself. Set <code className="text-scema-muted">SCEMATICA_API_TOKEN</code> on
          the instance and expose the API (LAN, Tailscale, or a reverse proxy).
        </p>
      </div>
    </div>
  )
}
