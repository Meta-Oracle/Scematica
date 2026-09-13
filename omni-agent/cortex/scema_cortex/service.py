"""CortexService -- the single object that owns the agent's judgement.

Wires embedder + TasteNet + memory + trainer into one coherent unit, so the
HTTP layer stays a thin translation of JSON to method calls and the whole
thing remains usable as a library (tests drive it directly, no server needed).

Ordering matters at construction: the embedder is built first because its
output dimension determines the network width, and a checkpoint trained at a
different width is refused rather than reshaped.
"""
from __future__ import annotations

import logging
import threading
import time
from typing import Any, Dict, List, Optional, Sequence

import numpy as np
import torch

from .config import CONFIG, CortexConfig
from .embed import Embedder, build_embedder
from .model import FEATURE_NAMES, TasteNet, build_model, count_params, features_to_vector
from .store import MemoryStore
from .train import FeedbackEvent, Trainer

log = logging.getLogger("scema.service")

# How the three heads combine into the single number the sense loop ranks by.
# Deliberately explicit: this is a policy, and it should be visible and
# tunable rather than buried in a comparison.
DEFAULT_PRIORITY_WEIGHTS = {"salience": 0.5, "taste": 0.35, "resonance": 0.15}


class CortexService:
    def __init__(self, cfg: Optional[CortexConfig] = None) -> None:
        self.cfg = cfg or CONFIG
        self.started_at = time.time()
        self._lock = threading.RLock()

        self.embedder: Embedder = build_embedder(self.cfg)
        self.device = self.cfg.resolve_device()

        self.model: TasteNet = build_model(self.embedder.dim, self.cfg)
        self.trainer = Trainer(self.model, cfg=self.cfg, device=self.device)
        self.resumed = self.trainer.load()

        self.memory = MemoryStore.load(dim=self.embedder.dim, path=self.cfg.memory_path)
        self._dirty = False

        log.info(
            "cortex ready: embed=%s(dim=%d) device=%s params=%s memories=%d resumed=%s",
            self.embedder.name,
            self.embedder.dim,
            self.device,
            f"{count_params(self.model):,}",
            len(self.memory),
            self.resumed,
        )

    # ------------------------------------------------------------- embeddings

    def embed(self, texts: Sequence[str]) -> np.ndarray:
        if not texts:
            return np.zeros((0, self.embedder.dim), dtype=np.float32)
        return self.embedder.encode(list(texts))

    # ----------------------------------------------------------------- scoring

    def score(
        self,
        items: List[Dict[str, Any]],
        weights: Optional[Dict[str, float]] = None,
    ) -> List[Dict[str, Any]]:
        """Score candidates in one batch.

        Each item: {"id"?, "text", "features"?: {...}}. Novelty is computed
        here rather than trusted from the caller -- it is a property of what
        this agent already remembers, which only the cortex knows.
        """
        if not items:
            return []
        weights = {**DEFAULT_PRIORITY_WEIGHTS, **(weights or {})}

        texts = [str(item.get("text") or "") for item in items]
        embeddings = self.embed(texts)

        feature_rows = []
        novelties = []
        for item, emb in zip(items, embeddings):
            feats = dict(item.get("features") or {})
            novelty = self.memory.novelty(emb)
            novelties.append(novelty)
            # The caller cannot know this; overwrite whatever it guessed.
            feats["novelty"] = novelty
            feats.setdefault("text_len_norm", min(len(str(item.get("text") or "")) / 280.0, 2.0))
            feature_rows.append(features_to_vector(feats))

        emb_t = torch.from_numpy(embeddings).to(self.device)
        feat_t = torch.from_numpy(np.stack(feature_rows)).to(self.device)
        with self._lock:
            scores = self.model.score(emb_t, feat_t)

        salience = scores["salience"].cpu().numpy()
        taste = scores["taste"].cpu().numpy()
        resonance = scores["resonance"].cpu().numpy()

        out: List[Dict[str, Any]] = []
        for i, item in enumerate(items):
            # Squash resonance into 0..1 before mixing, so a viral outlier
            # cannot swamp the two probabilities it is averaged with.
            resonance_norm = float(np.tanh(resonance[i] / 100.0))
            priority = (
                weights["salience"] * float(salience[i])
                + weights["taste"] * float(taste[i])
                + weights["resonance"] * resonance_norm
            )
            out.append(
                {
                    "id": item.get("id"),
                    "salience": round(float(salience[i]), 5),
                    "taste": round(float(taste[i]), 5),
                    "resonance": round(float(resonance[i]), 3),
                    "novelty": round(float(novelties[i]), 5),
                    "priority": round(priority, 5),
                }
            )
        out.sort(key=lambda row: row["priority"], reverse=True)
        return out

    # ---------------------------------------------------------------- feedback

    def feedback(
        self,
        text: str,
        salience: Optional[float] = None,
        taste: Optional[float] = None,
        resonance: Optional[float] = None,
        features: Optional[Dict[str, float]] = None,
        source: str = "unknown",
        ref: Optional[str] = None,
        autotrain: bool = True,
    ) -> Dict[str, Any]:
        """Record a label and (usually) take a training step.

        This is the endpoint that closes every loop in the system: the
        Telegram cockpit calls it on approve/reject, and the reflection pass
        calls it once engagement numbers land.
        """
        emb = self.embed([text])[0]
        feats = dict(features or {})
        feats.setdefault("novelty", self.memory.novelty(emb))
        event = FeedbackEvent(
            embedding=emb,
            features=features_to_vector(feats),
            salience=salience,
            taste=taste,
            resonance=resonance,
            source=source,
            ref=ref,
        )
        with self._lock:
            result = self.trainer.record(event, autotrain=autotrain)
        self._dirty = True
        return result

    def train(self, steps: int = 4) -> Dict[str, Any]:
        with self._lock:
            result = self.trainer.train(steps)
        self._dirty = True
        return result

    # ------------------------------------------------------------------ memory

    def remember(
        self,
        text: str,
        surface: str = "system",
        kind: str = "observation",
        meta: Optional[Dict[str, Any]] = None,
    ) -> Dict[str, Any]:
        emb = self.embed([text])[0]
        novelty = self.memory.novelty(emb)
        record = self.memory.add(text, emb, surface=surface, kind=kind, meta=meta)
        self._dirty = True
        return {"id": record.id, "novelty": round(novelty, 5), "count": len(self.memory)}

    def recall(
        self,
        query: str,
        k: int = 6,
        surfaces: Optional[Sequence[str]] = None,
        kinds: Optional[Sequence[str]] = None,
        half_life_hours: float = 72.0,
    ) -> List[Dict[str, Any]]:
        emb = self.embed([query])[0]
        hits = self.memory.recall(
            emb, k=k, surfaces=surfaces, kinds=kinds, half_life_hours=half_life_hours
        )
        return [hit.as_dict() for hit in hits]

    # ----------------------------------------------------------------- lifecycle

    def save(self, force: bool = False) -> Dict[str, Any]:
        if not self._dirty and not force:
            return {"saved": False, "reason": "no changes since last save"}
        with self._lock:
            checkpoint = self.trainer.save()
            memory_path = self.memory.save()
            self._dirty = False
        return {
            "saved": True,
            "checkpoint": str(checkpoint) if checkpoint else None,
            "memory": str(memory_path) if memory_path else None,
        }

    def stats(self) -> Dict[str, Any]:
        from .kernels import active_kernel

        return {
            "uptime_seconds": round(time.time() - self.started_at, 1),
            "embedder": {"backend": self.embedder.name, "dim": self.embedder.dim},
            "model": {
                "params": count_params(self.model),
                "hidden_dim": self.cfg.hidden_dim,
                "blocks": self.cfg.n_blocks,
                "features": list(FEATURE_NAMES),
            },
            "kernel": active_kernel(),
            "device": self.device,
            "resumed_from_checkpoint": self.resumed,
            "training": self.trainer.stats(),
            "memory": self.memory.stats(),
        }


_SERVICE: Optional[CortexService] = None
_SERVICE_LOCK = threading.Lock()


def get_service(cfg: Optional[CortexConfig] = None) -> CortexService:
    """Process-wide singleton. Building the embedder is expensive; do it once."""
    global _SERVICE
    with _SERVICE_LOCK:
        if _SERVICE is None:
            _SERVICE = CortexService(cfg)
        return _SERVICE
