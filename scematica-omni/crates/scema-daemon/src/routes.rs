//! The API, and the four things every request has to get past.
//!
//! ```text
//!   Host is local?  ──no──▶ 421   (DNS rebinding)
//!        │yes
//!   token matches?  ──no──▶ 401   (constant-time)
//!        │yes
//!   route exists?   ──no──▶ 404
//!        │yes
//!   path in workspace? ─no─▶ 403  (Workspace, resolve-then-compare)
//! ```
//!
//! `GET /health` skips the token, and only that. It answers whether a daemon is here and
//! nothing about what it can see — a pairing probe has to work before pairing.
//!
//! ## Endpoints
//!
//! | Route | Does |
//! |---|---|
//! | `GET /health` | liveness. No auth, no state. |
//! | `GET /policy` | λ weights, gates, observers, evaluators, workspace roots |
//! | `POST /observe` | `{locator}` → `WorldState` |
//! | `POST /simulate` | `{locator\|world, goal, must_not[], ground[]}` → cycle, **persists nothing** |
//! | `POST /decide` | same body → cycle, seals a record and appends memory |
//! | `GET /decisions` | summaries, newest first |
//! | `GET /decisions/{id}` | one record |
//! | `GET /decisions/{id}/verify` | recompute the commitment |
//! | `GET /memory/stats` | per-kind counts and calibration |
//!
//! ## `world` in the body is how the browser becomes a sensory organ
//!
//! `POST /simulate` accepts a `WorldState` inline instead of a `locator`. The extension's
//! content script perceives a page and posts the result; every layer above is unchanged,
//! because a world from a DOM and a world from a filesystem are the same type. This is the
//! reason `scema-world` takes two dependencies and holds no I/O.
//!
//! An inline world is **not** trusted to describe itself honestly — but it does not have to
//! be. Its `observer` field is rewritten to record that it arrived over the wire, so a
//! decision record can never claim a client-supplied world was locally observed.

use std::sync::Arc;

use scema_agent::{Agent, Cycle};
use scema_tools::Workspace;
use scema_verify::{verify, RecordStore};
use scema_world::{Constraint, Goal, WorldState};
use serde::{Deserialize, Serialize};

use crate::auth;
use crate::http::{Request, Response};

/// Everything a request needs.
pub struct State {
    pub agent: Arc<Agent>,
    pub workspace: Workspace,
    pub token: String,
    pub port: u16,
    pub root: std::path::PathBuf,
    /// When false, `POST /decide` answers 403. Sealing a record is a local write, and a
    /// front end that can be driven by a page or a model should have to be told it may.
    pub allow_decide: bool,
}

#[derive(Debug, Deserialize)]
struct CycleRequest {
    /// A path to observe. Resolved through the [`Workspace`].
    #[serde(default)]
    locator: Option<String>,
    /// A world observed elsewhere — the browser extension's path.
    #[serde(default)]
    world: Option<WorldState>,
    #[serde(default)]
    goal: String,
    #[serde(default)]
    must_not: Vec<String>,
    #[serde(default)]
    ground: Vec<String>,
}

#[derive(Debug, Serialize)]
struct CycleResponse {
    record: scema_verify::DecisionRecord,
    persisted: bool,
    record_path: Option<String>,
    remembered: usize,
    /// `--ground` ids naming no signal in the world. The simulator drops them; a client
    /// that never sees the list cannot tell a typo from a disagreement.
    dangling_grounds: Vec<String>,
}

fn ok_json<T: Serialize>(value: &T) -> Response {
    match serde_json::to_vec(value) {
        Ok(body) => Response::json(200, body),
        Err(e) => Response::error(500, "serialise_failed", e),
    }
}

fn parse_body<T: for<'de> Deserialize<'de>>(req: &Request) -> Result<T, Response> {
    if req.body.is_empty() {
        return Err(Response::error(400, "empty_body", "expected a JSON object"));
    }
    serde_json::from_slice(&req.body).map_err(|e| Response::error(400, "bad_json", e))
}

fn build_goal(r: &CycleRequest) -> Goal {
    let mut g = Goal::new("goal", r.goal.trim());
    for spec in &r.must_not {
        let (subject, detail) = match spec.split_once(':') {
            Some((a, b)) => (a.trim(), b.trim()),
            None => (spec.trim(), "declared by the client"),
        };
        // An empty subject matches every target by substring and would forbid the whole
        // matrix. Dropped, exactly as the CLI drops it.
        if !subject.is_empty() {
            g = g.with_constraint(Constraint::must_not(subject, detail));
        }
    }
    for id in &r.ground {
        if !id.trim().is_empty() {
            g = g.grounded(id.trim());
        }
    }
    g
}

/// Take the world from the request, either by observing a path or by accepting one inline.
fn world_for(state: &State, r: &CycleRequest) -> Result<WorldState, Response> {
    match (&r.world, &r.locator) {
        (Some(w), _) => {
            let mut w = w.clone();
            // A client-supplied world must never be able to claim it was observed here.
            // Whatever the client called its observer, the record says where it came from.
            if !w.observer.starts_with("client:") {
                w.observer = format!("client:{}", w.observer);
            }
            Ok(w)
        }
        (None, Some(locator)) => {
            let path = state
                .workspace
                .resolve(locator)
                .map_err(|e| Response::error(403, "outside_workspace", e))?;
            state
                .agent
                .observe(&path.to_string_lossy())
                .map_err(|e| Response::error(400, "observe_failed", e))
        }
        (None, None) => Err(Response::error(
            400,
            "no_world",
            "give either `locator` (a path) or `world` (a WorldState)",
        )),
    }
}

fn cycle_response(cycle: Cycle, dangling: Vec<String>) -> Response {
    ok_json(&CycleResponse {
        persisted: cycle.record_path.is_some(),
        record_path: cycle.record_path.as_ref().map(|p| p.display().to_string()),
        remembered: cycle.remembered,
        record: cycle.record,
        dangling_grounds: dangling,
    })
}

fn dangling_grounds(world: &WorldState, goal: &Goal) -> Vec<String> {
    goal.grounded_in
        .iter()
        .filter(|id| !world.signals.iter().any(|s| &&s.id == id))
        .cloned()
        .collect()
}

/// Run a cycle. `persist` decides whether it is a simulation or a decision.
fn run_cycle(state: &State, req: &Request, persist: bool) -> Response {
    let body: CycleRequest = match parse_body(req) {
        Ok(b) => b,
        Err(e) => return e,
    };
    let world = match world_for(state, &body) {
        Ok(w) => w,
        Err(e) => return e,
    };
    let goal = build_goal(&body);
    let dangling = dangling_grounds(&world, &goal);

    // `persist` is per-request, and `Agent::persist` is a field on a shared `Arc`, so the
    // simulate path cannot simply flip it — two concurrent requests would race and a
    // simulation could seal a record. A second agent over the same root is cheap: the DQ*
    // checkpoint is the only heavy part and the simulate path is the one that must never
    // write.
    let result = if persist {
        state.agent.cycle_over(world, goal)
    } else {
        let mut dry = Agent::new(state.root.clone(), None);
        dry.persist = false;
        dry.config = state.agent.config;
        dry.cycle_over(world, goal)
    };

    match result {
        Ok(c) => cycle_response(c, dangling),
        Err(e) => Response::error(500, "cycle_failed", e),
    }
}

/// Dispatch one request.
pub fn handle(state: &State, req: Request) -> Response {
    // 1. Host. A request that arrived through a name resolving here but not naming here is
    //    the shape of a DNS rebinding attack; 421 is the status for exactly that.
    if !auth::host_is_local(req.header("host"), state.port) {
        return Response::error(
            421,
            "bad_host",
            "this daemon answers only to 127.0.0.1 or localhost on its own port",
        );
    }

    // 2. Health first, because pairing has to work before there is a token to send.
    if req.path == "/health" {
        return ok_json(&serde_json::json!({
            "ok": true,
            "runtime": scema_agent::RUNTIME,
            "service": "scema-omnid",
        }));
    }

    // 3. Token.
    let presented = req.header("authorization").map(auth::bearer).unwrap_or("");
    if !auth::secret_eq(presented, &state.token) {
        return Response::error(
            401,
            "unauthorized",
            format!(
                "send `Authorization: Bearer <token>`; the token is in {}",
                auth::token_path(&state.root).display()
            ),
        );
    }

    let segments = req.segments();
    match (req.method.as_str(), segments.as_slice()) {
        ("GET", ["policy"]) => policy(state),
        ("POST", ["observe"]) => observe(state, &req),
        ("POST", ["simulate"]) => run_cycle(state, &req, false),
        ("POST", ["decide"]) => {
            if state.allow_decide {
                run_cycle(state, &req, true)
            } else {
                Response::error(
                    403,
                    "decide_disabled",
                    "sealing records over HTTP is off; restart with --allow-decide",
                )
            }
        }
        ("GET", ["decisions"]) => decisions(state),
        ("GET", ["decisions", id]) => decision(state, id),
        ("GET", ["decisions", id, "verify"]) => decision_verify(state, id),
        ("GET", ["memory", "stats"]) => memory_stats(state),
        (_, _) if segments.is_empty() => Response::error(
            404,
            "no_route",
            "try GET /health, GET /policy, POST /observe, POST /simulate",
        ),
        ("GET", _) | ("POST", _) => Response::error(404, "no_route", req.path.clone()),
        _ => Response::error(405, "method_not_allowed", req.method.clone()),
    }
}

fn policy(state: &State) -> Response {
    let c = state.agent.config;
    ok_json(&serde_json::json!({
        "runtime": scema_agent::RUNTIME,
        "equation": "U = R - l1*K - l2*C - l3*U + l4*V",
        "weights": c.weights,
        "min_coverage": c.min_coverage,
        "veto_at_or_below": c.veto_at_or_below,
        "allow_decide": state.allow_decide,
        "workspace_roots": state.workspace.root_labels(),
        "observers": state.agent.observers().iter()
            .map(|o| serde_json::json!({ "name": o.name(), "about": o.about() }))
            .collect::<Vec<_>>(),
        "evaluators": state.agent.evaluators().iter()
            .map(|e| serde_json::json!({ "name": e.name(), "about": e.about() }))
            .collect::<Vec<_>>(),
    }))
}

fn observe(state: &State, req: &Request) -> Response {
    #[derive(Deserialize)]
    struct Body {
        locator: String,
    }
    let body: Body = match parse_body(req) {
        Ok(b) => b,
        Err(e) => return e,
    };
    let path = match state.workspace.resolve(&body.locator) {
        Ok(p) => p,
        Err(e) => return Response::error(403, "outside_workspace", e),
    };
    match state.agent.observe(&path.to_string_lossy()) {
        Ok(w) => ok_json(&w),
        Err(e) => Response::error(400, "observe_failed", e),
    }
}

fn decisions(state: &State) -> Response {
    let store = RecordStore::new(state.root.clone());
    let ids = match store.ids() {
        Ok(i) => i,
        Err(e) => return Response::error(500, "listing_failed", e),
    };
    let summaries: Vec<serde_json::Value> = ids
        .iter()
        .map(|id| match store.load(id) {
            Ok(r) => serde_json::json!({
                "id": r.id,
                "at": r.at,
                "goal": r.goal.statement,
                "entity": r.world.entity.locator,
                "chosen": r.decision.chosen,
                "abstained": r.decision.abstention.as_ref().map(|a| a.headline()),
                "coverage": r.decision.coverage,
            }),
            // An unreadable record still gets a row. Hiding it makes a corrupt store look
            // like a smaller one — the same rule as `scema explain --list`.
            Err(e) => serde_json::json!({ "id": id, "unreadable": e.to_string() }),
        })
        .collect();
    ok_json(&summaries)
}

fn decision(state: &State, id: &str) -> Response {
    match RecordStore::new(state.root.clone()).load(id) {
        Ok(r) => ok_json(&r),
        Err(e) => Response::error(404, "no_record", e),
    }
}

fn decision_verify(state: &State, id: &str) -> Response {
    match RecordStore::new(state.root.clone()).load(id) {
        Ok(r) => ok_json(&verify(&r)),
        Err(e) => Response::error(404, "no_record", e),
    }
}

fn memory_stats(state: &State) -> Response {
    let mem = state.agent.memory();
    let counts = match mem.counts() {
        Ok(c) => c,
        Err(e) => return Response::error(500, "memory_unreadable", e),
    };
    let calibration = match mem.calibration() {
        Ok(c) => c,
        Err(e) => return Response::error(500, "memory_unreadable", e),
    };
    ok_json(&serde_json::json!({
        "counts": counts.iter().map(|(k, n, corrupt)| serde_json::json!({
            "kind": k, "records": n, "unreadable_lines": corrupt
        })).collect::<Vec<_>>(),
        "calibration": calibration,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;

    fn scratch() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "scema-omni-routes-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&p).unwrap();
        fs::write(p.join("Cargo.toml"), "[package]\nname = \"t\"\n").unwrap();
        fs::create_dir_all(p.join("src")).unwrap();
        fs::write(p.join("src/lib.rs"), "fn a() {}\n// TODO: something\n").unwrap();
        p
    }

    fn state(root: &PathBuf, allow_decide: bool) -> State {
        State {
            agent: Arc::new(Agent::new(root.clone(), None)),
            workspace: Workspace::new([root]).unwrap(),
            token: "t".repeat(64),
            port: 7842,
            root: root.clone(),
            allow_decide,
        }
    }

    fn req(method: &str, path: &str, token: Option<&str>, body: &str) -> Request {
        let mut headers = BTreeMap::new();
        headers.insert("host".to_string(), "127.0.0.1:7842".to_string());
        if let Some(t) = token {
            headers.insert("authorization".to_string(), format!("Bearer {t}"));
        }
        Request {
            method: method.into(),
            path: path.into(),
            query: BTreeMap::new(),
            headers,
            body: body.as_bytes().to_vec(),
        }
    }

    #[test]
    fn health_needs_no_token_because_pairing_precedes_having_one() {
        let root = scratch();
        let s = state(&root, false);
        let r = handle(&s, req("GET", "/health", None, ""));
        assert_eq!(r.status, 200);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn health_leaks_nothing_about_what_the_daemon_can_see() {
        let root = scratch();
        let s = state(&root, false);
        let r = handle(&s, req("GET", "/health", None, ""));
        let body = String::from_utf8(r.body).unwrap();
        assert!(!body.contains(&root.to_string_lossy().to_string()));
        assert!(!body.contains("token"));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn every_other_route_needs_the_token() {
        let root = scratch();
        let s = state(&root, false);
        for (m, p) in [
            ("GET", "/policy"),
            ("POST", "/observe"),
            ("POST", "/simulate"),
            ("GET", "/decisions"),
            ("GET", "/memory/stats"),
        ] {
            assert_eq!(handle(&s, req(m, p, None, "{}")).status, 401, "{m} {p}");
            assert_eq!(handle(&s, req(m, p, Some("wrong"), "{}")).status, 401, "{m} {p}");
        }
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_rebinding_host_is_refused_before_anything_else() {
        // Ahead of the token check on purpose: the point is to answer nothing at all to a
        // request that arrived through somebody else's name.
        let root = scratch();
        let s = state(&root, false);
        let mut r = req("GET", "/health", None, "");
        r.headers.insert("host".into(), "evil.example:7842".into());
        assert_eq!(handle(&s, r).status, 421);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn observing_outside_the_workspace_is_403_and_says_so() {
        let root = scratch();
        let s = state(&root, false);
        let outside = std::env::temp_dir();
        let body = serde_json::json!({ "locator": outside.to_string_lossy() }).to_string();
        let r = handle(&s, req("POST", "/observe", Some(&s.token), &body));
        assert_eq!(r.status, 403);
        assert!(String::from_utf8(r.body).unwrap().contains("outside this workspace"));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn observing_inside_the_workspace_returns_a_world() {
        let root = scratch();
        let s = state(&root, false);
        let r = handle(&s, req("POST", "/observe", Some(&s.token), r#"{"locator":"."}"#));
        assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
        let w: WorldState = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(w.observer, "repo");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn simulate_persists_nothing() {
        // The invariant that keeps a counterfactual from later reading as a decision.
        let root = scratch();
        let s = state(&root, true);
        let body = r#"{"locator":".","goal":"tidy up"}"#;
        let r = handle(&s, req("POST", "/simulate", Some(&s.token), body));
        assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
        let v: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(v["persisted"], serde_json::json!(false));
        assert!(RecordStore::new(root.clone()).ids().unwrap().is_empty());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn decide_is_off_until_it_is_switched_on() {
        let root = scratch();
        let s = state(&root, false);
        let r = handle(&s, req("POST", "/decide", Some(&s.token), r#"{"locator":".","goal":"x"}"#));
        assert_eq!(r.status, 403);
        assert!(String::from_utf8(r.body).unwrap().contains("--allow-decide"));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn decide_seals_a_record_when_it_is_switched_on() {
        let root = scratch();
        let s = state(&root, true);
        let r = handle(&s, req("POST", "/decide", Some(&s.token), r#"{"locator":".","goal":"x"}"#));
        assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
        let v: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(v["persisted"], serde_json::json!(true));
        assert_eq!(RecordStore::new(root.clone()).ids().unwrap().len(), 1);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_client_supplied_world_is_marked_as_client_supplied() {
        // The extension's path. A record must never be able to claim a world that arrived
        // over the wire was observed here.
        let root = scratch();
        let s = state(&root, true);
        let world = serde_json::json!({
            "observer": "page",
            "entity": { "kind": "website", "locator": "https://example.test/", "label": "example" },
            "domain": "software",
            "observed_at": 0,
            "objects": [],
            "facts": [],
            "signals": [],
            "extent": { "observed": 0, "total": 0, "note": "test" },
            "blind_spots": []
        });
        let body = serde_json::json!({ "world": world, "goal": "look at this page" }).to_string();
        let r = handle(&s, req("POST", "/decide", Some(&s.token), &body));
        assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
        let v: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(v["record"]["world"]["observer"], serde_json::json!("client:page"));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_request_with_neither_locator_nor_world_is_a_400_that_says_what_is_missing() {
        let root = scratch();
        let s = state(&root, false);
        let r = handle(&s, req("POST", "/simulate", Some(&s.token), r#"{"goal":"x"}"#));
        assert_eq!(r.status, 400);
        assert!(String::from_utf8(r.body).unwrap().contains("locator"));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn dangling_grounds_are_reported_rather_than_silently_dropped() {
        let root = scratch();
        let s = state(&root, false);
        let body = r#"{"locator":".","goal":"x","ground":["no-such-signal"]}"#;
        let r = handle(&s, req("POST", "/simulate", Some(&s.token), body));
        let v: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!(v["dangling_grounds"], serde_json::json!(["no-such-signal"]));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_record_can_be_fetched_and_verified_over_http() {
        let root = scratch();
        let s = state(&root, true);
        let r = handle(&s, req("POST", "/decide", Some(&s.token), r#"{"locator":".","goal":"x"}"#));
        let v: serde_json::Value = serde_json::from_slice(&r.body).unwrap();
        let id = v["record"]["id"].as_str().unwrap().to_string();

        let got = handle(&s, req("GET", &format!("/decisions/{id}"), Some(&s.token), ""));
        assert_eq!(got.status, 200);

        let ver = handle(&s, req("GET", &format!("/decisions/{id}/verify"), Some(&s.token), ""));
        assert_eq!(ver.status, 200);
        let vv: serde_json::Value = serde_json::from_slice(&ver.body).unwrap();
        assert_eq!(vv["valid"], serde_json::json!(true));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn an_unknown_route_is_404_not_a_silent_200() {
        let root = scratch();
        let s = state(&root, false);
        assert_eq!(handle(&s, req("GET", "/nope", Some(&s.token), "")).status, 404);
        assert_eq!(handle(&s, req("DELETE", "/policy", Some(&s.token), "")).status, 405);
        fs::remove_dir_all(&root).ok();
    }
}

