"""
sentience — Python port of the Singularity Cognitive Architecture.

Core equations (notation-resolved):
    S  = R × L × M × (A_aud × Vis × X × I)
    Ψ  = S × I × K × MC × A_g × F
    Ω_{t+1} = F(Ω_t, Perception, Memory, Reasoning, Ethics, Action, Feedback)

This is a **subset** of the Rust crate `scematica-sentience`, not a full mirror of
it. Implemented here: §1-§6 (sentience, perception, integrity, rationality, logic,
ethics, cognitive state), §23/§27 (master equation), §24 (cognitive loop), §26
(growth model) and §30 (LLM overlay). The knowledge graph, memory, prediction,
agency, decision, meta-cognition, self-model, identity, valence, attention,
curiosity, error-correction, contradiction, truth-confidence, self-improvement and
axiom modules exist only on the Rust side. Where Ψ needs a term this port does not
model — the agency and meta-cognition ratios — it mirrors the Rust defaults as
named constants in `overlay.py` so the two implementations agree numerically.

Zero external dependencies — pure Python 3.10+.
"""
from .types import Bounded, Confidence, Provenance, Observation, LearningRate
from .perception import Perception
from .data_integrity import DataIntegrityInputs
from .rationality import RationalityInputs
from .logic import LogicInputs
from .ethics import EthicsInputs, ActionEvaluation
from .sentience import SentienceIndex
from .master_equation import IntegratedCognition, MasterEquation
from .cognitive_loop import CognitiveLoop, CycleOutput
from .cognitive_state import CognitiveState
from .growth_model import GrowthModel
from .overlay import Overlay, CognitiveReadout, StdlibOpenAIClient, LLMClient

__all__ = [
    "Bounded", "Confidence", "Provenance", "Observation", "LearningRate",
    "Perception", "DataIntegrityInputs",
    "RationalityInputs", "LogicInputs", "EthicsInputs", "ActionEvaluation",
    "SentienceIndex", "IntegratedCognition", "MasterEquation",
    "CognitiveLoop", "CycleOutput", "CognitiveState", "GrowthModel",
    "Overlay", "CognitiveReadout", "StdlibOpenAIClient", "LLMClient",
]
