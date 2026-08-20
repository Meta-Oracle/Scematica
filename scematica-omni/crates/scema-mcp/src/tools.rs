//! The tool catalogue, and the two guards that matter when the caller is a model.
//!
//! ## 1. The model names a locator, and [`Workspace`] decides whether it exists
//!
//! Every path argument resolves through `scema_tools::Workspace` — resolve first, compare
//! second, symlinks followed. This is not paranoia about a hostile model; it is that a
//! perfectly cooperative model asked to "audit this project" will reason its way to
//! `~/.ssh` or `../.env` because those are genuinely relevant to a security audit, and the
//! observer would read them. The same reasoning as `alchem-link`'s `Workspace`, and the
//! same rule: a tool that opens a path directly bypasses the whole model.
//!
//! Note what confinement does *not* cover: `RepoObserver` reads file *contents* to count
//! tests and markers, and it does not currently exclude secrets the way `alchem-link`'s
//! `PROTECTED_PATTERNS` does. It emits only counts, never contents — no file body reaches
//! the model — but a signal label naming `.env` would still tell the model the file is
//! there. Point `--allow` at a project directory, not a home directory.
//!
//! ## 2. Writing is off unless asked for
//!
//! `omni_decide` seals a record and appends to memory. It is **not advertised at all**
//! unless `--allow-decide` is set — an MCP client that lists a tool which always fails
//! teaches the model to retry it, and a model that can write to the operator's decision
//! history without anyone enabling it is the wrong default. `omni_simulate` computes the
//! identical result and persists nothing, which is what a model should be using anyway.
//!
//! ## Output is clamped, and says when it clamped
//!
//! A monorepo produces hundreds of signals. Models ask for everything and then re-ask when
//! a result looks thin, and each round is a whole request, so the caps here are generous
//! but real — and a truncated list always states how many were dropped. A silently
//! truncated list is a wrong count, which is the one thing this workspace will not emit.

use scema_agent::Agent;
use scema_policy::render;
use scema_tools::Workspace;
use scema_verify::{verify, RecordStore};
use scema_world::{Constraint, Goal, WorldState};
use serde_json::{json, Value};

/// Signals listed in an `omni_observe` result before the list is capped.
pub const MAX_SIGNALS: usize = 40;
/// Objects summarised in an `omni_observe` result.
pub const MAX_OBJECTS: usize = 30;
/// Records listed by `omni_records`.
pub const MAX_RECORDS: usize = 40;

pub struct Tools {
    pub agent: Agent,
    pub workspace: Workspace,
    pub root: std::path::PathBuf,
    pub allow_decide: bool,
}

/// A tool result: text for the model, plus whether it is an error.
pub struct ToolResult {
    pub text: String,
    pub is_error: bool,
}

impl ToolResult {
    fn ok(text: impl Into<String>) -> Self {
        ToolResult { text: text.into(), is_error: false }
    }

    fn err(text: impl Into<String>) -> Self {
        ToolResult { text: text.into(), is_error: true }
    }
}

fn arg_str(args: &Value, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn arg_list(args: &Value, key: &str) -> Vec<String> {
    args.get(key)
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

impl Tools {
    /// The advertised catalogue. `omni_decide` appears only when it would work.
    pub fn definitions(&self) -> Value {
        let mut tools = vec![
            json!({
                "name": "omni_observe",
                "description": "Perceive a directory as a WorldState: units, counted signals, blind spots, and how complete the walk was. Counts only — nothing here is an estimate. Paths are confined to the server's allowed roots.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "locator": { "type": "string", "description": "Directory to observe, absolute or relative to the first allowed root." }
                    },
                    "required": ["locator"]
                }
            }),
            json!({
                "name": "omni_simulate",
                "description": "Run the full omni loop against a goal and return the simulation matrix: competing branches with expected gain, risk, cost, uncertainty and reversibility, plus the decision or the reason for abstaining. Writes nothing. An unmeasured term renders as an em dash and contributed nothing to the score.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "locator": { "type": "string", "description": "Directory to observe." },
                        "goal":    { "type": "string", "description": "What should be brought about, in the operator's words." },
                        "must_not": {
                            "type": "array", "items": { "type": "string" },
                            "description": "Things the agent must not touch, as `subject` or `subject:reason`. A branch touching one is excluded from ranking, not penalised."
                        },
                        "ground": {
                            "type": "array", "items": { "type": "string" },
                            "description": "Signal ids (from omni_observe) that this goal addresses. NOTHING infers this. Without it the goal branch has no measured expected gain and will score at or below zero, which is the correct answer when the observed world says nothing about the request."
                        }
                    },
                    "required": ["locator", "goal"]
                }
            }),
            json!({
                "name": "omni_records",
                "description": "List sealed decision records, newest first: id, goal, and what was chosen or why the agent abstained.",
                "inputSchema": { "type": "object", "properties": {} }
            }),
            json!({
                "name": "omni_explain",
                "description": "Re-read a sealed decision by id (or unique prefix): the world as perceived, every branch considered, the term-by-term arithmetic behind the choice, and which specialists declined.",
                "inputSchema": {
                    "type": "object",
                    "properties": { "id": { "type": "string", "description": "Record id or unique prefix." } },
                    "required": ["id"]
                }
            }),
            json!({
                "name": "omni_verify",
                "description": "Recompute a decision record's commitment and report any field that moved. This proves the record was not edited after sealing; it does NOT prove the world was as described.",
                "inputSchema": {
                    "type": "object",
                    "properties": { "id": { "type": "string" } },
                    "required": ["id"]
                }
            }),
            json!({
                "name": "omni_policy",
                "description": "The lambda weights, the abstention gates, the allowed workspace roots, and which specialist evaluators are registered.",
                "inputSchema": { "type": "object", "properties": {} }
            }),
            json!({
                "name": "omni_memory",
                "description": "Per-kind memory counts and projection calibration. Counterfactuals (branches not taken) are counted, never scored: a branch nobody ran has no outcome, so the mean error is reported as absent rather than as zero.",
                "inputSchema": { "type": "object", "properties": {} }
            }),
        ];

        if self.allow_decide {
            tools.push(json!({
                "name": "omni_decide",
                "description": "Everything omni_simulate does, and then seals a verifiable decision record and appends to memory. Writes to the operator's state directory. Prefer omni_simulate unless a durable record is wanted.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "locator": { "type": "string" },
                        "goal":    { "type": "string" },
                        "must_not": { "type": "array", "items": { "type": "string" } },
                        "ground":   { "type": "array", "items": { "type": "string" } }
                    },
                    "required": ["locator", "goal"]
                }
            }));
        }
        Value::Array(tools)
    }

    pub fn call(&self, name: &str, args: &Value) -> ToolResult {
        match name {
            "omni_observe" => self.observe(args),
            "omni_simulate" => self.cycle(args, false),
            "omni_decide" if self.allow_decide => self.cycle(args, true),
            "omni_decide" => ToolResult::err(
                "omni_decide is disabled on this server. Restart scema-mcp with --allow-decide, \
                 or use omni_simulate, which computes the same result and writes nothing.",
            ),
            "omni_records" => self.records(),
            "omni_explain" => self.explain(args),
            "omni_verify" => self.verify_record(args),
            "omni_policy" => self.policy(),
            "omni_memory" => self.memory(),
            other => ToolResult::err(format!("unknown tool `{other}`")),
        }
    }

    fn resolve(&self, args: &Value) -> Result<std::path::PathBuf, ToolResult> {
        let locator = arg_str(args, "locator")
            .ok_or_else(|| ToolResult::err("`locator` is required and must be a non-empty string"))?;
        self.workspace.resolve(&locator).map_err(|e| {
            // The error names the roots, so the model can correct itself rather than
            // retrying the same forbidden path.
            ToolResult::err(format!("{e}"))
        })
    }

    fn observe_world(&self, args: &Value) -> Result<WorldState, ToolResult> {
        let path = self.resolve(args)?;
        self.agent
            .observe(&path.to_string_lossy())
            .map_err(|e| ToolResult::err(format!("could not observe: {e}")))
    }

    fn observe(&self, args: &Value) -> ToolResult {
        let world = match self.observe_world(args) {
            Ok(w) => w,
            Err(e) => return e,
        };

        let mut out = render::world_header(&world);
        out.push_str("\n\n");
        out.push_str(&render::signals_capped(&world, MAX_SIGNALS));
        out.push_str("\n\nOBJECTS\n");
        for o in world.objects.iter().take(MAX_OBJECTS) {
            let attrs: Vec<String> = o
                .attrs
                .iter()
                .map(|(k, v)| format!("{k}={}", v.render()))
                .collect();
            out.push_str(&format!(
                "  {:<10} {:<28} {}\n",
                o.provenance.label(),
                o.label,
                if attrs.is_empty() {
                    "(no values — unseen, not empty)".to_string()
                } else {
                    attrs.join(" ")
                }
            ));
        }
        if world.objects.len() > MAX_OBJECTS {
            out.push_str(&format!(
                "  ... {} more object(s) not listed\n",
                world.objects.len() - MAX_OBJECTS
            ));
        }
        out.push_str(
            "\nTo let a goal inherit a measured expected gain from one of these signals, pass its\n\
             id in `ground` to omni_simulate. Nothing infers that link from wording.\n",
        );
        ToolResult::ok(out)
    }

    fn build_goal(&self, args: &Value) -> Goal {
        let mut g = Goal::new("goal", arg_str(args, "goal").unwrap_or_default());
        for spec in arg_list(args, "must_not") {
            let (subject, detail) = match spec.split_once(':') {
                Some((a, b)) => (a.trim().to_string(), b.trim().to_string()),
                None => (spec.clone(), "declared by the caller".to_string()),
            };
            if !subject.is_empty() {
                g = g.with_constraint(Constraint::must_not(subject, detail));
            }
        }
        for id in arg_list(args, "ground") {
            g = g.grounded(id);
        }
        g
    }

    fn cycle(&self, args: &Value, persist: bool) -> ToolResult {
        let world = match self.observe_world(args) {
            Ok(w) => w,
            Err(e) => return e,
        };
        let goal = self.build_goal(args);
        if goal.statement.trim().is_empty() {
            return ToolResult::err("`goal` is required and must be a non-empty string");
        }

        let dangling: Vec<String> = goal
            .grounded_in
            .iter()
            .filter(|id| !world.signals.iter().any(|s| &&s.id == id))
            .cloned()
            .collect();

        // A fresh non-persisting agent for the simulate path rather than mutating the
        // shared one. Same reasoning as the daemon: a flag flipped on a shared agent is a
        // race whose failure mode is a simulation quietly sealing a record.
        let result = if persist {
            self.agent.cycle_over(world, goal)
        } else {
            let mut dry = Agent::new(self.root.clone(), None);
            dry.persist = false;
            dry.config = self.agent.config;
            dry.cycle_over(world, goal)
        };

        let cycle = match result {
            Ok(c) => c,
            Err(e) => return ToolResult::err(format!("cycle failed: {e}")),
        };

        let mut out = render::world_header(&cycle.world);
        out.push_str("\n\n");
        out.push_str(&render::matrix(&cycle.decision, &cycle.projections));
        out.push('\n');
        out.push_str(&render::evaluators(&cycle.decision));
        out.push_str("\n\n");
        out.push_str(&render::verdict(&cycle.decision));

        if !dangling.is_empty() {
            out.push_str(&format!(
                "\n\nIGNORED GROUNDING  {} — no signal with that id exists in this world.\n\
                 Run omni_observe to see the real ids.",
                dangling.join(", ")
            ));
        }
        out.push_str(&match &cycle.record_path {
            Some(p) => format!(
                "\n\nRECORD    {}  sealed at {}\n          {} memory record(s) appended",
                cycle.record.id,
                p.display(),
                cycle.remembered
            ),
            None => format!(
                "\n\nRECORD    not written — omni_simulate is a counterfactual and leaves no trace.\n          \
                 It would seal as {}.",
                cycle.record.id
            ),
        });
        ToolResult::ok(out)
    }

    fn records(&self) -> ToolResult {
        let store = RecordStore::new(self.root.clone());
        let ids = match store.ids() {
            Ok(i) => i,
            Err(e) => return ToolResult::err(format!("could not list records: {e}")),
        };
        if ids.is_empty() {
            return ToolResult::ok(format!(
                "No decision records under {}. Nothing has been sealed here yet.",
                self.root.display()
            ));
        }
        let mut out = format!("{} record(s), newest first:\n", ids.len());
        for id in ids.iter().take(MAX_RECORDS) {
            match store.load(id) {
                Ok(r) => out.push_str(&format!(
                    "  {}  {:<44}  {}\n",
                    r.id,
                    truncate(&r.goal.statement, 44),
                    match (&r.decision.chosen, &r.decision.abstention) {
                        (Some(c), _) => format!("chose {c}"),
                        (None, Some(a)) => format!("abstained — {}", a.headline()),
                        _ => "—".into(),
                    }
                )),
                // An unreadable record still gets a line: hiding it makes a corrupt store
                // look like a smaller one.
                Err(e) => out.push_str(&format!("  {id}  <unreadable: {e}>\n")),
            }
        }
        if ids.len() > MAX_RECORDS {
            out.push_str(&format!("  ... {} older record(s) not listed\n", ids.len() - MAX_RECORDS));
        }
        ToolResult::ok(out)
    }

    fn explain(&self, args: &Value) -> ToolResult {
        let Some(id) = arg_str(args, "id") else {
            return ToolResult::err("`id` is required");
        };
        let record = match RecordStore::new(self.root.clone()).load(&id) {
            Ok(r) => r,
            Err(e) => return ToolResult::err(format!("{e}")),
        };
        let mut out = format!(
            "RECORD    {}  runtime {}\nGOAL      {}\n",
            record.id, record.runtime, record.goal.statement
        );
        for c in &record.goal.constraints {
            out.push_str(&format!(
                "          constraint {:?} `{}` — {}\n",
                c.kind, c.subject, c.detail
            ));
        }
        if !record.goal.grounded_in.is_empty() {
            out.push_str(&format!(
                "          operator asserted this addresses: {}\n",
                record.goal.grounded_in.join(", ")
            ));
        }
        out.push('\n');
        out.push_str(&render::world_header(&record.world));
        out.push_str("\n\n");
        out.push_str(&render::matrix(&record.decision, &record.projections));
        out.push('\n');
        out.push_str(&render::evaluators(&record.decision));
        out.push_str("\n\n");
        out.push_str(&render::verdict(&record.decision));

        let v = verify(&record);
        out.push_str(&format!(
            "\n\nCOMMITMENT {}\n           root {}",
            if v.valid { "VALID — the record matches its commitment" } else { "INVALID" },
            record.commitment.root
        ));
        ToolResult::ok(out)
    }

    fn verify_record(&self, args: &Value) -> ToolResult {
        let Some(id) = arg_str(args, "id") else {
            return ToolResult::err("`id` is required");
        };
        let record = match RecordStore::new(self.root.clone()).load(&id) {
            Ok(r) => r,
            Err(e) => return ToolResult::err(format!("{e}")),
        };
        let v = verify(&record);
        let mut out = format!("{}  {}\n", v.id, if v.valid { "VALID" } else { "INVALID" });
        for m in &v.mismatches {
            out.push_str(&format!(
                "  {:<12} committed {}...  recomputed {}...\n",
                m.field,
                &m.committed[..m.committed.len().min(12)],
                &m.recomputed[..m.recomputed.len().min(12)]
            ));
        }
        if v.root_only {
            out.push_str("  every part verifies but the root does not — the root was edited alone\n");
        }
        out.push_str(
            "\nThis proves the record was not edited after sealing. It does NOT prove the world\n\
             was as described — provenance carries that, not the digest. It also does not prove\n\
             this is the original record: both a record and its commitment can be regenerated by\n\
             whoever holds the file.",
        );
        ToolResult::ok(out)
    }

    fn policy(&self) -> ToolResult {
        let c = self.agent.config;
        let w = c.weights;
        let mut out = String::from("UTILITY   U = R - l1*K - l2*C - l3*U + l4*V\n");
        out.push_str(&format!("  l1 risk           {:.2}\n", w.risk));
        out.push_str(&format!("  l2 cost           {:.2}\n", w.cost));
        out.push_str(&format!("  l3 uncertainty    {:.2}\n", w.uncertainty));
        out.push_str(&format!("  l4 reversibility  {:.2}\n", w.reversibility));
        out.push_str(
            "\n  These are a stated preference, not a fitted parameter, and they are hashed into\n  every record.\n",
        );
        out.push_str("\nGATES\n");
        out.push_str(&format!("  min measured fraction  {:.0}%\n", c.min_coverage * 100.0));
        out.push_str(&format!("  specialist veto at     <= {:.2}\n", c.veto_at_or_below));
        out.push_str("\nWORKSPACE (paths outside these are refused)\n");
        for r in self.workspace.root_labels() {
            out.push_str(&format!("  {r}\n"));
        }
        out.push_str(&format!(
            "\nomni_decide  {}\n",
            if self.allow_decide { "enabled" } else { "disabled (not advertised)" }
        ));
        out.push_str("\nOBSERVERS\n");
        for o in self.agent.observers() {
            out.push_str(&format!("  {:<10} {}\n", o.name(), o.about()));
        }
        out.push_str("\nEVALUATORS\n");
        for e in self.agent.evaluators() {
            out.push_str(&format!("  {:<10} {}\n", e.name(), e.about()));
        }
        ToolResult::ok(out)
    }

    fn memory(&self) -> ToolResult {
        let mem = self.agent.memory();
        let counts = match mem.counts() {
            Ok(c) => c,
            Err(e) => return ToolResult::err(format!("memory unreadable: {e}")),
        };
        let cal = match mem.calibration() {
            Ok(c) => c,
            Err(e) => return ToolResult::err(format!("memory unreadable: {e}")),
        };
        let mut out = format!("MEMORY   {}\n", mem.root().join("memory").display());
        for (kind, n, corrupt) in counts {
            out.push_str(&format!(
                "  {:<16} {:>6} record(s){}\n",
                format!("{kind:?}"),
                n,
                if corrupt > 0 { format!("   {corrupt} unreadable line(s)") } else { String::new() }
            ));
        }
        out.push_str("\nCALIBRATION\n");
        out.push_str(&format!("  branches not taken, recorded   {}\n", cal.recorded));
        out.push_str(&format!("  of those, later resolved       {}\n", cal.resolved));
        out.push_str(&format!("  unresolved                     {}\n", cal.unresolved));
        out.push_str(&match cal.mean_abs_error {
            Some(e) => format!("  mean |projected - realised|    {e:.3}\n"),
            None => "  mean |projected - realised|    absent (nothing resolved; a branch nobody ran has no outcome)\n".to_string(),
        });
        ToolResult::ok(out)
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        return s.to_string();
    }
    s.chars().take(n.saturating_sub(1)).collect::<String>() + "…"
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn scratch() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "scema-omni-mcp-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(p.join("src")).unwrap();
        fs::write(p.join("Cargo.toml"), "[package]\nname = \"t\"\n").unwrap();
        fs::write(p.join("src/lib.rs"), "fn a() {}\n// TODO: tidy\n").unwrap();
        p
    }

    fn tools(root: &PathBuf, allow_decide: bool) -> Tools {
        Tools {
            agent: Agent::new(root.join(".scema"), None),
            workspace: Workspace::new([root]).unwrap(),
            root: root.join(".scema"),
            allow_decide,
        }
    }

    fn names(t: &Tools) -> Vec<String> {
        t.definitions()
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["name"].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn decide_is_not_advertised_unless_it_would_work() {
        // A listed tool that always fails teaches a model to retry it.
        let root = scratch();
        assert!(!names(&tools(&root, false)).contains(&"omni_decide".to_string()));
        assert!(names(&tools(&root, true)).contains(&"omni_decide".to_string()));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn calling_a_disabled_decide_explains_the_alternative() {
        let root = scratch();
        let r = tools(&root, false).call("omni_decide", &json!({ "locator": ".", "goal": "x" }));
        assert!(r.is_error);
        assert!(r.text.contains("--allow-decide"));
        assert!(r.text.contains("omni_simulate"));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn every_advertised_tool_has_a_schema_and_a_description() {
        let root = scratch();
        for d in tools(&root, true).definitions().as_array().unwrap() {
            assert!(d["description"].as_str().unwrap().len() > 40, "{d}");
            assert_eq!(d["inputSchema"]["type"], json!("object"));
        }
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_path_outside_the_workspace_is_refused_and_the_roots_are_named() {
        // The guard that matters most here: a cooperative model asked to audit a project
        // will reason its way to a home directory, and the observer would read it.
        let root = scratch();
        let t = tools(&root, false);
        let outside = std::env::temp_dir().to_string_lossy().to_string();
        let r = t.call("omni_observe", &json!({ "locator": outside }));
        assert!(r.is_error);
        assert!(r.text.contains("outside this workspace"));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn observe_reports_counted_signals_and_how_to_ground_them() {
        let root = scratch();
        let r = tools(&root, false).call("omni_observe", &json!({ "locator": "." }));
        assert!(!r.is_error, "{}", r.text);
        assert!(r.text.contains("counted"), "signals must cite counts: {}", r.text);
        assert!(r.text.contains("ground"), "the model has to be told grounding is explicit");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn simulate_persists_nothing_and_says_so() {
        let root = scratch();
        let t = tools(&root, true);
        let r = t.call("omni_simulate", &json!({ "locator": ".", "goal": "tidy up" }));
        assert!(!r.is_error, "{}", r.text);
        assert!(r.text.contains("not written"));
        assert!(RecordStore::new(t.root.clone()).ids().unwrap().is_empty());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn decide_seals_a_record_that_verifies() {
        let root = scratch();
        let t = tools(&root, true);
        let r = t.call("omni_decide", &json!({ "locator": ".", "goal": "tidy up" }));
        assert!(!r.is_error, "{}", r.text);
        let ids = RecordStore::new(t.root.clone()).ids().unwrap();
        assert_eq!(ids.len(), 1);
        let v = t.call("omni_verify", &json!({ "id": &ids[0] }));
        assert!(v.text.contains("VALID"), "{}", v.text);
        assert!(v.text.contains("does NOT prove"), "the limits must travel with the verdict");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn an_unmeasured_term_reaches_the_model_as_an_em_dash() {
        // Via `scema_policy::render`, the single definition. A model shown `0.00` for an
        // unmeasured gain will reason about it as an observation of zero.
        let root = scratch();
        let r = tools(&root, false)
            .call("omni_simulate", &json!({ "locator": ".", "goal": "rewrite everything" }));
        assert!(r.text.contains('—'), "{}", r.text);
        assert!(r.text.contains("not measured"));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_dangling_grounding_id_is_reported_to_the_model() {
        let root = scratch();
        let r = tools(&root, false).call(
            "omni_simulate",
            &json!({ "locator": ".", "goal": "x", "ground": ["typo:nope"] }),
        );
        assert!(r.text.contains("IGNORED GROUNDING"));
        assert!(r.text.contains("typo:nope"));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_missing_required_argument_is_an_error_not_a_default() {
        let root = scratch();
        let t = tools(&root, false);
        assert!(t.call("omni_observe", &json!({})).is_error);
        assert!(t.call("omni_simulate", &json!({ "locator": "." })).is_error);
        assert!(t.call("omni_explain", &json!({})).is_error);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn memory_reports_an_absent_calibration_rather_than_zero() {
        let root = scratch();
        let r = tools(&root, false).call("omni_memory", &json!({}));
        assert!(r.text.contains("absent"), "{}", r.text);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn policy_names_the_workspace_roots_it_will_accept() {
        let root = scratch();
        let r = tools(&root, false).call("omni_policy", &json!({}));
        assert!(r.text.contains("WORKSPACE"));
        assert!(r.text.contains("disabled (not advertised)"));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn an_unknown_tool_is_an_error() {
        let root = scratch();
        assert!(tools(&root, false).call("omni_delete_everything", &json!({})).is_error);
        fs::remove_dir_all(&root).ok();
    }
}
