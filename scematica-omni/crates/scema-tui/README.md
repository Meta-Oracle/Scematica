# scema-tui

The Scematica Omni console: the agent loop as a full-screen terminal application. Perceive
a world, tick the signals that ground a goal, watch branches compete, read back the records
that were sealed and whether their commitments still hold.

```console
$ cargo install scema-tui
$ scema-tui                    # the current directory
$ scema-tui /path/to/project
$ scema-tui --once             # one pass as plain text, pipeable
$ scema-tui --snapshot 120x40  # one frame as text
$ scema-tui --palette          # what colour this terminal can actually carry
```

If you have `scema-cli`, `scema tui` finds this binary next to itself and hands over.

## Five tabs

| | Answers |
|---|---|
| **WORLD** | what is out there, and what could not be seen? |
| **SIMULATE** | which branch wins, and does anything measured support it? |
| **RECORDS** | what was decided, and does the record still verify? |
| **MEMORY** | what has been retained, and how well did it project? |
| **POLICY** | under whose preferences, and with which specialists? |

```
  1·WORLD │ 2·SIMULATE │ 3·RECORDS │ 4·MEMORY │ 5·POLICY │        SCEMA OMNI scema-omni/0.1.0
  which branch wins, and does anything measured support it?                         ○ idle  .
────────────────────────────────────────────────────────────────────────────────────────────
┌ SIMULATION MATRIX  ·  NOT WRITTEN (would seal as c5ac7c5e) ───────────────────────────────┐
│ #   BRANCH                                GAIN   RISK   COST UNCERT REVERS  UTILITY  MEAS │
│▸  1 take: 11 marker(s) in `scema-tools`   0.22   0.40   0.10   0.00   0.70    0.125 ▰▰▰▰▰ │
│   2 give scema-cli tests                     —   0.40   0.10   0.00   0.70    0.093 ▰▰▰▰▱ │
│                                                                                           │
│ measured across the whole matrix: 35/35  (100%)   `—` = not measured; contributed nothing.│
└───────────────────────────────────────────────────────────────────────────────────────────┘
```

## Black and violet, with soft blue for exactly one thing

Every other terminal surface in this repository has its own identity — the sniper dashboard
is black and red, `mesh-dashboard` is indigo over slate, `sdk-dashboard` is the bond
pipeline's green. Omni is black and violet, and that is not decoration: an operator with
three of these open must be able to tell at a glance which one is making a claim about their
money and which one about a decision record.

Violet specifically, because omni's whole subject is the boundary between **measured** and
**unmeasured**, and that wants a palette where the interesting distinction is luminance
first rather than hue first. A measured value glows; an unmeasured one recedes to a note in
the same family. A red/green palette would put the emphasis on good-versus-bad, which is the
wrong axis — a measured `0.00` risk and an unmeasured risk are not good and bad, they are
known and unknown.

**Azure appears in two places only**: the branch that was chosen, and a commitment that
verifies. Both are *claims the agent made*. An opportunity signal and a live provenance are
*observations* and wear their own colours, because an accent that appears on every third row
is not an accent, it is a second body colour.

## Two rules, both mechanised

**A renderer names a role, never a colour.** `theme.rs` is the only file with a hex value in
it; `view.rs` is the only file that maps a state to a role; `render.rs` places rectangles.
Same split as `alchem_link.theme` and `lib/mesh/view.ts::toneFor` — a rule that encodes a
claim about trust gets exactly one implementation.

**Colour is decoration, never the message.** `NO_COLOR`, a pipe, `--no-color` and a
16-colour terminal all produce the same *text*: `—` for unmeasured, `▸` for chosen,
`LIVE`/`STALE`/`ABSENT` for provenance, `RISK`/`OPP `/`EST?` for signals, `EXCLUDED` for a
forbidden branch. A test walks every role in `Depth::Mono` and fails if one carries neither a
modifier nor a word — a role whose entire identity is a hue disappears on a monochrome
terminal, and that is a bug in the role.

`--palette` prints what the terminal negotiated, so an operator on an unfamiliar one can
confirm that "measured" and "unmeasured" are actually distinguishable there *before* trusting
a screen full of them.

## The coverage meter is a count, not a bar

`▰▰▰▱▱`. One cell per term. A proportional bar renders `2/5` and `4/10` identically, and the
denominator is the number that matters — a utility computed on two terms out of five and one
computed on five out of five are different claims. Above twelve terms it falls back to the
`30/90` label rather than to a proportional bar, for the same reason.

An empty coverage draws `∅`, never an empty meter. An empty container reads as "measured,
and it is zero"; the true statement is "there was nothing here".

## `enter` simulates. `D` decides.

Two keys, and a confirmation on the second.

`simulate` and `decide` compute *exactly* the same thing and differ only in whether they
leave a trace. The only protection against a counterfactual later reading as a decision
somebody made is that they are not the same keystroke — so the obvious button is the one
that writes nothing, and sealing takes a capital letter and a yes.

## Grounding is ticked, never inferred

The signal list is a multi-select, and what it selects is `Goal::grounded_in`. Nothing reads
the goal text looking for a matching signal.

An earlier version of the CLI inferred grounding by keyword overlap and, on its first real
run, grounded *"add tests to the scema-cli crate"* in a marker backlog in a **different**
crate — because `scema` is a substring of every unit name in this workspace. The branch then
inherited a *measured* expected gain from evidence that had nothing to do with it, which is
exactly the laundering the simulator refuses to do on its own. A test pins that a goal naming
a signal id verbatim still does not ground it.

## `--snapshot`, and why a TUI needs one

```console
$ scema-tui --snapshot 128x38
```

Draws one frame into an off-screen buffer and prints it as text. It exists because a TUI is
otherwise untestable — its output is a terminal — so layout bugs are found by a human
noticing a column is wrong, and ratatui's layout arithmetic underflows on small rectangles.
The production failure mode is the console dying on somebody's 80-column terminal with a
backtrace instead of a screen.

The crate's own tests draw every tab at four sizes and assert on the *symbols*, not the
styles: everything here has to survive `NO_COLOR` anyway, so the text is what must be right,
and a snapshot carrying escape sequences would churn on every palette tweak until nobody read
it.

## What it does not do

**Act.** The whole workspace ends at a decision and a record. A console that quietly gained a
write path would invalidate the claim every other crate here makes about being safe to point
at a live system.

**Talk to a daemon.** It holds no token and opens no socket; it drives the loop in-process
against a local path. `scema-omnid` exists for the case where something remote needs to drive
the loop, and folding that in would mean putting the whole pairing story on screen for no
gain.

## Why a separate crate

The same split `mesh-dashboard` / `scematica-mesh` makes in the bot workspace, and for the
same reason: `scema-agent` is a library other people embed, and forcing ratatui and crossterm
on a consumer that only wants the loop would be the mistake `sdk-dashboard` exists to avoid.
The rendering lives here; the crates next door own the thinking.
