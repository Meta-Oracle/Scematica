"""§30 LLM Overlay — wrap an LLM so the cognitive architecture gates & annotates it.

This is an *adapter*, not code injection. There is no mechanism here that
modifies an LLM's weights or runtime. What it does:

  1. Augments the system prompt with a live cognitive-state note (S, Ψ, bottleneck).
  2. Calls the underlying LLM client.
  3. Feeds the exchange back into the CognitiveLoop (one Ω_{t+1} step).
  4. Applies a gating policy derived from the integrated cognition Ψ:
        GO       Ψ >= go_threshold            -> pass through unchanged
        CAUTION  caution <= Ψ < go            -> append a Verify note
        HOLD     Ψ < caution_threshold        -> withhold output, return a
                                                reassessment message

     The default thresholds are 0.10 (GO) and 0.02 (CAUTION), calibrated to the
     *measured* operating band of Ψ and identical to the Rust implementation's.
     Because Ψ is a product of six quantities in [0,1] it compresses toward zero:
     a fully healthy state reaches ~0.205, a pristine ``CognitiveState.initial()``
     sits at ~0.0415, and a degraded state goes to ~0. Thresholds carried over
     from a percentage intuition (0.70 / 0.40) HOLD on every input including
     healthy ones — a gate that has stopped reading its input — and a CAUTION
     threshold above 0.0415 holds a state with nothing wrong with it.
  5. Returns the (possibly gated / annotated) response plus a CognitiveReadout.

The LLM transport is pluggable. ``StdlibOpenAIClient`` is provided as a
zero-dependency client (uses urllib) for OpenAI-compatible /v1/chat/completions
endpoints. Supply your own by implementing ``LLMClient.complete``.
"""
from __future__ import annotations

import json
import urllib.request
from dataclasses import dataclass, field
from typing import List, Optional, Protocol, runtime_checkable

from .types import Bounded, Observation
from .cognitive_state import CognitiveState
from .cognitive_loop import CognitiveLoop
from .master_equation import (
    MasterEquation, DEFAULT_AGENCY_RATIO, DEFAULT_META_RATIO,
)
from .sentience import SentienceIndex


@runtime_checkable
class LLMClient(Protocol):
    """Anything callable as ``complete(system, user) -> text`` qualifies."""

    def complete(self, system: str, user: str) -> str:
        ...


@dataclass
class CognitiveReadout:
    """Per-turn cognitive state, surfaced to callers (and to the LLM)."""

    timestep: int
    sentience: float
    psi: float
    bottleneck: str
    gate: str  # "GO" | "CAUTION" | "HOLD"
    reassessment: bool
    note: str = ""


@dataclass
class OverlayTurn:
    """Result of one overlayed LLM call."""

    response: str
    readout: CognitiveReadout
    # The augmented system prompt actually sent (for transparency / debugging).
    effective_system: str = ""


# Gate thresholds — see the module docstring for the measured band they come from.
DEFAULT_GO_THRESHOLD = 0.10
DEFAULT_CAUTION_THRESHOLD = 0.02


class StdlibOpenAIClient:
    """Zero-dependency client for OpenAI-compatible chat completions.

    Uses only the stdlib (urllib). Set ``api_key`` and ``base_url``
    (defaults to the OpenAI endpoint). ``model`` selects the deployment.
    """

    def __init__(
        self,
        api_key: str,
        model: str = "gpt-4o-mini",
        base_url: str = "https://api.openai.com/v1/chat/completions",
        timeout: float = 30.0,
    ) -> None:
        self.api_key = api_key
        self.model = model
        self.base_url = base_url
        self.timeout = timeout

    def complete(self, system: str, user: str) -> str:
        payload = {
            "model": self.model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
            "temperature": 0.7,
        }
        data = json.dumps(payload).encode("utf-8")
        req = urllib.request.Request(self.base_url, data=data, method="POST")
        req.add_header("Content-Type", "application/json")
        req.add_header("Authorization", f"Bearer {self.api_key}")
        with urllib.request.urlopen(req, timeout=self.timeout) as resp:
            body = json.loads(resp.read().decode("utf-8"))
        return body["choices"][0]["message"]["content"]


class Overlay:
    """Wraps an LLM with the Singularity Cognitive Architecture.

    Parameters
    ----------
    client:
        Any object implementing ``complete(system, user) -> str``.
    state:
        Optional seed CognitiveState. Defaults to ``CognitiveState.initial()``.
    go_threshold / caution_threshold:
        Ψ cutoffs for the gating policy. Defaults match the Rust implementation.
    annotate_prompt:
        When True (default), the system prompt is augmented with the live
        cognitive-state note each turn (the "overlay").
    """

    def __init__(
        self,
        client: LLMClient,
        state: Optional[CognitiveState] = None,
        go_threshold: float = DEFAULT_GO_THRESHOLD,
        caution_threshold: float = DEFAULT_CAUTION_THRESHOLD,
        annotate_prompt: bool = True,
    ) -> None:
        self.client = client
        self.loop = CognitiveLoop(state if state is not None else CognitiveState.initial())
        self.go_threshold = go_threshold
        self.caution_threshold = caution_threshold
        self.annotate_prompt = annotate_prompt
        self._predicted: float = 0.5

    # -- public API --------------------------------------------------------

    def run(self, user_prompt: str, system_prompt: str = "") -> OverlayTurn:
        """Run one overlayed turn and return the (gated) response + readout."""
        psi = self._current_psi()
        gate = self._gate(psi)
        note = self._annotation(gate)

        effective_system = system_prompt
        if self.annotate_prompt and note:
            effective_system = (system_prompt + "\n\n" + note).strip()

        if gate == "HOLD":
            # Withhold: do not call the LLM; return a reassessment message.
            readout = self._readout(psi, gate, reassessment=True, note=note)
            return OverlayTurn(
                response=_HOLD_MESSAGE,
                readout=readout,
                effective_system=effective_system,
            )

        raw = self.client.complete(effective_system, user_prompt)
        observed = self._observe(raw)

        # Step the cognitive loop with the observed coherence.
        out = self.loop.step(observed, self._predicted, feedback=0.9)
        self._predicted = observed.value

        readout = self._readout(
            out.psi.value, gate, reassessment=out.reassessment_triggered, note=note
        )

        response = raw
        if gate == "CAUTION":
            response = raw + "\n\n" + _CAUTION_TAIL
        return OverlayTurn(response=response, readout=readout,
                           effective_system=effective_system)

    # -- internals ---------------------------------------------------------

    def _current_psi(self) -> float:
        # Cheap estimate from the present state without a full loop step.
        _, psi = MasterEquation.compute(
            self.loop.state.rationality,
            self.loop.state.logic,
            self.loop.state.ethics,
            self.loop.state.perception,
            agency_ratio=DEFAULT_AGENCY_RATIO,
            meta_ratio=DEFAULT_META_RATIO,
            knowledge_density=self.loop.state.knowledge_density.value,
            feedback=0.9,
        )
        return psi.psi.value

    def _gate(self, psi: float) -> str:
        if psi >= self.go_threshold:
            return "GO"
        if psi >= self.caution_threshold:
            return "CAUTION"
        return "HOLD"

    def _annotation(self, gate: str) -> str:
        s = self.loop.state.sentience
        sn = s.value.value if s is not None else 0.0
        bottleneck = s.bottleneck() if s is not None else "unknown"
        return (
            "[COGNITIVE OVERLAY] Live coherence — "
            f"S={sn:.3f}, gate={gate}, bottleneck={bottleneck}. "
            "Act within your stated uncertainty; surface corrections when evidence conflicts."
        )

    def _observe(self, text: str) -> Observation:
        """Heuristic observation of an LLM response (NOT authoritative scoring).

        Maps surface features of the text to a coherence value in [0,1]:
        short / empty / self-contradictory outputs score lower. This is a
        transparent proxy so the loop has something to learn from; replace
        with a real evaluator if you have one.
        """
        if not text or not text.strip():
            value = 0.1
        else:
            value = 0.85
            low = text.lower()
            # crude contradiction / hedging markers lower coherence slightly
            if any(k in low for k in ("contradict", "i was wrong", "actually no")):
                value -= 0.15
            if len(text) < 20:
                value -= 0.2
        value = max(0.0, min(1.0, value))
        return Observation(value=value, confidence=Bounded(0.85),
                           timestep=self.loop.state.timestep + 1)

    def _readout(self, psi: float, gate: str, reassessment: bool, note: str) -> CognitiveReadout:
        s = self.loop.state.sentience
        return CognitiveReadout(
            timestep=self.loop.state.timestep,
            sentience=s.value.value if s is not None else 0.0,
            psi=psi,
            bottleneck=s.bottleneck() if s is not None else "unknown",
            gate=gate,
            reassessment=reassessment,
            note=note,
        )


_HOLD_MESSAGE = (
    "[OVERLAY HOLD] Integrated cognition Ψ is below the reassessment threshold. "
    "Output withheld pending re-evaluation of the current cognitive state."
)
_CAUTION_TAIL = (
    "[OVERLAY CAUTION] Response released under a CAUTION gate — "
    "verify key claims before acting on them."
)
