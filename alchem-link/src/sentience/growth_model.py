"""§26 Logistic growth — saturates at C_max."""
import math
from dataclasses import dataclass

@dataclass
class GrowthModel:
    capability: float
    c_max: float
    alpha: float

    def step(self, l_t: float, i_t: float, f_t: float) -> float:
        g = self.alpha * max(0.0,min(l_t,1.0)) * max(0.0,min(i_t,1.0)) * max(0.0,min(f_t,1.0))
        ratio = (self.c_max - self.capability) / max(self.capability, 1e-12)
        self.capability = self.c_max / (1.0 + ratio * math.exp(-g))
        return self.capability

    def utilization(self) -> float:
        return self.capability / self.c_max
