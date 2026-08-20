# scema-mcp

**The loop as Model Context Protocol tools.**

Part of [Scematica Omni](https://github.com/Meta-Oracle/Scematica/tree/main/scematica-omni) —
an agent runtime that perceives an environment, projects competing futures, ranks them under
a stated preference, decides *or refuses to*, and seals a verifiable record of what it did.

The organising idea across every crate: **each layer can say "I don't know", and saying it
costs nothing.** An agent that cannot express ignorance expresses a number of the right shape
instead, and nothing downstream can tell it from a measurement.

---

```jsonc
{ "mcpServers": { "scema-omni": { "command": "scema-mcp", "args": ["--allow", "/proj"] } } }
```

`omni_observe`, `omni_simulate`, `omni_explain`, `omni_verify`, `omni_policy`, `omni_memory`,
and `omni_decide` when enabled. Newline-delimited JSON-RPC on stdio, so **stdout is the
transport** and every diagnostic goes to stderr.

Links the loop directly rather than proxying a daemon: same library, one less hop, and no way
for two surfaces to disagree about what the loop does.

Two guards specific to a model caller. Paths resolve through `Workspace` — not paranoia about
a hostile model, but because a *cooperative* one asked to audit a project will reason its way
to `~/.ssh`, that being genuinely relevant to an audit. And `omni_decide` is **not advertised
at all** without `--allow-decide`, because a tool that is listed and always fails teaches a
model to retry it.

A refused path comes back as a tool result with `isError`, never a JSON-RPC error: clients
surface the latter as "the server broke", and a model told that stops trying, where one told
"that path is outside the workspace, which is X" corrects itself.

---

Licensed MIT.
