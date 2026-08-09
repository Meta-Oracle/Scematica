"""Tests for the sentience LLM overlay adapter (zero-dependency, offline)."""
import sys
import os
import unittest

# Ensure the src directory is on the path when run directly
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "src"))

from sentience import (
    Overlay, CognitiveState, CognitiveReadout, Bounded,
    RationalityInputs, LogicInputs, EthicsInputs, Perception, MasterEquation,
)
from sentience.master_equation import DEFAULT_AGENCY_RATIO, DEFAULT_META_RATIO
from sentience.overlay import DEFAULT_GO_THRESHOLD, DEFAULT_CAUTION_THRESHOLD


class _StubClient:
    """A fake LLM client that records the prompt it was given and echoes input."""

    def __init__(self):
        self.calls = []
        self.replies = ["A well-formed answer."]

    def complete(self, system, user):
        self.calls.append((system, user))
        idx = (len(self.calls) - 1) % len(self.replies)
        return self.replies[idx]


def _healthy_state():
    """Every component maxed — mirrors `healthy_state()` in overlay.rs."""
    st = CognitiveState.initial()
    st.rationality = RationalityInputs(evidence=1.0, consistency=1.0,
                                       uncertainty=1.0, bias=0.0)
    st.logic = LogicInputs(validity=1.0, consistency=1.0, causal=1.0, formal=1.0)
    st.ethics = EthicsInputs(harm=1.0, contextual=1.0, fairness=1.0, rights=1.0)
    st.perception = Perception(audio=1.0, visual=1.0, sensory=1.0, integrity=1.0)
    st.knowledge_density = Bounded(1.0)
    return st


def _degraded_state():
    st = CognitiveState.initial()
    st.rationality = RationalityInputs(evidence=0.0, consistency=0.0,
                                       uncertainty=0.0, bias=0.0)
    st.logic = LogicInputs(validity=0.0, consistency=0.0, causal=0.0, formal=0.0)
    st.ethics = EthicsInputs(harm=0.0, contextual=0.0, fairness=0.0, rights=0.0)
    return st


def _psi_of(state):
    """Ψ for a state, computed exactly as `Overlay._current_psi` does."""
    _, psi = MasterEquation.compute(
        state.rationality, state.logic, state.ethics, state.perception,
        agency_ratio=DEFAULT_AGENCY_RATIO,
        meta_ratio=DEFAULT_META_RATIO,
        knowledge_density=state.knowledge_density.value,
        feedback=0.9,
    )
    return psi.psi.value


class OverlayBasics(unittest.TestCase):
    def test_run_reaches_go_gate_with_healthy_state(self):
        ov = Overlay(_StubClient(), state=_healthy_state())
        turn = ov.run("What is 2+2?")
        self.assertIsInstance(turn.readout, CognitiveReadout)
        self.assertEqual(turn.readout.gate, "GO")
        self.assertEqual(turn.response, "A well-formed answer.")

    def test_system_prompt_is_augmented_when_enabled(self):
        client = _StubClient()
        ov = Overlay(client, state=_healthy_state(), annotate_prompt=True)
        ov.run("hi", system_prompt="You are helpful.")
        sent_sys = client.calls[0][0]
        self.assertIn("COGNITIVE OVERLAY", sent_sys)
        self.assertIn("You are helpful.", sent_sys)

    def test_pristine_default_state_is_caution_not_hold(self):
        """A fresh CognitiveState has nothing wrong with it — HOLD is for degradation."""
        client = _StubClient()
        ov = Overlay(client)
        turn = ov.run("explain")
        self.assertEqual(turn.readout.gate, "CAUTION")
        self.assertIn("OVERLAY CAUTION", turn.response)
        self.assertEqual(len(client.calls), 1)  # LLM WAS called under CAUTION

    def test_hold_withholds_and_does_not_call_llm(self):
        client = _StubClient()
        ov = Overlay(client, state=_degraded_state())
        turn = ov.run("anything")
        self.assertEqual(turn.readout.gate, "HOLD")
        self.assertIn("OVERLAY HOLD", turn.response)
        self.assertEqual(len(client.calls), 0)  # LLM never called under HOLD

    def test_loop_advances_each_turn(self):
        client = _StubClient()
        ov = Overlay(client, annotate_prompt=False)
        t0 = ov.run("a").readout.timestep
        t1 = ov.run("b").readout.timestep
        self.assertEqual(t1, t0 + 1)


class PsiOperatingBand(unittest.TestCase):
    """The band the thresholds are calibrated to — pinned against the Rust port.

    The same four numbers are asserted in `overlay.rs::psi_operating_band_is_pinned`.
    They diverged once, when this port held the agency and meta-cognition ratios as
    standalone scalars (~0.85) where Rust computes them as products of their factors
    (~0.50 and ~0.46): identical states then gated differently depending on which
    language was asked.
    """

    def test_psi_values_match_the_rust_implementation(self):
        self.assertAlmostEqual(_psi_of(_healthy_state()), 0.205493, places=5)
        self.assertAlmostEqual(_psi_of(CognitiveState.initial()), 0.041514, places=5)
        self.assertLess(_psi_of(_degraded_state()), 1e-9)

    def test_thresholds_sort_the_band_into_the_intended_gates(self):
        self.assertGreaterEqual(_psi_of(_healthy_state()), DEFAULT_GO_THRESHOLD)
        self.assertGreaterEqual(_psi_of(CognitiveState.initial()), DEFAULT_CAUTION_THRESHOLD)
        self.assertLess(_psi_of(CognitiveState.initial()), DEFAULT_GO_THRESHOLD)
        self.assertLess(_psi_of(_degraded_state()), DEFAULT_CAUTION_THRESHOLD)


if __name__ == "__main__":
    unittest.main()
