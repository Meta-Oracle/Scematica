/**
 * A GitHub repository, perceived in the browser and emitted as a `scema.world/1` WorldState.
 *
 * ## The fifth producer, and why it belongs here rather than in omni
 *
 * `scematica-omni/docs/PRODUCERS.md` lists four producers on this contract — a source tree, a DOM,
 * a running Scematica system and a network's oracle feeds — and states the reason there are four
 * rather than one implementation: **the observed thing describes itself in scema-world's
 * vocabulary**, in whatever language it already lives in, because omni's crates cannot link a
 * browser, a Python package, or a workspace pinned around `solana-sdk`.
 *
 * A repository on GitHub is the same situation one step further out. `scema_tools::RepoObserver`
 * perceives a source tree **on a disk**, in Rust, in a process with a filesystem. A player with a
 * URL has none of those. So this is a producer written where the observation can actually be made:
 * client-side TypeScript, talking to `api.github.com` over CORS, with no server of ours in the
 * path — which keeps `/scema-world`'s standing claim (there is no back end here) exactly true.
 *
 * ## What this produces, stated precisely, because the distinction is the whole product
 *
 * **An observation. Not a sealed decision record.**
 *
 * `scema observe --nft` draws a plate from a perceived world and the CLI documentation is careful
 * about what that plate is: *it commits to an observation, not to a judgement* — binding a picture
 * to a sealed decision is what `scema nft <record>` is for, and that needs the loop, the utility
 * weights and a signed record, none of which a browser can honestly produce.
 *
 * So a world observed here can be flown, and drawn, and its commitment is computed with the same
 * canonical encoder Rust uses (`lib/omni/canonical.ts`, pinned byte-for-byte by `check:omni`). It
 * cannot be *verified*, because there is nothing to verify against: nobody sealed it. The UI must
 * say so rather than letting an observation borrow a decision record's authority — that borrowing
 * is precisely the thing every rule in this project exists to prevent.
 *
 * ## The five producer rules, and what each one costs here
 *
 * 1. **An unreadable thing is a blind spot, never a zero.** GitHub refuses plenty: private repos,
 *    rate limits at sixty requests an hour unauthenticated, and a `git/trees` response it
 *    *truncates* past a size it will not tell you in advance. Every one of those becomes a blind
 *    spot naming the mechanism, never an empty list.
 * 2. **`measured: true` means somebody counted something.** Every signal here carries a count that
 *    came off a response body, with the count in `evidence`. There is no "repository health"
 *    figure and there must never be one.
 * 3. **Stale is not fresh.** The API answers now, so objects are `live` — but `pushed_at` is a
 *    fact about the repository rather than about the read, and it goes in an attribute rather than
 *    being smuggled into provenance.
 * 4. **An unknown denominator stays unknown.** A truncated tree sets `extent.total` to `null`. It
 *    is the single easiest rule to break here, because the truncated response still has a
 *    plausible-looking length.
 * 5. **Declare the schema.** `scema.world/1`.
 *
 * The whole module is pure of anything but its `fetch`: it takes one, so a test can drive it with
 * canned responses and no network.
 */

/** The contract version. Anything without one is refused on import — see rule 5. */
export const SCHEMA = 'scema.world/1'

/** How many tree entries to walk before giving up and admitting the denominator is unknown. */
export const TREE_CAP = 20_000

export interface RepoRef {
  owner: string
  repo: string
}

/**
 * Parse anything a person might paste.
 *
 * Deliberately generous about the *shape* and strict about the result: `owner/repo`, a web URL, a
 * clone URL with `.git`, a URL with a branch path after it. What it will not do is guess — a
 * string that does not contain two path segments returns `null` and the page says it could not
 * read a repository out of it, rather than fetching something and reporting a 404 as though the
 * repository were missing.
 */
export function parseRepo(input: string): RepoRef | null {
  const s = input.trim()
  if (!s) return null
  // A bare owner/repo, which is what most people type.
  const bare = /^([A-Za-z0-9._-]+)\/([A-Za-z0-9._-]+?)(?:\.git)?$/.exec(s)
  if (bare) return { owner: bare[1], repo: bare[2] }
  try {
    const u = new URL(s.includes('://') ? s : `https://${s}`)
    if (!/(^|\.)github\.com$/.test(u.hostname)) return null
    const parts = u.pathname.split('/').filter(Boolean)
    if (parts.length < 2) return null
    return { owner: parts[0], repo: parts[1].replace(/\.git$/, '') }
  } catch {
    return null
  }
}

/** A tagged scalar, exactly as the contract writes them. */
type Scalar = { t: 'int' | 'num' | 'text' | 'bool'; v: number | string | boolean }
const int = (v: number): Scalar => ({ t: 'int', v: Math.round(v) })
const text = (v: string): Scalar => ({ t: 'text', v })
const bool = (v: boolean): Scalar => ({ t: 'bool', v })

export interface WorldObject {
  id: string
  kind: string
  label: string
  attrs?: Record<string, Scalar>
  provenance: { kind: 'live' | 'stale' | 'absent' | 'simulated'; age_secs?: number; budget_secs?: number }
}

export interface WorldSignal {
  id: string
  polarity: 'risk' | 'opportunity'
  label: string
  detail?: string
  magnitude: number
  measured: boolean
  targets?: string[]
  evidence?: string[]
}

export interface WorldState {
  schema: string
  observer: string
  entity: { kind: string; locator: string; label?: string }
  domain: string
  observed_at: number
  objects: WorldObject[]
  facts: { subject: string; predicate: string; object: string; confidence: number }[]
  signals: WorldSignal[]
  extent: { observed: number; total: number | null; note?: string }
  blind_spots: string[]
}

export type Observation =
  | { ok: true; world: WorldState }
  | { ok: false; reason: 'bad_input' | 'not_found' | 'rate_limited' | 'forbidden' | 'network'; detail: string }

/** Clamp a count onto the contract's 0..1 magnitude without inventing a scale. */
function magnitudeOf(count: number, full: number): number {
  if (count <= 0) return 0
  return Math.max(0, Math.min(1, count / full))
}

/**
 * Perceive a repository.
 *
 * `now` is passed in rather than read, so the same repository observed twice in a test produces
 * the same bytes — a world whose commitment moved because a clock ticked would make the plate a
 * different artefact on every render, which is the failure `check:omni` asserts the *absence* of
 * ("no clock: a minted-at field would make every regeneration a different token").
 */
export async function observeRepo(
  ref: RepoRef,
  doFetch: typeof fetch,
  now: number,
): Promise<Observation> {
  const base = 'https://api.github.com'
  const headers = { Accept: 'application/vnd.github+json' }
  const blind: string[] = []

  const get = async (path: string): Promise<{ ok: true; body: any } | { ok: false; status: number }> => {
    try {
      const r = await doFetch(`${base}${path}`, { headers })
      if (!r.ok) return { ok: false, status: r.status }
      return { ok: true, body: await r.json() }
    } catch {
      // A thrown fetch is a network failure, which is genuinely different from a refusal — one is
      // about this machine and the other is about the repository.
      return { ok: false, status: 0 }
    }
  }

  const repoRes = await get(`/repos/${ref.owner}/${ref.repo}`)
  if (!repoRes.ok) {
    // Each of these sends the reader somewhere different, which is why they are not one message.
    // "You are rate limited", "that repository is private or does not exist" and "your network
    // refused" are three instructions and only one of them means try a different repository.
    if (repoRes.status === 404) {
      return { ok: false, reason: 'not_found', detail: 'no such repository, or it is private' }
    }
    if (repoRes.status === 403 || repoRes.status === 429) {
      return {
        ok: false,
        reason: 'rate_limited',
        detail:
          'GitHub is rate-limiting this browser. Unauthenticated reads are capped at sixty an ' +
          'hour, per address — the limit resets on the hour.',
      }
    }
    if (repoRes.status === 0) {
      return { ok: false, reason: 'network', detail: 'the request to api.github.com did not complete' }
    }
    return { ok: false, reason: 'forbidden', detail: `GitHub answered ${repoRes.status}` }
  }
  const repo = repoRes.body

  // ── the tree ───────────────────────────────────────────────────────────────
  //
  // The one read that can silently lie. `git/trees?recursive=1` returns `truncated: true` when the
  // response was cut, and the array it returns is still a plausible length — so taking
  // `tree.length` as the file count is a claim of completeness the response explicitly denies.
  const branch = typeof repo.default_branch === 'string' ? repo.default_branch : 'HEAD'
  const treeRes = await get(`/repos/${ref.owner}/${ref.repo}/git/trees/${branch}?recursive=1`)
  let files = 0
  let dirs = 0
  let truncated = false
  let treeRead = false
  if (treeRes.ok && Array.isArray(treeRes.body?.tree)) {
    treeRead = true
    truncated = treeRes.body.truncated === true
    for (const e of treeRes.body.tree.slice(0, TREE_CAP)) {
      if (e?.type === 'blob') files += 1
      else if (e?.type === 'tree') dirs += 1
    }
    if (treeRes.body.tree.length > TREE_CAP) truncated = true
  } else {
    blind.push(
      `${branch}: the file tree could not be read — unread, not empty`,
    )
  }
  if (truncated) {
    blind.push('the file tree was truncated by GitHub — the total is unknown, not the count above')
  }

  // ── languages and contributors ─────────────────────────────────────────────
  const langRes = await get(`/repos/${ref.owner}/${ref.repo}/languages`)
  const languages = langRes.ok && langRes.body ? Object.keys(langRes.body) : null
  if (!languages) blind.push('languages: unread')

  // `per_page=1` and read the pagination header would be the cheap way to a *total*; without it
  // this is a page, and a page is not a population — so what is reported is what was counted.
  const contribRes = await get(`/repos/${ref.owner}/${ref.repo}/contributors?per_page=100`)
  const contributors = contribRes.ok && Array.isArray(contribRes.body) ? contribRes.body.length : null
  if (contributors === null) blind.push('contributors: unread')

  // ── objects ────────────────────────────────────────────────────────────────
  const objects: WorldObject[] = []
  objects.push({
    id: 'repo',
    kind: 'repository',
    label: repo.full_name ?? `${ref.owner}/${ref.repo}`,
    attrs: {
      default_branch: text(branch),
      archived: bool(repo.archived === true),
      ...(typeof repo.size === 'number' ? { size_kb: int(repo.size) } : {}),
      ...(typeof repo.pushed_at === 'string' ? { pushed_at: text(repo.pushed_at) } : {}),
    },
    provenance: { kind: 'live', age_secs: 0 },
  })

  if (treeRead) {
    objects.push({
      id: 'tree',
      kind: 'directory',
      label: `${files} file(s) in ${dirs} director${dirs === 1 ? 'y' : 'ies'}`,
      attrs: { files: int(files), directories: int(dirs), truncated: bool(truncated) },
      provenance: { kind: 'live', age_secs: 0 },
    })
  } else {
    // **Carries no attributes at all.** An `Absent` object with numbers on it is the exact thing
    // rule 1 forbids: it would let a downstream reader treat "we could not look" as "we looked and
    // found nothing".
    objects.push({ id: 'tree', kind: 'directory', label: 'file tree', provenance: { kind: 'absent' } })
  }

  for (const l of languages ?? []) {
    objects.push({
      id: `language.${l.toLowerCase()}`,
      kind: 'component',
      label: l,
      provenance: { kind: 'live', age_secs: 0 },
    })
  }

  // ── signals: counts, and only counts ───────────────────────────────────────
  //
  // Every one of these is a number GitHub returned. There is deliberately no "activity score", no
  // "maintenance rating" and no aggregate of any kind: rule 2 exists because an invented figure is
  // indistinguishable downstream from a measured one, and this is the producer where inventing one
  // would be easiest and most tempting.
  const signals: WorldSignal[] = []

  if (typeof repo.open_issues_count === 'number') {
    signals.push({
      id: 'open-issues',
      polarity: 'risk',
      label: `${repo.open_issues_count} open issue(s) and pull request(s)`,
      detail: 'GitHub reports issues and pull requests under one counter.',
      magnitude: magnitudeOf(repo.open_issues_count, 400),
      measured: true,
      targets: ['repo'],
      evidence: [`open_issues_count = ${repo.open_issues_count}`],
    })
  }
  if (treeRead) {
    signals.push({
      id: 'source-files',
      polarity: 'opportunity',
      label: `${files} file(s) to work with`,
      magnitude: magnitudeOf(files, 4_000),
      measured: true,
      targets: ['tree'],
      evidence: [
        truncated
          ? `counted ${files} blob(s) before GitHub truncated the tree`
          : `counted ${files} blob(s) in the ${branch} tree`,
      ],
    })
  }
  if (contributors !== null) {
    signals.push({
      id: 'contributors',
      polarity: 'opportunity',
      label: `${contributors} contributor(s) on the first page`,
      detail: 'A page, not a population — GitHub was not asked for a total.',
      magnitude: magnitudeOf(contributors, 100),
      measured: true,
      targets: ['repo'],
      evidence: [`counted ${contributors} entries in one page of /contributors`],
    })
  }
  if (typeof repo.forks_count === 'number' && repo.forks_count > 0) {
    signals.push({
      id: 'forks',
      polarity: 'opportunity',
      label: `${repo.forks_count} fork(s)`,
      magnitude: magnitudeOf(repo.forks_count, 2_000),
      measured: true,
      targets: ['repo'],
      evidence: [`forks_count = ${repo.forks_count}`],
    })
  }
  if (repo.archived === true) {
    signals.push({
      id: 'archived',
      polarity: 'risk',
      label: 'the repository is archived',
      magnitude: 1,
      measured: true,
      targets: ['repo'],
      evidence: ['archived = true'],
    })
  }

  // ── facts ──────────────────────────────────────────────────────────────────
  const facts = (languages ?? []).map((l) => ({
    subject: 'repo',
    predicate: 'written-in',
    object: `language.${l.toLowerCase()}`,
    confidence: 1,
  }))

  return {
    ok: true,
    world: {
      schema: SCHEMA,
      // `github:` rather than a bare name, so a reader can see at a glance whose word this is —
      // the same reasoning as `ImportObserver` rewriting an imported world's observer, and the
      // daemon rewriting a wire-supplied one to `client:<name>`.
      observer: `github:${ref.owner}/${ref.repo}`,
      entity: {
        kind: 'repository',
        locator: `https://github.com/${ref.owner}/${ref.repo}`,
        label: repo.full_name ?? `${ref.owner}/${ref.repo}`,
      },
      domain: 'software',
      observed_at: Math.floor(now / 1000),
      objects,
      facts,
      signals,
      extent: {
        observed: objects.length,
        // **Null whenever the tree was capped or unread.** The single easiest rule to break here:
        // a truncated response still has a plausible length, and reporting it as the total claims
        // a completeness GitHub explicitly denied.
        total: treeRead && !truncated ? objects.length : null,
        note: treeRead
          ? truncated
            ? 'the file tree was truncated; the population is unknown'
            : `walked the ${branch} tree`
          : 'the file tree could not be read',
      },
      blind_spots: blind,
    },
  }
}
