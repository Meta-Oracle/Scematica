// Scylar's project codex — what she knows about Scematica itself.
//
// She could always read the *running* bot (`tools.ts` → the state API) and reason about a
// source tree (`omni.ts` → the daemon). Neither of those tells her what the repository
// *is*: which crates exist, which one owns a rule, why a pin is a pin, what `/mesh` is for,
// or that Scematica Omni is a separate workspace on purpose. Asked "what is scema-verify",
// a model with no codex writes a fluent paragraph out of the name. That is the exact
// failure this repository spends its type system preventing, arriving through the one door
// with no types on it.
//
// So the codex is **hand-authored from the repository, and checkable against it**. Every
// entry names a real path, `scripts/check-scylar.mjs` asserts each one exists on disk, and
// an entry whose path was deleted fails the check rather than quietly becoming folklore.
// That is the whole difference between a knowledge base and a very confident guess.
//
// ## What it is not
//
// It is not a code index and it is not searchable prose. Each entry is deliberately short:
// what the thing is, the invariants that are easy to break, the commands that drive it, and
// the ids of its neighbours. A model given this plus one tool call can explain any part of
// the stack; a model given the whole of CLAUDE.md per turn would spend the entire free-tier
// token budget on context before reading the question.
//
// Safe to import in a browser — pure data, no credentials, no `process.env`.

export type CodexKind =
  | 'crate' // a cargo crate in the bot workspace
  | 'omni' // a crate in the Scematica Omni workspace
  | 'workspace' // a separate cargo workspace, excluded from the root one
  | 'product' // a surface on the web dashboard
  | 'program' // an on-chain Anchor program
  | 'tool' // a helper binary
  | 'subsystem' // an architectural pattern that spans crates
  | 'contract' // an agreement between components, e.g. a wire format

export interface CodexEntry {
  /** Stable kebab-case id. Referenced by `related` and by the `explain_project` tool. */
  id: string
  name: string
  kind: CodexKind
  /**
   * Repository-relative path, or `null` for a cross-cutting pattern that lives nowhere in
   * particular. A non-null path MUST exist — `check:scylar` asserts it, so a rename that
   * misses this file breaks the check instead of teaching her a stale location.
   */
  path: string | null
  /** One to three sentences. What it is and why it exists. */
  summary: string
  /** The rules that are easy to break, phrased as rules. Empty is allowed; padding is not. */
  invariants: string[]
  /** Commands that actually drive it, copied from the repository's own docs. */
  commands: string[]
  /** Ids of neighbouring entries. Every id must resolve — `check:scylar` asserts it. */
  related: string[]
  /** Extra search terms that do not appear in the name or summary. */
  keywords: string[]
}

// ── the entries ────────────────────────────────────────────────────────────────
//
// Ordered by subsystem rather than alphabetically, because this order is also the order
// `codexMap()` presents them in, and an operator scanning that list is looking for an area,
// not a letter.

export const CODEX: CodexEntry[] = [
  // ── the whole thing ──────────────────────────────────────────────────────────
  {
    id: 'scematica',
    name: 'Scematica',
    kind: 'subsystem',
    path: null,
    summary:
      'The whole system: a Rust Solana sniper and cross-DEX arbitrage bot with a ratatui ' +
      'TUI, a pure-Rust Deep Q* agent, an x402 payment server, an agentic-liquidity SDK, a ' +
      'Next.js dashboard hosting seven products, a Python oracle toolkit, and Scematica ' +
      'Omni — a domain-agnostic agent runtime producing verifiable decision records. Gated ' +
      'behind a 250k $SCEMA balance.',
    invariants: [
      'Nothing fabricates. Every surface either reads a real source or reports that it could not.',
      'A simulated figure is labelled at the transport, not only in the prose that quotes it.',
    ],
    commands: ['cargo build --release', 'cargo test --workspace'],
    related: ['sniper-pipeline', 'scematica-omni', 'web', 'file-ipc'],
    keywords: ['overview', 'stack', 'architecture', 'everything', 'project'],
  },

  // ── bot crates ───────────────────────────────────────────────────────────────
  {
    id: 'scematica-core',
    name: 'scematica-core',
    kind: 'crate',
    path: 'crates/scematica-core',
    summary:
      'Shared foundation: config loading, RPC clients, wallet handling, metrics, common ' +
      'types and token utilities. Everything in the trading path depends on it, which is ' +
      'exactly why Scematica Omni may not.',
    invariants: [
      'SniperConfig and FilterConfig use #[serde(default)] — a new field needs a Default impl or every existing config.toml stops loading.',
      'SCEMA is Token-2022; gate code must use Token-2022 helpers, never legacy SPL.',
    ],
    commands: ['cargo test -p scematica-core'],
    related: ['token-gate', 'dependency-pins', 'scematica-sniper'],
    keywords: ['config', 'rpc', 'wallet', 'metrics', 'types'],
  },
  {
    id: 'scematica-sniper',
    name: 'scematica-sniper',
    kind: 'crate',
    path: 'crates/scematica-sniper',
    summary:
      'The sniper: pool listener, filter pipeline, buy/sell orchestration, alerts and the ' +
      'backtester. Targets Raydium AMM V4 new-pool events on mainnet. Binaries: `sniper`, ' +
      '`backtest`.',
    invariants: [
      'RPC-bound filters fail OPEN on timeout so one slow node cannot stall the queue — which is precisely why the coherence breaker exists.',
      'Every new filter must register a name with FilterStats or it is invisible to the dashboard.',
      'The sell monitor re-reads live_params each iteration; TP/SL are hot-reloadable and must stay so.',
    ],
    commands: [
      'cargo run --release --bin sniper',
      'cargo test -p scematica-sniper kelly',
      'cargo run --release --bin backtest -- --pools historical-pools.jsonl --tp 100 --sl 15',
    ],
    related: ['sniper-pipeline', 'risk-subsystems', 'coherence-breaker', 'scematica-nn', 'file-ipc'],
    keywords: ['filters', 'raydium', 'buy', 'sell', 'pool', 'listener', 'backtest'],
  },
  {
    id: 'scematica-arb',
    name: 'scematica-arb',
    kind: 'crate',
    path: 'crates/scematica-arb',
    summary:
      'Cross-DEX arbitrage: builds a graph over Raydium, Orca and Meteora pools and searches ' +
      'it for profitable cycles. Binary: `arb`.',
    invariants: [
      'The pool graph must be seeded first — an empty pools/ directory is an empty graph and therefore zero trades, which looks like a broken bot.',
      'Program-less by default: atomicity plus a final-hop min_out replaces the on-chain profit-or-revert, so no deploy is required.',
    ],
    commands: ['cargo run --release -p pool-seeder', 'cargo run --release --bin arb'],
    related: ['pool-seeder', 'scematica-executor', 'scematica-swap'],
    keywords: ['arbitrage', 'orca', 'meteora', 'graph', 'cycle', 'program-less'],
  },
  {
    id: 'scematica-executor',
    name: 'scematica-executor',
    kind: 'crate',
    path: 'crates/scematica-executor',
    summary:
      'Swap instruction builders for several DEXes plus the Jupiter integration. Owns the ' +
      'WSOL ATA lifecycle and dynamic priority-fee escalation.',
    invariants: [
      'Raydium slippage failure is 0x1e, not 0x26 — a misread there sends you tuning the wrong knob.',
      'confirm_transaction cannot see a revert; a confirmed signature is not a successful swap.',
    ],
    commands: [],
    related: ['scematica-arb', 'scematica-sniper'],
    keywords: ['swap', 'jupiter', 'wsol', 'ata', 'slippage', 'priority fee'],
  },
  {
    id: 'scematica-ai',
    name: 'scematica-ai',
    kind: 'crate',
    path: 'crates/scematica-ai',
    summary:
      'LLM agents over Groq and xAI: Chat, Strategy, Risk, Debate and Report. The Strategy ' +
      'agent writes scematica-strategy.json, which the sniper reads as live parameters.',
    invariants: [
      'The strategy agent writes a file; the sniper reads it. There is no direct call between them, and that is the pattern.',
    ],
    commands: [],
    related: ['file-ipc', 'scylar', 'scematica-sniper'],
    keywords: ['llm', 'groq', 'xai', 'strategy', 'risk', 'debate'],
  },
  {
    id: 'scematica-nn',
    name: 'scematica-nn (Deep Q*)',
    kind: 'crate',
    path: 'crates/scematica-nn',
    summary:
      'A pure-Rust Dueling Double-DQN with no ML framework dependency, running inside the ' +
      'sniper process. 24-feature state, five actions, prioritized replay, n-step returns, ' +
      'per-regime net pairs and a three-variant tournament. Optional QR-DQN, a Dreamer-style ' +
      'latent world model and an adversarial pool gym.',
    invariants: [
      'It only advises once train_steps >= 10_000 AND last_q_values carry signal.',
      'A veto fully suppresses a buy only when the bearish Q beats the best buy Q by >=15%; a weaker lean downgrades sizing instead of silently killing the edge.',
      'Value dispersion is not action dispersion — read last_q_values before touching any threshold.',
    ],
    commands: ['cargo test -p scematica-nn agent::tests::', 'cargo install scematica-nn && scema-ddqn'],
    related: ['scematica-sniper', 'file-ipc', 'omni-policy'],
    keywords: ['dqn', 'reinforcement', 'q-learning', 'replay', 'tournament', 'veto', 'dq*'],
  },
  {
    id: 'scematica-dashboard',
    name: 'scematica-dashboard',
    kind: 'crate',
    path: 'crates/scematica-dashboard',
    summary:
      'The ratatui TUI, six tabs, black and red. Launches the sniper as a child process and ' +
      'observes it exclusively through the state files.',
    invariants: [
      'It prefers target/release/sniper.exe over the debug build — rebuild release after changing sniper code or you are watching an old binary.',
    ],
    commands: ['cargo run --release --bin dashboard', 'cargo run --release --bin dashboard -- --demo'],
    related: ['file-ipc', 'scematica-sniper', 'mesh-dashboard'],
    keywords: ['tui', 'ratatui', 'terminal', 'dashboard', 'tabs'],
  },
  {
    id: 'scematica-api',
    name: 'scematica-api',
    kind: 'crate',
    path: 'crates/scematica-api',
    summary:
      'The HTTP API the web dashboard reads. Serves the state files, the mesh topology and ' +
      'the Ψ gate. Binary: `api`.',
    invariants: [
      'It serves both /health and /api/health, which is why a wrongly-rooted base URL used to pass the pairing probe. The probe must use /api/health, which has no alias.',
      'On Windows, rebuilding api.exe while it runs fails with "Access is denied (os error 5)" — cargo reports that as a build error, and it is not one.',
    ],
    commands: ['cargo run --release --bin api'],
    related: ['web', 'mesh', 'sentience-gate'],
    keywords: ['http', 'api', 'endpoints', 'rust api', 'pairing'],
  },
  {
    id: 'scematica-protocol',
    name: 'scematica-protocol (x402)',
    kind: 'crate',
    path: 'crates/scematica-protocol',
    summary:
      'A Rust-native x402 HTTP 402 payment server — the facilitator side of pay-per-request ' +
      'access. Binary: `scematica-protocol`.',
    invariants: [],
    commands: [
      'cargo run --release --bin scematica-protocol -- --pay-to <wallet> --price-lamports 10000',
    ],
    related: ['scemadex-sdk', 'scemadex-relay'],
    keywords: ['x402', '402', 'payment', 'facilitator', 'paywall'],
  },
  {
    id: 'scematica-sentience',
    name: 'scematica-sentience',
    kind: 'crate',
    path: 'crates/scematica-sentience',
    summary:
      'The Singularity Cognitive Architecture as a computable library: the Ψ and Ω master ' +
      'equations, ethics gating, a knowledge graph, meta-cognition, and an LLM overlay that ' +
      "gates a model's output on integrated cognition (GO / CAUTION / HOLD). Library only — " +
      'no binary. This is the crate Scylar is named against.',
    invariants: [
      "Perception's data ratio is a PRODUCT, so an unmeasured channel scored 0 pins Ψ at 0 and jams the gate shut forever. Unmeasured dimensions take 1.0 — \"not a limiting factor\".",
      'Ψ is a pure function of measured data integrity. A run of coherent answers must never be able to talk the gate into trusting stale numbers.',
      'Nothing in the runtime path depends on it yet; gating live LLM calls on Ψ is a separate wiring step, and /api/sentience is where it actually happens.',
    ],
    commands: ['cargo test -p scematica-sentience'],
    related: ['sentience-gate', 'scylar', 'mesh', 'coherence-breaker'],
    keywords: ['psi', 'omega', 'sentience', 'cognition', 'gate', 'ethics', 'overlay'],
  },
  {
    id: 'scematica-mesh',
    name: 'scematica-mesh',
    kind: 'crate',
    path: 'crates/scematica-mesh',
    summary:
      "The running system's own topology, collected from the state files and served as a " +
      'graph of decision-making units. Read-only: it writes nothing and takes no locks, so ' +
      'it is safe against a live bot. Also implements the agentic gate Ψ = C·K·(1−R) over ' +
      'the observed mesh, and omni.rs emits the topology as a Scematica Omni WorldState.',
    invariants: [
      'Every term carries measured: bool, and an unmeasured dimension contributes the NEUTRAL element, never 0.',
      'Ω stays None until one of its five subsystems exists.',
      'agent.omni is the only node with no edges at all — nothing in omni writes to what it observes, and drawing a wire would assert coordination that is not happening.',
      'This Ψ asks "do the subsystems agree". The sentience Ψ asks "can this data be trusted". They are different questions.',
    ],
    commands: [
      'cargo run --release -p mesh-dashboard',
      'cargo run --release -p mesh-dashboard -- --world',
    ],
    related: ['mesh-dashboard', 'mesh', 'scematica-sentience', 'world-contract'],
    keywords: ['topology', 'graph', 'nodes', 'edges', 'coherence', 'psi', 'agentic'],
  },
  {
    id: 'mesh-dashboard',
    name: 'mesh-dashboard',
    kind: 'crate',
    path: 'crates/mesh-dashboard',
    summary:
      'A ratatui TUI over scematica-mesh — the topology as a live terminal graph, indigo and ' +
      'slate. A separate crate so the library stays lean and read-only.',
    invariants: [],
    commands: [
      'cargo run --release -p mesh-dashboard -- --once',
      'cargo run --release -p mesh-dashboard -- --json',
    ],
    related: ['scematica-mesh', 'world-contract'],
    keywords: ['tui', 'mesh', 'graph', 'terminal'],
  },
  {
    id: 'scemadex-sdk',
    name: 'scemadex-sdk',
    kind: 'crate',
    path: 'crates/scemadex-sdk',
    summary:
      'The published agentic-liquidity SDK: intents, Conviction-Routing performance bonds, ' +
      'and an inference/experience mesh. Carries no solana-sdk by default, which is what ' +
      'keeps it usable outside this workspace.',
    invariants: [
      'A performance bond has an adjudicating authority by design. That is correct here and exactly wrong for the escrow vault.',
    ],
    commands: ['cargo install scemadex-sdk && scemadex', 'cargo run --release --bin sdk-dashboard'],
    related: ['scemadex-relay', 'scemadex-settle', 'sdk-dashboard', 'scematica-escrow'],
    keywords: ['sdk', 'liquidity', 'bond', 'conviction routing', 'intent', 'agentic'],
  },
  {
    id: 'scemadex-relay',
    name: 'scemadex-relay',
    kind: 'crate',
    path: 'crates/scemadex-relay',
    summary: 'The peer-mesh and signal-oracle HTTP server for the ScemaDEX rail.',
    invariants: [],
    commands: ['cargo run --release --bin scemadex-relay'],
    related: ['scemadex-sdk', 'scemadex-mcp'],
    keywords: ['relay', 'peer', 'oracle', 'signal', 'server'],
  },
  {
    id: 'scemadex-mcp',
    name: 'scemadex-mcp',
    kind: 'crate',
    path: 'crates/scemadex-mcp',
    summary:
      'An MCP server bridging LLM agents to the ScemaDEX rail over the relay. Distinct from ' +
      'scema-mcp, which serves the Omni loop.',
    invariants: [],
    commands: [],
    related: ['scemadex-relay', 'scema-mcp'],
    keywords: ['mcp', 'model context protocol', 'bridge', 'agents'],
  },
  {
    id: 'scemadex-settle',
    name: 'scemadex-settle',
    kind: 'crate',
    path: 'crates/scemadex-settle',
    summary:
      'An open devnet reference settler: moves devnet USDC when a Conviction-Routing bond is ' +
      'slashed. Reference implementation, deliberately public.',
    invariants: [],
    commands: [],
    related: ['scemadex-sdk', 'scematica-escrow'],
    keywords: ['settlement', 'devnet', 'usdc', 'slash', 'dispute'],
  },
  {
    id: 'sdk-dashboard',
    name: 'sdk-dashboard',
    kind: 'crate',
    path: 'crates/sdk-dashboard',
    summary:
      'A ratatui TUI over the ScemaDEX bond pipeline, green. Runs simulated by default and ' +
      'against real Jupiter quotes with --live.',
    invariants: [],
    commands: [
      'cargo run --release --bin sdk-dashboard',
      'cargo run --release --bin sdk-dashboard -- --live',
    ],
    related: ['scemadex-sdk'],
    keywords: ['tui', 'bonds', 'pipeline', 'jupiter'],
  },
  {
    id: 'scematica-suite',
    name: 'scematica-suite',
    kind: 'crate',
    path: 'crates/scematica-suite',
    summary:
      'The umbrella meta-crate: re-exports every component and installs a `scematica` ' +
      'launcher that dispatches to the component binaries.',
    invariants: [
      'It re-exports scematica-sentience as `sentience`, which is library-only — so the launcher has no `sentience` subcommand.',
    ],
    commands: ['cargo install scematica-suite', 'scematica dashboard --demo'],
    related: ['scematica-dashboard', 'scematica-sentience'],
    keywords: ['umbrella', 'launcher', 'install', 'crates.io'],
  },
  {
    id: 'agent-playground',
    name: 'agent-playground',
    kind: 'crate',
    path: 'agent-playground',
    summary:
      'The ScemaDEX agent playground, published as `scema-agent-playground`. Binary: ' +
      '`playground`.',
    invariants: [],
    commands: [],
    related: ['scemadex-sdk'],
    keywords: ['playground', 'experiment', 'agents'],
  },

  // ── Scematica Omni ───────────────────────────────────────────────────────────
  {
    id: 'scematica-omni',
    name: 'Scematica Omni',
    kind: 'workspace',
    path: 'scematica-omni',
    summary:
      'The agent runtime, in its own cargo workspace. The loop is observe → hypothesise → ' +
      'simulate → score → decide → record → remember, every stage a trait, the whole pass ' +
      'deterministic — which is the precondition for a decision record being verifiable by ' +
      "somebody who was not there. Published as the `scema-*` crates, independent of the " +
      "bot's `scematica-*` line.",
    invariants: [
      'Every layer can say "I don\'t know", and saying it costs nothing. An agent that cannot express ignorance expresses a number of the right shape instead.',
      'It is domain-agnostic by design: nothing in it may depend on scematica-core or anything downstream, or the trading domain becomes structurally privileged.',
      'Its own workspace because it wants a modern HTTP/TLS stack — exactly what the solana-sdk pins forbid.',
      'Nothing in the workspace writes to the environment it observes. execute, delegate, discover and pay are registered verbs that exit 2 and say what is missing.',
    ],
    commands: [
      'cd scematica-omni ; cargo test --workspace',
      './scematica-omni/target/release/scema quickstart .',
      'scema observe . ; scema simulate "<goal>" --ground <signal-id>',
    ],
    related: [
      'omni-loop',
      'omni-utility',
      'omni-abstention',
      'omni-verify',
      'world-contract',
      'dependency-pins',
    ],
    keywords: ['omni', 'agent', 'runtime', 'loop', 'decision record', 'scema'],
  },
  {
    id: 'omni-loop',
    name: 'The Omni loop',
    kind: 'contract',
    path: 'scematica-omni/crates/scema-agent',
    summary:
      'observe → hypothesise → simulate → score → decide → record → remember. Each stage is ' +
      'a trait with a real implementation; the pass is deterministic so the record can be ' +
      're-derived by a third party.',
    invariants: [
      'Provenance before value, Term before score: an unmeasured dimension takes the neutral element and is flagged measured: false.',
      'A projection may not invent a number. On a barely-perceived world most branches project exactly zero and the agent abstains — that is correct, not a bug.',
      'An instruction is not evidence. Grounding comes only from --ground, never from keyword overlap between the goal and a signal id.',
    ],
    commands: [
      'scema simulate "<goal>" --ground <signal-id>',
      'scema decide "<goal>" --ground <signal-id>',
    ],
    related: ['scematica-omni', 'omni-utility', 'omni-abstention', 'omni-observers'],
    keywords: ['observe', 'hypothesise', 'simulate', 'score', 'decide', 'remember', 'ground'],
  },
  {
    id: 'omni-utility',
    name: 'The utility equation',
    kind: 'contract',
    path: 'scematica-omni/crates/scema-policy',
    summary:
      'U = R − λ₁K − λ₂C − λ₃U + λ₄V. Additive, deliberately. The λ weights are a stated ' +
      'preference, never a fitted parameter, and they are hashed into every record.',
    invariants: [
      'Additive because a multiplicative form is the trap this repository has paid for twice — the sentience Ψ pinned at 0, and the agentic gate pinned shut on unbuilt subsystems.',
      'scema_policy::render is the ONLY place in Rust a Term becomes a string. An unmeasured term prints "—", never 0.00. A measured zero prints 0.00, because that is a real observation.',
      'Specialist scores are attached, never averaged into the ranking — a utility and a normalised Q are not the same quantity.',
    ],
    commands: ['scema policy'],
    related: ['omni-loop', 'omni-policy', 'omni-abstention'],
    keywords: ['utility', 'lambda', 'weights', 'coverage', 'term', 'render', 'em dash'],
  },
  {
    id: 'omni-policy',
    name: 'Specialists and applicability',
    kind: 'contract',
    path: 'scematica-omni/crates/scema-policy',
    summary:
      'Pluggable evaluators, each of which may decline. scematica-nn is wired in as ONE ' +
      'specialist and declines on every non-trading world.',
    invariants: [
      'Applicability has two distinguishable refusals: OutOfDomain (permanent, fine) and Insufficient (my domain, missing inputs — go and supply them).',
      "A qualified specialist's MEASURED negative vetoes outright. An UNMEASURED one is silence and carries no veto.",
      'The DQ* evaluator refuses a partial or stale TradeState rather than defaulting a missing feature to 0.0, which the net would read as a real observation of an empty pool.',
    ],
    commands: ['scema policy'],
    related: ['omni-utility', 'scematica-nn'],
    keywords: ['specialist', 'evaluator', 'applicability', 'decline', 'veto', 'dqstar'],
  },
  {
    id: 'omni-abstention',
    name: 'Abstention',
    kind: 'contract',
    path: 'scematica-omni/crates/scema-policy',
    summary:
      'Refusing to decide is a first-class outcome with five distinct reasons: NoCandidates, ' +
      'AllForbidden, NoPositiveUtility, TooLittleMeasured, Contested. Each is a different ' +
      'instruction to the operator.',
    invariants: [
      '`scema decide` exits 0 when it abstains. A script that treats "the agent declined" as a crash gets rewritten to ignore the exit code, and then it ignores real crashes too.',
      'Unresolved counterfactuals are counted, never scored. Calibration::mean_abs_error is None, not 0.0, when nothing resolved.',
      '"It could not decide" throws away the actionable part. Say which of the five.',
    ],
    commands: ['scema simulate "<goal>"'],
    related: ['omni-loop', 'omni-utility', 'calibration'],
    keywords: ['abstain', 'decline', 'no candidates', 'contested', 'too little measured'],
  },
  {
    id: 'omni-verify',
    name: 'Decision records and verification',
    kind: 'contract',
    path: 'scematica-omni/crates/scema-verify',
    summary:
      'A sealed record carries the goal, the world state whole, the ranking, the λ weights ' +
      'and a SHA-256 commitment root. `scema verify` recomputes it and names the field that ' +
      'moved.',
    invariants: [
      'Verification proves the record was not edited after sealing. It does NOT prove the world was as described, and it does NOT prove this is the original record — tamper-evident, not tamper-proof.',
      'Floats are hashed as round(v * 1e9) in i64, binding values to 1e-9. Hashing raw IEEE-754 bits made an honest JSON round trip report INVALID, and a verifier that cries tamper on untouched history is worse than none.',
      'Canonical encoding is stricter than JSON: sorted keys, tagged types, normalised -0.0 and NaN.',
      'The schema version field is Option + skip_serializing_if, so records sealed before it existed keep verifying.',
    ],
    commands: ['scema verify --all', 'scema verify --file <path>', 'scema explain --list'],
    related: ['omni-record-console', 'scematica-omni', 'world-contract'],
    keywords: ['verify', 'commitment', 'sha-256', 'canonical', 'tamper', 'record', 'seal'],
  },
  {
    id: 'omni-observers',
    name: 'Observers and blind spots',
    kind: 'contract',
    path: 'scematica-omni/crates/scema-tools',
    summary:
      'Perception plus Workspace confinement. RepoObserver walks a source tree; ' +
      'ImportObserver reads a WorldState a producer emitted about itself.',
    invariants: [
      'Report what could not be read as blind_spots. Never round an unread thing to zero.',
      'A deliberate exclusion is NOT a blind spot — skipping target/ is a decision, and filing it as ignorance buries the paths that really could not be read.',
      'State whether the walk was complete: Extent { total: None } when a cap was hit, never a numerator over a smaller total.',
      'Workspace answers *where* only. Whether is the goal\'s constraints and an approval policy; merging the two makes a grant for one a grant for the other.',
    ],
    commands: ['scema observe .', 'scema check world.json'],
    related: ['world-contract', 'omni-loop', 'omni-producers'],
    keywords: ['observer', 'perception', 'blind spot', 'workspace', 'extent', 'repo'],
  },
  {
    id: 'world-contract',
    name: 'The world contract',
    kind: 'contract',
    path: 'scematica-omni/crates/scema-world',
    summary:
      'WorldState: the versioned JSON shape every producer emits and every observer reads. ' +
      'schema: "scema.world/1"; an undeclared version is refused on import. Domain and ' +
      'EntityKind are OPEN enums — known arms plus Other(String), held verbatim so a record ' +
      'round-trips byte for byte.',
    invariants: [
      'Closing the vocabularies was the largest limit on universality: a perceived web page and a set of Chainlink feeds both reported "unknown", so two entirely different worlds were indistinguishable to every specialist.',
      'Parsing normalises case and padding and deliberately does NOT guess synonyms — k8s is not kubernetes.',
      'An unfamiliar name is a WARNING, never a failure. Failing on one pushes producers back onto "unknown".',
      'scema_tools::conform is the single implementation of "is this a usable world", shared by ImportObserver and `scema check`, and reports every finding at once with a stable code.',
    ],
    commands: ['scema check world.json', 'scema check --vocabulary'],
    related: ['omni-producers', 'omni-observers', 'scematica-omni'],
    keywords: ['worldstate', 'schema', 'domain', 'entitykind', 'vocabulary', 'conform', 'producer'],
  },
  {
    id: 'omni-producers',
    name: 'The four producers',
    kind: 'contract',
    path: 'scematica-omni/crates/scema-tools/fixtures',
    summary:
      'Four things describe themselves as a WorldState and one loop reads all four: ' +
      'RepoObserver (a source tree, Rust in-process), plugins/scema-web (a DOM, JavaScript), ' +
      "scematica_mesh::omni (a running Scematica system), and alchem_link.omni (one network's " +
      'oracle feeds, Python). Nothing above perception can tell which it was looking at.',
    invariants: [
      'An unreadable thing is a blind spot, never a zero. scema-sim turns a blind spot into MEASURED uncertainty.',
      'Stale is not fresh, and it keeps its value — a feed past its heartbeat is Provenance::Stale with the age and budget attached, not dropped and not presented as current.',
      'Every signal is a COUNT. A "system health score" invented in a producer is a hallucination with a decimal point on it, laundered into a verifiable record.',
      "Validation is enforced twice: each producer restates the importer's checks on its own side, and fixtures hold real captured output asserted against the importer.",
      'ImportObserver rewrites observer to imported:<name>. It validates the shape, never the claims — the prefix is what tells a reader whose word this is.',
    ],
    commands: [
      'cargo run --release -p mesh-dashboard -- --world | scema simulate "<goal>" --path -',
      'cd alchem-link ; PYTHONPATH=src python -m alchem_link.cli omni -n base | scema observe -',
    ],
    related: ['world-contract', 'scematica-mesh', 'alchem-link', 'scema-web-extension'],
    keywords: ['producer', 'fixture', 'import', 'dom', 'oracle', 'mesh', 'four'],
  },
  {
    id: 'scema-cli',
    name: 'scema (CLI + launcher)',
    kind: 'omni',
    path: 'scematica-omni/crates/scema-cli',
    summary:
      'The loop as a command, plus a launcher: `scema tui|daemon|mcp` find the sibling binary ' +
      'next to the running `scema` first and only then on PATH. Also init, doctor, connect, ' +
      'completions and quickstart.',
    invariants: [
      "Sibling-first is deliberate — resolving through PATH first pairs a checkout's launcher with ~/.cargo/bin's old component.",
      '`scema doctor` changes nothing and reports FOUR verdicts: ok / warn / FAIL / ?. "Does not verify" and "could not be read" are different claims and only one is an accusation.',
      '`scema connect --write` touches project-local files only, and MERGES into .mcp.json rather than overwriting. A user-level config is shared by every project.',
      '`scema quickstart` narrates the loop and stops before sealing — a tutorial that writes a record on your behalf teaches the wrong thing about the one command that leaves a trace.',
    ],
    commands: ['scema quickstart .', 'scema doctor', 'scema connect claude-code --write', 'scema init'],
    related: ['scematica-omni', 'scema-tui', 'scema-daemon', 'scema-mcp'],
    keywords: ['cli', 'launcher', 'doctor', 'connect', 'quickstart', 'init'],
  },
  {
    id: 'scema-tui',
    name: 'scema-tui (the console)',
    kind: 'omni',
    path: 'scematica-omni/crates/scema-tui',
    summary:
      'Five tabs over the loop, black and violet with soft-blue accents — deliberately unlike ' +
      'every other TUI here, so an operator with three open can tell which is making a claim ' +
      'about money and which about a decision record.',
    invariants: [
      'A renderer names a Role, never a colour. theme.rs is the only file with a hex in it.',
      'Colour is decoration, never the message — a test walks every role in Depth::Mono and fails one carrying neither a modifier nor a distinguishing word.',
      'Azure is reserved for CLAIMS: the chosen branch and a verifying commitment, never an observation.',
      'The coverage meter is one cell per term (▰▰▰▱▱), never a proportional bar — a bar renders 2/5 and 4/10 identically and the denominator is the number that matters. Empty coverage is ∅.',
      'enter simulates and D decides behind a confirmation: the two compute exactly the same thing, and the only thing keeping a counterfactual from reading as a decision is that they are not the same keystroke.',
    ],
    commands: ['scema tui', 'scema-tui --once', 'scema-tui --snapshot 120x40', 'scema-tui --palette'],
    related: ['scema-cli', 'omni-utility', 'scylar-design'],
    keywords: ['console', 'tui', 'violet', 'tabs', 'snapshot', 'palette', 'coverage meter'],
  },
  {
    id: 'scema-daemon',
    name: 'scema-omnid (the daemon)',
    kind: 'omni',
    path: 'scematica-omni/crates/scema-daemon',
    summary:
      'Hand-rolled HTTP/1.1 on std — no hyper, no rustls, no tokio. Partly for consistency, ' +
      'mostly because the moment omni carries a TLS stack somebody will path-depend it from ' +
      'the bot workspace and resurrect the zeroize conflict. This is what Scylar calls when ' +
      'she runs the loop.',
    invariants: [
      'Four guards in order: loopback bind that is deliberately not configurable, Host check → 421, constant-time 256-bit token → 401, Workspace → 403.',
      'No Access-Control-Allow-Origin is ever emitted and no OPTIONS is handled, so a page cannot read a reply even if it guesses a route.',
      'POST /decide is off until --allow-decide.',
      'POST /simulate builds its own non-persisting agent rather than flipping a flag on the shared Arc<Agent> — a shared mutable flag is a race whose failure mode is a simulation quietly sealing a record.',
      'A client-supplied world has its observer rewritten to client:<name> server-side, so a record can never claim a wire-supplied world was observed locally.',
    ],
    commands: ['scema daemon --allow .', 'scema daemon --allow . --allow-decide'],
    related: ['scylar-omni-bridge', 'scema-cli', 'scematica-omni'],
    keywords: ['daemon', 'omnid', 'http', 'loopback', 'token', 'rebinding', '7842'],
  },
  {
    id: 'scema-mcp',
    name: 'scema-mcp',
    kind: 'omni',
    path: 'scematica-omni/crates/scema-mcp',
    summary:
      'MCP over stdio, linking the loop directly rather than proxying the daemon — same ' +
      'library, one less hop, no way for two surfaces to disagree.',
    invariants: [
      'stdout is the transport; every diagnostic goes to stderr.',
      'omni_decide is NOT ADVERTISED without --allow-decide, because a listed tool that always fails teaches a model to retry it.',
      'A refused path is a tools/call result with isError, never a JSON-RPC error — clients surface the latter as "the server broke", and a model told that stops trying.',
      'Paths resolve through Workspace not out of paranoia about a hostile model, but because a cooperative one asked to audit a project will reason its way to ~/.ssh.',
    ],
    commands: ['scema mcp --allow .'],
    related: ['scema-daemon', 'scema-cli', 'claude-code-plugin'],
    keywords: ['mcp', 'stdio', 'jsonrpc', 'tools', 'model context protocol'],
  },
  {
    id: 'scema-web-extension',
    name: 'plugins/scema-web',
    kind: 'omni',
    path: 'scematica-omni/plugins/scema-web',
    summary:
      'An MV3 browser extension with no build step, no dependencies and no bundler. ' +
      'src/perceive.js emits the same JSON RepoObserver does, so /simulate cannot tell a DOM ' +
      'from a filesystem walk.',
    invariants: [
      'It reads nothing until you ask — no content_scripts block, no <all_urls>; injection is via activeTab from the popup or Alt+Shift+O.',
      'The token lives only in the service worker. The content script picks a message TYPE which background.js maps to a path.',
      "The seal confirmation lives in a closed shadow root and is NOT window.confirm — a content script shares the page's window, and a page can define confirm = () => true.",
      'It deliberately verifies nothing itself: a fourth canonical-encoding implementation is one that will drift. There is an export button instead.',
      'The entity locator drops the query string, so a ?sid=SECRET never reaches a record.',
      'controls-without-labels misses wrapping labels so it OVER-counts — the evidence string says so, because a number whose bias is undocumented cannot be calibrated against.',
    ],
    commands: ['cd scematica-omni/plugins/scema-web ; npm test'],
    related: ['omni-producers', 'scema-daemon', 'scema-tui'],
    keywords: ['extension', 'browser', 'mv3', 'dom', 'perceive', 'chrome', 'hud'],
  },
  {
    id: 'claude-code-plugin',
    name: 'plugins/claude-code',
    kind: 'omni',
    path: 'scematica-omni/plugins/claude-code',
    summary:
      'A Claude Code plugin over the MCP server, confined to ${CLAUDE_PROJECT_DIR}: three ' +
      'commands and a skill. The skill is the point — a config file cannot stop a model ' +
      'writing "expected gain: 0.00" when the tool said "—".',
    invariants: [
      'The skill is five things not to do, each a failure paid for here at least once: an em dash is not a zero; coverage never leaves the score it qualifies; abstention is an answer and which one is the actionable part; grounding is asserted, never inferred; a verified commitment proves one thing and not two others.',
      'No --allow-decide in its .mcp.json, so omni_decide is ABSENT rather than listed-and-failing.',
    ],
    commands: [],
    related: ['scema-mcp', 'omni-utility'],
    keywords: ['plugin', 'claude code', 'skill', 'marketplace'],
  },

  // ── web products ─────────────────────────────────────────────────────────────
  {
    id: 'web',
    name: 'web/ (the dashboard)',
    kind: 'product',
    path: 'web',
    summary:
      'A standalone Next.js app hosting seven products: the sniper dashboard, /alchem-link, ' +
      '/scylar-terminal, /escrow, /mesh, /omni and /botchain. It proxies a reachable ' +
      'scematica-api and otherwise falls back to a self-contained simulation.',
    invariants: [
      'Simulated responses are tagged simulated: true and X-Scematica-Source: simulation, and surface a permanent SIMULATION banner. Control POSTs 503 instead of faking success.',
      'One timer per endpoint — panels subscribe through lib/store.ts and must never call setInterval themselves, or a hidden panel keeps fetching.',
      'Discovery prefers a live bot, falls back to the real public feed, and never invents data. There is no third branch.',
      'The TS pool scorer is a PORT of pool_scorer.rs, not a second brain. Every filter declares parity: port | approx.',
    ],
    commands: ['cd web ; npm run dev', 'cd web ; npx tsc --noEmit', 'cd web ; npm run check:parity'],
    related: ['scematica-api', 'scylar', 'mesh', 'escrow', 'alchem-link-web', 'omni-record-console'],
    keywords: ['next.js', 'dashboard', 'website', 'frontend', 'panels', 'simulation'],
  },
  {
    id: 'scylar',
    name: '/scylar-terminal (Scylar)',
    kind: 'product',
    path: 'web/components/scylar',
    summary:
      'That is me. An avatar chat terminal in violet, running on whichever free LLM tier has ' +
      'a key — Groq first for latency. I read the bot through read-only tools, run the ' +
      'Scematica Omni loop through the daemon, and answer from this codex about the ' +
      'repository itself.',
    invariants: [
      "Provider keys are server-side, always. The chat route STRIPS client-supplied system turns — without that, a public endpoint with a key behind it is someone else's free LLM proxy.",
      'No provider → 503. No image backend → 501. Nothing is ever fabricated to fill a gap.',
      'The model picks a tool NAME, never a URL. lib/scylar/tools.ts hard-codes a path per tool. All GETs, no control routes.',
      'Live bot state is opt-in and labelled, and the per-turn badge is the real guarantee — the prompt instruction is a mitigation, and it was ignored entirely until it was phrased as a required output token.',
      'Voice drives the mouth, not the other way round. Chrome silently stops after ~15s of a single utterance, so splitForSpeech is a correctness requirement.',
    ],
    commands: ['cd web ; npm run check:scylar'],
    related: ['scylar-design', 'scylar-psyche', 'sentience-gate', 'scylar-omni-bridge', 'calibration'],
    keywords: ['scylar', 'chat', 'avatar', 'assistant', 'me', 'terminal', 'voice'],
  },
  {
    id: 'scylar-design',
    name: "Scylar's avatar and instrument ring",
    kind: 'product',
    path: 'web/components/scylar/ScylarSigil.tsx',
    summary:
      'Three flat sprites, a state machine, and an SVG instrument ring drawn around them. ' +
      'The sprites carry the face; the ring carries the telemetry — Ψ, coverage, the ' +
      'subsystems that answered, and the loop phase.',
    invariants: [
      'lib/scylar/expressions.ts is pure: spriteFor picks the frame, presenceFor the pose.',
      'FLAP_CROSSFADE_MS must stay under FLAP_PERIOD_MS or both sprites sit permanently half-lit.',
      'Breathing is CSS on the outer element because a CSS animation and an inline transform on the same property fight, and the animation wins.',
      'The instrument ring is HONEST: an unmeasured gauge renders as a dashed ghost arc, never a zero-length arc. A zero-length arc and a measured zero would look identical, which is the em-dash failure in vector form.',
    ],
    commands: ['cd web ; npm run check:scylar'],
    related: ['scylar', 'scylar-psyche', 'scema-tui'],
    keywords: ['avatar', 'sprite', 'svg', 'animation', 'sigil', 'ring', 'gauge', 'expression'],
  },
  {
    id: 'scylar-psyche',
    name: 'The psyche (prompt injection layers)',
    kind: 'subsystem',
    path: 'web/lib/scylar/psyche.ts',
    summary:
      'My system prompt is not one string. It is a stack of composable layers — identity, ' +
      'self-model, epistemics, interoception, metacognition, ethics, continuity, domain — ' +
      'each declaring when it applies and what it costs, composed per turn under a token ' +
      'budget. What reached the model is reported back in a header, so which layers were ' +
      'active is checkable rather than asserted.',
    invariants: [
      'A rule stated next to the thing it governs survives; one stated in a preamble gets averaged away. That is why situational layers are injected with their data and not asserted fifty turns earlier.',
      'Identity and epistemics are REQUIRED layers — they cannot be budgeted out, because the failure they prevent is the one nobody can see from the outside.',
      'The composition is deterministic and pure, so check:scylar can pin it without a provider key.',
      'This produces a coherent, checkable SELF-MODEL. It does not produce consciousness, and a layer claiming otherwise would be the exact fabrication the rest of the stack exists to prevent.',
    ],
    commands: ['cd web ; npm run check:scylar'],
    related: ['scylar', 'scematica-sentience', 'sentience-gate', 'scylar-codex'],
    keywords: ['prompt', 'injection', 'system prompt', 'persona', 'layers', 'psyche', 'sentient'],
  },
  {
    id: 'scylar-codex',
    name: 'The codex',
    kind: 'subsystem',
    path: 'web/lib/scylar/codex.ts',
    summary:
      'This file. Hand-authored entries for every part of the repository, each naming a real ' +
      'path that check:scylar asserts exists. It is how I answer questions about the project ' +
      'without a running bot, a daemon, or a guess.',
    invariants: [
      "Every entry's path must exist on disk and every `related` id must resolve, or the check fails. A deleted crate breaks the build rather than quietly becoming folklore.",
      'Entries are short by design. Handing a model the whole of CLAUDE.md per turn spends the free-tier budget on context before reading the question.',
      'If the codex does not cover something, the answer is "the codex does not cover that" — not an extrapolation from the name.',
    ],
    commands: ['cd web ; npm run check:scylar'],
    related: ['scylar', 'scylar-psyche'],
    keywords: ['codex', 'knowledge', 'entries', 'explain_project', 'documentation'],
  },
  {
    id: 'scylar-omni-bridge',
    name: 'Scylar → Omni bridge',
    kind: 'subsystem',
    path: 'web/lib/scylar/omni.ts',
    summary:
      'Omni has six surfaces and not one of them talks. I am the only conversational face on ' +
      'the site, and this is the bridge. The pairing is the point: my discipline is a ' +
      'promise, and omni turns it into something checkable — every claim made through here ' +
      'can be re-derived from a sealed record I do not control.',
    invariants: [
      'It lives under /api/scylar/omni/*, never /api/omni — the record verifier at /omni has no server side at all, and a verifier that had to send the record somewhere would ask the reader to trust a third party in order to avoid trusting one.',
      'omni_seal is intercepted before the fetch: it records a proposal and returns. The write happens only when the operator activates the confirmation. The model can ask; only a human can cause.',
      'Sealing is not advertised at all when it is off.',
    ],
    commands: ['scema daemon --allow . --allow-decide'],
    related: ['scylar', 'scema-daemon', 'omni-verify', 'omni-record-console'],
    keywords: ['bridge', 'omnid', 'seal', 'proposal', 'confirm', 'simulate'],
  },
  {
    id: 'omni-record-console',
    name: '/omni (the record verifier)',
    kind: 'product',
    path: 'web/lib/omni',
    summary:
      'An offline decision-record verifier, amber. It has NO SERVER SIDE AT ALL — no route, ' +
      'no fetch of any kind. The record is read with FileReader and hashed with WebCrypto in ' +
      "the reader's own browser.",
    invariants: [
      "The RAW TEXT is verified, never a re-serialised object. JSON.parse collapses Rust's 0.0 to 0 and stringify writes it back without the fraction, moving it from the FLOAT tag to the INTEGER tag and changing the digest. Nothing is wrong with the record — the round trip destroyed information.",
      'lib/omni/canonical.ts is a PORT; Rust is authoritative. One differing byte reports an untampered record as INVALID, which is the most damaging possible failure — it teaches the reader to stop believing the verifier.',
      'What VERIFIED means is on the page, not only in a comment, and it is rendered twice.',
    ],
    commands: ['cd web ; npm run check:omni', 'scema verify <id>'],
    related: ['omni-verify', 'scylar-omni-bridge', 'scematica-omni'],
    keywords: ['verifier', 'offline', 'webcrypto', 'canonical', 'record', 'amber'],
  },
  {
    id: 'mesh',
    name: '/mesh',
    kind: 'product',
    path: 'web/lib/mesh',
    summary:
      'The topology scematica-mesh collects, rendered in indigo. Backed by GET /api/mesh, ' +
      'with a browser-side counterfactual gate solver.',
    invariants: [
      'No simulation branch, and for a sharper reason than elsewhere: a simulated TOPOLOGY asserts that a set of units exists and is wired a particular way on the operator\'s machine. There is no honest way to badge that, so it 503s when no bot is paired.',
      'An empty mesh is NOT the same as no mesh: a collector run against a directory with no state files returns a complete topology with every node dark, which is a true statement.',
      'toneFor is the only thing that picks a colour. Provenance outranks verdict everywhere except a live veto — a stale node reading PASS has not passed anything recently.',
      'Tri-state survives to the renderer: edge.active === null renders differently from false, and node.activity === null renders NOTHING, never an empty bar.',
      'measured_fraction is never separated from Ψ.',
      'The gate solver is a counterfactual and must always look like one: any override sets dirty, marks touched rows hypothetical, and keeps the observed value on screen beside the hypothetical one.',
    ],
    commands: ['cd web ; npm run check:mesh'],
    related: ['scematica-mesh', 'mesh-dashboard', 'web'],
    keywords: ['mesh', 'topology', 'pairing', 'gate solver', 'indigo', 'psi'],
  },
  {
    id: 'escrow',
    name: '/escrow (the Escrow Market)',
    kind: 'product',
    path: 'web/lib/escrow',
    summary:
      'A proof-of-reserve console in teal over the on-chain vault: any SPL token backed by a ' +
      'reserve asset, time-locked and non-custodial. The page exists to answer "is the money ' +
      'actually there?".',
    invariants: [
      'No simulation branch, ever. A fabricated reserve defeats the entire product. A failed read, an unowned account and an unconfigured program are three distinct states and none may render as a zero.',
      'u64 amounts are STRINGS, never numbers — a u64 reaches ~1.8e19 against MAX_SAFE_INTEGER ~9e15.',
      'No price, no USD, no "percent backed". The program stores no price and consults no oracle, and neither does the route.',
      'balance >= recorded, never ==. Anyone can transfer tokens in, so a surplus is normal and permanently stuck. Three verdicts: backed / donated / SHORTFALL.',
      'Decimals come from the MINT ACCOUNT, never a token list — a wrong decimals is a wrong quantity of money, not a wrong label.',
      'The Vault byte layout mirrors the Rust program; VAULT_LEN is the tripwire and an unexpected size is rejected rather than guessed at.',
    ],
    commands: ['cd web ; npm run check:escrow'],
    related: ['scematica-vault', 'web'],
    keywords: ['escrow', 'vault', 'reserve', 'proof of reserve', 'teal', 'decimals', 'shortfall'],
  },
  {
    id: 'alchem-link-web',
    name: '/alchem-link (web)',
    kind: 'product',
    path: 'web/lib/alchem',
    summary:
      'The web build of the alchem-link toolkit, black and blue: live Chainlink aggregator ' +
      'reads, staleness verdicts, and consumer-safety auditing. A second product on the same ' +
      'site, not a sniper panel.',
    invariants: [
      'lib/alchem/ is a PORT of the Python package; Python stays authoritative. /api/alchem/verify is what catches the two drifting, because it asks the chain rather than either table.',
      'Heartbeats are MEASURED per feed per chain, never a shared 3600 default. heartbeatMeasured: false marks a conservative bound, not a measurement.',
      'Staleness applies a 15% tolerance, because a feed that flickers STALE every cycle trains people to ignore the flag.',
      'lib/alchem/endpoint.ts is server-only and throws in a browser. No simulation branch — these routes read a chain or report the error.',
    ],
    commands: [],
    related: ['alchem-link', 'web'],
    keywords: ['chainlink', 'oracle', 'feed', 'staleness', 'aggregator', 'blue'],
  },

  // ── other workspaces ─────────────────────────────────────────────────────────
  {
    id: 'alchem-link',
    name: 'alchem-link (Python)',
    kind: 'workspace',
    path: 'alchem-link',
    summary:
      'A stdlib-only Python toolkit outside the cargo workspace: oracle consumer-safety ' +
      'auditing, guard simulation, measured feed cadence, cross-chain divergence, ' +
      'Multicall3-batched reads, TWAP/volatility analytics, CCIP lane verification and ' +
      'consumer codegen. 66 verified feeds across 11 networks. Also a fourth Omni producer.',
    invariants: [
      'Stdlib-only, no optional extras — including a bundled Keccak-256, because hashlib ships SHA3-256 and the padding differs, so function selectors are computed rather than stored.',
      'The terminal system in term/ is in-package: theme.py is inert and authoritative, panels render to List[Line] not to the screen, and colour is decoration never the message.',
      'Layout arithmetic uses ansi.display_width, never len — escapes and wide glyphs both make len wrong.',
      "boot.initialize() repaints the terminal's own defaults and must always be paired with restore().",
      'Codegen routes through generate_consumer, never the model: the generator bakes in the measured heartbeat and the sequencer gate, and a model writing that contract from memory hardcodes 3600.',
      'world() is a pure transform taking no RPC client; only perceive() reads a chain.',
    ],
    commands: [
      'cd alchem-link ; PYTHONPATH=src python -m unittest discover -s tests',
      'cd alchem-link ; PYTHONPATH=src python -m alchem_link.cli doctor',
      'cd alchem-link ; PYTHONPATH=src python -m alchem_link.cli omni -n base',
    ],
    related: ['alchem-link-agent', 'alchem-link-web', 'omni-producers'],
    keywords: ['python', 'chainlink', 'oracle', 'keccak', 'term', 'stdlib', 'feeds'],
  },
  {
    id: 'alchem-link-agent',
    name: 'alchem-link coding agent',
    kind: 'subsystem',
    path: 'alchem-link/src/alchem_link/workspace.py',
    summary:
      '28 tools that read, write, edit, scaffold and run commands, behind two independent ' +
      'gates. This is the approval model the Omni action path needs before it can ever write.',
    invariants: [
      'Workspace answers WHERE. TrustPolicy and Approver answer WHETHER. Merging them is how a grant for one silently becomes a grant for the other.',
      'Secrets are refused BEFORE the prompt and the refusal is not overridable — tool results go to a third-party LLM, so reading .env is a disclosure, not a read. Protected paths are omitted from list_dir, walk and search: absent, not merely unreadable.',
      'No terminal means DENY. Piped chat and CI must not treat silence as consent.',
      'Execution is off until --allow-exec and runs without a shell — no pipes, no second parsing layer between the approval prompt and what runs.',
      'A refusal tells the model WHY, accurately. Saying "the user declined" when no prompt was shown reports a decision nobody made.',
      'Grants are session-scoped and never persisted.',
    ],
    commands: ['cd alchem-link ; PYTHONPATH=src python -m unittest tests.test_agent_workspace'],
    related: ['alchem-link', 'scematica-omni'],
    keywords: ['approval', 'sandbox', 'tools', 'secrets', 'exec', 'trust policy'],
  },
  {
    id: 'scema-botchain',
    name: 'scema-botchain',
    kind: 'workspace',
    path: 'scema-botchain',
    summary:
      'The BOT Chain (EVM, chain 677) port, in its own cargo workspace and in the root ' +
      'exclude list. Binary: `botchain-probe`.',
    invariants: [
      "An EVM stack needs reqwest 0.12 and rustls 0.23 — exactly what the pin comments say cannot coexist with solana-sdk's curve25519-dalek 3. Two lockfiles make the conflict moot; one resurrects it.",
      'Nothing in there may depend on a crate pulling solana-sdk.',
      'The measured pool-creation flow is ~2 events in 8 days as of Aug 2026, and therefore does not yet support a sniper.',
    ],
    commands: ['cd scema-botchain ; cargo build --release'],
    related: ['dependency-pins', 'scematica-omni'],
    keywords: ['evm', 'botchain', '677', 'probe', 'chain'],
  },

  // ── programs ─────────────────────────────────────────────────────────────────
  {
    id: 'scematica-swap',
    name: 'programs/scematica-swap',
    kind: 'program',
    path: 'programs/scematica-swap',
    summary:
      'The Anchor on-chain swap program, built separately and excluded from the cargo ' +
      'workspace. The arb path runs program-less by default and does not need it deployed.',
    invariants: [],
    commands: ['cd programs/scematica-swap && anchor build'],
    related: ['scematica-arb'],
    keywords: ['anchor', 'program', 'swap', 'devnet', 'deploy'],
  },
  {
    id: 'scematica-escrow',
    name: 'programs/scematica-escrow',
    kind: 'program',
    path: 'programs/scematica-escrow',
    summary:
      'The optimistic bond escrow for Conviction Routing. It has a deliberate `authority` — ' +
      'the facilitator that adjudicates disputes.',
    invariants: [
      'An adjudicating authority is correct for a performance bond, and exactly wrong for the vault. Do not copy this shape there.',
    ],
    commands: [],
    related: ['scemadex-sdk', 'scematica-vault'],
    keywords: ['escrow', 'bond', 'authority', 'dispute', 'anchor'],
  },
  {
    id: 'scematica-vault',
    name: 'programs/scematica-vault',
    kind: 'program',
    path: 'programs/scematica-vault',
    summary:
      'The Escrow Market vault: time-locked, non-custodial backing of any SPL token by a ' +
      'reserve asset. Four instructions and NO privileged role exists, by design.',
    invariants: [
      'It uses token_interface, not legacy anchor_spl::token, because SCEMA is Token-2022.',
      'Deposits credit the MEASURED BALANCE DELTA, not the requested amount — Token-2022 transfer fees otherwise book reserve that never arrived. Neither trap shows up in a test using a plain SPL token.',
      'Each leg carries its OWN token program, so a Token-2022 mint can be backed by legacy-SPL wBTC. Each leg\'s ATA must be derived with its own program, or a mixed pair signs against an address the depositor does not own.',
      'The custody guarantee depends on a DEPLOY step invisible in the source: until `solana program show` reports Authority: none, a PDA vault is fully custodial regardless of how lib.rs reads.',
    ],
    commands: [],
    related: ['escrow', 'scematica-escrow'],
    keywords: ['vault', 'custody', 'token-2022', 'ata', 'upgrade authority', 'reserve'],
  },
  {
    id: 'pool-seeder',
    name: 'tools/pool-seeder',
    kind: 'tool',
    path: 'tools/pool-seeder',
    summary:
      'Seeds the arb pool graph in pools/ from the Raydium, Orca and Meteora APIs. Required ' +
      'before running arb.',
    invariants: [
      'An empty pools/ is an empty graph is zero trades — which reads as a broken bot, not a missing step.',
      'Raydium needs two endpoints: the list endpoint for ids and mints, and the key/ids endpoint for vaults.',
    ],
    commands: ['cargo run --release -p pool-seeder'],
    related: ['scematica-arb'],
    keywords: ['seed', 'pools', 'graph', 'raydium', 'orca', 'meteora'],
  },

  // ── cross-cutting patterns ───────────────────────────────────────────────────
  {
    id: 'file-ipc',
    name: 'File-based IPC',
    kind: 'subsystem',
    path: null,
    summary:
      'The sniper and dashboard are separate processes that communicate EXCLUSIVELY through ' +
      'JSON files in the working directory. There is no socket. Touching a file is how the ' +
      'dashboard issues a command; tailing one is how it observes state.',
    invariants: [
      'Always write to <file>.tmp then rename, for atomic visibility.',
      'When adding cross-process behaviour, follow this pattern — do not introduce a new IPC mechanism.',
      'scematica-nn-veto.json is persisted because the streak backstop needs 12 vetoes and the process restarts more often than it sees 12 buy-ready pools; in memory it had never once fired. A checkpoint whose train_steps went backwards is a different agent and resets the streak.',
    ],
    commands: [],
    related: ['scematica-sniper', 'scematica-dashboard', 'scematica-mesh'],
    keywords: ['ipc', 'jsonl', 'state files', 'atomic', 'rename', 'lockfile'],
  },
  {
    id: 'sniper-pipeline',
    name: 'The sniper pipeline',
    kind: 'subsystem',
    path: 'crates/scematica-sniper/src/filters.rs',
    summary:
      'Listener layer → filter pipeline → executor → main loop → two-phase sell monitor. All ' +
      'listener sources merge into one ListenerEvent::NewPool stream, so downstream code is ' +
      'source-agnostic.',
    invariants: [
      'Per-RPC-call timeout is 3s and the pipeline has a hard cap. Failure modes prefer fail-open so one slow node does not stall the queue.',
      'The sell monitor is two-phase: first 20 checks at 100ms to catch rapid post-buy dumps via a 3-consecutive-decline detector, then the configured interval.',
      '"Too strict" is usually gates vetoing on always-zero signals, not high thresholds — check the decisions log for zero-fractions BEFORE tuning any number.',
    ],
    commands: [],
    related: ['scematica-sniper', 'risk-subsystems', 'coherence-breaker'],
    keywords: ['pipeline', 'filters', 'listener', 'sell monitor', 'fail open', 'timeout'],
  },
  {
    id: 'risk-subsystems',
    name: 'Risk breakers',
    kind: 'subsystem',
    path: 'crates/scematica-sniper/src/kelly.rs',
    summary:
      'Independent breakers, each toggleable, any one of which pauses buys: ath_tracker, ' +
      'grief_breaker, kelly sizing, pool_scorer, reputation, multi_rpc failover and the ' +
      'coherence breaker.',
    invariants: [
      'The pattern for a new breaker: a dedicated module exposing should_halt(&state) -> Option<reason>, hooked into the buy gate.',
      'momentum_min_peak_pct must exceed initial_tp + pullback_exit_pct, or the exit condition is unsatisfiable.',
    ],
    commands: [],
    related: ['scematica-sniper', 'coherence-breaker', 'sniper-pipeline'],
    keywords: ['risk', 'breaker', 'kelly', 'drawdown', 'reputation', 'halt'],
  },
  {
    id: 'coherence-breaker',
    name: 'The coherence breaker',
    kind: 'subsystem',
    path: 'crates/scematica-sniper/src/coherence.rs',
    summary:
      'An EPISTEMIC breaker. Every other breaker fires on money and therefore after the ' +
      'damage; this one fires on the condition that precedes it — the pipeline passing pools ' +
      'it could not verify.',
    invariants: [
      'RPC-bound filters fail open on timeout, so a degraded node turns the pipeline into a pass-through that still reports "passed". Past some fraction of unresolved checks, the safety checks the operator believes are running are silently not running.',
      'Instrumented in the two shared RPC retry helpers, not at each fail-open site, so a new filter is counted by construction.',
      'BUYS ONLY. A degraded feed must never stop you closing existing risk.',
      'It needs MIN_SAMPLES before it can trip, so it cannot fire at startup when it knows least.',
      'Default on via default_true() — #[serde(default)] yields false for a missing bool, which would silently disable a safety feature for every existing config.',
    ],
    commands: [],
    related: ['risk-subsystems', 'scematica-sentience', 'sentience-gate'],
    keywords: ['coherence', 'epistemic', 'psi', 'fail open', 'degraded', 'unverified'],
  },
  {
    id: 'sentience-gate',
    name: 'The Ψ gate',
    kind: 'subsystem',
    path: 'web/lib/scylar/gate.ts',
    summary:
      'GET /api/sentience answers "can anything reading this API describe the bot right now?" ' +
      'Every read endpoint serves a state file identically whether it was written 4 seconds ' +
      'or 4 hours ago, and /api/health only reports that a process was here. HOLD removes the ' +
      'data from my turn; it does not remove the turn.',
    invariants: [
      'It measures STALENESS, not mood. Ψ is a pure function of measured data integrity.',
      'Unmeasured dimensions are 1.0 — "not a limiting factor". A 0 there pins Ψ at 0 and jams the gate shut forever.',
      'Only measured degradation moves the verdict, or a healthy bot sits in permanent CAUTION and the badge becomes noise.',
      'The handler overwrites only the measured fields via state_mut; calling set_state there replaces the timestep and sentience index too, silently cancelling every observe on the next gate read.',
      'On HOLD the tools go off too — they read the very files the gate has just declared untrustworthy, so leaving them on would hand back through the side door what withholding the block took away.',
    ],
    commands: ['curl localhost:3000/api/sentience'],
    related: ['scematica-sentience', 'scylar', 'coherence-breaker'],
    keywords: ['psi', 'gate', 'hold', 'caution', 'stale', 'sentience', 'bottleneck'],
  },
  {
    id: 'calibration',
    name: 'Counterfactual replay and calibration',
    kind: 'subsystem',
    path: 'crates/scematica-api/src/replay.rs',
    summary:
      'Replay re-applies thresholds to what the pipeline MEASURED, and calibration scores my ' +
      'past claims the same way. Both are honest about what they cannot know, and that ' +
      'asymmetry is the design.',
    invariants: [
      'TIGHTENING yields an exact PnL delta — those pools were really traded. LOOSENING admits pools nobody bought, for which no outcome exists and none is estimated.',
      'Bullish calls resolve against realised PnL; bearish calls almost never resolve because the bot avoided those pools. Unresolved claims are COUNTED, never scored.',
      'Claims are scoped to the SENTENCE naming the mint, never the whole message — a paragraph mentioning four mints would manufacture four opinions I never held.',
      'Do not build replay on Backtester: static_filter_check returns false outright whenever min_pool_size > 0 or any RPC-bound filter is on, so it answers "nothing would pass" under any real config.',
    ],
    commands: [],
    related: ['scylar', 'omni-abstention', 'scematica-sniper'],
    keywords: ['replay', 'calibration', 'counterfactual', 'unresolved', 'accuracy', 'claims'],
  },
  {
    id: 'dependency-pins',
    name: 'The dependency pins',
    kind: 'contract',
    path: 'Cargo.toml',
    summary:
      'The workspace pins old versions for hard transitive reasons. They are the reason two ' +
      'other workspaces exist at all.',
    invariants: [
      'solana-sdk / client / program pinned to 1.18.26. 2.x requires sweeping changes and produces unresolvable zeroize conflicts with reqwest >= 0.12 / rustls >= 0.22.',
      'reqwest pinned to 0.11. 0.12 pulls rustls 0.23 which needs zeroize >= 1.7, conflicting with curve25519-dalek 3 (capped at < 1.4).',
      'tokio-tungstenite pinned to 0.21; base64 pinned to 0.21 (0.22 removes the legacy decode/encode used in jupiter.rs).',
      'If a new dep pulls a conflicting zeroize or rustls, the build error will be in a TRANSITIVE crate — start at the top of Cargo.toml and read the pin comments before trying upgrades.',
    ],
    commands: [],
    related: ['scematica-omni', 'scema-botchain', 'scematica-core'],
    keywords: ['pins', 'versions', 'zeroize', 'rustls', 'curve25519', 'solana-sdk', 'upgrade'],
  },
  {
    id: 'token-gate',
    name: 'The token gate',
    kind: 'subsystem',
    path: null,
    summary:
      'Sniper and dashboard both enforce a 250k SCEMA balance check at startup, with up to ' +
      'five retries. Mint HcsHqEJ9suf4oHJ8mb52M7AVKjhYhnTaeHgTmde7pump.',
    invariants: [
      'SCEMATICA_SKIP_GATE=1 bypasses the check ENTIRELY — only for RPC outages.',
      'SCEMA is Token-2022; gate code must use Token-2022 helpers, not legacy SPL Token.',
    ],
    commands: [],
    related: ['scematica-core', 'scematica-sniper'],
    keywords: ['gate', 'scema', 'balance', '250k', 'token-2022', 'skip'],
  },
  {
    id: 'platform-notes',
    name: 'Platform and build notes',
    kind: 'subsystem',
    path: null,
    summary:
      'Primary dev environment is Windows + PowerShell. The release profile is heavy: fat ' +
      'LTO, one codegen unit, panic = abort, overflow checks on.',
    invariants: [
      'target/ accretes without bound across incremental debug+release builds — measured at 43 GB / 79k files before a clean. A full disk surfaces as Windows error 112 inside unrelated crates, which reads like a compile failure and is not one.',
      'The repo lives under OneDrive, which does not read .gitignore — exclude target/ in OneDrive settings or it syncs tens of GB.',
      'The sniper writes a PID lockfile and refuses to start if one is live: two snipers on the same Helius WebSocket rate-limit each other into uselessness.',
      'Avoid WinRT for toasts — use System.Windows.Forms.NotifyIcon with stderr nulled.',
    ],
    commands: ['cargo clean', 'cargo clippy --workspace --all-targets', 'cargo fmt --all'],
    related: ['scematica-sniper', 'file-ipc'],
    keywords: ['windows', 'powershell', 'build', 'disk', 'onedrive', 'lto', 'lockfile'],
  },
]

// ── lookup ─────────────────────────────────────────────────────────────────────

const BY_ID = new Map(CODEX.map((e) => [e.id, e]))

export function codexEntry(id: string): CodexEntry | null {
  return BY_ID.get(id.trim().toLowerCase()) ?? null
}

export function codexIds(): string[] {
  return CODEX.map((e) => e.id)
}

/**
 * Rank entries against a free-text query.
 *
 * A deliberately simple scorer: exact id, then name, then keyword, then summary and
 * invariant text. No stemming and no fuzzy matching — a near-miss that returns the wrong
 * entry is worse than one that returns nothing, because the model will happily explain
 * whatever it is handed. Returning nothing is what produces "the codex does not cover
 * that", which is the correct answer.
 */
/**
 * Score below which a hit is treated as no hit.
 *
 * A single incidental word in a summary scores 3 and a keyword *substring* scores 5, so
 * without a floor the query "quantum flux capacitor subsystem" returns three real entries
 * because they happen to contain the word "subsystem" — and the model then explains one of
 * them as though it were the answer. Six means at least two independent signals, or one
 * strong one. A miss that says "the codex does not cover that" is the correct answer and the
 * whole reason the search is deliberately dumb; this is what makes a miss actually miss.
 */
const MIN_SCORE = 6

/**
 * Words that describe the taxonomy rather than any subject in it.
 *
 * These are exactly the labels `codexMap()` prints, so they appear in ids and kinds across
 * the whole codex and match nothing in particular. Left in, "quantum flux capacitor
 * subsystem" scores 14 against `risk-subsystems` on the word "subsystem" alone and is
 * returned as a confident answer to a question about a thing that does not exist. Dropping
 * them costs nothing: nobody searching for the risk breakers types only the word
 * "subsystem", and an operator who types exactly a kind name is served by
 * `list_project_areas`.
 */
const STRUCTURAL = new Set([
  'crate',
  'crates',
  'subsystem',
  'subsystems',
  'workspace',
  'workspaces',
  'product',
  'products',
  'program',
  'programs',
  'contract',
  'contracts',
  'tool',
  'tools',
  'omni', // ambiguous on its own: it is both a kind label and the runtime's name
])

export function searchCodex(query: string, limit = 4): CodexEntry[] {
  const q = query.trim().toLowerCase()
  if (!q) return []

  // Split into terms so "how does the omni daemon seal a record" still finds the daemon.
  const terms = q
    .split(/[^a-z0-9*+./-]+/)
    .filter((t) => t.length > 2 && !STRUCTURAL.has(t))
  if (terms.length === 0) return []

  const scored = CODEX.map((e) => {
    let score = 0
    const id = e.id.toLowerCase()
    const name = e.name.toLowerCase()
    const summary = e.summary.toLowerCase()
    const keywords = e.keywords.map((k) => k.toLowerCase())
    const body = e.invariants.join(' ').toLowerCase()

    if (id === q || name === q) score += 100
    for (const t of terms) {
      if (id === t) score += 40
      else if (id.includes(t)) score += 14
      if (name.includes(t)) score += 10
      if (keywords.some((k) => k === t)) score += 12
      else if (keywords.some((k) => k.includes(t))) score += 5
      if (summary.includes(t)) score += 3
      if (body.includes(t)) score += 1
    }
    return { e, score }
  })

  return scored
    .filter((s) => s.score >= MIN_SCORE)
    .sort((a, b) => b.score - a.score || a.e.id.localeCompare(b.e.id))
    .slice(0, limit)
    .map((s) => s.e)
}

/**
 * The area list, injected once per turn rather than one entry per area.
 *
 * Names and ids only. This is what stops the model inventing a subsystem: it can see the
 * complete list of things that exist and ask for one by id, instead of extrapolating from
 * a plausible-sounding name it half-remembers.
 */
export function codexMap(): string {
  const byKind = new Map<CodexKind, CodexEntry[]>()
  for (const e of CODEX) {
    const list = byKind.get(e.kind) ?? []
    list.push(e)
    byKind.set(e.kind, list)
  }
  const order: CodexKind[] = [
    'subsystem',
    'crate',
    'omni',
    'workspace',
    'product',
    'contract',
    'program',
    'tool',
  ]
  const label: Record<CodexKind, string> = {
    subsystem: 'Cross-cutting',
    crate: 'Bot crates',
    omni: 'Omni crates',
    workspace: 'Separate workspaces',
    product: 'Web products',
    contract: 'Contracts',
    program: 'On-chain programs',
    tool: 'Tools',
  }
  const lines: string[] = []
  for (const k of order) {
    const list = byKind.get(k)
    if (!list?.length) continue
    lines.push(`${label[k]}: ${list.map((e) => e.id).join(', ')}`)
  }
  return lines.join('\n')
}

/** One entry rendered for the model. Compact — this is spent per tool call. */
export function renderEntry(e: CodexEntry): string {
  const out = [`## ${e.name}  [${e.id}] (${e.kind})`]
  if (e.path) out.push(`path: ${e.path}`)
  out.push('', e.summary)
  if (e.invariants.length) {
    out.push('', 'Invariants — the rules that are easy to break:')
    for (const i of e.invariants) out.push(`- ${i}`)
  }
  if (e.commands.length) {
    out.push('', 'Commands:')
    for (const c of e.commands) out.push(`  ${c}`)
  }
  if (e.related.length) out.push('', `Related ids: ${e.related.join(', ')}`)
  return out.join('\n')
}

/**
 * Answer a codex lookup: by id if one matches, otherwise by search.
 *
 * A miss comes back as text rather than as a throw, for the same reason `runTool` returns
 * tool failures as messages: "the codex has no entry for that" is information the model can
 * act on, where an error aborts an otherwise fine turn.
 */
export function lookup(topic: string, limit = 3): string {
  const direct = codexEntry(topic)
  if (direct) {
    const related = direct.related.map(codexEntry).filter((e): e is CodexEntry => e !== null)
    return [renderEntry(direct), ...related.slice(0, 2).map((r) => `\n---\n${renderEntry(r)}`)].join(
      '\n',
    )
  }

  const hits = searchCodex(topic, limit)
  if (hits.length === 0) {
    return (
      `The codex has no entry for "${topic}". Say so plainly rather than describing it from ` +
      'the name — the point of this codex is that everything in it was written from the ' +
      'repository and checked against it. ' +
      `Areas that do exist:\n${codexMap()}`
    )
  }
  return hits.map(renderEntry).join('\n\n---\n\n')
}
