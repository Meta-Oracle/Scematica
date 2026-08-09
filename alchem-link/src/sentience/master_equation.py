"""§23/§27  Ψ_t = S × I × K × MC × A_g × F"""
from __future__ import annotations
from dataclasses import dataclass
from .types import Bounded
from .sentience import SentienceIndex
from .rationality import RationalityInputs
from .logic import LogicInputs
from .ethics import EthicsInputs
from .perception import Perception

# Ψ needs an agency ratio A_g (§12) and a meta-cognition ratio MC (§14). This subset
# port has no modules for either, so the values are mirrored from the Rust
# implementation's defaults — and they are *products* of their factors, not
# standalone scalars:
#     A_g = P × M_o × E_v × D_c × F_b = 0.9 × 0.85 × 0.85 × 0.9 × 0.85
#     MC  = R_c × E_c × U_c × S_c     = 0.8 × 0.75 × 0.85 × 0.9
# Held as scalars near 0.85 — which they were, separately, in three call sites — they
# inflated Ψ by ~3x against Rust, so the same state gated differently depending on
# which language was asked, and the overlay's own pre-call gate disagreed with the
# Ψ its post-call readout reported. Defined once here; change only alongside
# `agency.rs` / `meta_cognition.rs`.
DEFAULT_AGENCY_RATIO = 0.9 * 0.85 * 0.85 * 0.9 * 0.85   # ≈ 0.497441
DEFAULT_META_RATIO = 0.8 * 0.75 * 0.85 * 0.9            # ≈ 0.459


@dataclass
class IntegratedCognition:
    psi: Bounded
    sentience: Bounded
    information: Bounded
    knowledge: Bounded
    meta_cognition: Bounded
    agency: Bounded
    feedback: Bounded

    @classmethod
    def compute(cls, s: Bounded, i: Bounded, k: Bounded,
                mc: Bounded, ag: Bounded, f: Bounded) -> "IntegratedCognition":
        psi = Bounded(s.value * i.value * k.value * mc.value * ag.value * f.value)
        return cls(psi=psi, sentience=s, information=i, knowledge=k,
                   meta_cognition=mc, agency=ag, feedback=f)


class MasterEquation:
    @staticmethod
    def compute(
        rationality: RationalityInputs,
        logic: LogicInputs,
        ethics: EthicsInputs,
        perception: Perception,
        agency_ratio: float,
        meta_ratio: float,
        knowledge_density: float,
        feedback: float,
    ) -> tuple[SentienceIndex, IntegratedCognition]:
        sentience = SentienceIndex.compute(rationality, logic, ethics, perception)
        information = perception.data_ratio()
        psi = IntegratedCognition.compute(
            sentience.value,
            information,
            Bounded(knowledge_density),
            Bounded(meta_ratio),
            Bounded(agency_ratio),
            Bounded(feedback),
        )
        return sentience, psi
