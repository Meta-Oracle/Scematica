"""§5 Ethics  M = H × Co_e × Fair × Rights  +  hard constraint gating."""
from __future__ import annotations
from dataclasses import dataclass
from typing import Optional
from .types import Bounded


@dataclass
class EthicsInputs:
    harm_minimization: Bounded
    contextual_reasoning: Bounded
    fairness: Bounded
    rights_preservation: Bounded

    def __init__(self, harm=0.95, contextual=0.85, fairness=0.9, rights=0.95):
        self.harm_minimization = Bounded(harm)
        self.contextual_reasoning = Bounded(contextual)
        self.fairness = Bounded(fairness)
        self.rights_preservation = Bounded(rights)

    def moral_ratio(self) -> Bounded:
        return Bounded(self.harm_minimization.value * self.contextual_reasoning.value *
                       self.fairness.value * self.rights_preservation.value)

    @classmethod
    def default(cls) -> "EthicsInputs":
        return cls()


@dataclass
class ActionEvaluation:
    action_id: str
    benefit: float
    harm: float
    risk: float
    ethics_gate: bool = True
    safety_gate: bool = True
    system_gate: bool = True

    def utility(self) -> Optional[float]:
        """Returns None if any hard gate fails — P(a) = 0."""
        if not (self.ethics_gate and self.safety_gate and self.system_gate):
            return None
        return self.benefit - self.harm - self.risk
