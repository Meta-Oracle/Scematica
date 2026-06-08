# scematica-dashboard

The terminal (ratatui) **dashboard** for the
[Scematica](https://github.com/Meta-Oracle/Scematica) Solana trading stack — a
6-tab TUI for live monitoring of the sniper: metrics, trades, filter stats, the
Deep Q\* agent, and an LLM **chat agent** with tool access.

```bash
cargo install scematica-dashboard
dashboard --demo     # standalone — no keypair, RPC, or sniper required
dashboard            # live — reads the sniper's JSON artifacts in the cwd
```

`--demo` runs fully self-contained. In live mode the dashboard observes the
sniper through JSON files in the working directory and can launch the `sniper`
binary as a child process (install `scematica-sniper` to make it available on
`PATH`).

## License

MIT
