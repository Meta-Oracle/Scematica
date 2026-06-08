# scematica-executor

Multi-DEX swap instruction builders for the
[Scematica](https://github.com/Meta-Oracle/Scematica) Solana trading stack.

Builds swap transactions/instructions for Raydium, Orca, and Meteora, and wraps
the **Jupiter v6** aggregator (`JupiterBuilder`) for quotes and swap
transactions. Handles the WSOL ATA lifecycle and dynamic fee escalation used by
the sniper's executor.

## License

MIT
