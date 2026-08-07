"""§23/§27  Ψ_t = S × I × K × MC × A_g × F"""
from __future__ import annotations
from dataclasses import dataclass
from .types import Bounded
from .sentience import SentienceIndex
from .rationality import RationalityInputs
from .logic import LogicInputs
from .ethics import EthicsInputs
from .perception import Perception


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
