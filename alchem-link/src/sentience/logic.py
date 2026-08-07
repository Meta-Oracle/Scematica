"""§4 Logic  L = Val × Co × Q × Fq  (notation-resolved)"""
from __future__ import annotations
from dataclasses import dataclass
from .types import Bounded


@dataclass
class LogicInputs:
    validity: Bounded       # Val
    consistency: Bounded    # Co  (not C — avoids clash with Completeness)
    causal_coherence: Bounded  # Q
    formal_quality: Bounded    # Fq (not F — avoids clash with Feedback)

    def __init__(self, validity=0.9, consistency=0.9, causal=0.85, formal=0.85):
        self.validity = Bounded(validity)
        self.consistency = Bounded(consistency)
        self.causal_coherence = Bounded(causal)
        self.formal_quality = Bounded(formal)

    def logic_ratio(self) -> Bounded:
        return Bounded(self.validity.value * self.consistency.value *
                       self.causal_coherence.value * self.formal_quality.value)

    @classmethod
    def default(cls) -> "LogicInputs":
        return cls()
