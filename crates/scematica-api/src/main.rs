//! Scematica HTTP API — serves sniper state files as JSON, accepts control POSTs.
//!
//! GET  /api/metrics                   scematica-metrics.json
//! GET  /api/pools                     scematica-pool-radar.json (last N)
//! GET  /api/filters                   scematica-filter-stats.json
//! GET  /api/logs                      last N lines of scematica-sniper.log
//! GET  /api/trades                    scematica-trades.jsonl (last N)
//! GET  /api/nn                        scematica-nn-stats.json
//! GET  /api/health                    sniper liveness (lock file + process)
//! GET  /api/controls                  full control state snapshot
//! POST /api/controls/sell-mode        { "enabled": bool }
//! POST /api/controls/dump-mode        { "enabled": bool }
//! POST /api/controls/rate-mode        { "mode": "bearish"|"micro"|"safe"|"balanced"|"aggressive"|"degen"|"bullish"|"moon" }
//! POST /api/controls/high-speed       { "enabled": bool }
//! POST /api/controls/moon-chase       { "enabled": bool }
//! POST /api/controls/builder-mode     { "mode": "off"|"growth"|"builder"|"super_builder" }
//! GET  /health                        API server liveness

use axum::{
    extract::Query,
    http::Method,
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader, Seek, SeekFrom},
    net::SocketAddr,
};
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

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
const MOON_CHASE_FILE:   &str = "scematica-moon-chase.json";
const BUILDER_MODE_FILE: &str = "scematica-builder-mode.json";
const HIGH_SPEED_FILE:   &str = "scematica-highspeed-mode.json";

// ── query params ──────────────────────────────────────────────────────────────

#[derive(Deserialize)] struct LogQuery   { lines: Option<usize> }
#[derive(Deserialize)] struct LimitQuery { limit: Option<usize> }

// ── rate preset lookup ────────────────────────────────────────────────────────

/// Returns (tp_pct, sl_pct, multiplier) for a given rate mode name.
fn rate_preset(mode: &str) -> (f64, f64, f64) {
    match mode {
        "bearish"    => (45.0,   8.0,  0.3),
        "micro"      => (60.0,   10.0, 0.1),
        "safe"       => (75.0,   10.0, 0.5),
        "balanced"   => (150.0,  15.0, 1.0),
        "aggressive" => (300.0,  25.0, 2.0),
        "degen"      => (450.0,  40.0, 4.0),
        "bullish"    => (750.0,  50.0, 6.0),
        "moon"       => (1200.0, 60.0, 8.0),
        _            => (150.0,  15.0, 1.0), // balanced default
    }
}

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

fn file_exists(path: &str) -> bool {
    std::path::Path::new(path).exists()
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
    let sell    = read_json_file(SELL_MODE_FILE);
    let dump    = read_json_file(DUMP_MODE_FILE);
    let rate    = read_json_file(RATE_MODE_FILE);
    let builder = read_json_file(BUILDER_MODE_FILE);

    let sell_mode  = sell.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
    let dump_mode  = dump.get("enabled").and_then(|v| v.as_bool()).unwrap_or(false);
    let rate_mode  = rate.get("mode").and_then(|v| v.as_str()).unwrap_or("balanced").to_string();
    let high_speed = file_exists(HIGH_SPEED_FILE)
        || rate.get("high_speed").and_then(|v| v.as_bool()).unwrap_or(false);
    let moon_chase = file_exists(MOON_CHASE_FILE);

    let builder_mode       = builder.get("mode").and_then(|v| v.as_str()).unwrap_or("off").to_string();
    let builder_target_sol = builder.get("target_sol").and_then(|v| v.as_f64()).unwrap_or(0.0);

    // TP/SL/multiplier: prefer values written in the file, fall back to preset
    let (preset_tp, preset_sl, preset_mult) = rate_preset(&rate_mode);
    let tp_pct     = rate.get("tp_pct")    .and_then(|v| v.as_f64()).unwrap_or(preset_tp);
    let sl_pct     = rate.get("sl_pct")    .and_then(|v| v.as_f64()).unwrap_or(preset_sl);
    let multiplier = rate.get("multiplier").and_then(|v| v.as_f64()).unwrap_or(preset_mult);

    Json(json!({
        "sell_mode":          sell_mode,
        "dump_mode":          dump_mode,
        "rate_mode":          rate_mode,
        "high_speed":         high_speed,
        "moon_chase":         moon_chase,
        "builder_mode":       builder_mode,
        "builder_target_sol": builder_target_sol,
        "tp_pct":             tp_pct,
        "sl_pct":             sl_pct,
        "multiplier":         multiplier,
    }))
}

// ── control POST bodies ───────────────────────────────────────────────────────

#[derive(Deserialize)] struct EnabledBody     { enabled: bool }
#[derive(Deserialize)] struct RateModeBody    { mode: String }
#[derive(Deserialize)] struct BuilderModeBody { mode: String }

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
    let (tp_pct, sl_pct, multiplier) = rate_preset(&b.mode);
    let existing   = read_json_file(RATE_MODE_FILE);
    let high_speed = file_exists(HIGH_SPEED_FILE)
        || existing.get("high_speed").and_then(|v| v.as_bool()).unwrap_or(false);

    let v = json!({
        "mode":       b.mode,
        "tp_pct":     tp_pct,
        "sl_pct":     sl_pct,
        "multiplier": multiplier,
        "high_speed": high_speed,
    });
    if atomic_write(RATE_MODE_FILE, &v) {
        Json(json!({"ok": true, "mode": b.mode, "tp_pct": tp_pct, "sl_pct": sl_pct, "multiplier": multiplier})).into_response()
    } else {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"write failed"}))).into_response()
    }
}

async fn high_speed_handler(Json(b): Json<EnabledBody>) -> impl IntoResponse {
    // Write both the presence-file (sniper watches) and the flag in rate-mode (API state)
    if b.enabled {
        let _ = fs::write(HIGH_SPEED_FILE, "{}");
    } else {
        let _ = fs::remove_file(HIGH_SPEED_FILE);
    }
    let existing = read_json_file(RATE_MODE_FILE);
    let mode = existing.get("mode").and_then(|v| v.as_str()).unwrap_or("balanced").to_string();
    let (tp_pct, sl_pct, multiplier) = rate_preset(&mode);
    let v = json!({ "mode": mode, "tp_pct": tp_pct, "sl_pct": sl_pct, "multiplier": multiplier, "high_speed": b.enabled });
    atomic_write(RATE_MODE_FILE, &v);
    Json(json!({"ok": true, "high_speed": b.enabled}))
}

async fn moon_chase_handler(Json(b): Json<EnabledBody>) -> impl IntoResponse {
    if b.enabled {
        if fs::write(MOON_CHASE_FILE, "{}").is_ok() {
            Json(json!({"ok": true, "moon_chase": true})).into_response()
        } else {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"write failed"}))).into_response()
        }
    } else {
        let _ = fs::remove_file(MOON_CHASE_FILE);
        Json(json!({"ok": true, "moon_chase": false})).into_response()
    }
}

async fn builder_mode_handler(Json(b): Json<BuilderModeBody>) -> impl IntoResponse {
    if b.mode == "off" {
        let _ = fs::remove_file(BUILDER_MODE_FILE);
        return Json(json!({"ok": true, "builder_mode": "off"})).into_response();
    }
    let target_sol: f64 = match b.mode.as_str() {
        "growth"       => 0.2,
        "builder"      => 1.0,
        "super_builder" => 3.0,
        _ => return (StatusCode::BAD_REQUEST, Json(json!({"error":"unknown builder mode"}))).into_response(),
    };
    let v = json!({ "mode": b.mode, "target_sol": target_sol });
    if atomic_write(BUILDER_MODE_FILE, &v) {
        Json(json!({"ok": true, "builder_mode": b.mode, "target_sol": target_sol})).into_response()
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

    let app = Router::new()
        .route("/health",                          get(api_health))
        .route("/api/metrics",                     get(metrics_handler))
        .route("/api/pools",                       get(pools_handler))
        .route("/api/filters",                     get(filters_handler))
        .route("/api/logs",                        get(logs_handler))
        .route("/api/trades",                      get(trades_handler))
        .route("/api/nn",                          get(nn_handler))
        .route("/api/health",                      get(health_handler))
        .route("/api/controls",                    get(controls_get_handler))
        .route("/api/controls/sell-mode",          post(sell_mode_handler))
        .route("/api/controls/dump-mode",          post(dump_mode_handler))
        .route("/api/controls/rate-mode",          post(rate_mode_handler))
        .route("/api/controls/high-speed",         post(high_speed_handler))
        .route("/api/controls/moon-chase",         post(moon_chase_handler))
        .route("/api/controls/builder-mode",       post(builder_mode_handler))
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
