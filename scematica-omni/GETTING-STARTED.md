# Getting started with Scematica Omni

The README explains *why* this runtime is built the way it is, and it is long because the
design is the product. This page is the other thing: the shortest path from nothing to
understanding what you are looking at.

Fifteen minutes. You need Rust and a directory of code you already have.

```console
$ cargo install scema-cli
$ cd ~/some-project
$ scema quickstart
```

`quickstart` walks the loop over your own code and explains each stage as it appears. It
writes nothing. If you only do one thing on this page, do that one.

---

## The four words

Almost every "wait, is it broken?" moment with this tool is one of four words you do not
have yet. They are worth two minutes because each of them is a place where the *correct*
output looks like a malfunction.

### 1. A signal is a count

```
SIGNALS
  OPPORTUNITY  counted      0.22  11 marker(s) in `scema-tools`
                                  └ counted 11 marker(s)
```

`counted` means something was actually tallied, and the line underneath says what. Omni
does not have opinions about your code; it has counts, and everything downstream is built
out of them.

A signal that says `estimated` instead is a guess, and the runtime treats it very
differently — an estimate cannot produce a measured expected gain, so branches built on one
score at or below zero.

### 2. An em dash is not a zero

```
  #   BRANCH                             GAIN    RISK    COST  UNCERT  REVERS   UTILITY  MEASURED
   2  fix the flaky tests                   —    0.40    0.10    0.00    0.70    -0.095       4/5
```

`—` means **that term was not measured**. It is not zero, not "no gain", and not "we
checked and found nothing". Nobody looked, or nobody could.

`0.00` in the same column would be a real observation: something was measured and it came
out at zero.

**Read `MEASURED` before you read `UTILITY`.** A utility of `0.91` computed over two terms
out of nine is a statement about ignorance. The column is there so you cannot miss it.

### 3. Grounding is asserted, never inferred

This is the one that catches everybody.

```console
$ scema simulate "fix the flaky tests"
```

Your goal shows up as a branch with `—` for expected gain and a negative utility, and
something else gets chosen — or the agent abstains entirely. That is not the tool refusing
to help. Nothing it observed supports what you asked for, and **an instruction is not
evidence**.

You link the two yourself, by signal id:

```console
$ scema observe .                                  # lists the ids
$ scema simulate "fix the flaky tests" --ground untested:scema-cli
```

Omni will never guess this link. An earlier version did, by keyword overlap, and its first
run grounded "add tests to the scema-cli crate" in an unrelated crate's backlog because
`scema` is a substring of every unit name in this repository. The branch inherited a
measured gain from evidence that had nothing to do with it. That is the exact laundering the
whole design exists to prevent, so the inference was removed and will not come back.

Since v0.5.0 the tool tells you this when it happens, and lists the ids you could use.

### 4. Abstention is an answer

```
ABSTAINED  the best branch scores -0.095; acting is worse than not acting
```

`scema decide` exits **0** when it abstains. It did its job — the job's answer was "don't".

There are five distinct reasons and *which one* is the actionable part:

| Reason | What it means |
|---|---|
| `NoCandidates` | nothing was proposed at all |
| `AllForbidden` | your own `--must-not` excluded everything |
| `NoPositiveUtility` | acting scores worse than not acting |
| `TooLittleMeasured` | too little was observed to rank on — about the observation, not the branches |
| `Contested` | a qualified specialist vetoed the top branch |

The `NEXT` block under an abstention names the command for the one you hit.

---

## The commands, in the order you will want them

```console
scema quickstart              # start here — the loop, narrated, writes nothing
scema observe .               # the world on its own, with every signal id
scema simulate "<goal>"       # rank branches. never writes
scema decide "<goal>"         # same computation, but seals a record
scema explain --list          # what has been sealed
scema verify --all            # re-check every record
scema policy                  # the weights, and which specialists are loaded
scema doctor                  # what is installed, wired, or quietly broken
```

`simulate` and `decide` compute **exactly** the same thing. The only difference is that
`decide` leaves a trace. That is why they are two commands and not a flag.

---

## What a decision record proves

`scema decide` seals a JSON file under `.scema/decisions/`. It contains the world that was
observed (blind spots and unreadable things included), the branches, the projections, the
weights, the outcome, and six SHA-256 digests binding all of it.

```console
$ scema verify 234e11a0
234e11a0  INVALID
    projections  committed 93a8fc3e1f51…  recomputed c723b4db7b08…
    root         committed 234e11a0cd3c…  recomputed 83df34f1d80f…
```

Be precise about what that buys you:

- **It proves** the record was not edited after sealing, and names the field that moved.
- **It does not prove** the world was really like that. An observer that misread your
  project produces a perfectly verifiable record of a wrong observation. *Provenance*
  carries that, which is why the world is committed whole.
- **It does not prove** this is the original record. Anybody holding the file can regenerate
  both it and its commitment. This is tamper-**evident** to a third party holding an earlier
  copy, not tamper-proof — until the root is anchored somewhere the author does not control.

You can also check a record with no tooling at all: drop it into `/omni` on the web
dashboard, which reads and hashes it in your own browser and sends it nowhere.

---

## Beyond a source tree

A repository is one kind of world. So is a running system, a set of oracle feeds, and a web
page — they are all `WorldState`, and nothing above perception can tell which it was
looking at.

```console
$ mesh-dashboard --world | scema simulate "keep the pipeline honest" --path -
$ alchem-link omni -n base | scema simulate "price safely" --path -
```

If you want omni to perceive something it has never heard of, you do not need to change
omni. You write a producer that emits the same JSON, in whatever language that thing already
lives in, and pipe it in. See **[docs/PRODUCERS.md](docs/PRODUCERS.md)** — start with
`scema check --vocabulary`.

---

## Wiring it into an assistant

```console
$ scema connect --list
$ scema connect claude-code --write
```

`--write` touches project-local files only. User-level configs (Claude Desktop, Windsurf,
Zed, Codex) are printed with their path and never written: a user config is shared by every
project you open, and editing it on your behalf would mean a tool installed for one
repository quietly gaining the ability to observe all of them.

---

## Things that are deliberately not built

Worth knowing before you plan around them.

- **Nothing here writes to an environment it observed.** `execute`, `delegate`, `discover`
  and `pay` are registered verbs that exit non-zero and say what is missing. They are in
  `--help` on purpose — a verb that silently did not exist would be indistinguishable from
  one that failed.
- **No model-backed hypothesiser.** A model proposing branches is fine in principle and the
  slot exists for it, but its prompt, model id and raw output have to be committed into the
  record or verifiability is gone.
- **Firefox.** The browser extension is Chrome/Chromium MV3 today.

## Where to go next

- **[README.md](README.md)** — the design, and the six decisions behind it.
- **[docs/PRODUCERS.md](docs/PRODUCERS.md)** — teach omni to perceive something new.
- **[CHANGELOG.md](CHANGELOG.md)** — what changed in 0.5.0, and what is stable for beta.
