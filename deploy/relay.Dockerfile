# ─────────────────────────────────────────────────────────────────────────────
# scemadex-relay — public ScemaDEX peer-mesh + x402-gated signal oracle
#
# Build from the REPO ROOT (the build context must be the whole workspace, the
# relay depends on several path crates):
#
#   docker build -f deploy/relay.Dockerfile -t scemadex-relay .
#
# Run open (free signals, mesh only):
#   docker run -p 8080:8080 -v "$PWD:/signals" scemadex-relay \
#     --signal-dir /signals --persist-dir /signals/mesh
#
# Run x402-gated (pay-per-signal-call) — mount a fee-payer keypair:
#   docker run -p 8080:8080 -v "$PWD:/signals" -v "$PWD/payer.json:/payer.json:ro" \
#     scemadex-relay --signal-dir /signals \
#       --pay-to <YOUR_WALLET> --keypair /payer.json --rpc-url <RPC_URL> --price-usdc 0.001
#
# NOTE: the release profile uses fat LTO + codegen-units=1, so the first build is
# slow (~20-40 min). It is cached across rebuilds via the layered COPY below.
# ─────────────────────────────────────────────────────────────────────────────

FROM rust:1-bookworm AS builder

# Native deps: reqwest 0.11 uses native-tls (openssl) and solana-client needs a C toolchain.
RUN apt-get update && apt-get install -y --no-install-recommends \
        pkg-config libssl-dev build-essential \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build
# Copy the whole workspace. `.dockerignore` keeps target/, web/node_modules, .git out.
COPY . .

RUN cargo build --release -p scemadex-relay \
    && strip target/release/scemadex-relay

# ── Runtime image ────────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates libssl3 curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -r -u 10001 relay

COPY --from=builder /build/target/release/scemadex-relay /usr/local/bin/scemadex-relay

USER relay
EXPOSE 8080

# Health: GET /health -> "ok". Compose/fly use this to gate readiness.
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
    CMD curl -fsS http://localhost:8080/health || exit 1

ENTRYPOINT ["/usr/local/bin/scemadex-relay"]
CMD ["--port", "8080", "--signal-dir", "/signals", "--persist-dir", "/signals/mesh"]
