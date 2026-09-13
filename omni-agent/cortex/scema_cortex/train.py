"""Online training: the loop that turns operator decisions into weights.

The agent never has a training set. It has a trickle of labels arriving from
three places, at different times, about different things:

    approve/reject in Telegram   -> taste   (immediate, high signal, scarce)
    engagement metrics from X     -> resonance (delayed hours, noisy, plentiful)
    "was this worth reacting to"  -> salience (derived from both)

So training is a replay buffer plus small frequent steps, not epochs. Two
mechanisms keep that honest:

* **Replay.** Each step samples the buffer rather than training only on the
  newest event, which is what stops the net from swinging to whatever you
  clicked last. Recent events are oversampled, but never exclusively.
* **Class balance.** Approvals usually outnumber rejections badly. The BCE
  loss on the taste head is trained on a batch balanced by resampling, so the
  net cannot score well by predicting "you will approve" forever.
"""
from __future__ import annotations

import json
import logging
import random
import threading
import time
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Dict, List, Optional

import numpy as np
import torch

from .config import CONFIG, CortexConfig
from .model import HEADS, N_FEATURES, LossWeights, TasteNet, masked_losses

log = logging.getLogger("scema.train")


@dataclass
class FeedbackEvent:
    """One label-bearing thing that happened.

    Any head may be None: a fresh approve/reject has no engagement number yet.
    Masking in `masked_losses` is what lets those coexist in one buffer.
    """

    embedding: np.ndarray
    features: np.ndarray
    salience: Optional[float] = None
    taste: Optional[float] = None
    resonance: Optional[float] = None
    source: str = "unknown"
    ref: Optional[str] = None
    created_at: float = field(default_factory=time.time)

    def labelled_heads(self) -> List[str]:
        return [h for h in HEADS if getattr(self, h) is not None]

    def to_json(self) -> Dict[str, Any]:
        return {
            "salience": self.salience,
            "taste": self.taste,
            "resonance": self.resonance,
            "source": self.source,
            "ref": self.ref,
            "created_at": self.created_at,
            # Embeddings are recoverable from text, and storing them here would
            # multiply the log size by ~1500x. The audit trail keeps labels.
        }


class ReplayBuffer:
    def __init__(self, capacity: int) -> None:
        self.capacity = capacity
        self._items: List[FeedbackEvent] = []
        self._lock = threading.RLock()

    def __len__(self) -> int:
        return len(self._items)

    def add(self, event: FeedbackEvent) -> None:
        with self._lock:
            self._items.append(event)
            if len(self._items) > self.capacity:
                # Drop oldest: taste drifts, and year-old preferences should
                # not outvote this month's.
                del self._items[: len(self._items) - self.capacity]

    def sample(self, batch_size: int, recent_fraction: float = 0.35) -> List[FeedbackEvent]:
        """Mostly-uniform sample with a deliberate tilt toward recent events."""
        with self._lock:
            n = len(self._items)
            if n == 0:
                return []
            batch_size = min(batch_size, n)
            n_recent = min(int(batch_size * recent_fraction), n)
            recent_window = self._items[-max(n_recent * 4, 1) :]
            picked = random.sample(recent_window, min(n_recent, len(recent_window)))
            remaining = batch_size - len(picked)
            if remaining > 0:
                picked += random.choices(self._items, k=remaining)
            return picked

    def balanced_taste_sample(self, batch_size: int) -> List[FeedbackEvent]:
        """Resample the taste-labelled events to ~50/50 approve/reject.

        Without this the head learns the base rate and nothing else.
        """
        with self._lock:
            pos = [e for e in self._items if e.taste is not None and e.taste >= 0.5]
            neg = [e for e in self._items if e.taste is not None and e.taste < 0.5]
        if not pos or not neg:
            return []
        half = max(1, batch_size // 2)
        return random.choices(pos, k=half) + random.choices(neg, k=half)

    def label_counts(self) -> Dict[str, int]:
        with self._lock:
            counts = {h: 0 for h in HEADS}
            counts["taste_positive"] = 0
            counts["taste_negative"] = 0
            for event in self._items:
                for head in event.labelled_heads():
                    counts[head] += 1
                if event.taste is not None:
                    key = "taste_positive" if event.taste >= 0.5 else "taste_negative"
                    counts[key] += 1
            counts["total"] = len(self._items)
            return counts


class Trainer:
    """Owns the optimiser, the buffer, and the decision of when to step."""

    def __init__(
        self,
        model: TasteNet,
        cfg: Optional[CortexConfig] = None,
        device: Optional[str] = None,
    ) -> None:
        self.cfg = cfg or CONFIG
        self.device = device or self.cfg.resolve_device()
        self.model = model.to(self.device)
        self.opt = torch.optim.AdamW(
            self.model.parameters(), lr=self.cfg.lr, weight_decay=self.cfg.weight_decay
        )
        self.buffer = ReplayBuffer(self.cfg.replay_capacity)
        self.weights = LossWeights()
        self._lock = threading.RLock()
        self.events_since_step = 0
        self.total_steps = 0
        self.total_events = 0
        self.last_loss: Optional[float] = None
        self.loss_history: List[float] = []

    # -------------------------------------------------------------- ingestion

    def record(self, event: FeedbackEvent, autotrain: bool = True) -> Dict[str, Any]:
        if not event.labelled_heads():
            return {"accepted": False, "reason": "event carries no labels"}

        self.buffer.add(event)
        self.total_events += 1
        self.events_since_step += 1
        self._append_audit(event)

        trained: Optional[Dict[str, Any]] = None
        ready = len(self.buffer) >= self.cfg.min_replay_to_train
        if autotrain and ready and self.events_since_step >= self.cfg.train_every:
            trained = self.train(self.cfg.steps_per_trigger)
            self.events_since_step = 0

        return {
            "accepted": True,
            "buffer": len(self.buffer),
            "labels": event.labelled_heads(),
            "trained": trained,
        }

    def _append_audit(self, event: FeedbackEvent) -> None:
        path = self.cfg.replay_path
        try:
            with path.open("a", encoding="utf-8") as fh:
                fh.write(json.dumps(event.to_json(), ensure_ascii=False) + "\n")
        except OSError as exc:
            log.warning("train: audit append failed (%s)", exc)

    # --------------------------------------------------------------- training

    def _collate(self, events: List[FeedbackEvent]):
        emb = torch.from_numpy(np.stack([e.embedding for e in events])).to(self.device)
        feats = torch.from_numpy(np.stack([e.features for e in events])).to(self.device)
        targets, masks = {}, {}
        for head in HEADS:
            vals = [getattr(e, head) for e in events]
            masks[head] = torch.tensor(
                [v is not None for v in vals], dtype=torch.bool, device=self.device
            )
            targets[head] = torch.tensor(
                [float(v) if v is not None else 0.0 for v in vals],
                dtype=torch.float32,
                device=self.device,
            )
        return emb, feats, targets, masks

    def train(self, steps: int = 1) -> Dict[str, Any]:
        with self._lock:
            if len(self.buffer) < 2:
                return {"steps": 0, "reason": "buffer too small"}

            self.model.train()
            parts_acc: Dict[str, List[float]] = {}
            losses: List[float] = []

            for step in range(steps):
                # Alternate a general replay batch with a class-balanced taste
                # batch, so neither objective starves the other.
                if step % 2 == 1:
                    events = self.buffer.balanced_taste_sample(self.cfg.batch_size)
                    if not events:
                        events = self.buffer.sample(self.cfg.batch_size)
                else:
                    events = self.buffer.sample(self.cfg.batch_size)
                if len(events) < 2:
                    continue

                emb, feats, targets, masks = self._collate(events)
                self.opt.zero_grad(set_to_none=True)
                loss, parts = masked_losses(self.model(emb, feats), targets, masks, self.weights)
                if "skipped" in parts:
                    continue
                loss.backward()
                # Online batches are tiny and occasionally pathological; clip
                # so one weird label cannot wreck the trunk.
                torch.nn.utils.clip_grad_norm_(self.model.parameters(), 1.0)
                self.opt.step()

                self.total_steps += 1
                losses.append(float(loss.detach()))
                for key, val in parts.items():
                    parts_acc.setdefault(key, []).append(val)

            if losses:
                self.last_loss = float(np.mean(losses))
                self.loss_history.append(self.last_loss)
                self.loss_history = self.loss_history[-500:]

            self.model.eval()
            return {
                "steps": len(losses),
                "loss": self.last_loss,
                "per_head": {k: round(float(np.mean(v)), 5) for k, v in parts_acc.items()},
                "total_steps": self.total_steps,
            }

    # ----------------------------------------------------------- checkpointing

    def save(self) -> Optional[Path]:
        path = self.cfg.checkpoint_path
        try:
            torch.save(
                {
                    "model": self.model.state_dict(),
                    "optimizer": self.opt.state_dict(),
                    "embed_dim": self.model.embed_dim,
                    "n_features": self.model.n_features,
                    "total_steps": self.total_steps,
                    "total_events": self.total_events,
                    "loss_history": self.loss_history[-100:],
                    "saved_at": time.time(),
                },
                path,
            )
            log.info("train: checkpoint saved (%d steps) -> %s", self.total_steps, path.name)
            return path
        except OSError as exc:
            log.error("train: checkpoint save failed (%s)", exc)
            return None

    def load(self) -> bool:
        path = self.cfg.checkpoint_path
        if not path.exists():
            return False
        try:
            blob = torch.load(path, map_location=self.device, weights_only=False)
        except Exception as exc:
            log.error("train: checkpoint unreadable (%s) -- starting fresh", exc)
            return False

        # A checkpoint from a different embedding backend is not loadable. Say
        # so rather than reshaping into nonsense.
        if int(blob.get("embed_dim", -1)) != self.model.embed_dim:
            log.error(
                "train: checkpoint embed_dim %s != current %d -- refusing to load. "
                "The embedding backend changed; move %s aside to start fresh.",
                blob.get("embed_dim"),
                self.model.embed_dim,
                path.name,
            )
            return False
        if int(blob.get("n_features", N_FEATURES)) != self.model.n_features:
            log.error("train: checkpoint feature count mismatch -- refusing to load")
            return False

        self.model.load_state_dict(blob["model"])
        try:
            self.opt.load_state_dict(blob["optimizer"])
        except (ValueError, KeyError) as exc:
            log.warning("train: optimiser state not restored (%s); weights are fine", exc)
        self.total_steps = int(blob.get("total_steps", 0))
        self.total_events = int(blob.get("total_events", 0))
        self.loss_history = list(blob.get("loss_history", []))
        log.info("train: resumed from checkpoint at %d steps", self.total_steps)
        return True

    def stats(self) -> Dict[str, Any]:
        return {
            "total_steps": self.total_steps,
            "total_events": self.total_events,
            "events_since_step": self.events_since_step,
            "buffer_size": len(self.buffer),
            "last_loss": self.last_loss,
            "labels": self.buffer.label_counts(),
            "device": self.device,
            "lr": self.cfg.lr,
        }
