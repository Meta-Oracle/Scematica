use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tracing::{debug, info, warn};

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
