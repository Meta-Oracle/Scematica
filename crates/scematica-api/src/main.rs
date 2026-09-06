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
//! GET  /api/sentience                 cognitive gate — is this API's data trustworthy?
//! POST /api/sentience/observe         { "text": "..." }  feed a response back (ungated)
//! POST /api/replay                    counterfactual thresholds over the decision log
//! GET  /api/calibration               how often Scylar's past calls were right
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
mod calibration;
mod replay;

use scematica_sentience as sentience;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader, Seek, SeekFrom},
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};
use tower_http::cors::{Any, CorsLayer};
use tracing::{info, warn};

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
/// Append-only log of Scylar's claims about specific mints, scored by `calibration.rs`.
/// Written by this API only; nothing in the trading path reads it.
const SCYLAR_CLAIMS_FILE: &str = "scematica-scylar-claims.jsonl";

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

/// Append one JSON object as a line.
///
/// Not the `.tmp` + rename dance the state files use — that convention exists so a
/// reader never sees a half-written *snapshot*. This is an append-only log where a torn
/// final line costs one record, and `read_last_jsonl_values` already skips lines that
/// fail to parse.
fn append_jsonl(path: &str, value: &Value) -> bool {
    use std::io::Write;
    let path = artifact_path(path);
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    match fs::OpenOptions::new().create(true).append(true).open(&path) {
        Ok(mut f) => writeln!(f, "{value}").is_ok(),
        Err(_) => false,
    }
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

/// `GET /api/mesh` — the running system's own topology.
///
/// Every other read endpoint here serves one state file. This one reads them all and
/// returns the graph they collectively describe: which decision-making units exist, what
/// each last decided, and — before either — whether each can be seen at all.
///
/// Unlike its neighbours it has **no empty-object fallback**. `filters_handler` answering
/// `{"pools_seen":0}` for a missing file is harmless because the caller renders a counter;
/// answering an empty mesh would assert that the system has no units, which is a claim
/// about the bot rather than about the read. A collector run against an empty directory
/// already returns a complete topology with every node marked absent, so the honest
/// degraded response is the mesh itself.
async fn mesh_handler() -> impl IntoResponse {
    let mesh = scematica_mesh::Collector::new(".").collect();
    Json(serde_json::to_value(mesh).unwrap_or_else(|e| {
        json!({ "error": "mesh serialisation failed", "detail": e.to_string() })
    }))
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
    let mut reader = BufReader::new(file);

    // The seek above lands on an arbitrary byte, so whatever follows it up to the first
    // newline is the tail of a line, not a line. Two things go wrong if it is not discarded
    // at the *byte* level:
    //
    // 1. A fragment gets served as though it were a complete log entry. The dashboard
    //    renders it beside real lines with nothing marking it as half a sentence.
    // 2. That byte may be the middle of a multi-byte UTF-8 sequence, in which case reading
    //    it as a line is an `InvalidData` error rather than text — which is what made the
    //    original `filter_map(|l| l.ok())` dangerous. `Lines` is permitted to yield `Err`
    //    forever, and `filter_map` skips errors and keeps asking, so a persistent read
    //    error turns a log tail into a hung request thread. `map_while` stops instead, but
    //    only reaches the good lines because the fragment is consumed here first — swapping
    //    one for the other without this would truncate the whole tail to nothing whenever
    //    the seek split a character.
    if start > 0 {
        let mut fragment = Vec::new();
        if reader.read_until(b'\n', &mut fragment).is_err() {
            return vec![];
        }
    }

    let all: Vec<String> = reader.lines().map_while(Result::ok).collect();
    all.into_iter().rev().take(n).rev().collect()
}

// ── control GET ───────────────────────────────────────────────────────────────

// ── cognitive gate ────────────────────────────────────────────────────────────
//
// `GET /api/sentience` answers one question: **can anything reading this API be trusted
// to describe the bot right now?**
//
// It exists because of a failure this API cannot otherwise report. Every read endpoint
// serves the state files, and a state file is served identically whether the sniper
// wrote it four seconds ago or four hours ago before it died. `/api/health` reports the
// lock file, which says "a process was here", not "these numbers are current". So a
// consumer — the web dashboard, the mobile app, Scylar — can present hours-old figures
// as live with nothing anywhere in the response contradicting it.
//
// The mapping below is where the judgement is, so it is written out rather than tuned
// into opaque constants:
//
//   perception.integrity   how fresh the metrics file is. The sniper rewrites it every
//                          5s, so staleness is a direct measure of whether these numbers
//                          describe now. This is the dominant term, and deliberately so.
//   perception.sensory     how many of the core state files exist and parse at all.
//   logic.consistency      whether liveness and freshness agree. A lock file claiming a
//                          live sniper over a stale metrics file is a contradiction, and
//                          contradiction is precisely what should lower confidence.
//   rationality.evidence   whether there is any history to reason from.
//
// **Everything not measured is 1.0, not 0.5 or 0.9.** Ψ is a product of ratios, so an
// unmeasured dimension scored below 1.0 is a standing tax on the index: a completely
// healthy bot then lands in CAUTION, the badge shows on every answer, and a warning
// nobody can ever clear is a warning people learn to ignore — the same reason the
// alchem-link staleness check carries a tolerance instead of flickering every cycle.
// 1.0 here means "not a limiting factor", which is the honest reading of a dimension
// this gate has no instrument for. Only measured degradation moves the verdict.
//
// The gate is advisory. It returns a verdict; it does not block any other endpoint.

/// Metrics are rewritten every 5s by the sniper. Fresher than this is fully trusted.
const FRESH_SECS: f64 = 30.0;
/// Past this the file describes a session that is over, whatever the lock file says.
const STALE_SECS: f64 = 600.0;

/// Overlay state, persisted across requests so the cognitive loop can actually advance.
static OVERLAY: OnceLock<Mutex<sentience::Overlay<sentience::NoClient>>> = OnceLock::new();

fn overlay() -> &'static Mutex<sentience::Overlay<sentience::NoClient>> {
    OVERLAY.get_or_init(|| Mutex::new(sentience::Overlay::new(sentience::NoClient, None)))
}

/// Seconds since a file was last written, or `None` if it is missing/unreadable.
fn age_secs(path: &str) -> Option<f64> {
    let meta = fs::metadata(artifact_path(path)).ok()?;
    let modified = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
    Some((now.as_secs_f64() - modified.as_secs_f64()).max(0.0))
}

/// 1.0 while fresh, decaying linearly to 0.0 at `STALE_SECS`. Missing reads as 0.
fn freshness(path: &str) -> f64 {
    match age_secs(path) {
        None => 0.0,
        Some(age) if age <= FRESH_SECS => 1.0,
        Some(age) if age >= STALE_SECS => 0.0,
        Some(age) => 1.0 - (age - FRESH_SECS) / (STALE_SECS - FRESH_SECS),
    }
}

async fn sentience_handler() -> impl IntoResponse {
    use sentience::{
        ethics::EthicsInputs, logic::LogicInputs, perception::Perception,
        rationality::RationalityInputs, types::Bounded,
    };

    let metrics_fresh = freshness(METRICS_FILE);
    let (sniper_running, _) = check_sniper_liveness();

    let core = [METRICS_FILE, FILTER_STATS_FILE, NN_STATS_FILE, POSITIONS_FILE];
    let present = core.iter().filter(|f| age_secs(f).is_some()).count() as f64;
    let sensory = present / core.len() as f64;

    // The contradiction that matters: a lock file saying the sniper is alive while its
    // own metrics have gone cold. Either the process is wedged or the lock is stale;
    // either way nothing downstream should speak confidently about "current" state.
    let consistency = if sniper_running { metrics_fresh } else { 1.0 - metrics_fresh * 0.5 };

    let has_history = age_secs(TRADES_FILE).is_some() || age_secs(POOL_DECISIONS_FILE).is_some();
    let evidence = if has_history { 1.0 } else { 0.4 };

    let perception = Perception::new(
        // Audio and visual are 1.0, not 0.0, and the difference is the whole gate.
        // `Perception::data_ratio` is a *product* — D = A×V×X×I — so a channel scored 0
        // annihilates D, pins Ψ at exactly 0, and makes HOLD the permanent verdict no
        // matter how healthy the bot is. 1.0 is the crate's own `Default`, and it reads
        // "this channel is not a limiting factor", which is the true statement about a
        // sense a headless trading bot does not have. Scoring it 0 would claim the bot
        // is blind rather than that sight is irrelevant to it.
        1.0,
        1.0,
        sensory,
        metrics_fresh,
    );
    let readout = {
        let mut ov = overlay().lock().unwrap_or_else(|e| e.into_inner());

        // Overwrite only what is measured, and leave the rest of the state to evolve.
        //
        // Replacing the whole state here — which is what this did first — silently
        // undoes `/api/sentience/observe`: the loop steps, then the very next gate read
        // throws the result away along with the timestep. The two halves cancelled and
        // the feedback loop was decorative. Measured terms carry the signal; the rest
        // are neutral (see the note above on why they are 1.0 and not a "modest" 0.9).
        let st = ov.state_mut();
        st.perception = perception;
        st.rationality = RationalityInputs::new(evidence, consistency, 1.0, 0.0);
        st.logic = LogicInputs::new(1.0, consistency, 1.0, 1.0);
        st.ethics = EthicsInputs::new(1.0, 1.0, 1.0, 1.0);
        st.knowledge_density = Bounded::new(sensory.max(0.1));

        ov.assess()
    };

    Json(json!({
        "gate": readout.gate.as_str(),
        "psi": readout.psi,
        "sentience": readout.sentience,
        "bottleneck": readout.bottleneck,
        "note": readout.note,
        "reassessment": readout.reassessment,
        // The measurements behind the verdict. A gate whose inputs are not visible is an
        // oracle, and nobody can debug an oracle.
        "inputs": {
            "metrics_age_secs": age_secs(METRICS_FILE),
            "metrics_freshness": metrics_fresh,
            "sniper_running": sniper_running,
            "state_files_present": present as u64,
            "state_files_expected": core.len() as u64,
            "consistency": consistency,
            "has_history": has_history,
        },
    }))
}

/// Feed a generated response back so the loop advances. Body: `{ "text": "..." }`.
///
/// Also files any claims the answer made about specific mints, so they can be scored
/// later against what those mints actually did (see `calibration.rs`).
///
/// Ungated deliberately, unlike the control POSTs: it nudges an advisory index that is
/// recomputed from measured file freshness on the next read, and appends to a file
/// nothing in the trading path reads. It cannot influence a trade.
async fn sentience_observe_handler(Json(body): Json<Value>) -> impl IntoResponse {
    let text = body.get("text").and_then(|v| v.as_str()).unwrap_or("");

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let claims = calibration::extract_claims(text, now);
    for claim in &claims {
        append_jsonl(SCYLAR_CLAIMS_FILE, &json!(claim));
    }

    let readout = {
        let mut ov = overlay().lock().unwrap_or_else(|e| e.into_inner());
        ov.record(text)
    };
    Json(json!({
        "gate": readout.gate.as_str(),
        "psi": readout.psi,
        "timestep": readout.timestep,
        "reassessment": readout.reassessment,
        "claims_filed": claims.len(),
    }))
}

/// Claims read for a calibration score.
const CALIBRATION_MAX_CLAIMS: usize = 2_000;

/// `GET /api/calibration` — how often her calls have been right.
async fn calibration_handler() -> impl IntoResponse {
    let claims: Vec<calibration::Claim> = read_last_jsonl_values(SCYLAR_CLAIMS_FILE, CALIBRATION_MAX_CLAIMS)
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect();

    let pnl = replay::realised_pnl_by_mint(&read_last_jsonl_values(TRADES_FILE, CALIBRATION_MAX_CLAIMS));
    Json(json!({ "calibration": calibration::score(&claims, &pnl) }))
}

/// Decisions read for a replay. The log is append-only and can be very long; this bounds
/// the work a single unauthenticated request can ask for.
const REPLAY_MAX_DECISIONS: usize = 4_000;

/// `POST /api/replay` — re-apply thresholds to what the pipeline actually measured.
///
/// Ungated for the same reason as `/api/sentience`: it reads two files and writes
/// nothing. Bounded by `REPLAY_MAX_DECISIONS` so the compute per request is capped.
async fn replay_handler(Json(query): Json<replay::ReplayQuery>) -> impl IntoResponse {
    let limit = query.limit.unwrap_or(REPLAY_MAX_DECISIONS).min(REPLAY_MAX_DECISIONS);

    let raw = read_last_jsonl_values(POOL_DECISIONS_FILE, limit);
    let decisions = replay::parse_decisions(&raw);
    let pnl = replay::realised_pnl_by_mint(&read_last_jsonl_values(TRADES_FILE, REPLAY_MAX_DECISIONS));

    let outcome = replay::replay(&decisions, &pnl, &query);
    Json(json!({ "query_applied": !query.is_empty(), "outcome": outcome }))
}

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

/// Process image the lock file's PID must belong to for the sniper to count as alive.
///
/// Checked because a PID alone does not identify a process — see below.
#[cfg(target_os = "windows")]
const SNIPER_IMAGE: &str = "sniper.exe";
#[cfg(not(target_os = "windows"))]
const SNIPER_IMAGE: &str = "sniper";

/// Is the sniper named in the lock file actually running?
///
/// **The PID must be matched together with the process image.** A PID is not an identity:
/// the OS reuses it as soon as the process exits, so a stale lock file plus an unrelated
/// process that inherited the number reads as a healthy sniper. This is not theoretical —
/// it was found in the wild with lock PID 25952 belonging to `python.exe` hours after the
/// sniper had exited, and it propagated a long way:
///
///   `sniper_running: true` + cold metrics → `consistency = 0` in `sentience_handler` →
///   Ψ = 0 (it is a product) → permanent HOLD → Scylar's chat route 409s every turn.
///
/// A false negative here is cheap: the operator sees "stopped" and restarts. A false
/// positive is what jams the gate, so the check is written to fail toward "not running".
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
        // Both filters, ANDed by tasklist. On no match it prints "INFO: No tasks are
        // running which match the specified criteria." — which contains neither the
        // image name nor the PID, so requiring both is what makes the negative case
        // unambiguous rather than relying on parsing that sentence.
        let out = Command::new("tasklist")
            .args([
                "/FI",
                &format!("PID eq {}", pid),
                "/FI",
                &format!("IMAGENAME eq {}", SNIPER_IMAGE),
                "/NH",
            ])
            .output();
        if let Ok(o) = out {
            let stdout = String::from_utf8_lossy(&o.stdout).to_ascii_lowercase();
            let alive = stdout.contains(SNIPER_IMAGE) && stdout.contains(&pid.to_string());
            return (alive, Some(pid));
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        // `/proc/<pid>` existing has the same PID-reuse hole, so confirm the image too.
        // `comm` is truncated to 15 chars by the kernel; `sniper` is well inside that.
        let alive = fs::read_to_string(format!("/proc/{}/comm", pid))
            .map(|c| c.trim() == SNIPER_IMAGE)
            .unwrap_or(false);
        return (alive, Some(pid));
    }
    #[cfg(target_os = "windows")]
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

/// Constant-time secret comparison.
///
/// A port of `scema_daemon::auth::secret_eq`, which cannot be linked from here — it lives
/// in the `scematica-omni` workspace, kept separate by the dependency pins. It folds
/// across the full length of both inputs rather than returning at the first differing
/// byte, so its cost is a function of the two lengths and nothing else. The early-exit
/// `==` this replaces leaked the matching-prefix length, recoverable one byte at a time
/// against an endpoint that force-sells a live trading position.
///
/// The length difference accumulates at full width. The daemon's version narrows it with
/// `as u8`, where two lengths differing by a multiple of 256 contribute nothing — it does
/// not matter there, because the byte loop still runs to the longer length and a token is
/// never NUL-padded, but there is no reason to carry the hazard into a second copy.
fn secret_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut diff = a.len() ^ b.len();
    for i in 0..a.len().max(b.len()) {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        diff |= (x ^ y) as usize;
    }
    diff == 0
}

/// The control-route gate. **Fails closed.**
///
/// These routes are not read-only: `/api/controls/dump-mode` force-sells at
/// `min_out = 0`, `sell-mode` pauses buys, `params` retunes the strategy. The gate used
/// to pass every request when `SCEMATICA_API_TOKEN` was unset, described in a comment as
/// a local-dev default — but the listener binds a socket, and until this commit it bound
/// every interface, so "unauthenticated local dev" meant anyone who could route to the
/// host could liquidate the operator's positions.
///
/// An unconfigured deployment now refuses control routes instead of opening them, and
/// says which of the two things is wrong: 503 means the server has no token, which is an
/// operator problem, and 401 means the caller did not present the right one. Collapsing
/// both into 401 would send someone to look for a bad client when the server is the thing
/// that is misconfigured. Read-only routes are unaffected.
///
/// The omni daemon in this repository does the stronger thing — `auth::load_or_create`
/// generates a token into a `0600` file on first run, so there is no unconfigured state
/// at all. That is the better shape and worth adopting here.
async fn require_token(headers: HeaderMap, req: Request, next: Next) -> Result<Response, StatusCode> {
    let Ok(expected) = std::env::var("SCEMATICA_API_TOKEN") else {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    };
    if expected.is_empty() {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    match present_token(&headers) {
        Some(t) if secret_eq(&t, &expected) => Ok(next.run(req).await),
        _ => Err(StatusCode::UNAUTHORIZED),
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

    // ## CORS is opt-in, and never applies to the control routes
    //
    // `allow_origin(Any)` plus a loopback bind is the classic pairing: any web page the
    // operator happens to visit could POST to `127.0.0.1:3001/api/controls/dump-mode` and
    // read the reply. A browser will not send `Authorization` cross-origin without a
    // successful preflight, so the token was some protection — but only for deployments
    // that had set one, which A-01 is precisely about.
    //
    // Read-only routes keep a permissive CORS layer so the web dashboard works from any
    // origin the operator serves it from. The control routes are merged in **after** this
    // layer and therefore carry no CORS headers at all, which is the omni daemon's
    // posture: a page cannot read a reply even if it guesses the route.
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
        .route("/api/mesh", get(mesh_handler))
        .route("/api/nn", get(nn_handler))
        .route("/api/nn-advice", get(nn_advice_handler))
        .route("/api/positions", get(positions_handler))
        .route("/api/tournament", get(tournament_handler))
        .route("/api/intelligence", get(intelligence_handler))
        .route("/api/health", get(health_handler))
        .route("/api/sentience", get(sentience_handler))
        .route("/api/sentience/observe", post(sentience_observe_handler))
        .route("/api/replay", post(replay_handler))
        .route("/api/calibration", get(calibration_handler))
        .route("/api/controls", get(controls_get_handler))
        .layer(cors)
        .merge(gated);

    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3001);

    // ## Loopback unless somebody says otherwise
    //
    // This was `[0, 0, 0, 0]` — every interface — beside a fail-open auth gate. Either
    // alone is defensible; together they are an unauthenticated control plane on the
    // network. The default is now loopback, and widening it is an explicit act with a
    // name attached to it rather than the thing that happens when nobody chose.
    //
    // `SCEMATICA_API_BIND` takes an address (`0.0.0.0` to accept the old behaviour, or a
    // specific interface). A malformed value is refused rather than quietly falling back:
    // a typo that silently binds loopback would look like a firewall problem, and a typo
    // that silently binds the world is worse.
    let host: IpAddr = match std::env::var("SCEMATICA_API_BIND") {
        Ok(v) => v
            .parse()
            .map_err(|_| anyhow::anyhow!("SCEMATICA_API_BIND is not an IP address: {v}"))?,
        Err(_) => IpAddr::V4(Ipv4Addr::LOCALHOST),
    };
    if !host.is_loopback() {
        warn!(
            "binding {host} — the control routes are reachable off this host;              SCEMATICA_API_TOKEN must be set or they will refuse every request"
        );
    }
    let addr = SocketAddr::from((host, port));
    info!("Scematica API listening on http://{}", addr);
    axum::serve(tokio::net::TcpListener::bind(addr).await?, app).await?;
    Ok(())
}

#[cfg(test)]
mod tail_tests {
    use super::read_last_n_lines;

    /// A path in the system temp directory that no other run is using.
    ///
    /// The names used to be fixed, and two `cargo test --workspace` runs at once — two
    /// terminals, or an editor's test runner beside a shell — then had two processes writing
    /// and reading the same file. One truncates while the other seeks, and the tail comes
    /// back empty: a failure in code that is correct, on a machine where nothing is wrong.
    ///
    /// The pid separates processes and the counter separates tests within one, which is the
    /// same pair `scema_tools`'s scratch helper needed after a timestamp collided on
    /// Windows' ~15 ms clock. A timestamp is not enough here either.
    fn scratch(name: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("scematica-tail-{}-{}-{name}", std::process::id(), n))
    }

    /// Write a file of `count` lines, each exactly 21 bytes including the newline.
    ///
    /// Fixed-width on purpose: the seek offset `read_last_n_lines` computes is then
    /// predictable, which is what lets these tests assert *where* it lands rather than
    /// hoping it lands somewhere interesting.
    fn write_fixture(name: &str, line: &str, count: usize) -> String {
        assert_eq!(line.len() + 1, 21, "fixture lines must be 21 bytes with the newline");
        let path = scratch(name);
        let body: String = (0..count).map(|_| format!("{line}\n")).collect();
        std::fs::write(&path, body).expect("write fixture");
        path.display().to_string()
    }

    #[test]
    fn a_short_file_is_returned_whole() {
        // start == 0, so nothing is discarded: the first line here is a real first line.
        let p = write_fixture("short.log", &"a".repeat(20), 3);
        let got = read_last_n_lines(&p, 10);
        assert_eq!(got.len(), 3);
        assert!(got.iter().all(|l| l.len() == 20));
    }

    #[test]
    fn the_last_n_lines_come_back_in_order() {
        let path = scratch("order.log");
        std::fs::write(&path, "one\ntwo\nthree\nfour\n").expect("write");
        let got = read_last_n_lines(&path.display().to_string(), 2);
        assert_eq!(got, vec!["three".to_string(), "four".to_string()]);
    }

    #[test]
    fn the_fragment_left_by_the_seek_is_never_served_as_a_line() {
        // 100 lines of 21 bytes = 2100. With n = 3 the chunk is 600, so the seek lands at
        // byte 1500 — which is 9 bytes into line 71, not on a boundary. Everything after it
        // up to the newline is the tail of a line, and serving it beside real entries puts
        // half a sentence in the dashboard with nothing marking it as such.
        let p = write_fixture("fragment.log", &"a".repeat(20), 100);
        let got = read_last_n_lines(&p, 3);
        assert_eq!(got.len(), 3);
        assert!(
            got.iter().all(|l| l.len() == 20),
            "a short line means the fragment was served: {got:?}"
        );
    }

    #[test]
    fn a_seek_into_the_middle_of_a_utf8_character_still_returns_the_tail() {
        // The case that makes the bounded iterator safe to use at all. Ten two-byte chars
        // plus a newline is 21 bytes, so the same offset 1500 lands on the *second* byte of
        // a character — reading from there as text is `InvalidData`, not a line.
        //
        // The old code skipped that error and kept asking, and `Lines` is permitted to
        // produce `Err` forever, which is a hung request thread. Simply bounding the
        // iterator instead would truncate the whole tail to nothing here. Both are wrong;
        // consuming the fragment as bytes first is what makes the tail correct AND finite.
        let p = write_fixture("utf8.log", &"é".repeat(10), 100);
        let got = read_last_n_lines(&p, 3);
        assert_eq!(got.len(), 3, "a split character must not empty the tail: {got:?}");
        assert!(got.iter().all(|l| l.chars().count() == 10));
    }

    #[test]
    fn a_missing_file_is_empty_rather_than_an_error() {
        assert!(read_last_n_lines("scematica-tail-does-not-exist.log", 5).is_empty());
    }
}
