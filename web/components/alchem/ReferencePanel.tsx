'use client'

import { useState } from 'react'

import {
  ALCHEMY_CAPABILITIES,
  BLUEPRINT,
  BLUEPRINT_NEXT_STEPS,
  CHAINLINK_CAPABILITIES,
  INTEGRATION_MAP,
  RECIPES,
} from '@/lib/alchem/reference'

// The offline half of alchem-link — the same Blueprint / Alchemy / Chainlink /
// Integration / Recipes reference the TUI carries.
//
// Static prose, and presented as such: no source badge, no freshness, no polling. It
// sits below the live board precisely so the two are never confused for each other.

type Tab = 'blueprint' | 'alchemy' | 'chainlink' | 'integration' | 'recipes'

const TABS: { key: Tab; label: string }[] = [
  { key: 'blueprint', label: '⬡ Blueprint' },
  { key: 'alchemy', label: '◈ Alchemy' },
  { key: 'chainlink', label: '⬢ Chainlink' },
  { key: 'integration', label: '⇄ Integration' },
  { key: 'recipes', label: '✦ Recipes' },
]

function KeyValue({ name, value }: { name: string; value: string }) {
  return (
    <div className="mb-2">
      <p className="text-alchem-blue text-[0.65rem] font-bold uppercase tracking-wider">{name}</p>
      <p className="text-alchem-text text-xs">{value}</p>
    </div>
  )
}

export function ReferencePanel() {
  const [tab, setTab] = useState<Tab>('blueprint')
  const [openRecipe, setOpenRecipe] = useState<string | null>(null)

  return (
    <div className="alchem-panel">
      <div className="alchem-panel-header">Reference</div>

      <div className="flex flex-wrap gap-1 px-3 pt-2">
        {TABS.map(t => (
          <button
            key={t.key}
            onClick={() => setTab(t.key)}
            className={`px-2.5 py-1 text-[0.65rem] border transition-colors ${
              tab === t.key
                ? 'border-alchem-blue text-alchem-blue-hi bg-alchem-blue/10'
                : 'border-alchem-border text-alchem-muted hover:border-alchem-border-hi hover:text-alchem-text'
            }`}
          >
            {t.label}
          </button>
        ))}
      </div>

      <div className="p-3">
        {tab === 'blueprint' && (
          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            {Object.entries(BLUEPRINT).map(([section, fields]) => (
              <div key={section}>
                <p className="glow-blue text-xs font-bold uppercase tracking-widest mb-2">
                  {section}
                </p>
                {Object.entries(fields).map(([k, v]) => (
                  <KeyValue key={k} name={k} value={v} />
                ))}
              </div>
            ))}
            <div className="md:col-span-2 border-t border-alchem-border/50 pt-3">
              <p className="glow-blue text-xs font-bold uppercase tracking-widest mb-2">
                Next steps
              </p>
              {BLUEPRINT_NEXT_STEPS.map((step, i) => (
                <p key={step} className="text-xs mb-1">
                  <span className="text-alchem-amber font-bold">{i + 1}.</span>{' '}
                  <span className="text-alchem-text">{step}</span>
                </p>
              ))}
            </div>
          </div>
        )}

        {tab === 'alchemy' && (
          <div className="grid grid-cols-1 md:grid-cols-2 gap-x-6">
            {ALCHEMY_CAPABILITIES.map((c, i) => (
              <KeyValue key={`${c.key}-${i}`} name={c.key} value={c.value} />
            ))}
          </div>
        )}

        {tab === 'chainlink' && (
          <div className="grid grid-cols-1 md:grid-cols-2 gap-x-6">
            {CHAINLINK_CAPABILITIES.map(c => (
              <KeyValue key={c.key} name={c.key} value={c.value} />
            ))}
          </div>
        )}

        {tab === 'integration' && (
          <div className="flex flex-col gap-3">
            {INTEGRATION_MAP.map(row => (
              <div key={row.domain} className="border-t border-alchem-border/40 pt-2">
                <p className="glow-blue text-xs font-bold uppercase tracking-widest mb-1.5">
                  {row.domain}
                </p>
                <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
                  <div>
                    <p className="text-alchem-blue-dim text-[0.6rem] uppercase tracking-wider">
                      Alchemy
                    </p>
                    <p className="text-alchem-text text-xs">{row.alchemy}</p>
                  </div>
                  <div>
                    <p className="text-alchem-blue-dim text-[0.6rem] uppercase tracking-wider">
                      Chainlink
                    </p>
                    <p className="text-alchem-text text-xs">{row.chainlink}</p>
                  </div>
                </div>
              </div>
            ))}
          </div>
        )}

        {tab === 'recipes' && (
          <div className="flex flex-col gap-2">
            {RECIPES.map(recipe => {
              const open = openRecipe === recipe.id
              return (
                <div key={recipe.id} className="border border-alchem-border/60">
                  <button
                    onClick={() => setOpenRecipe(open ? null : recipe.id)}
                    className="w-full text-left px-3 py-2 hover:bg-alchem-hi transition-colors"
                    aria-expanded={open}
                  >
                    <div className="flex items-center justify-between gap-2">
                      <span className="glow-blue text-xs font-bold">{recipe.name}</span>
                      <span className="text-alchem-dim text-[0.6rem] shrink-0">
                        {open ? '−' : '+'}
                      </span>
                    </div>
                    <p className="text-alchem-muted text-[0.7rem] mt-0.5">{recipe.summary}</p>
                    <div className="flex flex-wrap gap-1.5 mt-1.5">
                      {recipe.tags.map(tag => (
                        <span key={tag} className="text-alchem-green text-[0.6rem]">
                          #{tag}
                        </span>
                      ))}
                    </div>
                  </button>

                  {open && (
                    <div className="border-t border-alchem-border/60 px-3 py-2 bg-alchem-black/40">
                      <p className="text-alchem-blue text-[0.6rem] uppercase tracking-widest mb-1.5">
                        Steps
                      </p>
                      {recipe.steps.map((step, i) => (
                        <p key={step} className="text-xs mb-1">
                          <span className="text-alchem-amber font-bold">{i + 1}.</span>{' '}
                          <span className="text-alchem-text">{step}</span>
                        </p>
                      ))}
                      <p className="text-alchem-dim text-[0.6rem] mt-2">
                        CLI: <span className="text-alchem-blue-dim">
                          alchem-link recipes {recipe.id}
                        </span>
                      </p>
                    </div>
                  )}
                </div>
              )
            })}
          </div>
        )}
      </div>
    </div>
  )
}
