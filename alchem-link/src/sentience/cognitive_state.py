"""§6 Cognitive state Ω_t."""
from __future__ import annotations
from dataclasses import dataclass, field
from typing import Optional, List
from .types import Bounded, Observation
from .sentience import SentienceIndex
from .rationality import RationalityInputs
from .logic import LogicInputs
from .ethics import EthicsInputs
from .perception import Perception


@dataclass
class CognitiveState:
    timestep: int = 0
    sentience: Optional[SentienceIndex] = None
    rationality: RationalityInputs = field(default_factory=RationalityInputs.default)
    logic: LogicInputs = field(default_factory=LogicInputs.default)
    ethics: EthicsInputs = field(default_factory=EthicsInputs.default)
    perception: Perception = field(default_factory=Perception.default)
    knowledge_density: Bounded = field(default_factory=lambda: Bounded(0.5))
    uncertainty: Bounded = field(default_factory=lambda: Bounded(0.5))
    last_observation: Optional[Observation] = None

    @classmethod
    def initial(cls) -> "CognitiveState":
        s = cls()
        s.sentience = SentienceIndex.compute(s.rationality, s.logic, s.ethics, s.perception)
        return s

    def tick(self) -> None:
        self.timestep += 1
