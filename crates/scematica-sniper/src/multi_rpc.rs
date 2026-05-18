use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tracing::{debug, info, warn};

/// Categorized RPC error — determines whether to failover, backoff, or ignore.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RpcErrorKind {
    /// 429 Too Many Requests — exponential backoff, don't failover.
    RateLimited,
    /// Node's slot is lagging behind cluster — failover immediately.
    NodeBehind,
    /// Account / pool not found — expected for fresh pools; don't penalise endpoint.
    AccountNotFound,
    /// Network timeout or connection refused — failover and log.
    NetworkTimeout,
    /// Any other RPC error (invalid tx, bad args, etc.)
    Other,
}

impl RpcErrorKind {
    /// Parse a client error string into a categorized kind.
    pub fn from_error_str(e: &str) -> Self {
        if e.contains("429") || e.contains("Too Many Requests") || e.contains("rate limit") {
            Self::RateLimited
        } else if e.contains("behind") || e.contains("slot") && e.contains("lag") {
            Self::NodeBehind
        } else if e.contains("AccountNotFound")
            || e.contains("could not find account")
            || e.contains("invalid account data")
        {
            Self::AccountNotFound
        } else if e.contains("timed out")
            || e.contains("connection refused")
            || e.contains("Connection reset")
            || e.contains("BrokenPipe")
        {
            Self::NetworkTimeout
        } else {
            Self::Other
        }
    }

    /// True when this error kind should trigger an endpoint failover.
    pub fn should_failover(self) -> bool {
        matches!(self, Self::NodeBehind | Self::NetworkTimeout)
    }

    /// True when this error kind warrants adding a latency penalty to the endpoint.
    pub fn penalise_endpoint(self) -> bool {
        !matches!(self, Self::AccountNotFound)
    }
}

/// Multi-RPC client with automatic failover and latency-based primary selection.
///
/// Maintains a list of RPC endpoints and tracks the round-trip latency to each.
/// The primary endpoint (used for all calls) is automatically switched to the
/// fastest responding node.  On RPC error the caller should call `failover()` to
/// rotate to the next endpoint.
pub struct MultiRpc {
    clients: Vec<Arc<RpcClient>>,
    primary_idx: AtomicUsize,
    latencies: parking_lot::Mutex<Vec<u64>>, // milliseconds, one per client
}

impl MultiRpc {
    /// Construct a `MultiRpc` from a list of endpoint URLs.
    ///
    /// If `endpoints` is empty a client for `https://api.mainnet-beta.solana.com`
    /// is created as a fallback so the struct is never in an empty state.
    pub fn new(endpoints: &[String], commitment: CommitmentConfig) -> Self {
        let mut urls = endpoints.to_vec();
        if urls.is_empty() {
            urls.push("https://api.mainnet-beta.solana.com".to_string());
        }

        let clients: Vec<Arc<RpcClient>> = urls
            .iter()
            .map(|url| Arc::new(RpcClient::new_with_commitment(url.clone(), commitment)))
            .collect();

        let count = clients.len();
        Self {
            clients,
            primary_idx: AtomicUsize::new(0),
            latencies: parking_lot::Mutex::new(vec![u64::MAX; count]),
        }
    }

    /// Return the currently selected primary RPC client.
    pub fn primary(&self) -> Arc<RpcClient> {
        let idx = self.primary_idx.load(Ordering::Relaxed) % self.clients.len();
        self.clients[idx].clone()
    }

    /// Rotate to the next endpoint after an RPC error.
    pub fn failover(&self) {
        let len = self.clients.len();
        if len <= 1 {
            return;
        }
        let prev = self.primary_idx.fetch_add(1, Ordering::Relaxed) % len;
        let next = (prev + 1) % len;
        warn!("MultiRpc: failing over from endpoint {} to {}", prev, next);
    }

    /// Handle an RPC error string with categorization.
    /// - Rate limits: returns backoff duration (caller should sleep).
    /// - Node-behind / timeout: triggers failover and returns None.
    /// - AccountNotFound: silently ignored, no penalty.
    /// - Other: logs and returns None.
    pub fn handle_error(&self, error: &str, attempt: u32) -> Option<std::time::Duration> {
        let kind = RpcErrorKind::from_error_str(error);
        match kind {
            RpcErrorKind::RateLimited => {
                let delay_ms = 250u64 << attempt.min(3); // 250ms / 500ms / 1s / 2s
                debug!("MultiRpc: rate-limited — backing off {}ms", delay_ms);
                Some(std::time::Duration::from_millis(delay_ms))
            }
            RpcErrorKind::NodeBehind | RpcErrorKind::NetworkTimeout => {
                warn!("MultiRpc: {} — triggering failover", error);
                self.failover();
                None
            }
            RpcErrorKind::AccountNotFound => {
                // Expected for fresh pools — don't log at warn level, no penalty
                debug!("MultiRpc: account not found (expected for fresh pools)");
                None
            }
            RpcErrorKind::Other => {
                warn!("MultiRpc: unclassified RPC error: {}", error);
                None
            }
        }
    }

    /// Measure the latency to each endpoint by calling `getSlot`.
    /// Sets the primary to the fastest responding endpoint.
    pub async fn update_latencies(&self) {
        let len = self.clients.len();
        let mut new_latencies = vec![u64::MAX; len];

        for (i, client) in self.clients.iter().enumerate() {
            let start = std::time::Instant::now();
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                client.get_slot(),
            ).await;
            let elapsed_ms = start.elapsed().as_millis() as u64;

            match result {
                Ok(Ok(_)) => {
                    new_latencies[i] = elapsed_ms;
                    debug!("MultiRpc: endpoint {} latency={}ms", i, elapsed_ms);
                }
                Ok(Err(e)) => {
                    warn!("MultiRpc: endpoint {} RPC error: {}", i, e);
                }
                Err(_) => {
                    warn!("MultiRpc: endpoint {} timed out", i);
                }
            }
        }

        // Find the fastest endpoint
        let best_idx = new_latencies
            .iter()
            .enumerate()
            .min_by_key(|(_, &lat)| lat)
            .map(|(i, _)| i)
            .unwrap_or(0);

        *self.latencies.lock() = new_latencies;
        self.primary_idx.store(best_idx, Ordering::Relaxed);

        let best_lat = self.latencies.lock()[best_idx];
        if best_lat < u64::MAX {
            info!("MultiRpc: primary → endpoint {} ({}ms)", best_idx, best_lat);
        } else {
            warn!("MultiRpc: all endpoints failed latency check");
        }
    }

    /// Return the number of configured endpoints.
    pub fn endpoint_count(&self) -> usize {
        self.clients.len()
    }
}
