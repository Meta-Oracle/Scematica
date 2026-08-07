"""Tests for the sentience package — zero external deps beyond pytest."""
import math
import sys
import os

# Ensure the src directory is on the path when run directly
sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "src"))

from sentience import (
    Bounded, Perception, RationalityInputs, LogicInputs, EthicsInputs,
    SentienceIndex, MasterEquation, GrowthModel, CognitiveLoop, CognitiveState,
)
from sentience.types import Observation
from sentience.data_integrity import DataIntegrityInputs
from sentience.ethics import ActionEvaluation


# ── §1: Sentience multiplicative structure ───────────────────────────────────

def test_sentience_zero_when_perception_zero():
    """S → 0 when any perception channel is 0."""
    d = Perception(audio=0.0, visual=1.0, sensory=1.0, integrity=1.0)
    s = SentienceIndex.compute(RationalityInputs(), LogicInputs(), EthicsInputs(), d)
    assert s.value.value == 0.0

def test_sentience_zero_when_ethics_zero():
    m = EthicsInputs(harm=0.0)
    s = SentienceIndex.compute(RationalityInputs(), LogicInputs(), m, Perception())
    assert s.value.value == 0.0

def test_sentience_bounded():
    s = SentienceIndex.compute(RationalityInputs(), LogicInputs(), EthicsInputs(), Perception())
    assert 0.0 < s.value.value <= 1.0

def test_sentience_bottleneck_identified():
    # Make logic the obvious weak link
    l = LogicInputs(validity=0.1, consistency=0.1, causal=0.1, formal=0.1)
    s = SentienceIndex.compute(RationalityInputs(), l, EthicsInputs(), Perception())
    assert s.bottleneck() == "logic"


# ── §2: Data integrity ───────────────────────────────────────────────────────

def test_data_integrity_perfect():
    di = DataIntegrityInputs(1.0, 1.0, 1.0, 1.0)
    assert abs(di.integrity().value - 1.0) < 1e-9

def test_data_integrity_zero_propagates():
    di = DataIntegrityInputs(0.0, 1.0, 1.0, 1.0)
    assert di.integrity().value == 0.0

def test_data_integrity_geometric_mean():
    di = DataIntegrityInputs(0.5, 0.5, 0.5, 0.5)
    expected = 0.5  # (0.5^4)^0.25 = 0.5
    assert abs(di.integrity().value - expected) < 1e-9


# ── §3: Rationality ──────────────────────────────────────────────────────────

def test_rationality_bias_penalty():
    low  = RationalityInputs(bias=0.01)
    high = RationalityInputs(bias=0.9)
    assert low.rationality().value > high.rationality().value

def test_rationality_bounded():
    r = RationalityInputs()
    assert 0.0 <= r.rationality().value <= 1.0


# ── §5: Ethics hard gate ─────────────────────────────────────────────────────

def test_ethics_gate_blocks():
    a = ActionEvaluation("bad", benefit=100, harm=1, risk=0, ethics_gate=False)
    assert a.utility() is None

def test_ethics_gate_permits():
    a = ActionEvaluation("good", benefit=10, harm=1, risk=0.5,
                         ethics_gate=True, safety_gate=True, system_gate=True)
    assert abs(a.utility() - 8.5) < 1e-9


# ── §23/§27: Master equation ─────────────────────────────────────────────────

def test_master_equation_psi_bounded():
    _, psi = MasterEquation.compute(
        RationalityInputs(), LogicInputs(), EthicsInputs(), Perception(),
        agency_ratio=0.85, meta_ratio=0.80, knowledge_density=0.7, feedback=0.9
    )
    assert 0.0 < psi.psi.value <= 1.0

def test_master_equation_psi_zero_when_sentience_zero():
    _, psi = MasterEquation.compute(
        RationalityInputs(), LogicInputs(), EthicsInputs(),
        Perception(audio=0.0),  # kills D → kills S
        agency_ratio=0.9, meta_ratio=0.9, knowledge_density=0.9, feedback=0.9
    )
    assert psi.psi.value == 0.0


# ── §26: Growth model ────────────────────────────────────────────────────────

def test_growth_saturates():
    gm = GrowthModel(capability=1.0, c_max=10.0, alpha=0.5)
    for _ in range(200):
        gm.step(1.0, 1.0, 1.0)
    assert gm.capability <= gm.c_max
    assert gm.utilization() > 0.99

def test_growth_zero_inputs_no_growth():
    gm = GrowthModel(capability=2.0, c_max=10.0, alpha=0.5)
    before = gm.capability
    gm.step(0.0, 0.0, 0.0)
    assert abs(gm.capability - before) < 1e-9


# ── §24: Cognitive loop ──────────────────────────────────────────────────────

def test_loop_timestep_advances():
    state = CognitiveState.initial()
    loop = CognitiveLoop(state)
    for i in range(1, 6):
        obs = Observation(value=0.7, confidence=Bounded(0.85), timestep=i)
        out = loop.step(obs, predicted=0.5, feedback=0.9)
        assert out.timestep == i

def test_loop_learning_reduces_error_over_constant_signal():
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
    assert errors[0] >= errors[-1]

def test_loop_reassessment_triggered_on_repeated_large_error():
    state = CognitiveState.initial()
    loop = CognitiveLoop(state, reassessment_threshold=2)
    for i in range(1, 6):
        obs = Observation(value=1.0, confidence=Bounded(0.9), timestep=i)
        out = loop.step(obs, predicted=0.0, feedback=0.9)  # error=1.0 every step
    assert out.reassessment_triggered


# ── Bounded helper ───────────────────────────────────────────────────────────

def test_bounded_clamps():
    assert Bounded(1.5).value == 1.0
    assert Bounded(-0.3).value == 0.0
    assert abs(Bounded(0.7).value - 0.7) < 1e-12
