# scema-daemon

**`scema-omnid` - the loop over loopback HTTP.**

Part of [Scematica Omni](https://github.com/Meta-Oracle/Scematica/tree/main/scematica-omni) —
an agent runtime that perceives an environment, projects competing futures, ranks them under
a stated preference, decides *or refuses to*, and seals a verifiable record of what it did.

The organising idea across every crate: **each layer can say "I don't know", and saying it
costs nothing.** An agent that cannot express ignorance expresses a number of the right shape
instead, and nothing downstream can tell it from a measurement.

---

```console
$ scema-omnid --allow /path/to/a/project
  listening   http://127.0.0.1:7842   (loopback only, not configurable)
  token       .scema/omnid.token
```

Hand-rolled HTTP/1.1 on the standard library — no hyper, no rustls, no async runtime. A
loopback JSON server for a known client does not need one, and a TLS stack here would be
something for other workspaces to path-depend on and regret.

Four guards, in order: **loopback bind that is deliberately not configurable** (the one thing
that reliably happens to a `--bind` flag is somebody setting it to `0.0.0.0`); a `Host` check
that rejects DNS rebinding with `421`; a constant-time 256-bit bearer token; and every
caller-supplied path resolved through `scema-tools`' `Workspace`.

No `Access-Control-Allow-Origin` is ever emitted and no `OPTIONS` is handled, so a web page
cannot read a reply even if it guesses a route.

`POST /decide` is off until `--allow-decide`. `POST /simulate` never persists, and builds its
own non-persisting agent rather than flipping a flag on the shared one — a shared mutable flag
is a race whose failure mode is a simulation quietly sealing a record.

---

Licensed MIT.
