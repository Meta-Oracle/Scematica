# scema-omni — a Claude Code plugin

Gives a coding assistant the omni loop: perceive the project as a world state, rank
competing branches under a stated preference, and read back records anyone can verify.

```console
$ cargo install scema-mcp          # the server this wraps
$ claude
> /plugin marketplace add Meta-Oracle/Scematica
> /plugin install scema-omni@scematica
```

Or, without the plugin machinery, wire the server straight in:

```console
$ scema connect claude-code --write     # merges into .mcp.json, nothing else touched
```

## What is in it

| Piece | Does |
|---|---|
| `.mcp.json` | the `scema-mcp` server, confined to `${CLAUDE_PROJECT_DIR}` |
| `/omni-observe` | perceive the project and report blind spots *before* numbers |
| `/omni-simulate` | rank branches against a goal, writing nothing |
| `/omni-verify` | re-check a record's commitment, and state what that proves |
| `skills/omni-decision-records` | how to read the output without misreporting it |

## The skill is the important part

The MCP server can be added with one line of JSON. What it cannot do by itself is stop a
model from writing *"expected gain: 0.00"* when the tool said `—`.

That is not a hypothetical. Omni's entire design is that every layer can say "I don't
know" — `Provenance` before value, `Term` before score, `Applicability` before opinion —
and the last layer is prose written by a model. A summary that reports an unmeasured term
as a zero has undone the whole type system underneath it in one sentence, and nothing
downstream can tell.

So the skill spends its length on five things not to do, each of which is a real failure
this repository has paid for at least once:

1. An em dash is not a zero.
2. Coverage never leaves the score it qualifies.
3. Abstention is an answer, and *which* abstention is the actionable part.
4. Grounding is asserted, never inferred from wording.
5. A verified commitment proves one thing and not two others.

## What it deliberately cannot do

**Seal records.** There is no `--allow-decide` in this plugin's `.mcp.json`, so
`omni_decide` is not advertised to the model at all — not listed-and-failing, which teaches
a model to retry, but absent. Sealing a decision record is a local write into the operator's
own decision history, and it should take a deliberate act to enable. Add the flag in your
project's own `.mcp.json` if you want it.

**Act.** Nothing in the omni workspace writes to an environment it observed. `execute`,
`delegate`, `discover` and `pay` are registered verbs that exit non-zero and say what is
missing; they are in `--help` on purpose, because a verb that silently did not exist would
be indistinguishable from one that failed.

**Leave the project.** `--allow ${CLAUDE_PROJECT_DIR}` is not decoration.
`scema_tools::Workspace` resolves paths fully — symlinks followed, `..` collapsed — and
*then* compares against the roots, because a string scan for `..` passes a symlink pointing
at `/`. This matters for a cooperative model, not a hostile one: asked to audit a project,
a model will reason its way to `~/.ssh` because that is genuinely relevant to an audit.

## Other assistants

The same server, five other shapes of config file:

```console
$ scema connect --list
$ scema connect cursor --write
$ scema connect vscode --write
$ scema connect zed            # user-level: printed, never written
```

Project-local files are written; user-level ones are printed with their path. A user config
is shared by every project you open, and editing it on your behalf would mean a tool
installed for one repository quietly gaining the ability to observe all of them.
