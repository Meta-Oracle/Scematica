//! Scematica HTTP API — serves sniper state files as JSON endpoints.
//!
//! All data is read from the same JSON files the sniper and dashboard use.
//! Run alongside the sniper; the frontend polls these endpoints.
//!
//! Endpoints:
//!   GET /api/metrics   — scematica-metrics.json
//!   GET /api/pools     — scematica-pool-radar.json (last 50 entries)
//!   GET /api/filters   — scematica-filter-stats.json
//!   GET /api/logs      — last 200 lines of scematica-sniper.log
//!   GET /api/health    — sniper liveness (lock file + process check)
//!   GET /api/nn        — scematica-nn-stats.json
//!   GET /health        — API server liveness

use axum::{
    extract::Query,
    http::{Method, StatusCode},
    response::{IntoResponse, Json},
    routing::get,
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
use tracing::info;

const METRICS_FILE: &str = "scematica-metrics.json";
const POOL_RADAR_FILE: &str = "scematica-pool-radar.json";
const FILTER_STATS_FILE: &str = "scematica-filter-stats.json";
const NN_STATS_FILE: &str = "scematica-nn-stats.json";
const LOG_FILE: &str = "scematica-sniper.log";
const LOCK_FILE: &str = "scematica-sniper.lock";
const TRADES_FILE: &str = "scematica-trades.jsonl";

#[derive(Deserialize)]
struct LogQuery {
    lines: Option<usize>,
}

#[derive(Deserialize)]
struct PoolQuery {
    limit: Option<usize>,
}

fn read_json_file(path: &str) -> Value {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Value::Null)
}

async fn metrics_handler() -> impl IntoResponse {
    let v = read_json_file(METRICS_FILE);
    if v.is_null() {
        (StatusCode::NOT_FOUND, Json(json!({"error": "metrics not available yet"}))).into_response()
    } else {
        Json(v).into_response()
    }
}

async fn pools_handler(Query(q): Query<PoolQuery>) -> impl IntoResponse {
    let limit = q.limit.unwrap_or(50);
    let raw = read_json_file(POOL_RADAR_FILE);
    let pools = match raw.as_array() {
        Some(arr) => {
            let start = arr.len().saturating_sub(limit);
            let slice: Vec<Value> = arr[start..].iter().cloned().rev().collect();
            Json(json!({ "pools": slice, "total": arr.len() }))
        }
        None => Json(json!({ "pools": [], "total": 0 })),
    };
    pools
}

async fn filters_handler() -> impl IntoResponse {
    let v = read_json_file(FILTER_STATS_FILE);
    if v.is_null() {
        Json(json!({"pools_seen": 0, "pools_passed": 0, "rejections": {}}))
    } else {
        Json(v)
    }
}

async fn nn_handler() -> impl IntoResponse {
    let v = read_json_file(NN_STATS_FILE);
    if v.is_null() {
        Json(json!({"step_count": 0, "epsilon": 1.0, "ready_to_advise": false}))
    } else {
        Json(v)
    }
}

async fn logs_handler(Query(q): Query<LogQuery>) -> impl IntoResponse {
    let n = q.lines.unwrap_or(200).min(500);
    let lines = read_last_n_lines(LOG_FILE, n);
    Json(json!({ "lines": lines }))
}

fn read_last_n_lines(path: &str, n: usize) -> Vec<String> {
    let Ok(mut file) = fs::File::open(path) else { return vec![] };
    let Ok(len) = file.seek(SeekFrom::End(0)) else { return vec![] };

    // Walk backwards to find the Nth newline
    let chunk = (n * 200).min(len as usize) as u64;
    let start = len.saturating_sub(chunk);
    if file.seek(SeekFrom::Start(start)).is_err() { return vec![] };

    let reader = BufReader::new(file);
    let all: Vec<String> = reader.lines().filter_map(|l| l.ok()).collect();
    all.into_iter().rev().take(n).rev().collect()
}

async fn trades_handler(Query(q): Query<PoolQuery>) -> impl IntoResponse {
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

#[derive(Serialize)]
struct HealthResponse {
    api: &'static str,
    sniper_running: bool,
    sniper_pid: Option<u32>,
}

async fn health_handler() -> Json<HealthResponse> {
    let (running, pid) = check_sniper_liveness();
    Json(HealthResponse {
        api: "ok",
        sniper_running: running,
        sniper_pid: pid,
    })
}

fn check_sniper_liveness() -> (bool, Option<u32>) {
    let Ok(content) = fs::read_to_string(LOCK_FILE) else { return (false, None) };
    let pid: u32 = match content.trim().parse() {
        Ok(p) => p,
        Err(_) => return (false, None),
    };

    // Check if the process is actually running
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        let out = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {}", pid), "/NH"])
            .output();
        if let Ok(o) = out {
            let s = String::from_utf8_lossy(&o.stdout);
            return (s.contains(&pid.to_string()), Some(pid));
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let path = std::path::Path::new(&format!("/proc/{}", pid));
        return (path.exists(), Some(pid));
    }

    (false, None)
}

async fn api_health() -> &'static str { "ok" }

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
        .allow_methods([Method::GET])
        .allow_headers(Any);

    let app = Router::new()
        .route("/health", get(api_health))
        .route("/api/metrics", get(metrics_handler))
        .route("/api/pools", get(pools_handler))
        .route("/api/filters", get(filters_handler))
        .route("/api/logs", get(logs_handler))
        .route("/api/trades", get(trades_handler))
        .route("/api/nn", get(nn_handler))
        .route("/api/health", get(health_handler))
        .layer(cors);

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3001);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!("Scematica API listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
