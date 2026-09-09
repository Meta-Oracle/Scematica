'use client'

// The product nav, at every width.
//
// Above `md` it is the chip row it has always been. Below `md` the chips were
// `hidden md:flex` — which is to say that on a phone none of the eight products were
// linked at all, and the only way to reach one was to already know its URL. The sitemap
// has carried a comment saying exactly that since it was written.
//
// Chips do not simply un-hide: eight of them at ~120px wrap to four rows on a 360px
// screen, inside a `sticky` header, which would eat the viewport above the fold. So the
// small screen gets a disclosure instead — and a menu row has space for a sentence, so
// each product says what it is rather than relying on a glyph and a colour the way a chip
// does.
//
// Both render from `PRODUCTS`. Two lists would drift, and the drift is invisible: a
// product added to the chips and not the menu is simply unreachable on a phone, which is
// the state this component exists to end.

import Link from 'next/link'
import { useCallback, useEffect, useId, useRef, useState } from 'react'

import { PRODUCTS } from './products'

export function ProductNav() {
  const [open, setOpen] = useState(false)
  const panelId = useId()
  const buttonRef = useRef<HTMLButtonElement>(null)
  const panelRef = useRef<HTMLDivElement>(null)

  const close = useCallback(() => setOpen(false), [])

  // Escape closes and returns focus to the button that opened it. Without the focus
  // return, dismissing the menu leaves a keyboard user at the top of the document.
  useEffect(() => {
    if (!open) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        setOpen(false)
        buttonRef.current?.focus()
      }
    }
    // A click anywhere outside dismisses it. `pointerdown` rather than `click` so the
    // menu is gone before whatever was tapped underneath reacts.
    const onPointer = (e: PointerEvent) => {
      const t = e.target as Node
      if (!panelRef.current?.contains(t) && !buttonRef.current?.contains(t)) setOpen(false)
    }
    document.addEventListener('keydown', onKey)
    document.addEventListener('pointerdown', onPointer)
    return () => {
      document.removeEventListener('keydown', onKey)
      document.removeEventListener('pointerdown', onPointer)
    }
  }, [open])

  return (
    <>
      {/* ── the chip row, md and up ─────────────────────────────────────── */}
      {PRODUCTS.map(p => (
        <Link
          key={p.href}
          href={p.href}
          className={`hidden md:flex items-center gap-1.5 px-2 py-0.5 border
                      transition-all text-xs tracking-widest ${p.chip}`}
        >
          {p.glyph} {p.label}
        </Link>
      ))}

      {/* ── the disclosure, below md ────────────────────────────────────── */}
      <button
        ref={buttonRef}
        type="button"
        onClick={() => setOpen(v => !v)}
        aria-expanded={open}
        aria-controls={panelId}
        aria-label={open ? 'Close the product menu' : 'Open the product menu'}
        className="md:hidden flex items-center gap-2 px-2.5 py-1 border border-scema-border
                   text-scema-text hover:border-scema-red hover:text-scema-red-hi
                   transition-all text-xs tracking-widest"
      >
        <span aria-hidden="true">{open ? '✕' : '☰'}</span>
        {/* The count is here because it is the honest answer to "is there anything in
            here?" — a bare hamburger on a trading dashboard reads as settings. */}
        <span>{open ? 'CLOSE' : `PRODUCTS · ${PRODUCTS.length}`}</span>
      </button>

      {open && (
        <div
          id={panelId}
          ref={panelRef}
          // Anchored to the header rather than the button, so it spans the full width on
          // a narrow screen instead of hanging off the right edge.
          className="md:hidden absolute left-0 right-0 top-full max-h-[70vh] overflow-y-auto
                     border-b border-scema-border bg-scema-black/98 backdrop-blur-sm"
        >
          <nav aria-label="Products">
            {PRODUCTS.map(p => (
              <Link
                key={p.href}
                href={p.href}
                onClick={close}
                className={`flex flex-col gap-0.5 px-4 py-3 border-b border-scema-border/60
                            last:border-b-0 border-l-2 ${p.chip}`}
              >
                <span className="text-xs tracking-widest">
                  <span aria-hidden="true">{p.glyph}</span> {p.label}
                </span>
                {/* Deliberately not the product's own colour: the accent belongs to the
                    name, and a paragraph in eight different hues is unreadable. */}
                <span className="text-[11px] leading-snug text-scema-muted">{p.blurb}</span>
              </Link>
            ))}
          </nav>
        </div>
      )}
    </>
  )
}
