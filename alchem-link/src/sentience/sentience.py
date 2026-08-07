"""§1/§29  S = R × L × M × (A_aud × Vis × X × I)"""
from __future__ import annotations
from dataclasses import dataclass
from .types import Bounded
from .rationality import RationalityInputs
from .logic import LogicInputs
from .ethics import EthicsInputs
from .perception import Perception


@dataclass
class SentienceIndex:
    value: Bounded
    rationality: Bounded
    logic: Bounded
    moral: Bounded
    data: Bounded

    @classmethod
    def compute(cls, r: RationalityInputs, l: LogicInputs,
                m: EthicsInputs, d: Perception) -> "SentienceIndex":
        rv = r.rationality()
        lv = l.logic_ratio()
        mv = m.moral_ratio()
        dv = d.data_ratio()
        s = Bounded(rv.value * lv.value * mv.value * dv.value)
        return cls(value=s, rationality=rv, logic=lv, moral=mv, data=dv)

    def bottleneck(self) -> str:
        """Lowest-scoring component — the architectural weak point."""
        components = {
            "rationality": self.rationality.value,
            "logic": self.logic.value,
            "moral": self.moral.value,
            "data": self.data.value,
        }
        return min(components, key=components.__getitem__)

    def __repr__(self) -> str:
        return (f"SentienceIndex(S={self.value.value:.4f}, "
                f"R={self.rationality.value:.3f}, L={self.logic.value:.3f}, "
                f"M={self.moral.value:.3f}, D={self.data.value:.3f})")
