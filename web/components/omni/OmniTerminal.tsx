'use client'

/**
 * /omni — the decision-record console.
 *
 * Drop a `.scema/decisions/<id>.json` file in and this page re-derives its commitment,
 * reports what moved if anything did, and renders the decision the way the CLI does.
 *
 * ## Three constraints
 *
 * 1. **No network, ever.** No fetch, no upload, no daemon, no `/api` route behind this.
 *    The record is read with `FileReader` and hashed with WebCrypto in the reader's own
 *    browser. A verifier that had to send the record somewhere to check it would be asking
 *    the reader to trust a third party in order to avoid trusting one. This is a stronger
 *    version of the "no simulation branch" rule on `/mesh` and `/escrow`: there is nothing
 *    here to simulate *and* nothing to phone home to.
 *
 * 2. **The raw text is the input, never a re-serialised object.** `JSON.parse` collapses
 *    Rust's `0.0` to `0` and `JSON.stringify` writes it back without the fraction, which
 *    moves it from the FLOAT tag to the INTEGER tag in the canonical encoding and changes
 *    the digest. Nothing would be wrong with the record — the round trip destroyed
 *    information the encoding depends on. So `text` is held alongside the parsed object and
 *    only `text` is verified. `npm run check:omni` pins this.
 *
 * 3. **What VERIFIED means is on the page, not only in a comment.** It is exactly the kind
 *    of word a reader will over-read into "this is true", and it means something much
 *    narrower.
 */

import { useCallback, useMemo, useRef, useState } from 'react'

import { verifyRecordText, webSha256, type Verification } from '@/lib/omni/verify'
import {
  looksLikeRecord,
  type DecisionRecord,
  type Projection,
  type WorldState,
} from '@/lib/omni/types'
import {
  abstentionAdvice,
  abstentionHeadline,
  cell,
  coverageFraction,
  coverageLabel,
  provenanceLabel,
  truncate,
} from '@/lib/omni/view'
import { dataUri, metadataFor, plateSourceFromText } from '@/lib/omni/nft'
import { renderFractal, renderFractalPng } from '@/lib/omni/fractal'

/**
 * Edge of the downloaded PNG, in pixels.
 *
 * 1024 because that is `scema nft --png`'s default, and the two files are meant to be the
 * same file. Changing it here without changing the CLI would leave two images with one name.
 */
const PNG_SIZE = 1024

/**
 * The plate, drawn from the same text the verifier hashed.
 *
 * `null` when the record's world could not be drawn at all, which is a different state from
 * "the world is empty" — an empty world draws a perfectly good plate saying nothing was
 * read. The panel is simply absent rather than showing a blank frame that would read as a
 * measurement.
 */
interface Plate {
  svg: string
  href: string
  metadata: string
  digest: string
  /**
   * Kept so the PNG can be rastered on demand rather than up front.
   *
   * A 1024px raster is 3 MB and about a quarter of a second of main thread — cheap when
   * somebody asks for it, and a stutter on every file drop if it were eager.
   */
  world: WorldState
}

interface Loaded {
  name: string
  text: string
  record: DecisionRecord
  verification: Verification
  plate: Plate | null
}

type LoadError = { name: string; message: string }

export function OmniTerminal() {
  const [loaded, setLoaded] = useState<Loaded | null>(null)
  const [error, setError] = useState<LoadError | null>(null)
  const [busy, setBusy] = useState(false)
  const [dragging, setDragging] = useState(false)
  const fileInput = useRef<HTMLInputElement>(null)

  const ingest = useCallback(async (name: string, text: string) => {
    setBusy(true)
    setError(null)
    try {
      // Parse for rendering. Verification uses `text`, never this object — see the note at
      // the top of the file.
      const parsed: unknown = JSON.parse(text)
      if (!looksLikeRecord(parsed)) {
        throw new Error(
          'That is JSON, but not a decision record. Expected the fields id, runtime, world, projections, decision and commitment.'
        )
      }
      const verification = await verifyRecordText(text, webSha256)

      // The plate is drawn from `text` for the same reason the verifier hashes it: a
      // `JSON.parse` round trip collapses Rust's `0.0` to `0`, and the digest printed on
      // the plate has to be the record's, not one derived from a re-serialised object.
      // A failure here must not lose the verification, which is the point of the page.
      let plate: Plate | null = null
      try {
        const source = await plateSourceFromText(text, webSha256)
        const svg = renderFractal(source.world, source.digest)
        plate = {
          svg,
          href: dataUri(svg),
          metadata: JSON.stringify(metadataFor(source.world, svg, source.digest), null, 2),
          digest: source.digest,
          world: source.world,
        }
      } catch {
        plate = null
      }

      setLoaded({ name, text, record: parsed, verification, plate })
    } catch (e) {
      setLoaded(null)
      setError({ name, message: e instanceof Error ? e.message : String(e) })
    } finally {
      setBusy(false)
    }
  }, [])

  const onFiles = useCallback(
    async (files: FileList | null) => {
      const file = files?.[0]
      if (!file) return
      await ingest(file.name, await file.text())
    },
    [ingest]
  )

  return (
    <div className="omni-root font-mono text-[13px] leading-relaxed">
      <div className="mx-auto max-w-6xl px-5 py-10">
        <Header />

        <div
          onDragOver={(e) => {
            e.preventDefault()
            setDragging(true)
          }}
          onDragLeave={() => setDragging(false)}
          onDrop={(e) => {
            e.preventDefault()
            setDragging(false)
            void onFiles(e.dataTransfer.files)
          }}
          className={`mt-8 rounded border border-dashed p-8 text-center transition-colors ${
            dragging ? 'border-omni-accent bg-omni-hi' : 'border-omni-border bg-omni-surface'
          }`}
        >
          <p className="text-omni-muted">
            Drop a decision record here — <code className="text-omni-text">.scema/decisions/&lt;id&gt;.json</code>
          </p>
          <p className="mt-2 text-omni-dim">
            Nothing leaves this tab. The file is read and hashed in your browser.
          </p>
          <button
            type="button"
            onClick={() => fileInput.current?.click()}
            className="mt-4 rounded border border-omni-border-hi px-4 py-1.5 text-omni-text hover:border-omni-accent"
          >
            Choose a file
          </button>
          <input
            ref={fileInput}
            type="file"
            accept="application/json,.json"
            className="hidden"
            onChange={(e) => void onFiles(e.target.files)}
          />
        </div>

        {busy && <p className="mt-6 text-omni-muted">verifying…</p>}

        {error && (
          <section className="mt-6 rounded border border-omni-invalid bg-omni-surface p-4">
            <h2 className="text-omni-invalid">COULD NOT READ {error.name}</h2>
            <p className="mt-1 text-omni-muted">{error.message}</p>
          </section>
        )}

        {loaded && <RecordView loaded={loaded} />}
        {!loaded && !error && !busy && <WhatThisIs />}
      </div>
    </div>
  )
}

function Header() {
  return (
    <header className="border-b border-omni-border pb-5">
      <h1 className="text-lg tracking-[0.18em] text-omni-accent">SCEMATICA OMNI</h1>
      <p className="mt-1 text-omni-muted">
        Decision records — what the agent decided, on what evidence, under which preferences.
      </p>
    </header>
  )
}

function WhatThisIs() {
  return (
    <section className="mt-8 space-y-4 text-omni-muted">
      <p>
        Every pass of the omni loop seals a record: the world as perceived, the goal as
        given, every branch considered with its projected terms, the λ weights in force, and
        the choice — or the reason for refusing to make one. This page re-derives the
        commitment over all of that and tells you whether the file has been edited since.
      </p>
      <div className="rounded border border-omni-border bg-omni-surface p-4">
        <h2 className="text-omni-accent">PRODUCE ONE</h2>
        <pre className="mt-2 overflow-x-auto text-omni-text">{`cargo install scema-cli
scema decide "reduce the marker backlog" --ground markers:my-crate
# → .scema/decisions/58898030.json`}</pre>
      </div>
      <WhereThisFits />
      <Limits />
    </section>
  )
}

/**
 * Where this page sits among the other surfaces.
 *
 * Worth stating explicitly because the arrangement is genuinely surprising: the browser
 * extension and this page are both "the web part", and they share no code path, no server
 * and no network. Somebody who installs the extension expecting it to talk to this site
 * will be looking for a connection that does not exist and should not.
 */
function WhereThisFits() {
  return (
    <div className="rounded border border-omni-border bg-omni-surface p-4">
      <h2 className="text-omni-accent">WHERE THIS PAGE SITS</h2>
      <p className="mt-2">
        Omni runs on your machine. This page is the one part of it that runs here, and it is
        a reader — it never contacts an agent, and no agent contacts it.
      </p>
      <pre className="mt-3 overflow-x-auto text-omni-muted">{`  scema            CLI            → seals records into .scema/decisions/
  scema-omnid      local daemon   → 127.0.0.1:7842, token-authenticated
  scema-mcp        MCP over stdio → the loop as tools, for a model
  browser extension               → reads the page you are on, posts it
                                    to YOUR daemon on 127.0.0.1 — never here
  this page                       → drop a sealed record in, verify it offline`}</pre>
      <p className="mt-3 text-omni-muted">
        The extension&apos;s only host permission is <code className="text-omni-text">http://127.0.0.1/*</code>.
        It has no access to this site and needs none: the page you are browsing is its
        input, and your own daemon is what processes it.
      </p>
      <p className="mt-2 text-omni-dim">
        Source, and the extension you load unpacked:{' '}
        <a
          href="https://github.com/Meta-Oracle/Scematica/tree/main/scematica-omni"
          className="text-omni-accent underline decoration-omni-border hover:decoration-omni-accent"
          target="_blank"
          rel="noopener noreferrer"
        >
          Meta-Oracle/Scematica · scematica-omni
        </a>
      </p>
    </div>
  )
}

/** The three sentences that keep "VERIFIED" from being over-read. */
function Limits() {
  return (
    <div className="rounded border border-omni-border bg-omni-surface p-4">
      <h2 className="text-omni-accent">WHAT A GREEN BADGE MEANS</h2>
      <ul className="mt-2 space-y-2">
        <li>
          <span className="text-omni-valid">It proves</span> the record was not edited after
          sealing, and names the field that moved if one was.
        </li>
        <li>
          <span className="text-omni-warn">It does not prove</span> the world was as
          described. An observer that misread a repository produces a perfectly verifiable
          record of a wrong observation — provenance carries that, not the digest, which is
          why the world state is committed whole, unreadable parts included.
        </li>
        <li>
          <span className="text-omni-warn">It does not prove</span> this is the original
          record. A record and its commitment can both be regenerated by whoever holds the
          file. Tamper-evident to someone holding an earlier copy; not tamper-proof, until
          the root is anchored somewhere the author does not control.
        </li>
      </ul>
    </div>
  )
}

function RecordView({ loaded }: { loaded: Loaded }) {
  const { record, verification } = loaded
  const byId = useMemo(
    () => new Map(record.projections.map((p) => [p.hypothesis, p])),
    [record.projections]
  )
  const d = record.decision

  return (
    <div className="mt-8 space-y-6">
      <VerdictBanner v={verification} name={loaded.name} />

      <Panel title="RECORD">
        <Row k="id" v={record.id} />
        <Row k="runtime" v={record.runtime} />
        <Row k="sealed" v={new Date(record.at * 1000).toISOString()} />
        <Row k="goal" v={record.goal.statement} />
        {record.goal.constraints.map((c, i) => (
          <Row key={i} k="constraint" v={`${c.kind} \`${c.subject}\` — ${c.detail}`} />
        ))}
        {record.goal.grounded_in.length > 0 && (
          <Row
            k="grounded"
            v={`the operator asserted this addresses ${record.goal.grounded_in.join(', ')}`}
          />
        )}
      </Panel>

      <WorldPanel record={record} />

      <Panel title="SIMULATION MATRIX">
        <div className="overflow-x-auto">
          <table className="w-full min-w-[52rem] border-collapse">
            <thead>
              <tr className="text-omni-dim">
                <th className="py-1 pr-3 text-left font-normal">BRANCH</th>
                <Th>GAIN</Th>
                <Th>RISK</Th>
                <Th>COST</Th>
                <Th>UNCERT</Th>
                <Th>REVERS</Th>
                <Th>UTILITY</Th>
                <Th>MEASURED</Th>
              </tr>
            </thead>
            <tbody>
              {d.ranked.map((r) => (
                <MatrixRow
                  key={r.hypothesis}
                  statement={r.statement}
                  projection={byId.get(r.hypothesis)}
                  utility={r.utility.value}
                  coverage={r.utility.coverage}
                  chosen={d.chosen === r.hypothesis}
                />
              ))}
              {d.excluded.map((e) => (
                <tr key={e.hypothesis} className="border-t border-omni-border">
                  <td className="py-1 pr-3 text-omni-dim">{truncate(e.statement, 52)}</td>
                  <td colSpan={7} className="py-1 text-omni-warn">
                    EXCLUDED — {e.reason}
                  </td>
                </tr>
              ))}
              {d.ranked.length === 0 && d.excluded.length === 0 && (
                <tr>
                  <td colSpan={8} className="py-2 text-omni-dim">
                    no branch was allowed to compete
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
        <p className="mt-3 text-omni-muted">
          measured across the whole matrix: {coverageLabel(d.coverage)} (
          {(coverageFraction(d.coverage) * 100).toFixed(0)}%)
        </p>
        <p className="text-omni-dim">
          <span className="text-omni-unmeasured">—</span> means the term was not measured; it
          contributed nothing to the utility. It is not a zero.
        </p>
      </Panel>

      <VerdictPanel record={record} byId={byId} />
      <EvaluatorPanel record={record} />
      {loaded.plate && <PlatePanel plate={loaded.plate} name={loaded.name} />}
      <CommitmentPanel record={record} v={verification} />
      <Limits />
    </div>
  )
}

/**
 * The world, drawn.
 *
 * Two decisions worth stating, because both look like details and are not.
 *
 * It renders through `<img src="data:...">` rather than inlining the markup. The SVG is
 * built by escaped code and injecting it would probably be safe, but "probably safe" is the
 * wrong standard for a page whose entire pitch is that it trusts nothing — and an `<img>`
 * is also a live demonstration of the claim being made about the file: it is self-contained,
 * so it renders with no fetch, no font and no script.
 *
 * The legend is not decoration either. The plate is an instrument, and an instrument whose
 * dashed-versus-solid distinction is only documented in a Rust doc comment is a picture
 * people will read confidently and wrongly. The three lines below are the whole key.
 */
function PlatePanel({ plate, name }: { plate: Plate; name: string }) {
  const base = name.replace(/\.json$/i, '')
  const [rastering, setRastering] = useState(false)

  /**
   * Raster and hand over the file.
   *
   * On demand for the cost reason above, and through a `Blob` rather than a `data:` URI
   * because a 3 MB URI is past what some browsers will follow — and a download that silently
   * does nothing is worse than a button that is not there.
   *
   * A frame is yielded before the work so the button can actually paint its busy state;
   * without it the render blocks the same tick that set the flag and nothing shows.
   */
  const downloadPng = async () => {
    setRastering(true)
    try {
      await new Promise((r) => requestAnimationFrame(() => r(null)))
      const bytes = renderFractalPng(plate.world, plate.digest, PNG_SIZE)
      const url = URL.createObjectURL(new Blob([bytes], { type: 'image/png' }))
      const a = document.createElement('a')
      a.href = url
      a.download = `${base}.png`
      a.click()
      URL.revokeObjectURL(url)
    } finally {
      setRastering(false)
    }
  }

  return (
    <Panel title="PLATE">
      <div className="flex flex-col gap-5 md:flex-row md:items-start">
        <img
          src={plate.href}
          alt="The world of this record, grown as a fractal: depth from how much was observed, spread from the balance of risk against opportunity, and a severed limb for every blind spot."
          width={512}
          height={512}
          className="w-full max-w-[320px] shrink-0 rounded border border-omni-border"
        />
        <div className="min-w-0 flex-1 space-y-3">
          <p className="text-omni-muted">
            The world this record committed to, grown. The shape is the reading: how deep it
            reaches is how much the observer saw, how wide it spreads is the balance of risk
            against opportunity, and the form is seeded by the world&rsquo;s own commitment —
            so the same file always grows the same tree, here and from{' '}
            <code className="text-omni-text">scema nft</code>.
          </p>
          <ul className="space-y-1 text-omni-dim">
            <li>
              <span className="text-omni-text">A severed limb</span> — a blind spot. One cut
              per reported blind spot, never a rate, so the void in the canopy is exactly the
              ignorance the observer declared. Ignorance is a missing branch, not a faded one.
            </li>
            <li>
              <span className="text-omni-text">Dashed growth at the frontier</span> — an
              extent whose denominator is unknown. The observer never said where it ends, so
              the outermost growth is drawn as unfinished rather than as complete.
            </li>
            <li>
              <span className="text-omni-text">A hollow mark</span> — a magnitude that was
              estimated rather than counted. Triangles are risks, discs are opportunities.
            </li>
          </ul>
          <p className="text-omni-dim">
            Nothing here is decoration laid over numbers. Every property of the form is a
            quantity somebody counted, and a world nobody could read grows into something
            visibly mutilated — which is an accurate report, and is meant to be uncomfortable
            to look at. Same rule as the <span className="text-omni-unmeasured">—</span> in
            the matrix above, in the language of growth rather than of gauges.
          </p>
          <Row k="world commitment" v={<span className="break-all">{plate.digest}</span>} />
          <div className="flex flex-wrap gap-3 pt-1">
            <a
              href={plate.href}
              download={`${base}.svg`}
              className="rounded border border-omni-border-hi px-3 py-1 text-omni-text hover:border-omni-accent"
            >
              Download SVG
            </a>
            <button
              type="button"
              onClick={downloadPng}
              disabled={rastering}
              className="rounded border border-omni-border-hi px-3 py-1 text-omni-text hover:border-omni-accent disabled:opacity-50"
            >
              {rastering ? `Rastering ${PNG_SIZE}×${PNG_SIZE}…` : 'Download PNG'}
            </button>
            <a
              href={`data:application/json;charset=utf-8,${encodeURIComponent(plate.metadata)}`}
              download={`${base}.metadata.json`}
              className="rounded border border-omni-border-hi px-3 py-1 text-omni-text hover:border-omni-accent"
            >
              Download token metadata
            </a>
          </div>
          <p className="text-omni-dim">
            All three were produced in this tab and nothing was uploaded. The PNG is not the
            SVG handed to a canvas: it is rastered by a port of the same integer code{' '}
            <span className="text-omni-text">scema nft --png</span> runs, so the file that
            lands in your downloads is byte-for-byte the file the command line writes. An
            image that came out differently depending on which browser drew it would not be a
            derivative of the record. The metadata carries no score, rank or rarity: every
            trait on it is a count an observer reported, and a ranking invented here would be
            a number of the right shape with nothing behind it.
          </p>
        </div>
      </div>
    </Panel>
  )
}

function VerdictBanner({ v, name }: { v: Verification; name: string }) {
  const good = v.valid
  return (
    <section
      className={`rounded border p-4 ${
        good ? 'border-omni-valid bg-omni-surface' : 'border-omni-invalid bg-omni-surface'
      }`}
    >
      <h2 className={good ? 'text-omni-valid' : 'text-omni-invalid'}>
        {good ? 'COMMITMENT VALID' : 'COMMITMENT INVALID'} · {v.id}
      </h2>
      <p className="mt-1 text-omni-muted">{name}</p>
      {!good && (
        <>
          <p className="mt-3 text-omni-text">
            {v.rootOnly
              ? 'Every part verifies but the root does not — the root was rewritten on its own, which is the signature of a hand edit.'
              : 'These fields do not match the digests stored in the record:'}
          </p>
          <ul className="mt-2 space-y-1">
            {v.mismatches.map((m) => (
              <li key={m.field} className="text-omni-muted">
                <span className="text-omni-invalid">{m.field}</span>{' '}
                <span className="text-omni-dim">
                  committed {m.committed.slice(0, 12)}… recomputed {m.recomputed.slice(0, 12)}…
                </span>
              </li>
            ))}
          </ul>
        </>
      )}
    </section>
  )
}

function WorldPanel({ record }: { record: DecisionRecord }) {
  const w = record.world
  const unbounded = w.extent.total === null
  return (
    <Panel title="WORLD">
      <Row k="entity" v={w.entity.locator} />
      <Row k="kind" v={`${w.entity.kind} · ${w.domain} · observer \`${w.observer}\``} />
      <Row
        k="extent"
        v={
          <>
            {w.extent.observed} observed{' '}
            {unbounded && <span className="text-omni-warn">— EXTENT UNBOUNDED</span>}{' '}
            <span className="text-omni-dim">({w.extent.note})</span>
          </>
        }
      />
      {w.blind_spots.length > 0 && (
        <div className="mt-2">
          <span className="text-omni-warn">blind spots ({w.blind_spots.length})</span>
          <ul className="mt-1 space-y-0.5">
            {w.blind_spots.slice(0, 8).map((b, i) => (
              <li key={i} className="text-omni-dim">
                · {b}
              </li>
            ))}
          </ul>
        </div>
      )}
      {w.signals.length > 0 && (
        <div className="mt-3">
          <span className="text-omni-dim">signals</span>
          <ul className="mt-1 space-y-1">
            {w.signals.map((s) => (
              <li key={s.id}>
                <span className={s.polarity === 'risk' ? 'text-omni-invalid' : 'text-omni-accent'}>
                  {s.polarity.toUpperCase()}
                </span>{' '}
                <span className={s.measured ? 'text-omni-muted' : 'text-omni-warn'}>
                  {s.measured ? 'counted' : 'ESTIMATED'}
                </span>{' '}
                <span className="text-omni-text">{s.magnitude.toFixed(2)}</span> {s.label}
                {s.evidence[0] && <div className="text-omni-dim">└ {s.evidence[0]}</div>}
              </li>
            ))}
          </ul>
        </div>
      )}
      <div className="mt-3">
        <span className="text-omni-dim">objects ({w.objects.length})</span>
        <ul className="mt-1 space-y-0.5">
          {w.objects.slice(0, 12).map((o) => (
            <li key={o.id}>
              <span
                className={
                  o.provenance.kind === 'live'
                    ? 'text-omni-valid'
                    : o.provenance.kind === 'absent'
                      ? 'text-omni-unmeasured'
                      : 'text-omni-warn'
                }
              >
                {provenanceLabel(o.provenance)}
              </span>{' '}
              <span className="text-omni-text">{o.label}</span>{' '}
              <span className="text-omni-dim">
                {Object.keys(o.attrs).length === 0
                  ? '(no values — unseen, not empty)'
                  : Object.entries(o.attrs)
                      .map(([k, s]) => `${k}=${String(s.v)}`)
                      .join(' ')}
              </span>
            </li>
          ))}
        </ul>
      </div>
    </Panel>
  )
}

function VerdictPanel({
  record,
  byId,
}: {
  record: DecisionRecord
  byId: Map<string, Projection>
}) {
  const d = record.decision
  if (d.chosen) {
    const top = d.ranked.find((r) => r.hypothesis === d.chosen)
    return (
      <Panel title="DECISION">
        <p className="text-omni-valid">{d.chosen}</p>
        <p className="text-omni-text">{top?.statement}</p>
        <p className="mt-3 text-omni-dim">because</p>
        <ul className="mt-1 space-y-0.5">
          {top?.utility.contributions.map((c) => (
            <li key={c.symbol}>
              <span
                className={
                  c.measured ? 'text-omni-text' : 'text-omni-unmeasured'
                }
              >
                {c.effect >= 0 ? '+' : ''}
                {c.effect.toFixed(3)}
              </span>{' '}
              <span className="text-omni-accent">{c.symbol}</span>{' '}
              <span className="text-omni-dim">{c.note}</span>
            </li>
          ))}
          <li className="pt-1 text-omni-text">
            = {top?.utility.value.toFixed(3)} utility
          </li>
        </ul>
        {byId.get(d.chosen)?.failure_modes.length ? (
          <div className="mt-4">
            <p className="text-omni-dim">failure modes</p>
            <ul className="mt-1 space-y-1">
              {byId.get(d.chosen)!.failure_modes.map((f, i) => (
                <li key={i}>
                  <span className={f.likelihood.measured ? 'text-omni-text' : 'text-omni-unmeasured'}>
                    {cell(f.likelihood)}
                  </span>{' '}
                  <span className="text-omni-warn">{f.label}</span>
                  <div className="text-omni-dim">{f.detail}</div>
                </li>
              ))}
            </ul>
            <p className="mt-2 text-omni-dim">
              A named failure mode with an unknown likelihood is the point, not a gap.
            </p>
          </div>
        ) : null}
      </Panel>
    )
  }
  return (
    <Panel title="ABSTAINED">
      <p className="text-omni-warn">{abstentionHeadline(d.abstention)}</p>
      <p className="mt-1 text-omni-muted">{abstentionAdvice(d.abstention)}</p>
    </Panel>
  )
}

function EvaluatorPanel({ record }: { record: DecisionRecord }) {
  const s = record.decision.evaluator_status
  if (s.length === 0) return null
  return (
    <Panel title="EVALUATORS">
      <ul className="space-y-1">
        {s.map((e) => (
          <li key={e.evaluator}>
            <span className="text-omni-text">{e.evaluator}</span>{' '}
            <span
              className={
                e.applicability.kind === 'applicable' ? 'text-omni-valid' : 'text-omni-dim'
              }
            >
              {e.applicability.kind.replace(/_/g, '-').toUpperCase()}
            </span>
            <div className="text-omni-dim">{e.applicability.note}</div>
          </li>
        ))}
      </ul>
      <p className="mt-2 text-omni-dim">
        A specialist that declined said nothing — which is different from a specialist that
        approved, and both are different from one that was never asked.
      </p>
    </Panel>
  )
}

function CommitmentPanel({ record, v }: { record: DecisionRecord; v: Verification }) {
  const moved = new Set(v.mismatches.map((m) => m.field))
  return (
    <Panel title="COMMITMENT">
      <ul className="space-y-0.5">
        {(
          ['world', 'goal', 'hypotheses', 'projections', 'policy', 'decision', 'root'] as const
        ).map((f) => (
          <li key={f}>
            <span className={moved.has(f) ? 'text-omni-invalid' : 'text-omni-muted'}>
              {f.padEnd(12, ' ')}
            </span>{' '}
            <span className="text-omni-dim break-all">{record.commitment[f]}</span>
          </li>
        ))}
      </ul>
      <p className="mt-2 text-omni-dim">
        sha256 over a canonical encoding — sorted keys, tagged types, floats bound at 1e-9.
        The root binds the six parts to their field names, and the record id is its first
        eight hex characters.
      </p>
    </Panel>
  )
}

function MatrixRow({
  statement,
  projection,
  utility,
  coverage,
  chosen,
}: {
  statement: string
  projection: Projection | undefined
  utility: number
  coverage: { measured: number; total: number }
  chosen: boolean
}) {
  const terms = [
    projection?.expected_gain,
    projection?.risk,
    projection?.cost,
    projection?.uncertainty,
    projection?.reversibility,
  ]
  return (
    <tr className={`border-t border-omni-border ${chosen ? 'text-omni-valid' : 'text-omni-text'}`}>
      <td className="py-1 pr-3">
        {chosen ? '▸ ' : ''}
        {truncate(statement, 52)}
      </td>
      {terms.map((t, i) => (
        <td
          key={i}
          className={`py-1 text-right tabular-nums ${
            t?.measured ? '' : 'text-omni-unmeasured'
          }`}
          title={t?.note}
        >
          {cell(t)}
        </td>
      ))}
      <td className="py-1 text-right tabular-nums font-bold">{utility.toFixed(3)}</td>
      <td className="py-1 text-right tabular-nums text-omni-dim">{coverageLabel(coverage)}</td>
    </tr>
  )
}

function Panel({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="rounded border border-omni-border bg-omni-surface p-4">
      <h2 className="mb-2 tracking-[0.14em] text-omni-accent">{title}</h2>
      {children}
    </section>
  )
}

function Row({ k, v }: { k: string; v: React.ReactNode }) {
  return (
    <div className="flex gap-3">
      <span className="w-24 shrink-0 text-omni-dim">{k}</span>
      <span className="text-omni-text break-all">{v}</span>
    </div>
  )
}

function Th({ children }: { children: React.ReactNode }) {
  return <th className="py-1 pl-3 text-right font-normal">{children}</th>
}
