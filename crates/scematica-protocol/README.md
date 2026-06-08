# scematica-protocol

A Rust-native implementation of the **x402** (HTTP `402 Payment Required`)
payment standard for Solana — the payment facilitator used by the
[Scematica](https://github.com/Meta-Oracle/Scematica) stack to meter and settle
paid API calls in USDC.

Ships a `protocol` binary (an axum HTTP server) plus the library types
(`PaymentRequirements`, `PaymentGate`, facilitator/settlement helpers) other
crates embed.

```bash
cargo run --bin protocol -- --pay-to <wallet> --price-lamports 10000
```

## License

MIT
