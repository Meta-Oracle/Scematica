//! Whether an action may happen. The gate Scematica Omni needs before it can act.
//!
//! A port of `alchem_link.approvals`, against the specification in
//! `alchem-link/docs/TRUST-MODEL.md` and the shared cases in
//! `alchem-link/vectors/trust-model.json`. Python is the reference implementation and this
//! crate is checked against it, exactly as `canonical.ts` is checked against
//! `canonical.rs` — one stated rule, shared vectors, and whichever side fails is wrong.
//!
//! ## Two gates, and they must stay apart
//!
//! [`scema_tools::Workspace`] answers **where** an action may reach. This crate answers
//! **whether** it may happen at all. Merging them is how a grant for one silently becomes a
//! grant for the other: a person who approved "write to `docs/`" has said nothing about
//! whether `~/.ssh` is inside the workspace, and a workspace that contains a path has said
//! nothing about whether writing is allowed today.
//!
//! ## Why this exists before `scema execute`
//!
//! `execute`, `delegate`, `discover` and `pay` exit 2 today, and the CLI says why: the
//! action path needs an approval model in front of it. That is not a placeholder sentiment.
//! Every other layer of this runtime is arranged so it cannot express a confidence it did
//! not earn; an action path without this would be the one place that could.
//!
//! ## What this crate deliberately does not do
//!
//! It does not perform actions, touch the filesystem, or prompt. [`TrustPolicy::preflight`]
//! is a pure function from a policy and a request to a decision or "ask", which is what
//! makes it checkable against a vector file at all. The prompting half is
//! [`Approver`], and its default in a non-interactive process is [`DenyApprover`].

use std::collections::BTreeMap;

/// What kind of act a tool performs.
///
/// Declared per tool and never inferred. A tool called `fetch_and_apply_patch` gets its risk
/// from whoever wrote it; any scheme that guesses from a name will eventually guess low on
/// the one tool where it matters, and it will do so silently.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Risk {
    /// Reads local files or directory structure.
    ///
    /// Cheap, and not free of consequence: the result goes to a third-party model, so this
    /// is a disclosure rather than an inspection.
    Read,
    /// Reads a chain or an HTTP endpoint. No local effect.
    ///
    /// Shares [`Risk::rank`] with [`Risk::Read`] but is a distinct arm on purpose, so a
    /// deployment can refuse one without the other.
    Network,
    /// Creates, modifies, moves or deletes inside the workspace.
    Write,
    /// Runs an arbitrary command. Unbounded.
    Execute,
}

impl Risk {
    /// Ordering used by policy, matching the reference implementation's table.
    pub fn rank(self) -> u8 {
        match self {
            Risk::Read | Risk::Network => 0,
            Risk::Write => 2,
            Risk::Execute => 3,
        }
    }

    /// Does this change anything?
    pub fn mutating(self) -> bool {
        matches!(self, Risk::Write | Risk::Execute)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Risk::Read => "read",
            Risk::Network => "network",
            Risk::Write => "write",
            Risk::Execute => "execute",
        }
    }

    /// Parse a wire spelling. `None` for an unknown name.
    ///
    /// Closed rather than open, unlike `Domain` and `EntityKind` in `scema-world`, and for
    /// the opposite reason: an unrecognised *domain* should degrade to a warning so that
    /// producers can describe new worlds, but an unrecognised *risk* has no safe default.
    /// Treating it as `Read` understates and treating it as `Execute` makes the vocabulary
    /// unusable, so the caller has to decide.
    pub fn parse(s: &str) -> Option<Risk> {
        match s.trim().to_ascii_lowercase().as_str() {
            "read" => Some(Risk::Read),
            "network" => Some(Risk::Network),
            "write" => Some(Risk::Write),
            "execute" => Some(Risk::Execute),
            _ => None,
        }
    }
}

/// The answer to one request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny,
    /// Allow this and every later call matching the same grant key, for this session.
    AllowAlways,
    /// Deny this and every later call matching the same grant key, for this session.
    DenyAlways,
}

impl Decision {
    pub fn allowed(self) -> bool {
        matches!(self, Decision::Allow | Decision::AllowAlways)
    }

    /// Does this decision persist for the session?
    pub fn sticky(self) -> bool {
        matches!(self, Decision::AllowAlways | Decision::DenyAlways)
    }

    /// The non-sticky form a sticky decision is stored as.
    pub fn settled(self) -> Decision {
        match self {
            Decision::AllowAlways => Decision::Allow,
            Decision::DenyAlways => Decision::Deny,
            other => other,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Decision::Allow => "allow",
            Decision::Deny => "deny",
            Decision::AllowAlways => "allow_always",
            Decision::DenyAlways => "deny_always",
        }
    }
}

/// One tool call awaiting a decision.
#[derive(Clone, Debug)]
pub struct Request {
    pub tool: String,
    pub risk: Risk,
    /// The workspace-relative path this would touch, when there is one.
    pub path: String,
    /// One line describing the effect, for the prompt.
    pub summary: String,
}

impl Request {
    pub fn new(tool: impl Into<String>, risk: Risk) -> Self {
        Request { tool: tool.into(), risk, path: String::new(), summary: String::new() }
    }

    pub fn at(mut self, path: impl Into<String>) -> Self {
        self.path = path.into();
        self
    }

    pub fn describing(mut self, summary: impl Into<String>) -> Self {
        self.summary = summary.into();
        self
    }

    /// The key a sticky decision is remembered under.
    ///
    /// Tool plus the *directory* of the path, never the file. Approving one write into a
    /// directory covers the rest of that directory — which is what makes a session usable —
    /// while still not covering the whole workspace. Prompting per file is how people learn
    /// to approve without reading.
    pub fn grant_key(&self) -> String {
        if self.path.is_empty() {
            return self.tool.clone();
        }
        let parent = match self.path.rfind('/') {
            Some(i) => &self.path[..i],
            None => ".",
        };
        format!("{}:{}", self.tool, parent)
    }
}

/// A standing decision, matched by tool glob and optional path glob.
#[derive(Clone, Debug)]
pub struct Rule {
    pub tool: String,
    pub decision: Decision,
    pub path: String,
}

impl Rule {
    pub fn new(tool: impl Into<String>, decision: Decision) -> Self {
        Rule { tool: tool.into(), decision, path: "*".into() }
    }

    pub fn at(mut self, path: impl Into<String>) -> Self {
        self.path = path.into();
        self
    }

    pub fn matches(&self, request: &Request) -> bool {
        if !glob_match(&self.tool, &request.tool) {
            return false;
        }
        if self.path == "*" || self.path.is_empty() {
            return true;
        }
        glob_match(&self.path, &request.path)
    }
}

/// `fnmatch`-style glob: `*` spans any run, `?` one character.
///
/// Hand-rolled rather than pulled in, so this crate keeps no dependencies and the port has
/// nothing to disagree with Python about. Iterative with backtracking, not recursive: a
/// hostile pattern like `*a*a*a*a*b` against a long name is a stack overflow in the naive
/// version, and this matches paths a model chose.
fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let (mut star, mut mark) = (usize::MAX, 0usize);

    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = pi;
            mark = ti;
            pi += 1;
        } else if star != usize::MAX {
            pi = star + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// What is allowed without asking, what must be asked, and what is refused.
#[derive(Clone, Debug, Default)]
pub struct TrustPolicy {
    /// Refuse every mutation, whatever else is set.
    pub read_only: bool,
    /// Stop prompting for writes.
    pub allow_writes: bool,
    /// Turn shell access on at all.
    pub allow_execute: bool,
    /// Standing decisions, in order. The first match wins.
    pub rules: Vec<Rule>,
    /// Sticky answers from this session. Never persisted.
    grants: BTreeMap<String, Decision>,
}

impl TrustPolicy {
    /// The default posture: reads allowed, writes prompt, execution refused.
    pub fn new() -> Self {
        Self::default()
    }

    pub fn read_only() -> Self {
        TrustPolicy { read_only: true, ..Default::default() }
    }

    /// A decision needing no prompt, or `None` meaning **ask**.
    ///
    /// The order is the specification and it is the reason this crate exists:
    ///
    /// 1. hard refusals
    /// 2. explicit rules
    /// 3. session grants
    /// 4. standing configuration
    ///
    /// **A refusal must never be reversible by a grant given for something else.** Hard
    /// refusals come first for exactly that: somebody who approved "always allow writes to
    /// `docs/`" has not consented to shell execution, and no ordering that consults grants
    /// before refusals can promise otherwise. Rules precede grants because a rule is the
    /// deployment's stated policy and a grant is one person's convenience during one
    /// session.
    pub fn preflight(&self, request: &Request) -> Option<Decision> {
        if self.read_only && request.risk.mutating() {
            return Some(Decision::Deny);
        }
        if request.risk == Risk::Execute && !self.allow_execute {
            return Some(Decision::Deny);
        }

        for rule in &self.rules {
            if rule.matches(request) {
                return Some(rule.decision);
            }
        }

        if let Some(granted) = self.grants.get(&request.grant_key()) {
            return Some(*granted);
        }

        if !request.risk.mutating() {
            return Some(Decision::Allow);
        }
        if request.risk == Risk::Write && self.allow_writes {
            return Some(Decision::Allow);
        }
        None
    }

    /// Record a sticky decision for the rest of the session.
    ///
    /// Nothing reaches disk, ever. A permission that survives the process turns one
    /// keystroke into standing authorisation, and whoever granted it will not remember.
    pub fn remember(&mut self, request: &Request, decision: Decision) {
        if decision.sticky() {
            self.grants.insert(request.grant_key(), decision.settled());
        }
    }

    /// Insert a grant directly. For reconstructing a session, and for the vector tests.
    pub fn grant(&mut self, key: impl Into<String>, decision: Decision) {
        self.grants.insert(key.into(), decision.settled());
    }

    pub fn grants(&self) -> &BTreeMap<String, Decision> {
        &self.grants
    }

    /// Drop session grants. Returns how many went.
    pub fn revoke(&mut self, key: &str) -> usize {
        if key.is_empty() {
            let n = self.grants.len();
            self.grants.clear();
            return n;
        }
        let matched: Vec<String> =
            self.grants.keys().filter(|k| glob_match(key, k)).cloned().collect();
        for k in &matched {
            self.grants.remove(k);
        }
        matched.len()
    }
}

/// Why a request did not go ahead.
///
/// Three outcomes, kept apart. Reporting "the user declined" when no prompt was shown
/// describes a decision nobody made, and sends somebody looking for a prompt they never
/// saw. Same distinction as `scema doctor`'s four verdicts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// A rule or a hard refusal fired. No prompt was shown.
    Policy,
    /// A prompt was shown and the answer was no.
    Declined,
}

/// The outcome of asking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Allowed,
    Refused(Refusal),
}

/// Answers the question a policy could not.
pub trait Approver {
    /// Ask. Only called when [`TrustPolicy::preflight`] returned `None`.
    fn prompt(&mut self, request: &Request) -> Decision;

    /// Preflight, then prompt if needed, remembering a sticky answer.
    fn decide(&mut self, policy: &mut TrustPolicy, request: &Request) -> Outcome {
        if let Some(d) = policy.preflight(request) {
            return if d.allowed() { Outcome::Allowed } else { Outcome::Refused(Refusal::Policy) };
        }
        let answer = self.prompt(request);
        policy.remember(request, answer);
        if answer.allowed() {
            Outcome::Allowed
        } else {
            Outcome::Refused(Refusal::Declined)
        }
    }
}

/// Refuses everything it is asked.
///
/// The default when standard input is not a terminal. Piped input and CI must not treat
/// silence as consent; an explicit opt-out has to be typed by somebody who meant it.
pub struct DenyApprover;

impl Approver for DenyApprover {
    fn prompt(&mut self, _request: &Request) -> Decision {
        Decision::Deny
    }
}

/// Allows everything it is asked. The explicit opt-out, never a default.
pub struct AutoApprover;

impl Approver for AutoApprover {
    fn prompt(&mut self, _request: &Request) -> Decision {
        Decision::Allow
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_is_not_reversible_by_a_grant_for_something_else() {
        // The central ordering property, and the reason hard refusals come first.
        let mut p = TrustPolicy::read_only();
        p.grant("write_file:docs", Decision::Allow);
        let r = Request::new("write_file", Risk::Write).at("docs/a.md");
        assert_eq!(p.preflight(&r), Some(Decision::Deny));
    }

    #[test]
    fn execution_is_refused_rather_than_asked_until_enabled() {
        // Not `None`. A prompt would let one keystroke turn shell access on.
        let p = TrustPolicy::new();
        assert_eq!(p.preflight(&Request::new("run", Risk::Execute)), Some(Decision::Deny));

        let p = TrustPolicy { allow_execute: true, ..Default::default() };
        assert_eq!(p.preflight(&Request::new("run", Risk::Execute)), None, "should ask");
    }

    #[test]
    fn a_grant_is_keyed_by_directory_and_does_not_leak_sideways() {
        let mut p = TrustPolicy::new();
        p.grant("write_file:docs", Decision::Allow);
        let same_dir = Request::new("write_file", Risk::Write).at("docs/other.md");
        let sibling = Request::new("write_file", Risk::Write).at("src/other.rs");
        let other_tool = Request::new("delete_file", Risk::Write).at("docs/a.md");
        assert_eq!(p.preflight(&same_dir), Some(Decision::Allow));
        assert_eq!(p.preflight(&sibling), None);
        assert_eq!(p.preflight(&other_tool), None);
    }

    #[test]
    fn a_pathless_request_is_keyed_by_tool_alone() {
        assert_eq!(Request::new("run", Risk::Execute).grant_key(), "run");
        assert_eq!(
            Request::new("write_file", Risk::Write).at("a.md").grant_key(),
            "write_file:."
        );
    }

    #[test]
    fn the_first_matching_rule_wins() {
        let mut p = TrustPolicy::new();
        p.rules.push(Rule::new("write_file", Decision::Allow).at("docs/*"));
        p.rules.push(Rule::new("write_file", Decision::Deny).at("*"));
        let r = Request::new("write_file", Risk::Write).at("docs/a.md");
        assert_eq!(p.preflight(&r), Some(Decision::Allow));
    }

    #[test]
    fn a_sticky_answer_is_stored_settled_and_never_leaves_the_process() {
        let mut p = TrustPolicy::new();
        let r = Request::new("write_file", Risk::Write).at("docs/a.md");
        p.remember(&r, Decision::AllowAlways);
        assert_eq!(p.grants().get("write_file:docs"), Some(&Decision::Allow));
        // A non-sticky answer is not remembered at all.
        let r2 = Request::new("write_file", Risk::Write).at("src/a.rs");
        p.remember(&r2, Decision::Allow);
        assert!(!p.grants().contains_key("write_file:src"));
    }

    #[test]
    fn a_policy_refusal_and_a_declined_prompt_are_different_outcomes() {
        // Saying "the user declined" when no prompt was shown describes a decision nobody
        // made, and sends them looking for a prompt they never saw.
        let mut p = TrustPolicy::new();
        let mut a = DenyApprover;
        let refused = a.decide(&mut p, &Request::new("run", Risk::Execute));
        assert_eq!(refused, Outcome::Refused(Refusal::Policy));

        let mut p = TrustPolicy { allow_execute: true, ..Default::default() };
        let declined = a.decide(&mut p, &Request::new("run", Risk::Execute));
        assert_eq!(declined, Outcome::Refused(Refusal::Declined));
    }

    #[test]
    fn an_unknown_risk_does_not_get_a_default() {
        // Closed on purpose, unlike `Domain`. `Read` understates and `Execute` makes the
        // vocabulary unusable, so the caller decides.
        assert_eq!(Risk::parse("write"), Some(Risk::Write));
        assert_eq!(Risk::parse(" EXECUTE "), Some(Risk::Execute));
        assert_eq!(Risk::parse("frobnicate"), None);
    }

    #[test]
    fn globbing_matches_the_reference_semantics() {
        assert!(glob_match("write_*", "write_file"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("src/*", "src/main.rs"));
        assert!(!glob_match("src/*", "docs/a.md"));
        assert!(glob_match("a?c", "abc"));
        assert!(!glob_match("a?c", "ac"));
        assert!(glob_match("*.rs", "main.rs"));
        assert!(!glob_match("*.rs", "main.rss"));
    }

    #[test]
    fn a_pathological_glob_does_not_blow_the_stack() {
        // These patterns match paths a model chose, so the matcher has to survive one.
        let text = "a".repeat(2_000);
        assert!(!glob_match("*a*a*a*a*a*a*b", &text));
    }

    #[test]
    fn revoking_drops_grants_by_glob_or_wholesale() {
        let mut p = TrustPolicy::new();
        p.grant("write_file:docs", Decision::Allow);
        p.grant("write_file:src", Decision::Allow);
        p.grant("run", Decision::Allow);
        assert_eq!(p.revoke("write_file:*"), 2);
        assert_eq!(p.grants().len(), 1);
        assert_eq!(p.revoke(""), 1);
        assert!(p.grants().is_empty());
    }
}
