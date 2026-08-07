Alchem-Link: A Complete Thesis on Reading Oracles Honestly

Version 0.23.0

Preface

This is the full story of Alchem-Link, told in plain language and in order. It covers what the toolkit is, the observation it was built on, why each capability exists, and what it refuses to do and why. It is written as an article rather than a manual, so that a reader who has never opened the code can follow both the engineering and the reasoning. Where a number appears it was measured, not assumed, and where the toolkit cannot measure something it says so rather than guessing. That last sentence is not a stylistic note. It is the thesis.

Part One: What Alchem-Link Is

Alchem-Link is a developer toolkit that sits at the join between two pieces of infrastructure almost every on-chain application depends on: Alchemy, which is how you reach a chain, and Chainlink, which is how you learn a price. It reads live aggregators, audits them the way a careful consumer contract would, measures how they actually behave rather than how they are documented, compares the same asset across every chain that carries it, simulates what your own contract's checks would have done in a crisis, and emits any of it as JSON, CSV, a Markdown table, or a Prometheus scrape body.

It is a Python package, a command line tool, a full-screen terminal dashboard, and a library, and it has no dependencies at all. Not "no dependencies except the user interface", which is the usual version of that claim and is the version this project made until version 0.23.0. The terminal system is in the package. So is the hash function. That decision costs real effort and buys something specific, and Part Eleven argues for it properly.

Part Two: The Observation It Was Built On

Every Chainlink integration in the world calls a function named latestRoundData. It returns a round identifier, an answer, two timestamps, and the round in which that answer was actually produced. Almost every integration reads the answer and ignores the rest.

The observation that Alchem-Link exists to act on is this: latestRoundData succeeding tells you almost nothing.

It succeeds when the feed has not published in a day. It succeeds when the round it is reporting was started but never finalised. It succeeds when the answer is a carried-over duplicate from an earlier round, which is the feed's way of saying it had nothing new to tell you. It succeeds when the price is pinned against a circuit breaker and is therefore wrong by orders of magnitude. And on a rollup it succeeds when the sequencer has been down for an hour and the price is a fossil of the moment it stopped.

Every one of those returns a well-formed number and raises nothing. There is no exception to catch, no null to check, no error code to branch on. The call worked. The data is garbage. Each of those failure modes has cost real protocols real money, and each of them is invisible to the integration that most tutorials teach.

That is the whole premise. The interesting failures in oracle consumption are not the ones where the call fails. They are the ones where the call succeeds and the answer is wrong, and finding those requires deliberately going and looking.

Part Three: The Checks a Careful Consumer Would Run

The audit command is the direct expression of Part Two. It runs, against a live feed, the checks that a well-written consumer contract performs and that most consumer contracts do not.

Some are cheap. Is the answer positive? A zero or negative price is never a real quote and is trivially detectable, yet a surprising amount of production code multiplies by it without looking. Is the update timestamp non-zero? A zero means the round started and never finalised. Does the round in which the answer was produced match the round being reported? If the produced round is lower, the feed carried an older answer forward rather than generating a new one; Chainlink's own guidance says to reject that, and almost nobody checks. Is the answer inside its heartbeat, which is to say has the feed published recently enough that the number can be trusted at all?

One check is not cheap, and it is the one worth explaining, because it is the most instructive failure mode in the whole domain.

An aggregator can be deployed with circuit breakers, a minimum and a maximum answer it is physically incapable of reporting outside. The intent is protective. The consequence, when the real price leaves that range, is that the feed keeps returning the boundary value, and it returns it fresh, well-formed, correctly timestamped, and catastrophically wrong. This is what happened when LUNA fell through the floor of its feed: the oracle went on reporting the floor as though it were the market, and the protocols reading it went on lending against a price that no longer existed. Not one of the cheap checks catches this. The answer is positive, the round is complete, the timestamp is current, nothing is carried over. Everything is fine except the number.

Seeing it requires work. The address a consumer holds is almost never the contract that holds the price; it is a proxy that forwards to whichever aggregator is current, and the bounds live one hop down on the implementation. So Alchem-Link resolves the proxy, reads the bounds off the implementation, and reports how far the current price sits from each boundary as a multiple. Modern deployments set the bounds to the extremes of the underlying integer type, giving headroom in the trillions, which is the same as having no circuit breaker at all and is reported as not binding. Older aggregators sometimes carry genuinely tight bounds, and those are the ones worth knowing about before you build on them.

The rollup check is the same shape of argument. An optimistic rollup's price feed does not stop answering when the sequencer goes down; it keeps returning the last price it had, indefinitely, with no indication that the chain underneath it has stopped. Chainlink publishes a separate uptime feed for exactly this, and a consumer is supposed to gate its price read on that feed reporting up. There is a second half to the check that almost everyone omits: when the sequencer comes back, there is a grace period before the price can be trusted, because the queued transactions that accumulated during the outage all execute at once. Reopening a protocol the instant the sequencer returns reopens it precisely into the flush it was written to survive. Alchem-Link applies the grace period.

Part Four: Measuring Instead of Copying

For several versions this project carried a table of feed heartbeats in which every entry was 3600 seconds. That number was inherited from Ethereum mainnet, where it is correct, and copied everywhere else, where it is not.

It is wrong in the dangerous direction. Polygon's price feeds publish roughly every sixty seconds. Optimism and Base publish every twelve hundred. Arbitrum's USDC feed publishes every three hundred. A staleness check written against 3600 seconds will not fire on a dead Polygon feed until it has been dead for a full hour. The check exists to warn you, and for the first hour of the emergency it is silent.

The fix was to stop copying and start measuring, and the technique is worth describing because it involves a genuine inference rather than a lookup. Chainlink publishes on either of two triggers: a heartbeat, meaning the configured maximum time between updates has elapsed, or a deviation threshold, meaning the price moved by more than some percentage. The two leave different fingerprints in the round history. Heartbeat-triggered publishes pile up against a ceiling, because the clock fires at the same interval every time. Deviation-triggered publishes arrive well inside that ceiling and at irregular spacing, because the market decides when. Walk a feed's round history, separate the intervals into those two populations, and you recover both parameters: the heartbeat is the ceiling, and the deviation threshold is bounded above by the largest price move that failed to trigger a publish.

Every heartbeat in the registry now comes from that measurement. So does the honesty constraint that goes with it, which matters more than the numbers. On a fast rollup the price may never sit still long enough for the clock to fire at all, and in that case the window contains no heartbeat-triggered publish and the measurement has not been made. The toolkit reports that as not observed and says the heartbeat is at least the longest gap seen, rather than inventing a value from wherever the sampling window happened to end. Those entries are marked in the registry as bounds rather than measurements, and a bound produces a staleness verdict that fires later than a real measurement would. A user is told that, because a conservative check presented as a precise one is a lie by omission.

A related discipline governs the feed registry itself. Every address in it was called for its own description before being written down, and is filed under the pair the contract reports rather than the pair it is popularly known by. That check keeps earning its place. The address widely circulated as Base BTC/USD reports WBTC/USD, which is a wrapper that can depeg from what it wraps; it is registered under its real name with a note explaining the difference. The Gnosis address commonly labelled xDAI/USD reports DAI/USD. Two candidate CCIP routers turned out to have no code deployed at all and were dropped. The verify command re-runs the whole check against live chains, so the registry and reality can be compared on demand rather than trusted.

Part Five: Reading Chains Cheaply, and the Hash Underneath

Reading one feed properly means three contract calls: the round data, the decimals, and the description. Reading a network's worth of feeds means dozens. Done one HTTP request at a time against a public endpoint at a few hundred milliseconds each, that is twenty seconds of waiting, which is the difference between a tool people use and a tool people mean to use.

Alchem-Link collapses it three ways, preferring the best available. Multicall3 is a contract deployed at the same address on essentially every chain that executes many sub-calls inside a single invocation; where it is present, dozens of reads become one request. Where it is not, JSON-RPC batching sends an array of requests in one POST, which is still one round trip. Below that, sequential requests, which always work. Ethereum's sixteen feeds went from forty-eight round trips and about twenty seconds to two round trips and six hundred milliseconds.

The tier matters beyond speed, and the toolkit reports which one it used. Only Multicall3 executes everything inside one block. Comparing two feeds read a round trip apart is comparing two different moments, and for divergence analysis that timing noise is the same order of magnitude as the signal being measured. A number is qualified by how it was obtained or it is not qualified at all.

Underneath all of that sits a decision that looks like a detour and is not. Encoding a contract call requires a function selector, which is the first four bytes of the Keccak-256 hash of the function signature. Python's standard library ships SHA3-256, which is not Keccak-256: the standards body changed a single padding byte between the original Keccak submission and the final SHA-3 standard, and Ethereum froze on the original. That one byte is why so much Ethereum tooling in Python reaches for a native extension.

Rather than take the dependency or hardcode a handful of hashes, the project implements Keccak-256 in about a hundred lines of integer arithmetic. Selectors are therefore computed rather than trusted, pinned in tests against the published standard vectors and against the four constants the package had previously shipped as hand-verified values. That unlocked a full ABI codec, which unlocked Multicall3, whose aggregate function takes a tuple array that nothing simpler can encode. A hundred lines of hash function is the load-bearing member under the batching, the log decoding, and the code generation.

Part Six: From Auditing a Feed to Auditing Your Own Defences

Everything to this point describes a feed. Version 0.23.0 adds the question that actually decides whether a protocol survives, which is a question about the reader rather than the source: given the checks your contract performs, what would have happened?

The simulate command answers it. You describe your consumer's guards once, the staleness window, whether you reject carried rounds, your own sanity bounds on the price, whether you gate on the sequencer, whether you cap how far a single update may move the price, and the toolkit replays those guards against a set of scenarios drawn from the failure modes that have already cost people money. A feed pinned to its circuit breaker. A feed that has silently stopped publishing. A rollup sequencer going down and coming back inside its grace period. Rounds that finalise without producing a fresh answer. A single round that prints forty percent away and reverts on the next one. A round timestamped in the future.

The result is a table of which failure modes get through, and the default answer is uncomfortable in a useful way. The guard most integrations actually have, a staleness window and a positivity check, handles four of the eight scenarios. It misses the pinned circuit breaker entirely, because every reading after the pin is fresh, positive, complete, and orders of magnitude wrong, and there is nothing about it for a staleness check to notice. It misses the sequencer outage, because the price feed answers throughout. It misses carried rounds and it misses the flash spike. Turning on every guard handles all eight. Turning them all off, the bare latestRoundData integration, handles two.

Two design details make that table trustworthy rather than merely alarming.

The first is that the set contains a healthy scenario, a well-behaved feed publishing on schedule, which a correct guard must accept in full. Without it, a guard that rejected everything would score perfectly on every other scenario, and the whole exercise would reward paranoia instead of judgement.

The second is the complementary command. Scoring well against invented disasters says nothing about whether your guards are usable, so backtest replays the same guards against a feed's real round history, where every rejection is a false positive rather than a catch. A guard that handles all eight scenarios and rejects a third of real history is not a guard anyone can ship; it is a protocol that halts for forty minutes at a time. The two commands run in opposite directions on purpose, and a guard is only good when it passes both.

There is a smaller detail in the replay that is worth mentioning because it is the kind of thing that quietly invalidates a simulation. When the move limit compares an incoming price against the previous one, the previous one is the last price the guard accepted, not the last price it saw. That is what a real consumer does, because a rejected round never gets stored. Comparing against the last observed value instead would let a spike become the baseline for the recovery and produce a rejection pattern no contract would ever exhibit.

Part Seven: Statistics That Account for How Oracles Actually Publish

Version 0.23.0 also computes statistics over a feed's history, and two of them are calculated differently from the obvious implementation for reasons specific to oracles.

The first is the time-weighted average price. The naive version averages the published answers. That is wrong here, and it is wrong in a direction nobody would guess without thinking about the publish trigger. An oracle publishes on a heartbeat or on a deviation, which means it publishes most frequently exactly when the price is moving fastest. The set of published answers is therefore not a fair sample of the price over time; it is systematically over-weighted toward volatile periods.

A worked example makes the size of the effect concrete. Take a feed that sits at 1,900 for fifty minutes, printing twice because nothing is happening, and then walks to 1,950 across six one-minute rounds because the price started moving. The mean of those seven answers is 1,921. The time-weighted mean is 1,902. The flat stretch occupied ninety percent of the window and contributed two of the seven prints, and the difference between those two numbers is entirely that discrepancy. The second figure is what a time-weighted oracle would have reported and is the one that describes where the price actually was; the last print, 1,950, sits 253 basis points above it. So each observation is weighted by how long it stood, not by the fact of having been printed.

That last comparison is worth surfacing on its own, and the toolkit does. The gap between the current answer and the window's time-weighted average is precisely the exposure that a protocol pricing off spot carries and a protocol pricing off a TWAP does not.

The second is volatility. Annualising a standard deviation requires knowing the sampling interval, and assuming one is how the same asset comes to report wildly different volatility on two chains: Polygon's sixty-second publishes and Ethereum's hourly publishes describe the same market, and a fixed assumed interval will scale them by a factor of sixty against each other. The interval is derived from the timestamps of the series being measured, taken as a median so one long quiet gap cannot drag it.

Both of these are examples of the same idea. An oracle is not a price sensor sampling at regular intervals; it is an event stream with its own trigger logic, and a statistic that ignores the trigger logic measures the publishing policy as much as the market.

The history itself can be obtained two ways, and version 0.23.0 adds the cheaper one. Walking rounds backwards from the latest costs one contract call per round. Reading the aggregator's own update events costs a single log query for the whole window. There is a catch worth knowing, and it is the sort of thing that makes a feature look broken when it is merely subtle: the proxy address a consumer holds emits no events at all, because the events come from the implementation underneath. Filtering on the address you know returns an empty list and looks exactly like a dead feed. The toolkit resolves the proxy first.

Part Eight: One Object Per Session

The functional interface, a module-level function per capability each taking a network argument, is the right shape for a script that reads one price. It is the wrong shape for anything longer, because each call builds its own connection, performs its own probe for whether Multicall3 is available on this chain, and accumulates its own statistics that add up to nothing you can report.

So version 0.23.0 adds a session object that holds the network, the connection and a cache. Five reads become one client, one probe, and one set of statistics that can answer what the session actually cost in round trips.

The cache is where a generic implementation would go wrong, and the fix is available only because of Part Four. A fixed time-to-live is either far too long or far too short depending on the feed: five seconds wastes six hundred redundant reads against an hourly mainnet feed, and two minutes serves a stale price from a Polygon feed that has published twice in the meantime. The correct time-to-live is per-feed, and the toolkit already knows it, because the measured heartbeat is in the registry. Each feed caches for a fraction of its own publish interval. Measuring the heartbeats was done to make staleness verdicts honest; it turned out to also be the thing that makes caching correct.

The session adds one small piece of API judgement worth recording. Reading a price returns a reading that reports whether it is stale, and the caller is expected to check. That is the right default for interactive use, where you want the number and the verdict together. It is the wrong default for a contract-facing path, where forgetting to check is the entire bug, so a strict mode raises an exception instead of returning a reading that quietly says do not use me.

Part Nine: The Terminal System

Until version 0.23.0 the dashboard was built on a third-party terminal framework, and the package described itself as having no dependencies with an asterisk: the user interface was an optional extra. That asterisk was doing real work. Anybody who ran the dashboard installed a framework and everything it pulls in, and the honest description of the package was that it had no dependencies except for the part most people would actually look at.

Version 0.23.0 removes the asterisk by writing the terminal system. It is six modules in a strictly layered stack. One knows about escape sequences and colour, one about a grid of cells, one about keyboard input, one about rectangles and widgets, one about events, and one about the process. Nothing in it imports anything from the rest of the package except the colour palette, which is an inert table.

Three problems in that stack are worth describing, because each is invisible until it is catastrophic and each is the kind of thing a framework normally hides.

The first is colour. A terminal that speaks only sixteen colours does not gracefully degrade when handed a twenty-four-bit colour instruction; it prints the digits as literal text across the screen. So the depth is negotiated per output stream and the palette is converted down to whatever is available: twenty-four bit, then the two-hundred-fifty-six colour cube, then the basic sixteen, then none at all. There is a specific trap in the conversion to two hundred and fifty-six. The obvious implementation maps a colour to the nearest entry in the colour cube, which turns every near-black surface into pure black, and the interface loses every edge and border in one step. The greyscale ramp has to be a candidate alongside the cube, and the nearer of the two wins.

The second is Windows. A Windows console ignores escape sequences entirely until a specific flag is set on its output handle through the Win32 API. Without that call the entire interface renders as visible escape-sequence garbage. The call is made first, and whether it succeeded is reported, so the program can fall back rather than paint noise.

The third is redraw cost. Repainting a full screen each frame is what makes terminal interfaces flicker, and over a remote connection it streams tens of kilobytes for every update. The screen is therefore double-buffered: widgets paint into a back buffer, and the renderer compares that against what is actually on the terminal and emits only the runs that changed, with one cursor movement per run. A frame in which nothing moved costs zero bytes. That is the difference between a dashboard that is usable over a slow link and one that is not.

Part Ten: Black and Blue Before the First Frame

There is a detail in the terminal system that deserves its own section because it is where the aesthetic argument and the engineering argument turn out to be the same argument.

Drawing dark rectangles gets you a dark pane. It does not get you a dark terminal. The columns past the last painted cell, the scrollback above where the program started, and anything a subprocess writes all remain whatever colour the terminal already was, and the result is a themed window sitting inside somebody else's colours. The fix is to repaint the terminal's own defaults, its background, its foreground and its cursor, through the escape sequences that exist for exactly that purpose. Then everything, including the plain text output of a single command that draws no interface at all, sits on the product's surface.

That initialisation runs from every entry point: the command line tool, the interactive console, and the compiled binary. The compiled binary is the case that matters most and is the reason the feature exists in the form it does. A binary launched by double-click lands in a brand-new console with default colours and no environment hints about what it supports, which is precisely where colour detection has the least to go on and where a program is most likely to give up and render plain. It would be the one place the product does not look like itself, so the frozen case is detected and themed anyway.

The obligation that comes with this is absolute and is easy to get wrong: a program that repaints somebody's terminal and exits without undoing it has broken their terminal, not themed it. The restore is wired to normal exit, to the signal handlers, and to the interpreter's shutdown hook, and it is written to tolerate being called twice and to never raise, because it runs during unwinding from exceptions and a restore that fails would replace a clean error message with a traceback about the terminal.

Underneath both halves is a single palette module, and the rule it enforces is that render code names a role and never a colour. There is one file where a colour is a hexadecimal string, and it contains no escape sequences at all; the encoder turns a role into bytes for whatever depth was negotiated, the line-output layer uses the same roles, and the web build of the same toolkit mirrors the same values. The test suite fails the build if a render module hardcodes a colour, if a role paints on an undefined surface, or if the three status colours collapse into one another once quantised down to sixteen. That last check is the interesting one: a palette tweak that looks harmless at full colour can make a stale-feed warning indistinguishable from a healthy one on a plain console, which is a correctness bug wearing a design costume.

Part Eleven: What Zero Dependencies Actually Bought

It is fair to ask whether any of Part Nine was worth doing. Writing a terminal toolkit to avoid installing one is, on its face, the kind of decision that looks like pride.

The argument has three parts, and only the third is really about dependencies.

The first is that the claim was load-bearing. This is a security-adjacent tool. It tells people whether a price feed can be trusted, and it is the sort of thing that gets run in a continuous integration pipeline against production configuration. A package with no dependencies has a supply chain consisting of the standard library, which is a genuinely different risk posture from one that pulls a rendering framework and its transitive tree. Making that claim true without an asterisk is worth more here than it would be in most projects.

The second is that the binary got simpler. There is nothing to collect, no data files to bundle, no hidden imports to chase, and the packaging specification is a dozen lines. A single-file executable that runs on a machine with no Python at all is now a build step rather than a project.

The third is the one that was not anticipated and is the strongest. Writing the interface forced a shape that made it testable. Because there was no widget tree to inherit, the panels were written as pure functions returning a list of styled text lines, with the application painting a window onto that list. Scrolling and clipping became one slice of one list. And every panel became testable without a terminal, which means all fifteen of them are exercised in their loading, empty and error states in the test suite. Those three states are where terminal interfaces crash, because the happy path never produces the null that the error path does, and a crash in a full-screen interface takes the whole screen with it. That coverage exists because of the constraint, not despite it.

Part Twelve: The Refusals

A toolkit is defined as much by what it declines to do, and Alchem-Link has four standing refusals that are worth stating together because they come from the same place.

It will not invent a measurement. When the sampling window contains no heartbeat-triggered publish, the heartbeat is reported as not observed with a lower bound, and the registry entry is marked as a bound rather than a measurement. The temptation is to report the largest observed gap as though it were the answer, which would look more complete and be false.

It will not simulate a price. The rest of the wider project this toolkit lives in has simulation modes for its dashboards, which is reasonable there. Here there is no such branch anywhere: a route reads a chain or it reports the error, and an unreadable feed renders as a failure row rather than being quietly dropped from the list. A fabricated price would defeat the entire purpose of a staleness verdict, and a feed that silently disappears from a table is the same lie in a more polite form.

It will not colour the meaning. Every piece of output produces identical text with colour switched off, and the test suite asserts the two are character-for-character the same. This output goes into continuous integration logs, issue reports and pipes at least as often as it goes to a screen, and layout that only survives with escape sequences in it breaks exactly where it is hardest to debug.

And it will not test against a live chain. The entire suite, four hundred and seventy-nine cases, is offline. That is a design constraint rather than a convenience, and it is the one most likely to be mistaken for laziness. The statistics and simulation modules compute numbers that people size positions and write guards against. A number that can only be verified by reading a live chain cannot be verified at all, because the chain will have moved by the time anybody checks. Fixtures make the arithmetic checkable; a live integration test makes it merely plausible.

Part Thirteen: The Philosophy That Ties It Together

Step back and the same instinct appears in every part of the toolkit.

The first thread is that success is not evidence. The founding observation is that the oracle call succeeds when everything is wrong, and the pattern recurs at every level: a registry entry that looks right until you ask the contract its own name, a heartbeat that looks measured until you ask whether the clock ever fired, a guard that looks strong until you ask what it would have caught, a colour scheme that looks fine until you quantise it. The work is always in going and asking rather than accepting the absence of an error.

The second thread is measure, then use the measurement twice. The heartbeats were measured to make staleness honest, and turned out to be what makes caching correct. The hash function was written to avoid a dependency, and turned out to be what makes Multicall3 batching and log decoding possible. The pure-function panel design was adopted to avoid a framework, and turned out to be what makes the interface testable. Good measurements and honest primitives keep paying out in places nobody was aiming at.

The third thread is that the tool must not become another thing to distrust. Hence the refusals in Part Twelve, hence colour that carries no meaning, hence a staleness tolerance so feeds do not flicker a warning every cycle and train people to ignore it, hence marking a bound as a bound. A diagnostic tool that overstates its own certainty is worse than no tool, because it converts a known unknown into an unknown one.

The fourth thread is that the interesting question is usually one level up. Reading a price is easy; auditing the feed is harder and more useful. Auditing the feed is good; replaying your own defences against it is harder and more useful still. Each version of this project has moved the question from the data to the reader of the data, and version 0.23.0 is where that arrived at its natural destination: not is this feed healthy, but would my code have noticed if it were not.

Conclusion

Alchem-Link began from one sentence about a function call that succeeds when it should not, and every capability in it is downstream of taking that sentence seriously. The audit exists because the failures return successfully. The measured heartbeats exist because a copied constant makes the audit silent for the first hour of an emergency. The batching exists because an honest read of a whole network is otherwise too slow to do. The hash function exists because the batching needs it. The simulator exists because auditing the feed only tells you half of what determines whether you get hurt. The terminal system exists because the claim of having no dependencies was worth making without an asterisk, and it turned out to buy a testable interface as well.

What it will not do is guess. It reports what it measured, marks what it could only bound, refuses to fabricate what it could not read, and fails a build if its own colours stop meaning what they say. That is a small kind of integrity for a piece of software to have. In a domain whose defining hazard is a well-formed number that happens to be wrong, it is the only kind that matters.
