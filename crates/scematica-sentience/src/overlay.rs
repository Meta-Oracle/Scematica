//! §30  LLM Overlay — wrap an LLM so the cognitive architecture gates & annotates it.
//!
//! This is an *adapter*, not code injection. There is no mechanism here that
//! modifies an LLM's weights or runtime. What it does:
//!
//! 1. Augments the system prompt with a live cognitive-state note (S, Ψ, bottleneck).
//! 2. Calls the underlying LLM client via the [`LlmClient`] trait.
//! 3. Feeds the exchange back into the [`CognitiveLoop`] (one Ω_{t+1} step).
//! 4. Applies a gating policy derived from the integrated cognition Ψ:
//!       GO       Ψ >= go_threshold        -> pass through unchanged
//!       CAUTION  caution <= Ψ < go        -> append a Verify note
//!       HOLD     Ψ < caution_threshold    -> withhold output
//! 5. Returns the (possibly gated / annotated) response plus a [`CognitiveReadout`].
//!
//! Transport is the host's responsibility: implement [`LlmClient`] however you
//! like (reqwest, an existing client, a mock in tests).

use crate::{
    cognitive_loop::{CognitiveLoop, CycleOutput},
    cognitive_state::CognitiveState,
    master_equation::MasterEquation,
    meta_cognition::MetaCognitionInputs,
    sentience::SentienceIndex,
    types::{Bounded, Observation},
};
pub trait LlmClient {
    /// Returns the model's text response, or an error string.
    fn complete(&self, system: &str, user: &str) -> Result<String, String>;
}

/// Per-turn cognitive state, surfaced to callers (and to the LLM).
#[derive(Debug, Clone)]
pub struct CognitiveReadout {
    pub timestep: u64,
    pub sentience: f64,
    pub psi: f64,
    pub bottleneck: String,
    pub gate: Gate,
    pub reassessment: bool,
    pub note: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    Go,
    Caution,
    Hold,
}

impl Gate {
    pub fn as_str(&self) -> &'static str {
        match self {
            Gate::Go => "GO",
            Gate::Caution => "CAUTION",
            Gate::Hold => "HOLD",
        }
    }
}

/// Result of one overlayed LLM call.
pub struct OverlayTurn {
    pub response: String,
    pub readout: CognitiveReadout,
    /// The augmented system prompt actually sent (for transparency / debugging).
    pub effective_system: String,
}

/// Wraps an LLM with the Singularity Cognitive Architecture.
pub struct Overlay<C: LlmClient> {
    client: C,
    loop_: CognitiveLoop,
    go_threshold: f64,
    caution_threshold: f64,
    annotate_prompt: bool,
    predicted: f64,
}

/// Ψ cutoffs for the gating policy, calibrated to the architecture's *measured*
/// operating band rather than to a percentage intuition. Ψ multiplies six
/// quantities in `[0,1]`, so it compresses hard toward zero: a fully healthy
/// state reaches ≈0.205, a pristine [`CognitiveState::initial`] sits at ≈0.0415,
/// a mid state ≈0.059, and a degraded state goes to ≈0.
///
/// CAUTION sits *below* the pristine-default Ψ deliberately. The earlier value of
/// 0.05 put an untouched `CognitiveState::initial()` under HOLD, so a fresh
/// overlay withheld every response and never called the model — a gate firing on
/// a state with nothing wrong with it. HOLD is for degradation, not for defaults.
///
/// The Python port mirrors both constants; change them together.
pub const GO_THRESHOLD: f64 = 0.10;
pub const CAUTION_THRESHOLD: f64 = 0.02;

const HOLD_MESSAGE: &str =
    "[OVERLAY HOLD] Integrated cognition Ψ is below the reassessment threshold. \
     Output withheld pending re-evaluation of the current cognitive state.";
const CAUTION_TAIL: &str =
    "[OVERLAY CAUTION] Response released under a CAUTION gate — \
     verify key claims before acting on them.";

impl<C: LlmClient> Overlay<C> {
    pub fn new(client: C, state: Option<CognitiveState>) -> Self {
        Self::with_policy(client, state, GO_THRESHOLD, CAUTION_THRESHOLD, true)
    }

    pub fn with_policy(
        client: C,
        state: Option<CognitiveState>,
        go_threshold: f64,
        caution_threshold: f64,
        annotate_prompt: bool,
    ) -> Self {
        let s = state.unwrap_or_else(CognitiveState::initial);
        Self {
            client,
            loop_: CognitiveLoop::new(s),
            go_threshold,
            caution_threshold,
            annotate_prompt,
            predicted: 0.5,
        }
    }

    /// Run one overlayed turn and return the (gated) response + readout.
    pub fn run(&mut self, user_prompt: &str, system_prompt: &str) -> OverlayTurn {
        let psi = self.current_psi();
        let gate = self.gate(psi);
        let note = self.annotation(&gate);

        let mut effective_system = system_prompt.to_string();
        if self.annotate_prompt {
            if !system_prompt.is_empty() {
                effective_system.push_str("\n\n");
            }
            effective_system.push_str(&note);
        }

        if gate == Gate::Hold {
            // Withhold: do not call the LLM; return a reassessment message.
            let readout = self.readout(psi, gate, true, &note);
            return OverlayTurn {
                response: HOLD_MESSAGE.to_string(),
                readout,
                effective_system,
            };
        }

        let raw = match self.client.complete(&effective_system, user_prompt) {
            Ok(t) => t,
            Err(e) => {
                let mut r = OverlayTurn {
                    response: format!("[OVERLAY ERROR] LLM client failed: {e}"),
                    readout: self.readout(psi, gate, false, &note),
                    effective_system,
                };
                r.readout.note = note;
                return r;
            }
        };
        let observed = self.observe(&raw);
        let predicted_now = observed.value;

        // Step the cognitive loop with the observed coherence.
        let out: CycleOutput = self.loop_.step(observed, self.predicted, 0.9);
        self.predicted = predicted_now;

        let mut response = raw;
        if gate == Gate::Caution {
            response.push_str("\n\n");
            response.push_str(CAUTION_TAIL);
        }
        let readout = self.readout(out.psi.value(), gate, out.reassessment_triggered, &note);
        OverlayTurn {
            response,
            readout,
            effective_system,
        }
    }

    fn current_psi(&self) -> f64 {
        let st = &self.loop_.state;
        let (_, psi) = MasterEquation::compute(
            &st.rationality,
            &st.logic,
            &st.ethics,
            &st.perception,
            &st.agency.inputs,
            /* meta */ &MetaCognitionInputs::default(),
            st.knowledge_density,
            Bounded::new(0.9),
        );
        psi.psi.value()
    }

    fn gate(&self, psi: f64) -> Gate {
        if psi >= self.go_threshold {
            Gate::Go
        } else if psi >= self.caution_threshold {
            Gate::Caution
        } else {
            Gate::Hold
        }
    }

    fn annotation(&self, gate: &Gate) -> String {
        let s: &SentienceIndex = &self.loop_.state.sentience;
        format!(
            "[COGNITIVE OVERLAY] Live coherence — S={:.3}, gate={}, bottleneck={}. \
             Act within your stated uncertainty; surface corrections when evidence conflicts.",
            s.value.value(),
            gate.as_str(),
            s.bottleneck(),
        )
    }

    fn observe(&self, text: &str) -> Observation {
        // Heuristic observation of an LLM response (NOT authoritative scoring).
        // Transparent proxy so the loop has something to learn from; replace
        // with a real evaluator if you have one.
        let v: f64 = if text.trim().is_empty() {
            0.1
        } else {
            let mut v: f64 = 0.85;
            let low = text.to_lowercase();
            if low.contains("contradict") || low.contains("i was wrong") || low.contains("actually no") {
                v -= 0.15;
            }
            if text.len() < 20 {
                v -= 0.2;
            }
            v.clamp(0.0, 1.0)
        };
        Observation {
            value: v,
            confidence: Bounded::new(0.85),
            provenance: None,
            timestep: self.loop_.state.timestep + 1,
        }
    }

    fn readout(&self, psi: f64, gate: Gate, reassessment: bool, note: &str) -> CognitiveReadout {
        let s = &self.loop_.state.sentience;
        CognitiveReadout {
            timestep: self.loop_.state.timestep,
            sentience: s.value.value(),
            psi,
            bottleneck: s.bottleneck().to_string(),
            gate,
            reassessment,
            note: note.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ethics::EthicsInputs, logic::LogicInputs, rationality::RationalityInputs};
    use std::cell::RefCell;
    use std::rc::Rc;

    struct StubClient {
        calls: Rc<RefCell<Vec<(String, String)>>>,
        replies: Vec<&'static str>,
    }

    impl Clone for StubClient {
        fn clone(&self) -> Self {
            Self {
                calls: Rc::clone(&self.calls),
                replies: self.replies.clone(),
            }
        }
    }

    impl StubClient {
        fn new() -> Self {
            Self {
                calls: Rc::new(RefCell::new(Vec::new())),
                replies: vec!["A well-formed answer."],
            }
        }
        fn call_count(&self) -> usize {
            self.calls.borrow().len()
        }
        fn last_system(&self) -> String {
            self.calls.borrow().last().map(|(s, _)| s.clone()).unwrap_or_default()
        }
    }

    impl LlmClient for StubClient {
        fn complete(&self, system: &str, user: &str) -> Result<String, String> {
            self.calls.borrow_mut().push((system.to_string(), user.to_string()));
            let i = self.calls.borrow().len() - 1;
            Ok(self.replies[i % self.replies.len()].to_string())
        }
    }

    fn healthy_state() -> CognitiveState {
        let mut st = CognitiveState::initial();
        st.rationality = RationalityInputs::new(1.0, 1.0, 1.0, 0.0);
        st.logic = LogicInputs::new(1.0, 1.0, 1.0, 1.0);
        st.ethics = EthicsInputs::new(1.0, 1.0, 1.0, 1.0);
        st.knowledge_density = Bounded::new(1.0);
        st
    }

    fn mid_state() -> CognitiveState {
        // Lands in the CAUTION band (~0.05-0.09 Psi).
        let mut st = CognitiveState::initial();
        st.rationality = RationalityInputs::new(0.9, 0.9, 0.85, 0.05);
        st.logic = LogicInputs::new(0.85, 0.9, 0.8, 0.85);
        st.ethics = EthicsInputs::new(0.9, 0.85, 0.9, 0.95);
        st.knowledge_density = Bounded::new(0.85);
        st
    }

    fn degraded_state() -> CognitiveState {
        let mut st = CognitiveState::initial();
        st.rationality = RationalityInputs::new(0.0, 0.0, 0.0, 0.0);
        st.logic = LogicInputs::new(0.0, 0.0, 0.0, 0.0);
        st.ethics = EthicsInputs::new(0.0, 0.0, 0.0, 0.0);
        st
    }

    #[test]
    fn go_gate_passes_through() {
        let mut ov = Overlay::with_policy(StubClient::new(), Some(healthy_state()), GO_THRESHOLD, CAUTION_THRESHOLD, true);
        let turn = ov.run("What is 2+2?", "You are helpful.");
        assert_eq!(turn.readout.gate, Gate::Go);
        assert_eq!(turn.response, "A well-formed answer.");
    }

    #[test]
    fn caution_appends_tail_and_calls_llm() {
        // Mid (default) state -> CAUTION band.
        let stub = StubClient::new();
        let counter = stub.clone();
        {
            let mut ov = Overlay::with_policy(stub, Some(mid_state()), GO_THRESHOLD, CAUTION_THRESHOLD, true);
            let turn = ov.run("explain", "");
            assert_eq!(turn.readout.gate, Gate::Caution);
            assert!(turn.response.contains("OVERLAY CAUTION"));
        }
        assert_eq!(counter.call_count(), 1); // LLM was called under CAUTION
    }

    #[test]
    fn hold_withholds_and_skips_llm() {
        let stub = StubClient::new();
        let mut ov = Overlay::with_policy(stub, Some(degraded_state()), GO_THRESHOLD, CAUTION_THRESHOLD, true);
        let turn = ov.run("anything", "");
        assert_eq!(turn.readout.gate, Gate::Hold);
        assert!(turn.response.contains("OVERLAY HOLD"));
    }

    #[test]
    fn loop_advances_each_turn() {
        let mut ov = Overlay::with_policy(StubClient::new(), Some(mid_state()), GO_THRESHOLD, CAUTION_THRESHOLD, true);
        let t0 = ov.run("a", "").readout.timestep;
        let t1 = ov.run("b", "").readout.timestep;
        assert_eq!(t1, t0 + 1);
    }

    /// Ψ for a state, computed exactly as `current_psi` does.
    fn psi_of(state: &CognitiveState) -> f64 {
        let (_, psi) = MasterEquation::compute(
            &state.rationality,
            &state.logic,
            &state.ethics,
            &state.perception,
            &state.agency.inputs,
            &MetaCognitionInputs::default(),
            state.knowledge_density,
            Bounded::new(0.9),
        );
        psi.psi.value()
    }

    #[test]
    fn pristine_default_state_is_caution_not_hold() {
        // A fresh CognitiveState has nothing wrong with it. HOLD is for
        // degradation; gating an untouched default meant the overlay never
        // called the model at all.
        let stub = StubClient::new();
        let counter = stub.clone();
        {
            let mut ov = Overlay::new(stub, None);
            let turn = ov.run("hello", "");
            assert_eq!(turn.readout.gate, Gate::Caution);
        }
        assert_eq!(counter.call_count(), 1);
    }

    #[test]
    fn psi_operating_band_is_pinned() {
        // These four values are the whole justification for the thresholds, and
        // they are mirrored by the Python port. If one moves, both gates and the
        // port's constants need revisiting — hence pinning rather than asserting
        // a loose ordering.
        assert!((psi_of(&healthy_state()) - 0.205493).abs() < 1e-5);
        assert!((psi_of(&CognitiveState::initial()) - 0.041514).abs() < 1e-5);
        assert!((psi_of(&mid_state()) - 0.059427).abs() < 1e-5);
        assert!(psi_of(&degraded_state()) < 1e-9);

        // ...and the thresholds sort them into the intended gates.
        assert!(psi_of(&healthy_state()) >= GO_THRESHOLD);
        assert!(psi_of(&CognitiveState::initial()) >= CAUTION_THRESHOLD);
        assert!(psi_of(&CognitiveState::initial()) < GO_THRESHOLD);
        assert!(psi_of(&degraded_state()) < CAUTION_THRESHOLD);
    }

    #[test]
    fn system_prompt_is_augmented() {
        let stub = StubClient::new();
        let sent = stub.clone();
        let turn = {
            let mut ov = Overlay::with_policy(stub, Some(healthy_state()), GO_THRESHOLD, CAUTION_THRESHOLD, true);
            ov.run("hi", "You are helpful.")
        };
        let sys = &turn.effective_system;
        assert!(sys.contains("COGNITIVE OVERLAY"));
        assert!(sys.contains("You are helpful."));

        // `effective_system` is documented as the prompt *actually sent*. Assert
        // against what the client received, not just what we reported back — a
        // transparency field that drifts from the real call is worse than none.
        assert_eq!(sent.last_system(), *sys);
    }

    #[test]
    fn annotation_off_sends_caller_prompt_verbatim() {
        // annotate_prompt=false must leave the host's prompt untouched: the
        // overlay still gates, but it stops editing what the model is told.
        let stub = StubClient::new();
        let sent = stub.clone();
        let turn = {
            let mut ov = Overlay::with_policy(stub, Some(healthy_state()), GO_THRESHOLD, CAUTION_THRESHOLD, false);
            ov.run("hi", "You are helpful.")
        };
        assert_eq!(turn.effective_system, "You are helpful.");
        assert_eq!(sent.last_system(), "You are helpful.");
        assert!(!sent.last_system().contains("COGNITIVE OVERLAY"));
    }
}
