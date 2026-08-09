"""§24 Recursive cognitive loop — one full Ω_{t+1} = F(Ω_t,...) step."""
from __future__ import annotations
from dataclasses import dataclass
from .types import Bounded, Observation
from .cognitive_state import CognitiveState
from .sentience import SentienceIndex
from .master_equation import (
    MasterEquation, DEFAULT_AGENCY_RATIO, DEFAULT_META_RATIO,
)


@dataclass
class CycleOutput:
    timestep: int
    sentience: SentienceIndex
    psi: Bounded
    learning_delta: float
    error: float
    reassessment_triggered: bool


class CognitiveLoop:
    def __init__(self, state: CognitiveState, learning_rate: float = 0.1,
                 reassessment_threshold: int = 3):
        self.state = state
        self.alpha = learning_rate
        self.knowledge = state.knowledge_density.value
        self._streak = 0
        self._thresh = reassessment_threshold

    def step(self, observation: Observation, predicted: float,
             feedback: float = 1.0) -> CycleOutput:
        # Learning
        err = observation.value - predicted
        delta = self.alpha * observation.confidence.value * err
        self.knowledge = max(0.0, min(1.0, self.knowledge + delta))

        # Error streak → reassessment
        if abs(err) > 0.2:
            self._streak += 1
        else:
            self._streak = 0
        reassess = self._streak >= self._thresh

        # Master equation
        sentience, psi = MasterEquation.compute(
            self.state.rationality, self.state.logic,
            self.state.ethics, self.state.perception,
            agency_ratio=DEFAULT_AGENCY_RATIO, meta_ratio=DEFAULT_META_RATIO,
            knowledge_density=self.knowledge,
            feedback=max(0.0, min(1.0, feedback)),
        )

        self.state.sentience = sentience
        self.state.knowledge_density = Bounded(self.knowledge)
        self.state.last_observation = observation
        self.state.tick()

        return CycleOutput(
            timestep=self.state.timestep,
            sentience=sentience,
            psi=psi.psi,
            learning_delta=delta,
            error=abs(err),
            reassessment_triggered=reassess,
        )
