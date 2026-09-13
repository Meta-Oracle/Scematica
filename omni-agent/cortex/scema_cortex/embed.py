"""Text -> vector, with graceful backend degradation.

Resolution order for ``embed_backend="auto"``:

1. ``local``   - sentence-transformers on the GPU. Best quality, no network,
                 no per-call cost. Used when the package is importable.
2. ``openai``  - hosted embeddings, used when OPENAI_API_KEY is present.
3. ``hashing`` - dependency-free deterministic n-gram hashing. Always works,
                 so the cortex degrades instead of dying.

The chosen backend fixes ``dim`` for the life of the process; TasteNet is
built lazily against that dim, and a checkpoint trained on a different dim is
rejected rather than silently reshaped.
"""
from __future__ import annotations

import hashlib
import logging
import os
import re
from typing import List, Sequence

import numpy as np

from .config import CONFIG, CortexConfig

log = logging.getLogger("scema.embed")

_TOKEN_RE = re.compile(r"[a-z0-9']+")


def _l2(mat: np.ndarray) -> np.ndarray:
    norms = np.linalg.norm(mat, axis=-1, keepdims=True)
    return mat / np.maximum(norms, 1e-9)


class Embedder:
    """Base contract: ``encode(list[str]) -> (n, dim) float32, L2-normalised``."""

    name: str = "base"
    dim: int = 0

    def encode(self, texts: Sequence[str]) -> np.ndarray:  # pragma: no cover
        raise NotImplementedError


class HashingEmbedder(Embedder):
    """Signed-hash bag of word unigrams/bigrams plus character trigrams.

    Not semantic, but stable and surprisingly serviceable for ranking within a
    narrow topical stream -- and it guarantees the agent still boots offline.
    """

    name = "hashing"

    def __init__(self, dim: int = 512) -> None:
        self.dim = dim

    @staticmethod
    def _bucket(token: str, dim: int) -> tuple[int, float]:
        # hashlib rather than builtins.hash so buckets survive process
        # restarts -- PYTHONHASHSEED randomisation would otherwise shift them.
        digest = hashlib.blake2b(token.encode("utf-8"), digest_size=8).digest()
        raw = int.from_bytes(digest, "little")
        return raw % dim, 1.0 if (raw >> 63) & 1 else -1.0

    def encode(self, texts: Sequence[str]) -> np.ndarray:
        out = np.zeros((len(texts), self.dim), dtype=np.float32)
        for row, text in enumerate(texts):
            low = (text or "").lower()
            words = _TOKEN_RE.findall(low)
            features: List[str] = list(words)
            features += [f"{a}_{b}" for a, b in zip(words, words[1:])]
            squashed = re.sub(r"\s+", " ", low)
            features += [squashed[i : i + 3] for i in range(max(0, len(squashed) - 2))]
            for feat in features:
                idx, sign = self._bucket(feat, self.dim)
                out[row, idx] += sign
        return _l2(out)


class LocalEmbedder(Embedder):
    """sentence-transformers, pinned to the cortex device."""

    name = "local"

    def __init__(self, model_name: str, device: str) -> None:
        from sentence_transformers import SentenceTransformer  # noqa: PLC0415

        self.model = SentenceTransformer(model_name, device=device)
        self.dim = int(self.model.get_sentence_embedding_dimension())

    def encode(self, texts: Sequence[str]) -> np.ndarray:
        vecs = self.model.encode(
            list(texts),
            convert_to_numpy=True,
            normalize_embeddings=True,
            show_progress_bar=False,
        )
        return vecs.astype(np.float32, copy=False)


class OpenAIEmbedder(Embedder):
    """Hosted fallback; only used when a key is configured."""

    name = "openai"

    def __init__(self, model: str = "text-embedding-3-small") -> None:
        from openai import OpenAI  # noqa: PLC0415

        self.client = OpenAI()
        self.model = model
        self.dim = 1536 if "3-small" in model else 3072

    def encode(self, texts: Sequence[str]) -> np.ndarray:
        # The API rejects empty strings, so substitute a single space.
        payload = [t if (t and t.strip()) else " " for t in texts]
        resp = self.client.embeddings.create(model=self.model, input=payload)
        mat = np.asarray([d.embedding for d in resp.data], dtype=np.float32)
        return _l2(mat)


def _try_local(cfg: CortexConfig) -> Embedder | None:
    try:
        emb = LocalEmbedder(cfg.embed_model, cfg.resolve_device())
        log.info("embeddings: local %s (dim=%d, %s)", cfg.embed_model, emb.dim, cfg.resolve_device())
        return emb
    except Exception as exc:
        log.info("embeddings: local backend unavailable (%s)", exc.__class__.__name__)
        return None


def _try_openai() -> Embedder | None:
    if not os.getenv("OPENAI_API_KEY"):
        return None
    try:
        emb = OpenAIEmbedder()
        log.info("embeddings: openai (dim=%d)", emb.dim)
        return emb
    except Exception as exc:
        log.info("embeddings: openai backend unavailable (%s)", exc.__class__.__name__)
        return None


def build_embedder(cfg: CortexConfig | None = None) -> Embedder:
    cfg = cfg or CONFIG
    want = (cfg.embed_backend or "auto").lower()

    if want == "hashing":
        return HashingEmbedder(cfg.embed_dim_hashing)
    if want == "local":
        emb = _try_local(cfg)
        if emb is None:
            raise RuntimeError("SCEMA_EMBED_BACKEND=local but sentence-transformers is unusable")
        return emb
    if want == "openai":
        emb = _try_openai()
        if emb is None:
            raise RuntimeError("SCEMA_EMBED_BACKEND=openai but OPENAI_API_KEY/client is unusable")
        return emb

    for candidate in (lambda: _try_local(cfg), _try_openai):
        emb = candidate()
        if emb is not None:
            return emb

    log.warning("embeddings: falling back to dependency-free hashing backend")
    return HashingEmbedder(cfg.embed_dim_hashing)
