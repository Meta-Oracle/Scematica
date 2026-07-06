'use client'

import { useMemo, useState } from 'react'
import { QRCodeSVG } from 'qrcode.react'

// Operator-facing pairing-QR generator. Runs on the web dashboard (visit /pair). The
// operator enters their API base URL and the instance's SCEMATICA_API_TOKEN; this
// renders a `scematica://pair?…` QR the mobile app scans/pastes to pair in one step.
// The token is used only client-side to build the string — it is never sent anywhere.

function buildPairString(url: string, token: string, label: string): string {
  const p = new URLSearchParams()
  p.set('url', url.trim())
  if (token.trim()) p.set('token', token.trim())
  if (label.trim()) p.set('label', label.trim())
  return `scematica://pair?${p.toString()}`
}

export default function PairPage() {
  const [url, setUrl] = useState('')
  const [token, setToken] = useState('')
  const [label, setLabel] = useState('')
  const [copied, setCopied] = useState(false)

  const valid = /^https?:\/\//.test(url.trim())
  const pairString = useMemo(
    () => (valid ? buildPairString(url, token, label) : ''),
    [valid, url, token, label],
  )

  async function copy() {
    if (!pairString) return
    try {
      await navigator.clipboard.writeText(pairString)
      setCopied(true)
      setTimeout(() => setCopied(false), 1200)
    } catch {
      /* clipboard blocked */
    }
  }

  return (
    <div className="min-h-screen bg-scema-black text-scema-muted px-6 py-10 flex justify-center">
      <div className="w-full max-w-md">
        <div className="flex items-center gap-3 mb-8">
          <div className="relative w-8 h-8 flex items-center justify-center">
            <div className="absolute inset-0 border border-scema-red rotate-45 animate-glow-pulse" />
            <span className="text-scema-red-hi text-sm font-bold relative z-10">S</span>
          </div>
          <div>
            <div className="font-bold tracking-wide">PAIR A DEVICE</div>
            <div className="text-xs text-scema-dim">generate a QR for the mobile app</div>
          </div>
        </div>

        <label className="block text-xs text-scema-dim mb-1">Instance URL</label>
        <input
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          placeholder="http://192.168.1.50:3001"
          autoCapitalize="none"
          autoCorrect="off"
          spellCheck={false}
          className="w-full mb-4 bg-scema-dim/20 border border-scema-border rounded px-3 py-2 text-sm outline-none focus:border-scema-red/60"
        />

        <label className="block text-xs text-scema-dim mb-1">
          API token <span className="text-scema-dim/70">(SCEMATICA_API_TOKEN)</span>
        </label>
        <input
          value={token}
          onChange={(e) => setToken(e.target.value)}
          placeholder="paste the instance token"
          autoCapitalize="none"
          autoCorrect="off"
          spellCheck={false}
          type="password"
          className="w-full mb-4 bg-scema-dim/20 border border-scema-border rounded px-3 py-2 text-sm outline-none focus:border-scema-red/60"
        />

        <label className="block text-xs text-scema-dim mb-1">Label (optional)</label>
        <input
          value={label}
          onChange={(e) => setLabel(e.target.value)}
          placeholder="my-vps"
          className="w-full mb-6 bg-scema-dim/20 border border-scema-border rounded px-3 py-2 text-sm outline-none focus:border-scema-red/60"
        />

        {valid ? (
          <div className="flex flex-col items-center">
            <div className="bg-white p-4 rounded-lg">
              <QRCodeSVG value={pairString} size={220} level="M" includeMargin={false} />
            </div>
            <button
              onClick={copy}
              className="mt-4 w-full py-2 rounded border border-scema-border text-xs text-scema-muted hover:border-scema-red/60"
            >
              {copied ? 'COPIED ✓' : 'COPY PAIRING LINK'}
            </button>
            <code className="mt-3 text-[10px] text-scema-dim break-all text-center">{pairString}</code>
          </div>
        ) : (
          <div className="text-xs text-scema-dim text-center py-10 border border-dashed border-scema-border rounded">
            Enter an instance URL (http:// or https://) to generate the QR.
          </div>
        )}

        <p className="text-[11px] text-scema-dim mt-6 leading-relaxed">
          Scan this in the Scematica app to pair it to this instance. The token stays in
          your browser — it is only encoded into the QR you choose to show.
        </p>
      </div>
    </div>
  )
}
