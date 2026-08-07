"""Tests for the sentience LLM overlay adapter (zero-dependency, offline)."""
import unittest
from sentience import Overlay, CognitiveState, CognitiveReadout, RationalityInputs


class _StubClient:
    """A fake LLM client that records the prompt it was given and echoes input."""

    def __init__(self):
        self.calls = []
        self.replies = ["A well-formed answer."]

    def complete(self, system, user):
        self.calls.append((system, user))
        idx = (len(self.calls) - 1) % len(self.replies)
        return self.replies[idx]


class OverlayBasics(unittest.TestCase):
    def test_run_reaches_go_gate_with_healthy_state(self):
        # Build a high-coherence state so Psi clears the GO threshold.
        st = CognitiveState.initial()
        st.rationality = RationalityInputs(evidence=1.0, consistency=1.0,
                                          uncertainty=1.0, bias=0.0)
        st.knowledge_density = __import__("sentience").Bounded(1.0)
        ov = Overlay(_StubClient(), state=st, go_threshold=0.18,
                     caution_threshold=0.10)
        turn = ov.run("What is 2+2?")
        self.assertIsInstance(turn.readout, CognitiveReadout)
        self.assertEqual(turn.readout.gate, "GO")
        self.assertEqual(turn.response, "A well-formed answer.")

    def test_system_prompt_is_augmented_when_enabled(self):
        client = _StubClient()
        st = CognitiveState.initial()
        st.rationality = RationalityInputs(evidence=1.0, consistency=1.0,
                                          uncertainty=1.0, bias=0.0)
        st.knowledge_density = __import__("sentience").Bounded(1.0)
        ov = Overlay(client, state=st, annotate_prompt=True,
                     go_threshold=0.18, caution_threshold=0.10)
        ov.run("hi", system_prompt="You are helpful.")
        sent_sys = client.calls[0][0]
        self.assertIn("COGNITIVE OVERLAY", sent_sys)
        self.assertIn("You are helpful.", sent_sys)

    def test_caution_appends_verify_tail(self):
        # Default state yields Psi ~0.12, which is CAUTION under calibrated bands.
        client = _StubClient()
        ov = Overlay(client, go_threshold=0.18, caution_threshold=0.10)
        turn = ov.run("explain")
        self.assertEqual(turn.readout.gate, "CAUTION")
        self.assertIn("OVERLAY CAUTION", turn.response)
        self.assertEqual(len(client.calls), 1)  # LLM WAS called under CAUTION

    def test_hold_withholds_and_does_not_call_llm(self):
        client = _StubClient()
        # Degrade state so Psi drops below the HOLD threshold.
        st = CognitiveState.initial()
        st.rationality.evidence_utilization = __import__("sentience").Bounded(0.0)
        st.logic.validity = __import__("sentience").Bounded(0.0)
        st.ethics.harm = __import__("sentience").Bounded(0.0)
        st.perception.integrity = __import__("sentience").Bounded(0.0)
        ov = Overlay(client, state=st, go_threshold=0.18, caution_threshold=0.10)
        turn = ov.run("anything")
        self.assertEqual(turn.readout.gate, "HOLD")
        self.assertIn("OVERLAY HOLD", turn.response)
        self.assertEqual(len(client.calls), 0)  # LLM never called under HOLD

    def test_loop_advances_each_turn(self):
        client = _StubClient()
        ov = Overlay(client, annotate_prompt=False,
                     go_threshold=0.18, caution_threshold=0.10)
        t0 = ov.run("a").readout.timestep
        t1 = ov.run("b").readout.timestep
        self.assertEqual(t1, t0 + 1)


if __name__ == "__main__":
    unittest.main()
