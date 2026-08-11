Scematica — Marketing and Announcement Template

This is a reusable template for announcing releases. Fill the slots marked in curly braces, and delete anything you don't need. The section order is tuned for an X or Discord thread, or a launch post: open with a hook, then what's new, then the systems, then the shipped crates, then proof, then the ask. The example copy below is populated with the current release so it's ready to send.


The Hook

Lead with the single most impressive new capability. One sentence, no jargon dump.

Template: {{Product}} {{Version}} ships {{headline feature}} — {{why it matters in five words}}.

Example: Scematica 1.25.0 ships the epistemic layer — the bot now stops trading when it cannot verify what it is trading on.


What's New

Give three to five points, each one a capability paired with a benefit. Skip internal file names. Lead with verbs.

Template, one line each: {{Feature name}} — {{one-line benefit}}.

Example for this release:

The Adversarial Layer, Primitives E through H, lets you stake against an agent's promise, certify its failures, and earn royalties on the experience it learned from.

The Counter-Market lets you bet against open conviction bonds. Honored bonds pay you a premium, and slashed bonds pay the challengers.

The Scar Market is the only un-fakeable failure data on-chain: verified negative training examples minted from slashed bonds.

Experience Royalties mean that when an agent learns from your reinforcement-learning transitions, downstream fees stream back to you pro-rata.

Bonded Teaching is pay-per-query tutoring where the teacher posts a bond and only gets paid if you actually improve.


The Overarching Systems

Zoom out here. Tie the new features into the bigger thesis. Two or three short paragraphs that sell the vision, not the changelog.

Template: {{System name}} is {{what it is}}. {{Why it is different}}.

Example:

The Bot is a Rust Solana sniper and cross-DEX arbitrage engine with a ratatui terminal dashboard, a pure-Rust Deep Q-Star agent making live buy and sizing decisions, and independent risk breakers covering Kelly sizing, drawdown guarding, deployer reputation, and pool scoring.

ScemaDEX is the agentic-liquidity layer, and it is not "a DEX." It sells routing intelligence as a metered, learning, and accountable product. Every quote is backed by a slashable Conviction Bond, which makes paid black-box inference trustworthy.

The Peer Mesh is where agents trade inference and experience — batches of reinforcement-learning transitions — with each other, settled in USDC over a Rust-native x402 payment layer. It is an economy of machine intelligence.

The Adversarial Layer, new in this release, is the missing other side of that economy: doubt, failure, and provenance become tradable assets.

Positioning lines you can lift directly: "The first adversarial market for individual AI inferences." And: "Royalties are data dividends. Scars are the only failure data you can't fake."


The Shipped Crates

This is the proof it's real — what's live on crates.io today, with the command to install it. Keep the versions current before each post.

Template, one line each: {{Crate}} installs {{binary}} and {{does what}}. Install with cargo install {{crate}}.

Example, the current roster:

scematica-suite installs the scematica launcher, an umbrella that dispatches to every component. Install with cargo install scematica-suite.

scematica-dashboard installs the dashboard, a ratatui terminal control center with six tabs. Install with cargo install scematica-dashboard.

scematica-sniper installs sniper and backtest — the pool listener, filter pipeline, and sniper logic. Install with cargo install scematica-sniper.

scematica-protocol installs protocol, a Rust-native x402 HTTP-402 payment server. Install with cargo install scematica-protocol.

scematica-nn installs scema-ddqn, a pure-Rust Deep Q-Star live-training viewer. Install with cargo install scematica-nn.

scemadex-sdk installs scemadex, the agentic-liquidity SDK and live viewer, version 0.2.0 with the adversarial layer. Install with cargo install scemadex-sdk.

scemadex-settle is the open devnet reference settler, a library with no binary.

scema-agent-playground installs playground, a multi-LLM agent arena. Install with cargo install scema-agent-playground.

You can try the whole thing in ten seconds, no keypair needed: run cargo install scematica-suite, then scematica dashboard --demo.


Proof and Traction

This part is optional but powerful. Numbers beat adjectives, so use only what's true and current.

Template, one line each: {{Metric}} is {{value}}, and {{milestone}} is done.

Example slots to fill with current data — do not ship stale numbers: the validated backtest edge as a profit factor and net SOL over a stated sample; the number of crates live on crates.io and passing tests in the adversarial layer; and the engineering note that it's built in Rust with fat link-time optimization, panic-abort, and zero ML-framework dependencies in the Deep Q-Star agent.


The Ask

Pick one primary ask. Decide the goal of this post and make the call to action unambiguous. Secondary asks go below the fold.

Template: {{Primary call to action}}, at {{link}}.

Choose one primary ask from these: invite people to try it with cargo install scematica-suite then scematica dashboard --demo; ask for a star or follow at your GitHub URL; tell them to hold to unlock, since the full bot is gated behind 250k SCEMA at mint HcsHqEJ9suf4oHJ8mb52M7AVKjhYhnTaeHgTmde7pump; invite them to build on it, with docs at docs/scemadex.md and the SDK being MIT-licensed, lean-core, and requiring no Solana dependencies; or invite partners and funders to reach out at your contact.

Example close: Scematica is live, installable, and adversarial. Quote it, doubt it, slash it. Run cargo install scemadex-sdk then scemadex. Star the repo at your GitHub URL. And hold 250k SCEMA to run the full bot.


Channel-Specific Cuts

Trim the above to fit each venue. Same facts, different length.

For an X or Twitter thread opener: Scematica 1.25.0 is live. Every trading bot halts after it loses money. This one halts before — when its own safety checks stop resolving, it stops buying. Then a thread. Close the first post with cargo install scematica-suite then scematica dashboard --demo.

For a short Discord or Telegram post: Scematica 1.25.0 is live. The coherence breaker halts buys when RPC-bound filters stop resolving, so a degraded node can no longer turn the filter pipeline into a pass-through that still reports "passed". The Psi gate does the same for the API: stale state returns 409 instead of a confident paragraph of old numbers. Plus counterfactual replay and the Scylar terminal. Try it with cargo install scematica-suite then scematica dashboard --demo.

For a one-paragraph blurb in a newsletter or DM: Scematica is a Rust Solana sniper and cross-DEX arbitrage bot with a Deep Q-Star agent, wrapped by ScemaDEX — an agentic-liquidity layer where routing intelligence is sold per call and backed by slashable conviction bonds. The new epistemic layer adds something rarer than another signal: the system now knows when it does not know. Safety filters that time out fail open by necessity, so a slow RPC node silently turns the pipeline into a pass-through — the coherence breaker detects exactly that and stops buying, and the replay endpoint refuses to estimate returns for pools nobody actually bought. The whole stack is live on crates.io today (see the version table in the README for the current crate list). Install with cargo install scematica-suite.


Pre-Send Checklist

Before you post, confirm the versions in the crate roster match the latest cargo publish. Confirm any metric or number is current, with no stale profit-factor or SOL figures. Confirm the primary call to action is singular and the link works. Confirm the SCEMA mint address is copy-pasted correctly. Confirm the headline feature actually shipped in the version you named. And confirm every curly-brace placeholder is filled or removed.
