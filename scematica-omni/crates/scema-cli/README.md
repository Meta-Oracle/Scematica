# scema-cli

**`scema` - the agent runtime from a terminal.**

Part of [Scematica Omni](https://github.com/Meta-Oracle/Scematica/tree/main/scematica-omni) —
an agent runtime that perceives an environment, projects competing futures, ranks them under
a stated preference, decides *or refuses to*, and seals a verifiable record of what it did.

The organising idea across every crate: **each layer can say "I don't know", and saying it
costs nothing.** An agent that cannot express ignorance expresses a number of the right shape
instead, and nothing downstream can tell it from a measurement.

---

```console
$ scema observe .
$ scema simulate "clear the marker backlog" --ground markers:scema-tools   # writes nothing
$ scema decide   "clear the marker backlog" --ground markers:scema-tools   # seals a record
$ scema explain 58898030 ; scema verify --all
$ scema policy
```

`simulate` never persists — it is a counterfactual, and a record it left behind would later
read as a decision the agent made. `decide` seals a record and appends memory. Both compute
exactly the same thing.

`execute`, `delegate`, `discover` and `pay` are registered verbs that exit non-zero saying
what is missing. They are in `--help` on purpose: the shape of the runtime includes an action
path, an agent-to-agent path and a payment path, and finding that out from the tool beats
finding it out later.

An unmeasured term renders as an em dash, never `0.00`. A column of numbers is the most
persuasive thing a program can emit, and the moment a neutral element renders as a number the
distinction the type system has been protecting is gone in the last hundred lines of the
program.

## One entry point to the whole runtime

```console
$ scema tui                # the console        (scema-tui)
$ scema daemon --allow .   # loopback HTTP      (scema-omnid)
$ scema mcp    --allow .   # tools for a model  (scema-mcp)
```

`scema` locates its siblings **next to itself first**, then on `PATH`. Sibling-first is
load-bearing: a developer with `cargo install`ed binaries in `~/.cargo/bin` and a
`target/release` build in a checkout expects the checkout's console, and resolving through
`PATH` first silently pairs a new launcher with an old component — the symptom being a flag
that "does not exist" in a binary where it plainly does.

They stay separate crates rather than being linked in, so that installing the CLI on a CI
machine that will only ever run `scema verify` does not drag in a terminal stack. A missing
one names the crate that provides it; "command not found" sends people to a search engine.

## Setting up

```console
$ scema init                      # create .scema/, which ignores itself
$ scema doctor                    # what is installed, wired up, or quietly broken
$ scema connect --list            # assistants this can wire the MCP server into
$ scema connect claude-code --write
$ scema completions powershell
```

**`doctor` changes nothing.** Every finding names the command that would fix it and stops
there — a diagnostic that repaired things would need the whole approval story in front of
it, and an operator running a check does not expect their editor configuration to be edited.
Its verdicts are four, not two: `ok`, `warn`, `FAIL`, and `?` for a check that could not be
run at all. "The record store does not verify" and "the record store could not be read" are
different claims and only one is an accusation. It exits non-zero only on a real failure,
because a diagnostic that fails a build over an optional component is one people delete from
the pipeline.

**`connect` merges, never overwrites.** A project `.mcp.json` routinely holds three servers
somebody else set up, and a tool that "adds" a fourth by rewriting the file deletes them. An
unparseable config is refused rather than replaced.

**`connect --write` only touches project-local files.** Claude Desktop, Windsurf, Zed and
Codex keep their configuration in the operator's home directory, and those are printed with
their path and never written. That is not timidity: a user config is shared by every project
you open, so editing it on somebody's behalf means a tool they installed for one repository
quietly gained the ability to observe all of them. It costs one paste.

`scema init` writes `.scema/.gitignore` containing `*` rather than editing the project's own
`.gitignore`. The directory is machine-local and full of absolute paths; a self-ignoring
directory works whatever the project's ignore rules say, and this tool has no business
rewriting a file the whole repository shares.

---

Licensed MIT.
