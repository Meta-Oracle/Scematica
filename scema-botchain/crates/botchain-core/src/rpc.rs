//! JSON-RPC client with ordered endpoint failover.
//!
//! Deliberately hand-rolled over `reqwest` rather than pulling an EVM SDK. Everything
//! the port needs first — chain id, block height, gas price, `eth_getLogs` — is plain
//! JSON-RPC, and staying dependency-light keeps the first build minutes rather than tens
//! of minutes. An SDK earns its place when ABI encoding and transaction signing arrive;
//! not before.
//!
//! Failover is not defensive padding. Measured in August 2026: `rpc.botchain.ai`
//! resolves in DNS but never completes a TCP connection from some networks, while the
//! Cloudflare-fronted explorer proxy answers every time. A client hard-coded to one URL
//! is a client that is simply down for those operators, with no signal as to why.
//!
//! What failover must **not** do is hide which endpoint answered. A read served by the
//! explorer proxy and one served by a node are not interchangeable — the proxy cannot
//! broadcast a transaction — so every response carries its source and the caller decides
//! whether that is acceptable.

use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::{debug, warn};

use crate::chain::{Endpoint, EndpointKind, Network};

/// Per-endpoint timeout. Short: the point of a list is to move on, not to wait.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Debug, Clone)]
pub struct Response {
    pub result: Value,
    /// Which endpoint produced it.
    pub endpoint: &'static str,
    pub kind: EndpointKind,
    pub elapsed: Duration,
}

#[derive(Debug, Deserialize)]
struct RpcEnvelope {
    result: Option<Value>,
    error: Option<RpcError>,
}

#[derive(Debug, Deserialize)]
struct RpcError {
    code: i64,
    message: String,
}

pub struct Client {
    http: reqwest::Client,
    network: &'static Network,
    /// Extra endpoints from the operator, tried before the built-in list.
    overrides: Vec<Endpoint>,
}

impl Client {
    pub fn new(network: &'static Network) -> Result<Self> {
        Ok(Self {
            http: reqwest::Client::builder().timeout(REQUEST_TIMEOUT).build()?,
            network,
            overrides: Vec::new(),
        })
    }

    /// Put an operator-supplied endpoint at the front of the list.
    ///
    /// Leaked from `BOTCHAIN_RPC_URL`, a private node, whatever. It is tried first and
    /// the built-ins remain as fallback, so a private node going down degrades to public
    /// reads instead of to nothing.
    pub fn with_endpoint(mut self, url: &'static str) -> Self {
        self.overrides.push(Endpoint {
            url,
            kind: EndpointKind::Node,
            note: "operator override",
        });
        self
    }

    pub fn network(&self) -> &'static Network {
        self.network
    }

    fn endpoints(&self) -> impl Iterator<Item = &Endpoint> {
        self.overrides.iter().chain(self.network.endpoints.iter())
    }

    /// Call a method, walking the endpoint list until one answers.
    ///
    /// A *transport* failure moves to the next endpoint. A well-formed JSON-RPC **error**
    /// does not: if a node says "execution reverted", every other node will say the same,
    /// and retrying turns one honest error into N slow ones.
    pub async fn call(&self, method: &str, params: Value) -> Result<Response> {
        let mut last_err: Option<anyhow::Error> = None;

        for ep in self.endpoints() {
            let started = Instant::now();
            match self.call_one(ep, method, &params).await {
                Ok(result) => {
                    return Ok(Response {
                        result,
                        endpoint: ep.url,
                        kind: ep.kind,
                        elapsed: started.elapsed(),
                    })
                }
                Err(CallError::Rpc { code, message }) => {
                    // The chain answered and said no. That is the answer.
                    return Err(anyhow!("{method} rejected by {}: {message} (code {code})", ep.url));
                }
                Err(CallError::Transport(e)) => {
                    warn!(endpoint = ep.url, method, error = %e, "endpoint unreachable, trying next");
                    last_err = Some(e);
                }
            }
        }

        Err(anyhow!(
            "no endpoint answered {method} for {} ({} tried). Last error: {}",
            self.network.name,
            self.endpoints().count(),
            last_err
                .map(|e| e.to_string())
                .unwrap_or_else(|| "none".into()),
        ))
    }

    async fn call_one(&self, ep: &Endpoint, method: &str, params: &Value) -> Result<Value, CallError> {
        let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });

        let res = self
            .http
            .post(ep.url)
            .json(&body)
            .send()
            .await
            .map_err(|e| CallError::Transport(e.into()))?;

        if !res.status().is_success() {
            return Err(CallError::Transport(anyhow!("HTTP {}", res.status())));
        }

        let env: RpcEnvelope = res
            .json()
            .await
            .map_err(|e| CallError::Transport(anyhow!("malformed JSON-RPC body: {e}")))?;

        if let Some(err) = env.error {
            return Err(CallError::Rpc { code: err.code, message: err.message });
        }
        env.result
            .ok_or_else(|| CallError::Transport(anyhow!("JSON-RPC reply had neither result nor error")))
    }

    // ── typed helpers ─────────────────────────────────────────────────────────

    pub async fn chain_id(&self) -> Result<u64> {
        let r = self.call("eth_chainId", json!([])).await?;
        hex_u64(&r.result)
    }

    pub async fn block_number(&self) -> Result<u64> {
        let r = self.call("eth_blockNumber", json!([])).await?;
        hex_u64(&r.result)
    }

    pub async fn gas_price(&self) -> Result<u128> {
        let r = self.call("eth_gasPrice", json!([])).await?;
        hex_u128(&r.result)
    }

    /// Fetch logs. `topics` may be empty to match everything in the range.
    pub async fn get_logs(
        &self,
        from_block: u64,
        to_block: u64,
        topics: &[&str],
    ) -> Result<Vec<Value>> {
        let mut filter = json!({
            "fromBlock": format!("0x{from_block:x}"),
            "toBlock": format!("0x{to_block:x}"),
        });
        if !topics.is_empty() {
            filter["topics"] = json!([topics]);
        }
        let r = self.call("eth_getLogs", json!([filter])).await?;
        Ok(r.result.as_array().cloned().unwrap_or_default())
    }

    /// Logs emitted **by a contract**, with no topic filter.
    ///
    /// The topic-free form is the one to reach for when surveying an unfamiliar chain.
    /// Filtering by a guessed event signature answers "does this fork emit the event I
    /// assumed" and silently returns zero when it does not — which reads identically to
    /// "nothing happened here". Filtering by address cannot be wrong about the fork.
    pub async fn logs_for_address(
        &self,
        address: &str,
        from_block: u64,
        to_block: u64,
    ) -> Result<Vec<Value>> {
        let filter = json!({
            "address": address,
            "fromBlock": format!("0x{from_block:x}"),
            "toBlock": format!("0x{to_block:x}"),
        });
        let r = self.call("eth_getLogs", json!([filter])).await?;
        Ok(r.result.as_array().cloned().unwrap_or_default())
    }

    /// Verify we are talking to the network we think we are.
    ///
    /// Always call this before trusting an endpoint. Chain ids are not unique — 968 is
    /// claimed by BOT Chain testnet, by Datagram in the public registry, and by BSC's
    /// Rialto — so the check that means something is "this pinned endpoint reports the id
    /// I expect", never "some registry says 968 is BOT Chain".
    pub async fn verify(&self) -> Result<Response> {
        let r = self.call("eth_chainId", json!([])).await?;
        let got = hex_u64(&r.result)?;
        if got != self.network.chain_id {
            return Err(anyhow!(
                "{} answered with chain id {} but {} was expected — wrong network, or a \
                 chain-id collision. Refusing to continue.",
                r.endpoint,
                got,
                self.network.chain_id
            ));
        }
        debug!(endpoint = r.endpoint, chain_id = got, "endpoint verified");
        Ok(r)
    }
}

enum CallError {
    /// Could not reach or parse — try the next endpoint.
    Transport(anyhow::Error),
    /// The node answered with a JSON-RPC error — do not retry elsewhere.
    Rpc { code: i64, message: String },
}

/// Parse a `0x`-prefixed quantity.
pub fn hex_u64(v: &Value) -> Result<u64> {
    let s = v.as_str().ok_or_else(|| anyhow!("expected a hex string, got {v}"))?;
    u64::from_str_radix(s.trim_start_matches("0x"), 16).map_err(|e| anyhow!("bad hex {s}: {e}"))
}

pub fn hex_u128(v: &Value) -> Result<u128> {
    let s = v.as_str().ok_or_else(|| anyhow!("expected a hex string, got {v}"))?;
    u128::from_str_radix(s.trim_start_matches("0x"), 16).map_err(|e| anyhow!("bad hex {s}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain::{MAINNET, TESTNET};

    #[test]
    fn hex_parsing_handles_real_values() {
        assert_eq!(hex_u64(&json!("0x2a5")).unwrap(), 677);
        assert_eq!(hex_u64(&json!("0x3c8")).unwrap(), 968);
        // Compared against the literal rather than a hand-computed decimal: I got that
        // conversion wrong twice writing this test, which is the argument for it.
        assert_eq!(hex_u64(&json!("0x123ea5c")).unwrap(), 0x123ea5c);
        assert!(hex_u64(&json!(677)).is_err(), "a bare number is not a quantity");
    }

    #[test]
    fn an_override_is_tried_before_the_built_ins() {
        let c = Client::new(&MAINNET).unwrap().with_endpoint("https://example.invalid");
        let first = c.endpoints().next().unwrap();
        assert_eq!(first.url, "https://example.invalid");
        // and the built-ins survive as fallback
        assert!(c.endpoints().count() > MAINNET.endpoints.len());
    }

    #[test]
    fn testnet_has_no_silent_read_fallback() {
        // Worth asserting so nobody later "helpfully" adds one: the testnet explorer sits
        // on the same host as its RPC, so it is not an independent path and would give
        // false confidence.
        assert!(TESTNET.endpoints.iter().all(|e| e.kind == EndpointKind::Node));
    }
}
