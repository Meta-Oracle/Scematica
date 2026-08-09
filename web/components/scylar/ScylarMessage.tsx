'use client'

import { useMemo, useState } from 'react'

import { parseMarkdown, type Block, type Inline } from '@/lib/scylar/markdown'

// Renders a parsed message. Elements only — nothing here ever builds an HTML string, so
// there is no path from model output to `dangerouslySetInnerHTML`. See lib/scylar/markdown.ts.

interface Props {
  content: string
  /** Streaming: suppresses the copy affordance on a block still being written. */
  streaming?: boolean
}

export function ScylarMessage({ content, streaming = false }: Props) {
  const blocks = useMemo(() => parseMarkdown(content), [content])

  return (
    <div className="space-y-2 text-sm leading-relaxed text-scylar-text">
      {blocks.map((block, i) => (
        <BlockView key={i} block={block} streaming={streaming} />
      ))}
    </div>
  )
}

function BlockView({ block, streaming }: { block: Block; streaming: boolean }) {
  switch (block.kind) {
    case 'code':
      return <CodeBlock lang={block.lang} text={block.text} open={block.open || streaming} />

    case 'heading': {
      const size = block.level === 1 ? 'text-sm' : 'text-xs'
      return (
        <p className={`${size} font-bold tracking-wide text-scylar-violet-hi`}>
          <InlineRun nodes={block.inline} />
        </p>
      )
    }

    case 'list':
      return (
        <ul className="space-y-1 pl-1">
          {block.items.map((item, i) => (
            <li key={i} className="flex gap-2">
              <span className="select-none text-scylar-violet-dim">
                {block.ordered ? `${i + 1}.` : '·'}
              </span>
              <span className="min-w-0 flex-1">
                <InlineRun nodes={item} />
              </span>
            </li>
          ))}
        </ul>
      )

    case 'para':
      return (
        <p className="whitespace-pre-wrap break-words">
          <InlineRun nodes={block.inline} />
        </p>
      )
  }
}

function InlineRun({ nodes }: { nodes: Inline[] }) {
  return (
    <>
      {nodes.map((node, i) => {
        switch (node.kind) {
          case 'code':
            return (
              <code key={i} className="scylar-inline-code">
                {node.text}
              </code>
            )
          case 'strong':
            return (
              <strong key={i} className="font-bold text-scylar-violet-hi">
                {node.text}
              </strong>
            )
          case 'link':
            return (
              <a
                key={i}
                href={node.href}
                target="_blank"
                // `noreferrer` alongside `noopener` because the destination came from a
                // model relaying user text; there is no reason to hand it this page.
                rel="noopener noreferrer"
                className="text-scylar-violet underline decoration-scylar-violet-dim underline-offset-2 hover:text-scylar-violet-hi"
              >
                {node.text}
              </a>
            )
          default:
            return <span key={i}>{node.text}</span>
        }
      })}
    </>
  )
}

function CodeBlock({ lang, text, open }: { lang: string; text: string; open: boolean }) {
  const [copied, setCopied] = useState(false)

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(text)
      setCopied(true)
      setTimeout(() => setCopied(false), 1600)
    } catch {
      // Clipboard access is refused on insecure origins and in some embedded webviews —
      // including the Capacitor build. The text is selectable either way, so this
      // reports the failure rather than pretending the copy happened.
      setCopied(false)
    }
  }

  return (
    <div className="scylar-code group relative">
      <div className="flex items-center justify-between border-b border-scylar-border/60 px-2 py-1">
        <span className="text-[0.6rem] tracking-widest text-scylar-dim">
          {lang ? lang.toUpperCase() : 'CODE'}
        </span>
        {/* Hidden while the fence is still open: copying a half-written function
            produces something that does not compile, silently. */}
        {!open && (
          <button
            onClick={() => void copy()}
            className="text-[0.6rem] tracking-widest text-scylar-dim transition-colors hover:text-scylar-violet-hi"
          >
            {copied ? 'COPIED' : 'COPY'}
          </button>
        )}
      </div>
      <pre className="overflow-x-auto px-3 py-2">
        <code className="text-xs leading-relaxed">{text}</code>
      </pre>
    </div>
  )
}
