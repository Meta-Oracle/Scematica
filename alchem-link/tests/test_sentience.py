"""Tests for the sentience package — zero external dependencies.

Written as `unittest.TestCase` classes rather than bare `test_*` functions, because
the project's documented runner is `python -m unittest discover -s tests`. Bare
functions pass under pytest and are silently invisible to unittest discovery, which
means coverage that reads as adequate right up until the run that needed it.
"""
import io
import sys
import os
import unittest

# Ensure the src directory is on the path when run directly
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "src"))

from sentience import (
    Bounded, Perception, RationalityInputs, LogicInputs, EthicsInputs,
    SentienceIndex, MasterEquation, GrowthModel, CognitiveLoop, CognitiveState,
)
from sentience.types import Observation
from sentience.data_integrity import DataIntegrityInputs
from sentience.ethics import ActionEvaluation


class SentienceStructure(unittest.TestCase):
    """§1 — the multiplicative form, and the bottleneck it exposes."""

    def test_sentience_zero_when_perception_zero(self):
        """S → 0 when any perception channel is 0."""
        d = Perception(audio=0.0, visual=1.0, sensory=1.0, integrity=1.0)
        s = SentienceIndex.compute(RationalityInputs(), LogicInputs(), EthicsInputs(), d)
        self.assertEqual(s.value.value, 0.0)

    def test_sentience_zero_when_ethics_zero(self):
        m = EthicsInputs(harm=0.0)
        s = SentienceIndex.compute(RationalityInputs(), LogicInputs(), m, Perception())
        self.assertEqual(s.value.value, 0.0)

    def test_sentience_bounded(self):
        s = SentienceIndex.compute(
            RationalityInputs(), LogicInputs(), EthicsInputs(), Perception()
        )
        self.assertGreater(s.value.value, 0.0)
        self.assertLessEqual(s.value.value, 1.0)

    def test_sentience_bottleneck_identified(self):
        # Make logic the obvious weak link
        l = LogicInputs(validity=0.1, consistency=0.1, causal=0.1, formal=0.1)
        s = SentienceIndex.compute(RationalityInputs(), l, EthicsInputs(), Perception())
        self.assertEqual(s.bottleneck(), "logic")


class DataIntegrity(unittest.TestCase):
    """§2 — geometric mean: a weak component degrades, a zero still annihilates."""

    def test_data_integrity_perfect(self):
        di = DataIntegrityInputs(1.0, 1.0, 1.0, 1.0)
        self.assertAlmostEqual(di.integrity().value, 1.0, places=9)

    def test_data_integrity_zero_propagates(self):
        di = DataIntegrityInputs(0.0, 1.0, 1.0, 1.0)
        self.assertEqual(di.integrity().value, 0.0)

    def test_data_integrity_geometric_mean(self):
        di = DataIntegrityInputs(0.5, 0.5, 0.5, 0.5)
        expected = 0.5  # (0.5^4)^0.25 = 0.5
        self.assertAlmostEqual(di.integrity().value, expected, places=9)


class Rationality(unittest.TestCase):
    """§3 — R = (E × Co_r × U) / (B + ε), clamped to [0,1]."""

    def test_rationality_bias_penalty(self):
        low = RationalityInputs(bias=0.01)
        high = RationalityInputs(bias=0.9)
        self.assertGreater(low.rationality().value, high.rationality().value)

    def test_rationality_bounded(self):
        r = RationalityInputs()
        self.assertGreaterEqual(r.rationality().value, 0.0)
        self.assertLessEqual(r.rationality().value, 1.0)

    def test_rationality_saturates_below_the_bias_knee(self):
        """The clamp means R reports 1.0 for any bias well under the numerator.

        Pinned because a rationality of 1.000 is evidence that bias is below the
        point where this equation starts to discriminate — not evidence of
        excellence — and the defaults sit in that saturated band.
        """
        self.assertEqual(RationalityInputs().rationality().value, 1.0)
        knee = RationalityInputs(evidence=0.9, consistency=0.9, uncertainty=0.8, bias=0.648)
        self.assertLess(knee.rationality().value, 1.0)


class EthicsGate(unittest.TestCase):
    """§5 — a hard gate returns nothing, not a small number."""

    def test_ethics_gate_blocks(self):
        a = ActionEvaluation("bad", benefit=100, harm=1, risk=0, ethics_gate=False)
        self.assertIsNone(a.utility())

    def test_ethics_gate_permits(self):
        a = ActionEvaluation("good", benefit=10, harm=1, risk=0.5,
                             ethics_gate=True, safety_gate=True, system_gate=True)
        self.assertAlmostEqual(a.utility(), 8.5, places=9)


class MasterEquationPsi(unittest.TestCase):
    """§23/§27 — Ψ = S × I × K × MC × A_g × F."""

    def test_master_equation_psi_bounded(self):
        _, psi = MasterEquation.compute(
            RationalityInputs(), LogicInputs(), EthicsInputs(), Perception(),
            agency_ratio=0.85, meta_ratio=0.80, knowledge_density=0.7, feedback=0.9
        )
        self.assertGreater(psi.psi.value, 0.0)
        self.assertLessEqual(psi.psi.value, 1.0)

    def test_master_equation_psi_zero_when_sentience_zero(self):
        _, psi = MasterEquation.compute(
            RationalityInputs(), LogicInputs(), EthicsInputs(),
            Perception(audio=0.0),  # kills D → kills S
            agency_ratio=0.9, meta_ratio=0.9, knowledge_density=0.9, feedback=0.9
        )
        self.assertEqual(psi.psi.value, 0.0)


class Growth(unittest.TestCase):
    """§26 — logistic saturation against an explicit ceiling."""

    def test_growth_saturates(self):
        gm = GrowthModel(capability=1.0, c_max=10.0, alpha=0.5)
        for _ in range(200):
            gm.step(1.0, 1.0, 1.0)
        self.assertLessEqual(gm.capability, gm.c_max)
        self.assertGreater(gm.utilization(), 0.99)

    def test_growth_zero_inputs_no_growth(self):
        gm = GrowthModel(capability=2.0, c_max=10.0, alpha=0.5)
        before = gm.capability
        gm.step(0.0, 0.0, 0.0)
        self.assertAlmostEqual(gm.capability, before, places=9)


class Loop(unittest.TestCase):
    """§24 — the recursive Ω_{t+1} step."""

    def test_loop_timestep_advances(self):
        state = CognitiveState.initial()
        loop = CognitiveLoop(state)
        for i in range(1, 6):
            obs = Observation(value=0.7, confidence=Bounded(0.85), timestep=i)
            out = loop.step(obs, predicted=0.5, feedback=0.9)
            self.assertEqual(out.timestep, i)

    def test_loop_learning_reduces_error_over_constant_signal(self):
        """When the true signal is constant and predicted tracks it, error should shrink."""
        state = CognitiveState.initial()
        loop = CognitiveLoop(state, learning_rate=0.5)
        predicted = 0.5
        errors = []
        for i in range(1, 10):
            obs = Observation(value=0.8, confidence=Bounded(0.9), timestep=i)
            out = loop.step(obs, predicted, feedback=0.9)
            errors.append(out.error)
            predicted = 0.8 - out.learning_delta  # crude tracker
        # Error at step 1 should be larger than error near the end
        self.assertGreaterEqual(errors[0], errors[-1])

    def test_loop_reassessment_triggered_on_repeated_large_error(self):
        state = CognitiveState.initial()
        loop = CognitiveLoop(state, reassessment_threshold=2)
        for i in range(1, 6):
            obs = Observation(value=1.0, confidence=Bounded(0.9), timestep=i)
            out = loop.step(obs, predicted=0.0, feedback=0.9)  # error=1.0 every step
        self.assertTrue(out.reassessment_triggered)


class BoundedHelper(unittest.TestCase):
    def test_bounded_clamps(self):
        self.assertEqual(Bounded(1.5).value, 1.0)
        self.assertEqual(Bounded(-0.3).value, 0.0)
        self.assertAlmostEqual(Bounded(0.7).value, 0.7, places=12)


class Cli(unittest.TestCase):
    """The CLI prints Ψ on every line, which cp1252 cannot encode."""

    def _run_on_legacy_codepage(self, argv):
        from sentience import cli

        buf = io.BytesIO()
        stream = io.TextIOWrapper(buf, encoding="cp1252", newline="")
        real_out, real_err = sys.stdout, sys.stderr
        sys.stdout = sys.stderr = stream
        try:
            cli.main(argv)
            stream.flush()
        finally:
            sys.stdout, sys.stderr = real_out, real_err
        return buf.getvalue()

    def test_compute_survives_a_cp1252_console(self):
        out = self._run_on_legacy_codepage(["compute"])
        self.assertIn(b"Sentience Index", out)

    def test_demo_survives_a_cp1252_console(self):
        out = self._run_on_legacy_codepage(["demo", "--steps", "3"])
        self.assertTrue(out)


if __name__ == "__main__":
    unittest.main()
