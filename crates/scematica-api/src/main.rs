//! Scematica HTTP API — serves sniper state files as JSON, accepts control POSTs.
//!
//! GET  /api/metrics          scematica-metrics.json
//! GET  /api/pools            scematica-pool-radar.json (last N)
//! GET  /api/filters          scematica-filter-stats.json
//! GET  /api/logs             last N lines of scematica-sniper.log
//! GET  /api/trades           scematica-trades.jsonl (last N)
//! GET  /api/nn               scematica-nn-stats.json
//! GET  /api/health           sniper liveness (lock file + process)
//! GET  /api/controls         current sell/dump/rate/high-speed state
//! POST /api/controls/sell-mode    { "enabled": bool }
//! POST /api/controls/dump-mode    { "enabled": bool }
//! POST /api/controls/rate-mode    { "mode": "conservative"|"normal"|"aggressive"|"runner" }
//! POST /api/controls/high-speed   { "enabled": bool }
//! GET  /health               API server liveness

use axum::{
    extract::Query,
    http::{Method, Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Json, Response},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader, Seek, SeekFrom},
    net::SocketAddr,
};
use tower_http::cors::{Any, CorsLayer};
use tracing::{info, debug};

// ── x402 payment gate ─────────────────────────────────────────────────────────

const FEE_LAMPORTS: u64 = 10_000_000; // 0.01 SOL
const FEE_RECIPIENT: &str = "CvLUHUooCN8k3vJunor9qwX7oJNCt8Q6VhyKi5EMBKet";
const X_PAYMENT_HEADER: &str = "x-payment";

async fn x402_middleware(req: Request<axum::body::Body>, next: Next) -> Response {
    if req.headers().contains_key(X_PAYMENT_HEADER) {
        let payment_proof = req.headers()
            .get(X_PAYMENT_HEADER)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        debug!(proof = payment_proof, "x402 payment accepted");
        next.run(req).await
    } else {
        let body = json!({
            "x402_version": 2,
            "resource": {
                "url": req.uri().to_string(),
                "method": "POST",
                "description": "Scematica control command — pay 0.01 SOL to execute"
            },
            "accepts": [{
                "scheme": "sol-transfer",
                "network": "solana-mainnet",
                "asset": "native",
                "amount": FEE_LAMPORTS,
                "pay_to": FEE_RECIPIENT,
                "max_timeout_seconds": 120
            }]
        });
        (StatusCode::PAYMENT_REQUIRED, Json(body)).into_response()
    }
}

// ── file paths ────────────────────────────────────────────────────────────────

const METRICS_FILE:      &str = "scematica-metrics.json";
const POOL_RADAR_FILE:   &str = "scematica-pool-radar.json";
const FILTER_STATS_FILE: &str = "scematica-filter-stats.json";
const NN_STATS_FILE:     &str = "scematica-nn-stats.json";
const LOG_FILE:          &str = "scematica-sniper.log";
const LOCK_FILE:         &str = "scematica-sniper.lock";
const TRADES_FILE:       &str = "scematica-trades.jsonl";
const SELL_MODE_FILE:    &str = "scematica-sell-mode.json";
const DUMP_MODE_FILE:    &str = "scematica-dump-mode.json";
const RATE_MODE_FILE:    &str = "scematica-rate-mode.json";

// ── query params ──────────────────────────────────────────────────────────────

#[derive(Deserialize)] struct LogQuery  { lines: Option<usize> }
#[derive(Deserialize)] struct LimitQuery { limit: Option<usize> }

// ── helpers ───────────────────────────────────────────────────────────────────

fn read_json_file(path: &str) -> Value {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Value::Null)
}

fn atomic_write(path: &str, value: &Value) -> bool {
    let tmp = format!("{}.tmp", path);
    if let Ok(s) = serde_json::to_string(value) {
        if fs::write(&tmp, &s).is_ok() {
            return fs::rename(&tmp, path).is_ok();
        }
    }
    false
}

// ── GET handlers ──────────────────────────────────────────────────────────────

async fn metrics_handler() -> impl IntoResponse {
    let v = read_json_file(METRICS_FILE);
    if v.is_null() {
        (StatusCode::NOT_FOUND, Json(json!({"error":"metrics not available"}))).into_response()
    } else {
        Json(v).into_response()
    }
}

async fn pools_handler(Query(q): Query<LimitQuery>) -> impl IntoResponse {
    let limit = q.limit.unwrap_or(50);
    let raw = read_json_file(POOL_RADAR_FILE);
    match raw.as_array() {
        Some(arr) => {
            let start = arr.len().saturating_sub(limit);
            let slice: Vec<Value> = arr[start..].iter().cloned().rev().collect();
            Json(json!({ "pools": slice, "total": arr.len() }))
        }
        None => Json(json!({ "pools": [], "total": 0 })),
    }
}

async fn filters_handler() -> impl IntoResponse {
    let v = read_json_file(FILTER_STATS_FILE);
    if v.is_null() {
        Json(json!({"pools_seen":0,"pools_passed":0,"rejections":{}}))
    } else {
        Json(v)
    }
}

async fn nn_handler() -> impl IntoResponse {
    let v = read_json_file(NN_STATS_FILE);
    if v.is_null() {
        Json(json!({"step_count":0,"epsilon":1.0,"ready_to_advise":false}))
    } else {
        Json(v)
    }
}

async fn logs_handler(Query(q): Query<LogQuery>) -> impl IntoResponse {
    let n = q.lines.unwrap_or(200).min(500);
    let lines = read_last_n_lines(LOG_FILE, n);
    Json(json!({ "lines": lines }))
}

async fn trades_handler(Query(q): Query<LimitQuery>) -> impl IntoResponse {
    let limit = q.limit.unwrap_or(50);
    let content = fs::read_to_string(TRADES_FILE).unwrap_or_default();
    let trades: Vec<Value> = content
        .lines()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect::<Vec<Value>>()
        .into_iter()
        .rev()
        .take(limit)
        .collect();
    Json(json!({ "trades": trades }))
}

fn read_last_n_lines(path: &str, n: usize) -> Vec<String> {
    let Ok(mut file) = fs::File::open(path) else { return vec![] };
    let Ok(len) = file.seek(SeekFrom::End(0)) else { return vec![] };
    let chunk = (n * 200).min(len as usize) as u64;
    let start = len.saturating_sub(chunk);
    if file.seek(SeekFrom::Start(start)).is_err() { return vec![] }
    let reader = BufReader::new(file);
    let all: Vec<String> = reader.lines().filter_map(|l| l.ok()).collect();
    all.into_iter().rev().take(n).rev().collect()
}

// ── control GET ───────────────────────────────────────────────────────────────

async fn controls_get_handler() -> impl IntoResponse {
    let sell = read_json_file(SELL_MODE_FILE);
    let dump = read_json_file(DUMP_MODE_FILE);
    let rate = read_json_file(RATE_MODE_FILE);

    let sell_mode  = sell.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
    let dump_mode  = dump.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
    let rate_mode  = rate.get("mode").and_then(|v| v.as_str()).unwrap_or("normal").to_string();
    let high_speed = rate.get("high_speed").and_then(|v| v.as_bool()).unwrap_or(false);

    Json(json!({
        "sell_mode":  sell_mode,
        "dump_mode":  dump_mode,
        "rate_mode":  rate_mode,
        "high_speed": high_speed,
    }))
}

// ── control POST bodies ───────────────────────────────────────────────────────

#[derive(Deserialize)] struct EnabledBody { enabled: bool }
#[derive(Deserialize)] struct RateModeBody { mode: String }

async fn sell_mode_handler(Json(b): Json<EnabledBody>) -> impl IntoResponse {
    let v = json!({ "enabled": b.enabled, "paused_by": "web" });
    if atomic_write(SELL_MODE_FILE, &v) {
        Json(json!({"ok": true, "sell_mode": b.enabled})).into_response()
    } else {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"write failed"}))).into_response()
    }
}

async fn dump_mode_handler(Json(b): Json<EnabledBody>) -> impl IntoResponse {
    let v = json!({ "enabled": b.enabled, "triggered_by": "web" });
    if atomic_write(DUMP_MODE_FILE, &v) {
        Json(json!({"ok": true, "dump_mode": b.enabled})).into_response()
    } else {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"write failed"}))).into_response()
    }
}

async fn rate_mode_handler(Json(b): Json<RateModeBody>) -> impl IntoResponse {
    let existing = read_json_file(RATE_MODE_FILE);
    let high_speed = existing.get("high_speed").and_then(|v| v.as_bool()).unwrap_or(false);
    let v = json!({ "mode": b.mode, "high_speed": high_speed });
    if atomic_write(RATE_MODE_FILE, &v) {
        Json(json!({"ok": true, "mode": b.mode})).into_response()
    } else {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"write failed"}))).into_response()
    }
}

async fn high_speed_handler(Json(b): Json<EnabledBody>) -> impl IntoResponse {
    let existing = read_json_file(RATE_MODE_FILE);
    let mode = existing.get("mode").and_then(|v| v.as_str()).unwrap_or("normal").to_string();
    let v = json!({ "mode": mode, "high_speed": b.enabled });
    if atomic_write(RATE_MODE_FILE, &v) {
        Json(json!({"ok": true, "high_speed": b.enabled})).into_response()
    } else {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"write failed"}))).into_response()
    }
}

// ── health ────────────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct HealthResponse { api: &'static str, sniper_running: bool, sniper_pid: Option<u32> }

async fn health_handler() -> Json<HealthResponse> {
    let (running, pid) = check_sniper_liveness();
    Json(HealthResponse { api: "ok", sniper_running: running, sniper_pid: pid })
}

fn check_sniper_liveness() -> (bool, Option<u32>) {
    let Ok(content) = fs::read_to_string(LOCK_FILE) else { return (false, None) };
    let pid: u32 = match content.trim().parse() { Ok(p) => p, Err(_) => return (false, None) };
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        let out = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {}", pid), "/NH"])
            .output();
        if let Ok(o) = out {
            return (String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()), Some(pid));
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        return (std::path::Path::new(&format!("/proc/{}", pid)).exists(), Some(pid));
    }
    (false, None)
}

async fn api_health() -> &'static str { "ok" }

// ── main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "scematica_api=info,tower_http=warn".into()),
        )
        .init();

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST])
        .allow_headers(Any);

    // POST control routes — each requires a valid X-Payment header (x402)
    let control_routes = Router::new()
        .route("/api/controls/sell-mode",  post(sell_mode_handler))
        .route("/api/controls/dump-mode",  post(dump_mode_handler))
        .route("/api/controls/rate-mode",  post(rate_mode_handler))
        .route("/api/controls/high-speed", post(high_speed_handler))
        .layer(axum::middleware::from_fn(x402_middleware));

    let app = Router::new()
        .route("/health",           get(api_health))
        .route("/api/metrics",      get(metrics_handler))
        .route("/api/pools",        get(pools_handler))
        .route("/api/filters",      get(filters_handler))
        .route("/api/logs",         get(logs_handler))
        .route("/api/trades",       get(trades_handler))
        .route("/api/nn",           get(nn_handler))
        .route("/api/health",       get(health_handler))
        .route("/api/controls",     get(controls_get_handler))
        .merge(control_routes)
        .layer(cors);

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3001);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!("Scematica API listening on http://{}", addr);
    axum::serve(tokio::net::TcpListener::bind(addr).await?, app).await?;
    Ok(())
}
