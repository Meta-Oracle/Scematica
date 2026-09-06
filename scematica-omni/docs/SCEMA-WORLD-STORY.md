# Scema-World — the campaign

A story for a game whose map is a sealed observation of somewhere real.

## The rule this whole document is written under

`ship.ts`, `raiders.ts`, `factions.ts`, `respawn.ts`, `claim.ts`, `roles.ts` and `quests.ts` all
carry the same invariant, and `check:scemaworld` asserts it by source scan:

> **No quantity in the record may translate into a reward.**

Attach a payout to `blind_spots` and you have paid somebody to hide them. Attach one to signal
magnitude and you have paid them to understate it. A producer with an incentive to misreport is the
one failure this project cannot absorb.

A story is not a reward, and this is the distinction the campaign is built on:

> **The record may shape what you are told. It may never shape what you are paid.**

A world with more blind spots is worth exactly the same salvage as one with fewer. It is a
*different story*. That asymmetry is not a limitation to work around — it is the most interesting
thing available here, because it means the narrative can read the record as deeply as it likes
while the economy stays incorruptible. Every beat below obeys it, and the section at the end says
what would break it.

---

## Premise

You do not fly *a place*. You fly **somebody's act of perceiving a place**, frozen and sealed.

Every world is a decision record: an observer looked at something, wrote down what they saw, said
plainly what they could not see, and sealed the result so nobody could edit it afterwards. The
fractal you fly through is that record's commitment made into a volume. The stations are objects
they perceived. The derelicts are things that were there and stopped answering. The markers are
places they expected something and found nothing.

And the rifts — the lanes that end in nothing — are where **the observer did not look**.

You are a pilot licensed to fly inside sealed observations. That is a real job in this world,
because an observation is the only thing anybody trusts, and somebody has to go and check.

---

## The three institutions

### The Assay — *marshals, yellow, `CITADEL I–III`*

They seal records and they certify pilots. Their whole authority rests on one narrow, honest claim:
**we can prove a record has not been edited since it was sealed.** Their citadels are ringed because
the rings are the tiers of certification — a three-ring seat can seal a record, a one-ring outpost
can only witness one.

They are not villains and the campaign must never let them become simple ones. They are an
institution doing something genuinely valuable and quietly overreaching, because everybody has
started treating "the Assay certified it" as meaning *true* when it only ever meant *unedited*.

### The Freeholds — *raiders, orange, `FREEHOLD I–III`*

Everyone flying on observations the Assay will not certify. That covers a spectrum the game should
never flatten: outright forgers at one end, and at the other, observers whose records were refused
certification because what they saw was inconvenient.

A freehold takes a smuggler's business and its wings still rob them. That is not a contradiction; it
is what a place with no adjudicator looks like.

### The Couriers' Compact — *couriers and freighters, blue*

Neutral carriage. They move sealed records between Assay seats, which makes them the circulatory
system of the entire trust economy and the reason a courier is worth robbing. They do not fight and
they do not take sides, and the campaign's quietest tragedy is what happens to them in Act IV.

---

## The mystery

`scema verify` proves exactly one thing, and the documentation has always been careful about the
two things it does not:

1. It proves the record was **not edited after sealing**.
2. It does **not** prove the world was as described — provenance carries that.
3. It does **not** prove this is the **original** record — it is tamper-*evident*, not tamper-*proof*.

Gaps 2 and 3 are the plot.

**Somebody is sealing records of worlds nobody observed.** The forgeries verify perfectly, because
they were never edited — they were false the moment they were written. The Assay's instruments say
`VERIFIED` and the instruments are not wrong; they are answering a different question from the one
everyone is asking.

### The tell, and why it is beautiful

A forged record is not detectable by verification. It is detectable by **honesty about ignorance**.

A real observer reports blind spots, because nobody sees everything. Their signals carry
`measured: false` where they estimated. Their extent says `total: null` where the walk was capped.
Their sensor range is `null` when nothing was perceived — *unknown*, not zero.

A forger writes a world they never visited, and a world you invent has **nothing in it you failed to
see**. Every signal measured. No blind spots. A boundary that is known because you drew it.

> **A record that claims to know everything is the one to distrust.**

In the game this is immediate and visual. A forged sector has **no rifts**. Its contacts are all
solid — no ghosts. Its sensor range is a confident number. It is the most comfortable place you have
ever flown, and that is what is wrong with it.

The em-dash rule, which exists in this codebase so an unmeasured quantity can never masquerade as a
measurement, becomes the detective's tool: **you learn to trust the sectors that admit what they do
not know.**

---

## Five acts

### Act I — *The frozen sky*

You fly your first record. The tutorial is diegetic: an Assay outpost teaches you to read a sector
as an observation rather than a place.

- A rift is not a hazard, it is an **absence of testimony**. Fly into one and the HUD tells you the
  lane ends because nobody looked, not because something is blocking it.
- A ghost contact reads on sensors and may not be there. You are told, once, plainly, that this is
  not a bug.
- A phantom station offers no services because it was **modelled, not seen**. The first time a
  player routes to one and it refuses to refuel them is the lesson landing.

**Beat:** your instructor's own sealed record is the first map you fly. It has four blind spots and
she is not embarrassed about them.

### Act II — *Two true maps of one place*

You are hired to carry a sealed record between Assay seats. It verifies. So does another record of
the *same subject*, sealed a week earlier, carried by somebody else.

They describe different worlds.

Both verify. Neither was edited. The Assay's instruments have nothing to say about which is true,
and this is the act where the player understands that verification was never truth.

**Beat:** you can carry both to a three-ring citadel and watch an adjudicator fail to choose.

### Act III — *The comfortable sector*

You are sent into a world with no rifts.

Everything is measured. Nothing is estimated. The sector is legible to its edges and the sensor
board is full of solid, confident contacts. It is the safest sector in the game and every instinct
the first two acts trained says it is wrong.

**Beat:** the ghosts here are real. In an honest record a ghost is a signal somebody estimated; in
this one, the ghosts are things the forger inserted and never had to estimate, because they wrote
them. They are solid, and they are hostile, and they were not there when the record was sealed.

This is the act that turns the game's central epistemic joke — *the things the record described are
the uncertain ones* — inside out.

### Act IV — *Provenance is a chain of who said so*

The forger is not a pirate lord. It is a **workshop**: a small operation producing records to order,
for clients who want a place to be a particular way on paper.

The Assay knows. Some of them have known for a long time. Certification is a business, and a
certifier who refuses too many records loses the seat.

**Beat:** the Couriers' Compact is what breaks. They carried everything, neutrally, and it turns out
neutral carriage of a forgery is how a forgery becomes consensus. Couriers start being shot at by
both sides — and the player, who has spent the whole game watching blue ships go quietly about their
business, notices the sector getting quieter.

### Act V — *What you do about it*

There is no ending where the truth wins cleanly, because the tool cannot deliver that. The best
available outcome is **a chain of custody people can actually inspect** — anchoring roots somewhere
the author does not control, which is what `scema anchor` is for and what the record format has been
building toward the whole time.

Each role ends differently. None of them ends with the problem solved.

---

## Four roles, four campaigns

The role is chosen before a world is flown and decides who shoots at you (`roles.ts`). It should
decide the story just as completely. Same acts, same events, four irreconcilable readings.

### Bounty hunter — *the enforcement arc*

You work Assay contracts. Your arc is the most comfortable and the most compromised: you are the
instrument that makes certification enforceable, and in Act IV you are asked to bring in an observer
whose only crime was refusing to seal a record they had been paid to seal.

**Ends:** you can take the contract or refuse it. Refusing costs you the Assay's citadels — every
board in the game closes to you except the freeholds, and the game does not tell you in advance that
this is what refusal means.

### Trader — *the circulation arc*

You are the Compact. You carry sealed records and you do not read them.

Your arc is about discovering that neutrality is a position. Every forgery in the sector reached its
destination in somebody's hold, and some of them were in yours.

**Ends:** you can keep carrying, or you can start reading. Reading makes you a smuggler.

### Smuggler — *the testimony arc*

You carry the records the Assay will not certify. Some are forgeries. Some are the only surviving
account of something that happened, written by somebody with no seat and no seal.

You cannot tell which from the outside — that is the whole point of the role, and the reason it is
the only role hunted by both sides. The contraband mechanic is already exactly this: a live run
makes the patrol hostile *while you are carrying*.

**Ends:** the strongest ending in the game. You can deliver an uncertified record to a three-ring
citadel and force it into the chain of custody — where it can be argued with, which is the most any
record can hope for.

### Pirate — *the refusal arc*

You prey on the Assay because certification is a monopoly on what counts as true, and you are not
interested in reforming it.

Your arc is the one that gets to be right for the longest and costs the most. In Act IV you can
break the workshop yourself — and destroying the forger's seals destroys the only evidence anybody
could have used to prove the forgeries were forgeries.

**Ends:** the sector is freer and less knowable. The game does not tell you this is bad.

---

## How the existing mechanics become story

Nothing below needs a new system. Every one of these is a thing the game already simulates, given a
reason.

| Mechanic | Story it already tells |
|---|---|
| **Rift** (one per blind spot, a count) | Where the observer did not look. Act III's horror is a sector with none. |
| **Ghost contact** (`measured: false`) | A signal somebody estimated. In a forgery, the ghosts are real — inserted, never estimated. |
| **Phantom** (`Simulated`) | Something modelled, never seen. A station that refuses to refuel you is the epistemics biting. |
| **Derelict** (`Stale`) | It was there and stopped answering. Every derelict is a small unwritten story. |
| **Marker** (`Absent`) | The observer expected something and found nothing. The most unsettling node in the game. |
| **Sensor range = legibility** | An illegible record is *literally dark*. Flying a badly-observed world is the felt cost of bad observation. |
| **`extent.total === null`** | A volume with no known boundary. Act I's instructor flies one and is fine with it. |
| **Citadel rings = tiers** | Certification authority, visible from across a sector. |
| **Contraband run** | Uncertified testimony. The patrol scans for it because that is what the patrol is *for*. |
| **`scema verify` in `/omni`** | The player can verify the records the story hands them, in the browser, offline. **The plot's central instrument is a real tool that really works.** |

That last row is the thing worth protecting. The game's story is about the limits of verification,
and the player can *actually run the verifier*. The fiction and the software make the same claim.

---

## What the campaign must never do

Each of these is a specific way to break something the project has already paid for.

1. **Never pay for record content.** No bounty, salvage, contract reward or unlock may read
   `blind_spots`, `signals`, `magnitude`, `legibility` or `extent`. A story beat may. A payout may
   not. The source scan in `check:scemaworld` is what enforces this and the campaign's modules must
   be added to it.
2. **Never make a forged record profitable.** Act III must cost more than it pays. If "no rifts"
   ever becomes the sector players farm, the campaign has recreated the exact incentive the whole
   design exists to remove — a producer with a reason to claim they saw everything.
3. **Never let a ghost quietly become solid outside Act III.** The em-dash rule is the spine. Act III
   works *because* it is the one violation, it is deliberate, and it is the antagonist's signature.
4. **Never resolve the two gaps `verify` leaves open.** No story beat may let the player prove a
   record is *true*, or prove it is the *original*, because the tool cannot and the fiction must not
   claim otherwise. Anchoring is the honest partial answer and it is Act V's whole subject.
5. **Never make the Assay simply wrong or the Freeholds simply right.** The Assay's narrow claim is
   correct and valuable. The forgeries are real. Both stay true.
6. **No cutscene may take the ship's controls.** The sector keeps simulating. This game's texture is
   that things happen whether or not you are watching — `respawn.ts` and the marshals hunting
   raiders with nobody there to see it. A story that pauses the world contradicts its own setting.

---

## Implementation sketch

Not built. This is the shape it would take, smallest first, so it can land in pieces that each work
alone.

- **`lib/scemaworld/story.ts`** — pure. Acts, beats, and the predicate for each. Reads the record
  freely; returns text and flags, never numbers that reach a wallet. Added to the reward-path source
  scan with an inverted assertion: it *may* read record fields, and must export nothing a payout
  path imports.
- **Beat state on `GameState`**, beside `quests`, persisted the way the account is (`wallet.ts`) so a
  campaign survives the tab. Per-record, unlike the wallet — the story is about *that* world.
- **A logbook panel**, sharing the market's tab bar. The place beats accumulate, readable at any
  time. No modal, no pause.
- **Act III as a generated world** rather than a hand-authored one: a `WorldState` with zero blind
  spots, every signal measured, and a fixed digest — shipped as a fixture so it is the same
  suspicious sector for everybody, and so `check:scemaworld` can assert it verifies while being
  false. That fixture is the campaign's keystone and the one artefact worth building first.
- **Dialogue at citadels**, gated by tier: a one-ring outpost cannot advance a beat that needs a
  seal. The rings already say who can do what; the story should mean it.

Estimated shape: `story.ts` and the fixture are most of the value and are independently testable
without a GPU, in the same way `quests.ts` is.
