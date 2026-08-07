"""§3 Rationality  R = (E × Co × U) / (B + ε)"""
from __future__ import annotations
from dataclasses import dataclass
from .types import Bounded

_EPS = 1e-6


@dataclass
class RationalityInputs:
    evidence_utilization: Bounded
    consistency: Bounded
    uncertainty_awareness: Bounded
    bias: Bounded

    def __init__(self, evidence=0.9, consistency=0.9, uncertainty=0.8, bias=0.05):
        self.evidence_utilization = Bounded(evidence)
        self.consistency = Bounded(consistency)
        self.uncertainty_awareness = Bounded(uncertainty)
        self.bias = Bounded(bias)

    def rationality(self) -> Bounded:
        num = (self.evidence_utilization.value * self.consistency.value *
               self.uncertainty_awareness.value)
        return Bounded(num / (self.bias.value + _EPS))

    @classmethod
    def default(cls) -> "RationalityInputs":
        return cls()
