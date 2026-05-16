use axum::{middleware, routing::get, Json, Router};
use std::{net::SocketAddr, sync::Arc};
use tracing::info;

use crate::{
    facilitator::Facilitator,
    middleware::{payment_middleware, PaymentGate},
    types::PaymentRequirements,
};

pub struct ProtocolServer {
    gate: Arc<PaymentGate>,
    addr: SocketAddr,
}

impl ProtocolServer {
    pub fn new(
        facilitator: Arc<Facilitator>,
        requirements: Vec<PaymentRequirements>,
        addr: SocketAddr,
    ) -> Self {
        let gate = Arc::new(PaymentGate { facilitator, requirements });
        Self { gate, addr }
    }

    pub async fn run(self) -> anyhow::Result<()> {
        let gate = self.gate.clone();

        // Paid routes — each requires a valid X-Payment header
        let paid = Router::new()
            .route("/signals/pools",  get(pools_handler))
            .route("/signals/trades", get(trades_handler))
            .route("/stats/nn",       get(nn_stats_handler))
            .route("/stats/metrics",  get(metrics_handler))
            .layer(middleware::from_fn_with_state(gate.clone(), payment_middleware));

        // Free routes
        let accepts = gate.requirements.clone();
        let free = Router::new()
            .route("/health",    get(health_handler))
            .route("/supported", get(move || {
                let a = accepts.clone();
                async move { Json(serde_json::json!({ "accepts": a })) }
            }));

        let app = Router::new().merge(paid).merge(free);

        info!("🌐 Scematica Protocol API on {}", self.addr);
        axum::Server::bind(&self.addr)
            .serve(app.into_make_service())
            .await?;
        Ok(())
    }
}

// ── paid handlers ─────────────────────────────────────────────────────────────

async fn pools_handler() -> Json<serde_json::Value> {
    let data = read_json("scematica-filter-stats.json");
    Json(serde_json::json!({
        "pool_stats": data,
        "timestamp": chrono::Utc::now(),
    }))
}

async fn trades_handler() -> Json<serde_json::Value> {
    let trades: Vec<serde_json::Value> = std::fs::read_to_string("scematica-trades.jsonl")
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .take(200)
        .collect();
    let count = trades.len();
    Json(serde_json::json!({ "trades": trades, "count": count, "timestamp": chrono::Utc::now() }))
}

async fn nn_stats_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "nn_stats": read_json("scematica-nn-stats.json"),
        "timestamp": chrono::Utc::now(),
    }))
}

async fn metrics_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "metrics": read_json("scematica-metrics.json"),
        "strategy": read_json("scematica-strategy.json"),
        "timestamp": chrono::Utc::now(),
    }))
}

// ── free handlers ─────────────────────────────────────────────────────────────

async fn health_handler() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "protocol": "scematica-x402",
        "version": crate::types::X402_VERSION,
    }))
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn read_json(path: &str) -> serde_json::Value {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::Value::Null)
}
