"""§2 Information Integrity  I = f(C, T, S_rel, R_cor) — geometric mean."""
from __future__ import annotations
from dataclasses import dataclass
from .types import Bounded


@dataclass
class DataIntegrityInputs:
    completeness: Bounded
    temporal_relevance: Bounded
    source_reliability: Bounded
    corroboration: Bounded

    def __init__(self, completeness=1.0, temporal_relevance=1.0,
                 source_reliability=1.0, corroboration=1.0):
        self.completeness = Bounded(completeness)
        self.temporal_relevance = Bounded(temporal_relevance)
        self.source_reliability = Bounded(source_reliability)
        self.corroboration = Bounded(corroboration)

    def integrity(self) -> Bounded:
        p = (self.completeness.value * self.temporal_relevance.value *
             self.source_reliability.value * self.corroboration.value)
        if p <= 0.0:
            return Bounded(0.0)
        return Bounded(p ** 0.25)  # geometric mean of four factors
