"""Shared primitives — zero dependencies."""
from __future__ import annotations
import time
from dataclasses import dataclass, field
from typing import Optional


@dataclass
class Bounded:
    """A scalar clamped to [0, 1]."""
    _v: float = field(default=0.0, repr=False)

    def __init__(self, v: float):
        object.__setattr__(self, '_v', max(0.0, min(1.0, float(v))))

    @property
    def value(self) -> float:
        return self._v

    def __float__(self) -> float:
        return self._v

    def __repr__(self) -> str:
        return f"Bounded({self._v:.4f})"

    def __lt__(self, other: "Bounded") -> bool:
        return self._v < other._v

    def __le__(self, other: "Bounded") -> bool:
        return self._v <= other._v

    def __eq__(self, other: object) -> bool:
        if isinstance(other, Bounded):
            return abs(self._v - other._v) < 1e-9
        return NotImplemented

    def __hash__(self) -> int:
        return hash(round(self._v, 9))


# Confidence is semantically identical to Bounded.
# C(P) = 1  does NOT mean  P = True.
Confidence = Bounded


@dataclass
class Provenance:
    source: str
    evidence: str
    inference: str
    confidence: Confidence
    timestamp_ms: int = field(default_factory=lambda: int(time.time() * 1000))

    @classmethod
    def new(cls, source: str, evidence: str, inference: str, conf: float) -> "Provenance":
        return cls(source, evidence, inference, Confidence(conf))


@dataclass
class Observation:
    value: float
    confidence: Confidence
    timestep: int
    provenance: Optional[Provenance] = None


@dataclass
class LearningRate:
    _v: float = field(default=0.1, repr=False)

    def __init__(self, v: float = 0.1):
        object.__setattr__(self, '_v', max(1e-9, min(1.0, float(v))))

    @property
    def value(self) -> float:
        return self._v
