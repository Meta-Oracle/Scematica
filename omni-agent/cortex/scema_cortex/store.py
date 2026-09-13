"""Episodic memory: one recollection shared by every surface.

This is what makes "one mind, many surfaces" more than a slogan. A Telegram
DM, a tweet reply and a CLI turn all write into the same store and all recall
from it, so continuity is a property of the architecture rather than something
each plugin re-implements.

Design choices worth defending:

* **Vectors live in one contiguous float32 matrix.** Recall is a single
  `topk_cosine` against it (see kernels/). Rows are appended with geometric
  growth so appends stay amortised O(1) and the matrix stays contiguous for
  the kernel.
* **Records are plain dicts persisted as JSONL.** They are meant to be read by
  a human with `tail`, and hand-editable when the agent learns something wrong.
* **Novelty is a first-class query.** `novelty()` is how the sense loop avoids
  reacting to the same discourse twice, and it is a TasteNet input feature,
  so it belongs here rather than in the caller.
* **Decay is applied at read time, never at write time.** Recency weighting is
  a ranking policy that callers may want to vary; rewriting stored scores
  would destroy information.
"""
from __future__ import annotations

import json
import logging
import threading
import time
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Any, Dict, Iterable, List, Optional, Sequence

import numpy as np

from .kernels import topk_cosine

log = logging.getLogger("scema.store")

# Surfaces the agent can remember something from.
SURFACES = ("twitter", "telegram", "cli", "sense", "reflection", "system")


@dataclass
class MemoryRecord:
    id: str
    text: str
    surface: str = "system"
    kind: str = "observation"      # observation | draft | posted | decision | lesson
    created_at: float = field(default_factory=time.time)
    meta: Dict[str, Any] = field(default_factory=dict)

    def to_json(self) -> str:
        return json.dumps(asdict(self), ensure_ascii=False)


@dataclass
class Recollection:
    record: MemoryRecord
    score: float          # raw cosine similarity
    weighted: float       # similarity after recency decay

    def as_dict(self) -> Dict[str, Any]:
        return {
            "id": self.record.id,
            "text": self.record.text,
            "surface": self.record.surface,
            "kind": self.record.kind,
            "created_at": self.record.created_at,
            "age_hours": max(0.0, (time.time() - self.record.created_at) / 3600.0),
            "meta": self.record.meta,
            "score": round(self.score, 6),
            "weighted": round(self.weighted, 6),
        }


class MemoryStore:
    """Append-only vector memory with recency-weighted recall.

    Thread-safe: the FastAPI server handles concurrent requests, and a recall
    racing an append must never observe a matrix whose row count disagrees
    with its record list.
    """

    def __init__(self, dim: int, path: Optional[Path] = None) -> None:
        self.dim = dim
        self.path = Path(path) if path else None
        self._lock = threading.RLock()
        self._records: List[MemoryRecord] = []
        # Capacity-managed backing buffer; only the first _size rows are live.
        self._buf = np.zeros((256, dim), dtype=np.float32)
        self._size = 0
        self._next_id = 1

    # ------------------------------------------------------------------ state

    def __len__(self) -> int:
        return self._size

    @property
    def vectors(self) -> np.ndarray:
        """A view of the live rows. Do not mutate; do not hold across appends."""
        return self._buf[: self._size]

    def stats(self) -> Dict[str, Any]:
        with self._lock:
            by_surface: Dict[str, int] = {}
            by_kind: Dict[str, int] = {}
            for rec in self._records:
                by_surface[rec.surface] = by_surface.get(rec.surface, 0) + 1
                by_kind[rec.kind] = by_kind.get(rec.kind, 0) + 1
            oldest = self._records[0].created_at if self._records else None
            return {
                "count": self._size,
                "dim": self.dim,
                "capacity": int(self._buf.shape[0]),
                "by_surface": by_surface,
                "by_kind": by_kind,
                "oldest_at": oldest,
                "bytes_resident": int(self._buf.nbytes),
            }

    # ----------------------------------------------------------------- writes

    def _grow(self, needed: int) -> None:
        capacity = self._buf.shape[0]
        if needed <= capacity:
            return
        new_cap = capacity
        while new_cap < needed:
            new_cap *= 2
        grown = np.zeros((new_cap, self.dim), dtype=np.float32)
        grown[: self._size] = self._buf[: self._size]
        self._buf = grown
        log.debug("memory: grew capacity %d -> %d", capacity, new_cap)

    def add(
        self,
        text: str,
        vector: np.ndarray,
        surface: str = "system",
        kind: str = "observation",
        meta: Optional[Dict[str, Any]] = None,
        record_id: Optional[str] = None,
    ) -> MemoryRecord:
        vector = np.asarray(vector, dtype=np.float32).reshape(-1)
        if vector.shape[0] != self.dim:
            raise ValueError(f"vector dim {vector.shape[0]} != store dim {self.dim}")
        # Normalise on the way in so recall can assume unit rows.
        norm = float(np.linalg.norm(vector))
        if norm > 0:
            vector = vector / norm

        with self._lock:
            if record_id is None:
                record_id = f"m{self._next_id:08d}"
                self._next_id += 1
            record = MemoryRecord(
                id=record_id,
                text=text,
                surface=surface if surface in SURFACES else "system",
                kind=kind,
                meta=dict(meta or {}),
            )
            self._grow(self._size + 1)
            self._buf[self._size] = vector
            self._size += 1
            self._records.append(record)
            self._append_jsonl(record)
            return record

    def _append_jsonl(self, record: MemoryRecord) -> None:
        if self.path is None:
            return
        jsonl = self.path.with_suffix(".jsonl")
        try:
            with jsonl.open("a", encoding="utf-8") as fh:
                fh.write(record.to_json() + "\n")
        except OSError as exc:
            log.warning("memory: could not append to %s (%s)", jsonl, exc)

    # ----------------------------------------------------------------- reads

    def recall(
        self,
        query_vector: np.ndarray,
        k: int = 6,
        surfaces: Optional[Sequence[str]] = None,
        kinds: Optional[Sequence[str]] = None,
        half_life_hours: float = 72.0,
        min_score: float = 0.0,
    ) -> List[Recollection]:
        """Top-k recall, recency-weighted, optionally filtered by surface/kind.

        Filtering happens *after* retrieval over an over-fetched candidate set
        rather than by masking the matrix: masking would mean rebuilding a
        contiguous array per query, which costs more than over-fetching.
        """
        with self._lock:
            if self._size == 0:
                return []
            query_vector = np.asarray(query_vector, dtype=np.float32).reshape(-1)
            if query_vector.shape[0] != self.dim:
                raise ValueError(f"query dim {query_vector.shape[0]} != store dim {self.dim}")

            filtering = bool(surfaces or kinds)
            fetch = min(self._size, max(k * 8, k) if filtering else k)
            idx, scores = topk_cosine(query_vector, self.vectors, fetch)

            now = time.time()
            out: List[Recollection] = []
            for i, raw in zip(idx.tolist(), scores.tolist()):
                rec = self._records[i]
                if surfaces and rec.surface not in surfaces:
                    continue
                if kinds and rec.kind not in kinds:
                    continue
                if raw < min_score:
                    continue
                age_h = max(0.0, (now - rec.created_at) / 3600.0)
                decay = 0.5 ** (age_h / half_life_hours) if half_life_hours > 0 else 1.0
                out.append(Recollection(record=rec, score=float(raw), weighted=float(raw) * decay))
                if len(out) >= k:
                    break

            out.sort(key=lambda r: r.weighted, reverse=True)
            return out

    def novelty(self, query_vector: np.ndarray, against: int = 32) -> float:
        """1 - (max similarity to anything remembered). Empty memory is fully novel.

        This is the feature that stops the agent from posting the same take
        twice in a week, and it feeds TasteNet directly.
        """
        with self._lock:
            if self._size == 0:
                return 1.0
            _, scores = topk_cosine(
                np.asarray(query_vector, dtype=np.float32).reshape(-1),
                self.vectors,
                min(against, self._size),
            )
        if scores.size == 0:
            return 1.0
        return float(np.clip(1.0 - float(scores.max()), 0.0, 1.0))

    def get(self, record_id: str) -> Optional[MemoryRecord]:
        with self._lock:
            for rec in reversed(self._records):  # recent lookups dominate
                if rec.id == record_id:
                    return rec
        return None

    def recent(self, n: int = 20, surface: Optional[str] = None) -> List[MemoryRecord]:
        with self._lock:
            pool = [r for r in self._records if surface is None or r.surface == surface]
            return pool[-n:][::-1]

    # ------------------------------------------------------------ persistence

    def save(self) -> Optional[Path]:
        """Snapshot vectors + records so a restart is not amnesia."""
        if self.path is None:
            return None
        with self._lock:
            tmp = self.path.with_suffix(".npz.tmp")
            try:
                # Write through an open handle, not a path: np.savez_compressed
                # silently appends ".npz" to any filename lacking it, which
                # would land the data next to the file we then try to rename.
                with tmp.open("wb") as fh:
                    np.savez_compressed(
                        fh,
                        vectors=self.vectors,
                        records=np.array([r.to_json() for r in self._records], dtype=object),
                        dim=np.int64(self.dim),
                        next_id=np.int64(self._next_id),
                    )
                # Atomic-ish replace: a crash mid-write must not corrupt memory.
                tmp.replace(self.path)
            except OSError as exc:
                log.error("memory: save failed (%s)", exc)
                return None
            log.info("memory: saved %d records to %s", self._size, self.path.name)
            return self.path

    @classmethod
    def load(cls, dim: int, path: Path) -> "MemoryStore":
        store = cls(dim=dim, path=path)
        if not path.exists():
            return store
        try:
            with np.load(path, allow_pickle=True) as blob:
                stored_dim = int(blob["dim"])
                if stored_dim != dim:
                    # Silently reshaping would corrupt every similarity in the
                    # system. Start clean and say so loudly instead.
                    log.error(
                        "memory: checkpoint dim %d != embedder dim %d -- refusing to load %s. "
                        "Change SCEMA_EMBED_BACKEND back, or move the file aside to start fresh.",
                        stored_dim,
                        dim,
                        path.name,
                    )
                    return store
                vectors = np.asarray(blob["vectors"], dtype=np.float32)
                records = [MemoryRecord(**json.loads(s)) for s in blob["records"].tolist()]
                store._grow(max(len(records), 256))
                store._buf[: len(records)] = vectors[: len(records)]
                store._size = len(records)
                store._records = records
                store._next_id = int(blob["next_id"])
            log.info("memory: loaded %d records from %s", store._size, path.name)
        except Exception as exc:
            log.error("memory: load failed (%s) -- starting empty", exc)
        return store
