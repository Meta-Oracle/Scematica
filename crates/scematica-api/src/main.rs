//! Scematica HTTP API — serves sniper state files as JSON, accepts control POSTs.
//!
//! GET  /api/metrics                   scematica-metrics.json
//! GET  /api/pools                     scematica-pool-radar.json (last N)
//! GET  /api/filters                   scematica-filter-stats.json
//! GET  /api/logs                      last N lines of scematica-sniper.log
//! GET  /api/trades                    scematica-trades.jsonl (last N)
//! GET  /api/decisions                 scematica-pool-decisions.jsonl (last N)
//! GET  /api/tx-telemetry              scematica-tx-telemetry.jsonl (last N)
//! GET  /api/nn                        scematica-nn-stats.json
//! GET  /api/nn-advice                 scematica-nn-advice.json
//! GET  /api/positions                 scematica-positions.json (live open positions)
//! GET  /api/tournament                scematica-nn-tournament.json (DQ* agent tournament)
//! GET  /api/intelligence              combined NN/decision/tx snapshot
//! GET  /api/health                    sniper liveness (lock file + process)
//! GET  /api/controls                  full control state snapshot
//! POST /api/controls/sell-mode        { "enabled": bool }
//! POST /api/controls/dump-mode        { "enabled": bool }
//! POST /api/controls/rate-mode        { "mode": "bearish"|"micro"|"safe"|"balanced"|"aggressive"|"degen"|"bullish"|"moon" }
//! POST /api/controls/params           { "tp_pct"?: f64, "sl_pct"?: f64, "multiplier"?: f64 }  (sliders)
//! POST /api/controls/high-speed       { "enabled": bool }
//! POST /api/controls/moon-chase       { "enabled": bool }
//! POST /api/controls/builder-mode     { "mode": "off"|"growth"|"builder"|"super_builder" }
//! POST /api/push/register             { "token": "<fcm>", "platform"?: "android" }  (gated)
//! POST /api/push/test                 send a test push to registered devices        (gated)
//! GET  /health                        API server liveness
//!
//! Control + push POSTs are gated by `SCEMATICA_API_TOKEN` (Bearer). Trade push
//! delivery is enabled by `FCM_SERVER_KEY`; both are no-ops when unset.

use axum::http::StatusCode;
use axum::{
    extract::{Query, Request},
    http::{HeaderMap, Method},
    middleware::{self, Next},
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
    path::{Path, PathBuf},
};
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

// ── file paths ────────────────────────────────────────────────────────────────

const DATA_DIR_ENV: &str = "SCEMATICA_DATA_DIR";
const METRICS_FILE: &str = "scematica-metrics.json";
const POOL_RADAR_FILE: &str = "scematica-pool-radar.json";
const FILTER_STATS_FILE: &str = "scematica-filter-stats.json";
const NN_STATS_FILE: &str = "scematica-nn-stats.json";
const NN_ADVICE_FILE: &str = "scematica-nn-advice.json";
const POSITIONS_FILE: &str = "scematica-positions.json";
const TOURNAMENT_FILE: &str = "scematica-nn-tournament.json";
const LOG_FILE: &str = "scematica-sniper.log";
const LOCK_FILE: &str = "scematica-sniper.lock";
const TRADES_FILE: &str = "scematica-trades.jsonl";
const POOL_DECISIONS_FILE: &str = "scematica-pool-decisions.jsonl";
const TX_TELEMETRY_FILE: &str = "scematica-tx-telemetry.jsonl";
const SELL_MODE_FILE: &str = "scematica-sell-mode.json";
const DUMP_MODE_FILE: &str = "scematica-dump-mode.json";
const RATE_MODE_FILE: &str = "scematica-rate-mode.json";
const MOON_CHASE_FILE: &str = "scematica-moon-chase.json";
const BUILDER_MODE_FILE: &str = "scematica-builder-mode.json";
const HIGH_SPEED_FILE: &str = "scematica-highspeed-mode.json";

// ── query params ──────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct LogQuery {
    lines: Option<usize>,
}
#[derive(Deserialize)]
struct LimitQuery {
    limit: Option<usize>,
}

// ── rate preset lookup ────────────────────────────────────────────────────────

/// Returns (tp_pct, sl_pct, multiplier) for a given rate mode name.
fn rate_preset(mode: &str) -> (f64, f64, f64) {
    match mode {
        "bearish" => (45.0, 8.0, 0.3),
        "micro" => (60.0, 10.0, 0.1),
        "safe" => (75.0, 10.0, 0.5),
        "balanced" => (150.0, 15.0, 1.0),
        "aggressive" => (300.0, 25.0, 2.0),
        "degen" => (450.0, 40.0, 4.0),
        "bullish" => (750.0, 50.0, 6.0),
        "moon" => (1200.0, 60.0, 8.0),
        _ => (150.0, 15.0, 1.0), // balanced default
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn artifact_dir() -> PathBuf {
    if let Ok(value) = std::env::var(DATA_DIR_ENV) {
        let value = value.trim();
        if !value.is_empty() {
            return PathBuf::from(value);
        }
    }

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if cwd.join("config.toml").exists() || cwd.join(".git").exists() {
        return cwd;
    }

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for ancestor in manifest_dir.ancestors() {
        if ancestor.join("Cargo.toml").exists() && ancestor.join("config.toml").exists() {
            return ancestor.to_path_buf();
        }
    }

    cwd
}

fn artifact_path(path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        artifact_dir().join(path)
    }
}

fn read_json_file(path: &str) -> Value {
    fs::read_to_string(artifact_path(path))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(Value::Null)
}

fn atomic_write(path: &str, value: &Value) -> bool {
    let path = artifact_path(path);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    if let Ok(s) = serde_json::to_string(value) {
        if fs::write(&tmp, &s).is_ok() {
            return fs::rename(&tmp, path).is_ok();
        }
    }
    false
}

fn file_exists(path: &str) -> bool {
    artifact_path(path).exists()
}

fn read_last_jsonl_values(path: &str, limit: usize) -> Vec<Value> {
    fs::read_to_string(artifact_path(path))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect::<Vec<Value>>()
        .into_iter()
        .rev()
        .take(limit)
        .collect()
}

// ── GET handlers ──────────────────────────────────────────────────────────────

async fn metrics_handler() -> impl IntoResponse {
    let v = read_json_file(METRICS_FILE);
    if v.is_null() {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error":"metrics not available"})),
        )
            .into_response()
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

async fn nn_advice_handler() -> impl IntoResponse {
    let v = read_json_file(NN_ADVICE_FILE);
    if v.is_null() {
        Json(json!({
            "action": "NoAdvice",
            "action_index": 0,
            "q_values": [],
            "top_reason": "No DQ* advice snapshot has been written yet.",
            "confidence": 0.0
        }))
    } else {
        Json(v)
    }
}

/// Live open-position snapshots (unrealized PnL, dynamic TP/SL, escalations) —
/// written every 1s by the sniper's position-flush task to `scematica-positions.json`.
async fn positions_handler() -> impl IntoResponse {
    let v = read_json_file(POSITIONS_FILE);
    Json(if v.is_array() { v } else { json!([]) })
}

/// Deep Q* multi-agent tournament state (conservative/balanced/aggressive variants
/// racing in paper-trading mode; the highest-reward variant is promoted to primary).
async fn tournament_handler() -> impl IntoResponse {
    let v = read_json_file(TOURNAMENT_FILE);
    if v.is_null() {
        Json(json!({
            "primary_idx": 1,
            "steps_since_eval": 0,
            "eval_freq": 1000,
            "agent_names": ["conservative", "balanced", "aggressive"],
            "agent_total_rewards": [0.0, 0.0, 0.0],
            "agent_epsilons": [1.0, 1.0, 1.0]
        }))
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
    let trades = read_last_jsonl_values(TRADES_FILE, limit);
    Json(json!({ "trades": trades }))
}

async fn decisions_handler(Query(q): Query<LimitQuery>) -> impl IntoResponse {
    let limit = q.limit.unwrap_or(80).min(250);
    let decisions = read_last_jsonl_values(POOL_DECISIONS_FILE, limit);
    Json(json!({ "decisions": decisions }))
}

async fn tx_telemetry_handler(Query(q): Query<LimitQuery>) -> impl IntoResponse {
    let limit = q.limit.unwrap_or(80).min(250);
    let telemetry = read_last_jsonl_values(TX_TELEMETRY_FILE, limit);
    Json(json!({ "telemetry": telemetry }))
}

async fn intelligence_handler(Query(q): Query<LimitQuery>) -> impl IntoResponse {
    let limit = q.limit.unwrap_or(80).min(250);
    let nn = read_json_file(NN_STATS_FILE);
    let advice = read_json_file(NN_ADVICE_FILE);
    let decisions = read_last_jsonl_values(POOL_DECISIONS_FILE, limit);
    let telemetry = read_last_jsonl_values(TX_TELEMETRY_FILE, limit);
    Json(json!({
        "nn": if nn.is_null() {
            json!({"step_count":0,"epsilon":1.0,"ready_to_advise":false})
        } else {
            nn
        },
        "advice": if advice.is_null() {
            json!({
                "action": "NoAdvice",
                "action_index": 0,
                "q_values": [],
                "top_reason": "No DQ* advice snapshot has been written yet.",
                "confidence": 0.0
            })
        } else {
            advice
        },
        "decisions": decisions,
        "telemetry": telemetry,
        "paths": {
            "nn_advice": artifact_path(NN_ADVICE_FILE).display().to_string(),
            "pool_decisions": artifact_path(POOL_DECISIONS_FILE).display().to_string(),
            "tx_telemetry": artifact_path(TX_TELEMETRY_FILE).display().to_string()
        }
    }))
}

fn read_last_n_lines(path: &str, n: usize) -> Vec<String> {
    let Ok(mut file) = fs::File::open(artifact_path(path)) else {
        return vec![];
    };
    let Ok(len) = file.seek(SeekFrom::End(0)) else {
        return vec![];
    };
    let chunk = (n * 200).min(len as usize) as u64;
    let start = len.saturating_sub(chunk);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return vec![];
    }
    let reader = BufReader::new(file);
    let all: Vec<String> = reader.lines().filter_map(|l| l.ok()).collect();
    all.into_iter().rev().take(n).rev().collect()
}

// ── control GET ───────────────────────────────────────────────────────────────

async fn controls_get_handler() -> impl IntoResponse {
    let sell = read_json_file(SELL_MODE_FILE);
    let dump = read_json_file(DUMP_MODE_FILE);
    let rate = read_json_file(RATE_MODE_FILE);
    let builder = read_json_file(BUILDER_MODE_FILE);

    let sell_mode = sell
        .get("enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let dump_mode = dump
        .get("enabled")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let rate_mode = rate
        .get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("balanced")
        .to_string();
    let high_speed = file_exists(HIGH_SPEED_FILE)
        || rate
            .get("high_speed")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
    let moon_chase = file_exists(MOON_CHASE_FILE);

    let builder_mode = builder
        .get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("off")
        .to_string();
    let builder_target_sol = builder
        .get("target_sol")
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);

    // TP/SL/multiplier: prefer values written in the file, fall back to preset
    let (preset_tp, preset_sl, preset_mult) = rate_preset(&rate_mode);
    let tp_pct = rate
        .get("tp_pct")
        .and_then(|v| v.as_f64())
        .unwrap_or(preset_tp);
    let sl_pct = rate
        .get("sl_pct")
        .and_then(|v| v.as_f64())
        .unwrap_or(preset_sl);
    let multiplier = rate
        .get("multiplier")
        .and_then(|v| v.as_f64())
        .unwrap_or(preset_mult);

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

#[derive(Deserialize)]
struct EnabledBody {
    enabled: bool,
}
#[derive(Deserialize)]
struct RateModeBody {
    mode: String,
}
#[derive(Deserialize)]
struct BuilderModeBody {
    mode: String,
}
#[derive(Deserialize)]
struct ParamsBody {
    #[serde(default)]
    tp_pct: Option<f64>,
    #[serde(default)]
    sl_pct: Option<f64>,
    #[serde(default)]
    multiplier: Option<f64>,
}

async fn sell_mode_handler(Json(b): Json<EnabledBody>) -> impl IntoResponse {
    let v = json!({ "enabled": b.enabled, "paused_by": "web" });
    if atomic_write(SELL_MODE_FILE, &v) {
        Json(json!({"ok": true, "sell_mode": b.enabled})).into_response()
    } else {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":"write failed"})),
        )
            .into_response()
    }
}

async fn dump_mode_handler(Json(b): Json<EnabledBody>) -> impl IntoResponse {
    let v = json!({ "enabled": b.enabled, "triggered_by": "web" });
    if atomic_write(DUMP_MODE_FILE, &v) {
        Json(json!({"ok": true, "dump_mode": b.enabled})).into_response()
    } else {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":"write failed"})),
        )
            .into_response()
    }
}

async fn rate_mode_handler(Json(b): Json<RateModeBody>) -> impl IntoResponse {
    let (tp_pct, sl_pct, multiplier) = rate_preset(&b.mode);
    let existing = read_json_file(RATE_MODE_FILE);
    let high_speed = file_exists(HIGH_SPEED_FILE)
        || existing
            .get("high_speed")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

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
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":"write failed"})),
        )
            .into_response()
    }
}

async fn params_handler(Json(b): Json<ParamsBody>) -> impl IntoResponse {
    // Custom TP/SL/multiplier from the sliders — override the numeric params while
    // keeping the current rate mode + high-speed flag. controls_get already prefers file
    // values over the preset, so slider values take effect live.
    let existing = read_json_file(RATE_MODE_FILE);
    let mode = existing
        .get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("balanced")
        .to_string();
    let (ptp, psl, pmult) = rate_preset(&mode);
    let cur = |k: &str, d: f64| existing.get(k).and_then(|v| v.as_f64()).unwrap_or(d);
    let tp = b.tp_pct.unwrap_or_else(|| cur("tp_pct", ptp)).clamp(1.0, 10_000.0);
    let sl = b.sl_pct.unwrap_or_else(|| cur("sl_pct", psl)).clamp(1.0, 99.0);
    let mult = b.multiplier.unwrap_or_else(|| cur("multiplier", pmult)).clamp(0.1, 10.0);
    let high_speed = existing.get("high_speed").and_then(|v| v.as_bool()).unwrap_or(false);
    let v = json!({ "mode": mode, "tp_pct": tp, "sl_pct": sl, "multiplier": mult, "high_speed": high_speed });
    if atomic_write(RATE_MODE_FILE, &v) {
        Json(json!({"ok": true, "tp_pct": tp, "sl_pct": sl, "multiplier": mult})).into_response()
    } else {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":"write failed"})),
        )
            .into_response()
    }
}

async fn high_speed_handler(Json(b): Json<EnabledBody>) -> impl IntoResponse {
    // Write both the presence-file (sniper watches) and the flag in rate-mode (API state)
    if b.enabled {
        let _ = fs::write(artifact_path(HIGH_SPEED_FILE), "{}");
    } else {
        let _ = fs::remove_file(artifact_path(HIGH_SPEED_FILE));
    }
    let existing = read_json_file(RATE_MODE_FILE);
    let mode = existing
        .get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("balanced")
        .to_string();
    let (tp_pct, sl_pct, multiplier) = rate_preset(&mode);
    let v = json!({ "mode": mode, "tp_pct": tp_pct, "sl_pct": sl_pct, "multiplier": multiplier, "high_speed": b.enabled });
    atomic_write(RATE_MODE_FILE, &v);
    Json(json!({"ok": true, "high_speed": b.enabled}))
}

async fn moon_chase_handler(Json(b): Json<EnabledBody>) -> impl IntoResponse {
    if b.enabled {
        if fs::write(artifact_path(MOON_CHASE_FILE), "{}").is_ok() {
            Json(json!({"ok": true, "moon_chase": true})).into_response()
        } else {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error":"write failed"})),
            )
                .into_response()
        }
    } else {
        let _ = fs::remove_file(artifact_path(MOON_CHASE_FILE));
        Json(json!({"ok": true, "moon_chase": false})).into_response()
    }
}

async fn builder_mode_handler(Json(b): Json<BuilderModeBody>) -> impl IntoResponse {
    if b.mode == "off" {
        let _ = fs::remove_file(artifact_path(BUILDER_MODE_FILE));
        return Json(json!({"ok": true, "builder_mode": "off"})).into_response();
    }
    let target_sol: f64 = match b.mode.as_str() {
        "growth" => 0.2,
        "builder" => 1.0,
        "super_builder" => 3.0,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error":"unknown builder mode"})),
            )
                .into_response()
        }
    };
    let v = json!({ "mode": b.mode, "target_sol": target_sol });
    if atomic_write(BUILDER_MODE_FILE, &v) {
        Json(json!({"ok": true, "builder_mode": b.mode, "target_sol": target_sol})).into_response()
    } else {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error":"write failed"})),
        )
            .into_response()
    }
}

// ── health ────────────────────────────────────────────────────────────────────

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
    let Ok(content) = fs::read_to_string(artifact_path(LOCK_FILE)) else {
        return (false, None);
    };
    let pid: u32 = match content.trim().parse() {
        Ok(p) => p,
        Err(_) => return (false, None),
    };
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        let out = Command::new("tasklist")
            .args(["/FI", &format!("PID eq {}", pid), "/NH"])
            .output();
        if let Ok(o) = out {
            return (
                String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()),
                Some(pid),
            );
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        return (
            std::path::Path::new(&format!("/proc/{}", pid)).exists(),
            Some(pid),
        );
    }
    (false, None)
}

async fn api_health() -> &'static str {
    "ok"
}

// ── auth ────────────────────────────────────────────────────────────────────────
//
// The control POSTs mutate a live sniper (pause buys, force-sell, dump at min_out=0),
// so a self-hosted operator exposing this API to a phone over the network MUST gate
// them. When `SCEMATICA_API_TOKEN` is set, every control route requires the token via
// `Authorization: Bearer <token>` (or `X-Scematica-Token: <token>`); the token is the
// pairing secret the mobile app stores. When it is unset the gate is open, preserving
// the local-dev / same-host default. Read routes are unaffected — front them with a
// reverse proxy if the whole instance must be private.

fn present_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(str::to_string)
        .or_else(|| {
            headers
                .get("x-scematica-token")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        })
}

async fn require_token(headers: HeaderMap, req: Request, next: Next) -> Result<Response, StatusCode> {
    match std::env::var("SCEMATICA_API_TOKEN") {
        Ok(expected) if !expected.is_empty() => {
            // Constant-ish comparison is overkill for a LAN pairing secret, but reject
            // cleanly on any mismatch or absence.
            if present_token(&headers).as_deref() == Some(expected.as_str()) {
                Ok(next.run(req).await)
            } else {
                Err(StatusCode::UNAUTHORIZED)
            }
        }
        _ => Ok(next.run(req).await), // no token configured → open (local dev)
    }
}

// ── push notifications (FCM) ────────────────────────────────────────────────────
//
// The single biggest retention lever for the companion app: alert the operator on new
// pools / fills / PnL with the app closed. Devices register their FCM token (gated,
// so only paired phones enrol); a background task tails the sniper's trade log and
// pushes on each new trade. Everything is a no-op unless `FCM_SERVER_KEY` is set, so
// the default local build is unaffected. Legacy HTTP server-key API — migrate to FCM
// HTTP v1 (OAuth service account) when you outgrow it.

const FCM_ENDPOINT: &str = "https://fcm.googleapis.com/fcm/send";
const PUSH_TOKENS_FILE: &str = "scematica-push-tokens.json";

#[derive(Deserialize)]
struct PushRegisterBody {
    token: String,
    #[serde(default)]
    platform: Option<String>,
}

fn push_tokens() -> Vec<String> {
    read_json_file(PUSH_TOKENS_FILE)
        .get("tokens")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|t| t.as_str().map(String::from)).collect())
        .unwrap_or_default()
}

fn add_push_token(token: &str) -> usize {
    let mut tokens = push_tokens();
    if !tokens.iter().any(|t| t == token) {
        tokens.push(token.to_string());
        // Bound the store so a leaked endpoint can't grow it without limit.
        if tokens.len() > 200 {
            let excess = tokens.len() - 200;
            tokens.drain(0..excess);
        }
        atomic_write(PUSH_TOKENS_FILE, &json!({ "tokens": tokens }));
    }
    tokens.len()
}

/// True when push delivery is configured — a service account (HTTP v1) or a legacy key.
fn fcm_configured() -> bool {
    std::env::var("FCM_SERVICE_ACCOUNT").map(|s| !s.is_empty()).unwrap_or(false)
        || std::env::var("FCM_SERVER_KEY").map(|s| !s.is_empty()).unwrap_or(false)
}

/// Send one notification to every registered device. Prefers **FCM HTTP v1** (service
/// account at `FCM_SERVICE_ACCOUNT`); falls back to the **legacy** server key
/// (`FCM_SERVER_KEY`, deprecated). No-op when neither is set or no devices registered.
async fn fcm_send(title: &str, body: &str, data: Value) -> bool {
    let tokens = push_tokens();
    if tokens.is_empty() {
        return false;
    }
    if let Some(sa) = load_service_account() {
        return fcm_send_v1(&sa, &tokens, title, body, &data).await;
    }
    fcm_send_legacy(&tokens, title, body, &data).await
}

async fn fcm_send_legacy(tokens: &[String], title: &str, body: &str, data: &Value) -> bool {
    let key = match std::env::var("FCM_SERVER_KEY") {
        Ok(k) if !k.is_empty() => k,
        _ => return false,
    };
    let payload = json!({
        "registration_ids": tokens,
        "notification": { "title": title, "body": body },
        "data": data,
        "priority": "high",
    });
    match reqwest::Client::new()
        .post(FCM_ENDPOINT)
        .header("Authorization", format!("key={key}"))
        .json(&payload)
        .send()
        .await
    {
        Ok(r) => r.status().is_success(),
        Err(e) => {
            tracing::warn!("fcm legacy send failed: {e}");
            false
        }
    }
}

// ── FCM HTTP v1: service account → OAuth2 access token → per-token send ──────────

struct ServiceAccount {
    client_email: String,
    private_key: String,
    project_id: String,
    token_uri: String,
}

fn load_service_account() -> Option<ServiceAccount> {
    let path = std::env::var("FCM_SERVICE_ACCOUNT").ok().filter(|p| !p.is_empty())?;
    let v: Value = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
    Some(ServiceAccount {
        client_email: v.get("client_email")?.as_str()?.to_string(),
        private_key: v.get("private_key")?.as_str()?.to_string(),
        project_id: v.get("project_id")?.as_str()?.to_string(),
        token_uri: v
            .get("token_uri")
            .and_then(|s| s.as_str())
            .unwrap_or("https://oauth2.googleapis.com/token")
            .to_string(),
    })
}

#[derive(Serialize)]
struct JwtClaims<'a> {
    iss: &'a str,
    scope: &'a str,
    aud: &'a str,
    iat: u64,
    exp: u64,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Cached `(access_token, expiry_unix)`. FCM access tokens live ~1h; refresh early.
static FCM_TOKEN: std::sync::OnceLock<std::sync::Mutex<Option<(String, u64)>>> =
    std::sync::OnceLock::new();

async fn fcm_access_token(sa: &ServiceAccount) -> Option<String> {
    let cache = FCM_TOKEN.get_or_init(|| std::sync::Mutex::new(None));
    if let Ok(g) = cache.lock() {
        if let Some((tok, exp)) = g.as_ref() {
            if now_secs() + 60 < *exp {
                return Some(tok.clone());
            }
        }
    }
    let now = now_secs();
    let claims = JwtClaims {
        iss: &sa.client_email,
        scope: "https://www.googleapis.com/auth/firebase.messaging",
        aud: &sa.token_uri,
        iat: now,
        exp: now + 3600,
    };
    let key = match jsonwebtoken::EncodingKey::from_rsa_pem(sa.private_key.as_bytes()) {
        Ok(k) => k,
        Err(e) => {
            tracing::warn!("fcm: bad service-account private_key: {e}");
            return None;
        }
    };
    let jwt = jsonwebtoken::encode(
        &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256),
        &claims,
        &key,
    )
    .ok()?;
    let resp: Value = reqwest::Client::new()
        .post(&sa.token_uri)
        .form(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
            ("assertion", jwt.as_str()),
        ])
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    let token = resp.get("access_token")?.as_str()?.to_string();
    if let Ok(mut g) = cache.lock() {
        *g = Some((token.clone(), now + 3300));
    }
    Some(token)
}

async fn fcm_send_v1(
    sa: &ServiceAccount,
    tokens: &[String],
    title: &str,
    body: &str,
    data: &Value,
) -> bool {
    let access = match fcm_access_token(sa).await {
        Some(t) => t,
        None => return false,
    };
    let url = format!(
        "https://fcm.googleapis.com/v1/projects/{}/messages:send",
        sa.project_id
    );
    let client = reqwest::Client::new();
    let mut any = false;
    // v1 sends one message per token; `data` must be a string→string map.
    for dev in tokens {
        let msg = json!({
            "message": {
                "token": dev,
                "notification": { "title": title, "body": body },
                "data": data,
            }
        });
        match client.post(&url).bearer_auth(&access).json(&msg).send().await {
            Ok(r) if r.status().is_success() => any = true,
            Ok(r) => tracing::debug!("fcm v1 non-2xx for a device: {}", r.status()),
            Err(e) => tracing::warn!("fcm v1 send failed: {e}"),
        }
    }
    any
}

fn summarize_trade(v: &Value) -> String {
    let sym = v.get("symbol").and_then(|s| s.as_str()).unwrap_or("token");
    let action = v
        .get("action")
        .or_else(|| v.get("side"))
        .and_then(|s| s.as_str())
        .unwrap_or("");
    let pnl = v
        .get("pnl_pct")
        .or_else(|| v.get("pnl"))
        .and_then(|p| p.as_f64());
    match (action.is_empty(), pnl) {
        (false, Some(p)) => format!("{} {} {:+.1}%", action.to_uppercase(), sym, p),
        (false, None) => format!("{} {}", action.to_uppercase(), sym),
        (true, Some(p)) => format!("{} {:+.1}%", sym, p),
        (true, None) => sym.to_string(),
    }
}

fn read_range(path: &Path, start: u64, end: u64) -> String {
    use std::io::Read;
    let mut f = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return String::new(),
    };
    if f.seek(SeekFrom::Start(start)).is_err() {
        return String::new();
    }
    let mut buf = vec![0u8; end.saturating_sub(start) as usize];
    if f.read_exact(&mut buf).is_err() {
        return String::new();
    }
    String::from_utf8_lossy(&buf).into_owned()
}

/// Watches the trade log and pushes on each new appended trade. Starts from the log's
/// current end so a restart doesn't replay history. No-op without `FCM_SERVER_KEY`.
async fn trade_notifier() {
    use tokio::time::{sleep, Duration};
    if !fcm_configured() {
        info!("push: FCM_SERVICE_ACCOUNT / FCM_SERVER_KEY unset — trade notifications disabled");
        return;
    }
    let path = artifact_path("scematica-trades.jsonl");
    let mut offset = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    info!("push: notifier watching {} from offset {offset}", path.display());
    loop {
        sleep(Duration::from_secs(3)).await;
        let len = match fs::metadata(&path) {
            Ok(m) => m.len(),
            Err(_) => continue,
        };
        if len < offset {
            offset = 0; // rotated / truncated
        }
        if len == offset {
            continue;
        }
        let chunk = read_range(&path, offset, len);
        offset = len;
        let mut count = 0usize;
        let mut latest = String::new();
        for line in chunk.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<Value>(line) {
                latest = summarize_trade(&v);
                count += 1;
            }
        }
        if count == 0 {
            continue;
        }
        let (title, body) = if count == 1 {
            ("Scematica trade".to_string(), latest)
        } else {
            ("Scematica".to_string(), format!("{count} new trades — latest: {latest}"))
        };
        let _ = fcm_send(&title, &body, json!({ "kind": "trade", "count": count.to_string() })).await;
    }
}

async fn push_register_handler(Json(b): Json<PushRegisterBody>) -> impl IntoResponse {
    if b.token.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "empty token" })));
    }
    let count = add_push_token(b.token.trim());
    info!("push: device registered ({count} total, platform {:?})", b.platform);
    (StatusCode::OK, Json(json!({ "registered": true, "count": count })))
}

async fn push_test_handler() -> impl IntoResponse {
    let sent = fcm_send("Scematica", "Test push — you're paired \u{2705}", json!({ "kind": "test" })).await;
    (StatusCode::OK, Json(json!({ "sent": sent })))
}

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

    // Control (write) + push routes — token-gated when SCEMATICA_API_TOKEN is set.
    let gated = Router::new()
        .route("/api/controls/sell-mode", post(sell_mode_handler))
        .route("/api/controls/dump-mode", post(dump_mode_handler))
        .route("/api/controls/rate-mode", post(rate_mode_handler))
        .route("/api/controls/params", post(params_handler))
        .route("/api/controls/high-speed", post(high_speed_handler))
        .route("/api/controls/moon-chase", post(moon_chase_handler))
        .route("/api/controls/builder-mode", post(builder_mode_handler))
        .route("/api/push/register", post(push_register_handler))
        .route("/api/push/test", post(push_test_handler))
        .route_layer(middleware::from_fn(require_token));

    // Background: push a notification on each new trade (no-op without FCM_SERVER_KEY).
    tokio::spawn(trade_notifier());

    let app = Router::new()
        .route("/health", get(api_health))
        .route("/api/metrics", get(metrics_handler))
        .route("/api/pools", get(pools_handler))
        .route("/api/filters", get(filters_handler))
        .route("/api/logs", get(logs_handler))
        .route("/api/trades", get(trades_handler))
        .route("/api/decisions", get(decisions_handler))
        .route("/api/tx-telemetry", get(tx_telemetry_handler))
        .route("/api/nn", get(nn_handler))
        .route("/api/nn-advice", get(nn_advice_handler))
        .route("/api/positions", get(positions_handler))
        .route("/api/tournament", get(tournament_handler))
        .route("/api/intelligence", get(intelligence_handler))
        .route("/api/health", get(health_handler))
        .route("/api/controls", get(controls_get_handler))
        .merge(gated)
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
