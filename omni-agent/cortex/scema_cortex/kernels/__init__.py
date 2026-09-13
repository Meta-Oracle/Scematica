"""Top-k cosine retrieval: the cortex hot path, with a pluggable kernel.

Every sense-loop cycle scores each candidate for *novelty* (how unlike
anything we already remember it is), which means one top-k cosine search per
candidate against the whole memory matrix. That is the only O(n*d) inner loop
in the system, so it gets a swappable implementation:

    mojo   -- SIMD kernel from similarity.mojo, compiled to a shared library
              and loaded over ctypes. Selected when the artifact exists.
    numpy  -- argpartition on the CPU. The active path on this machine.
    torch  -- GPU matmul + topk. Opt-in only; see below.

Selection is mojo -> numpy, overridable with SCEMA_KERNEL.

Why numpy over the GPU, on a machine that has CUDA: measured, not assumed.
`python -m scema_cortex.bench` on the RTX 2070 SUPER reports torch at 0.24x
numpy (28.3ms vs 8.2ms at n=100k, d=384) because every call pays a host->device
copy of the entire memory matrix, which dwarfs the matmul it enables. Memory
rows are owned by numpy on the host, so that copy is per-call and unavoidable
without a residency cache -- and at the memory sizes this agent actually
reaches (thousands of rows, <1ms in numpy) retrieval is not the bottleneck
worth that complexity. The GPU earns its place on batched TasteNet
inference and training instead, where the data is already resident.

Keep SCEMA_KERNEL=torch available anyway: it wins if memory ever grows past the
point where the copy amortises. Re-run the bench before believing that.

Note on Mojo: Modular ships Mojo for Linux and macOS only -- there is no
native Windows toolchain. On Windows, build the artifact inside WSL
(see cortex/scema_cortex/kernels/README.md). Until then the torch path runs,
and the numbers are identical; only the throughput differs.
"""
from __future__ import annotations

import ctypes
import logging
import os
from pathlib import Path
from typing import Tuple

import numpy as np

log = logging.getLogger("scema.kernels")

KERNEL_DIR = Path(__file__).resolve().parent
_LIB_NAMES = ("libscemasim.so", "scemasim.dll", "libscemasim.dylib")

_ACTIVE: str | None = None
_MOJO_LIB = None


def _load_mojo_lib():
    """Look for a compiled Mojo artifact next to the source."""
    global _MOJO_LIB
    if _MOJO_LIB is not None:
        return _MOJO_LIB
    for name in _LIB_NAMES:
        candidate = KERNEL_DIR / name
        if not candidate.exists():
            continue
        try:
            lib = ctypes.CDLL(str(candidate))
            # void topk_cosine(float* q, float* m, int n, int d, int k,
            #                  int* out_idx, float* out_score)
            lib.topk_cosine.restype = None
            lib.topk_cosine.argtypes = [
                ctypes.POINTER(ctypes.c_float),
                ctypes.POINTER(ctypes.c_float),
                ctypes.c_int,
                ctypes.c_int,
                ctypes.c_int,
                ctypes.POINTER(ctypes.c_int),
                ctypes.POINTER(ctypes.c_float),
            ]
            _MOJO_LIB = lib
            log.info("kernels: loaded Mojo artifact %s", candidate.name)
            return lib
        except OSError as exc:
            log.warning("kernels: %s present but unloadable (%s)", candidate.name, exc)
    return None


def _topk_mojo(query: np.ndarray, matrix: np.ndarray, k: int) -> Tuple[np.ndarray, np.ndarray]:
    lib = _load_mojo_lib()
    if lib is None:
        raise RuntimeError("Mojo kernel artifact not available")
    n, d = matrix.shape
    q = np.ascontiguousarray(query, dtype=np.float32)
    m = np.ascontiguousarray(matrix, dtype=np.float32)
    out_idx = np.zeros(k, dtype=np.int32)
    out_score = np.zeros(k, dtype=np.float32)
    lib.topk_cosine(
        q.ctypes.data_as(ctypes.POINTER(ctypes.c_float)),
        m.ctypes.data_as(ctypes.POINTER(ctypes.c_float)),
        ctypes.c_int(n),
        ctypes.c_int(d),
        ctypes.c_int(k),
        out_idx.ctypes.data_as(ctypes.POINTER(ctypes.c_int)),
        out_score.ctypes.data_as(ctypes.POINTER(ctypes.c_float)),
    )
    return out_idx.astype(np.int64), out_score


def _topk_torch(query: np.ndarray, matrix: np.ndarray, k: int) -> Tuple[np.ndarray, np.ndarray]:
    import torch

    from ..config import CONFIG

    device = CONFIG.resolve_device()
    q = torch.from_numpy(np.ascontiguousarray(query, dtype=np.float32)).to(device)
    m = torch.from_numpy(np.ascontiguousarray(matrix, dtype=np.float32)).to(device)
    scores = m @ q  # rows are pre-normalised, so this is cosine
    k = min(k, scores.shape[0])
    top = torch.topk(scores, k)
    return top.indices.cpu().numpy().astype(np.int64), top.values.cpu().numpy()


def _topk_numpy(query: np.ndarray, matrix: np.ndarray, k: int) -> Tuple[np.ndarray, np.ndarray]:
    scores = matrix @ query
    k = min(k, scores.shape[0])
    # argpartition is O(n) vs a full O(n log n) sort; only the k head is sorted.
    part = np.argpartition(-scores, k - 1)[:k]
    order = part[np.argsort(-scores[part])]
    return order.astype(np.int64), scores[order]


def active_kernel() -> str:
    """Resolve (once) which implementation backs topk_cosine."""
    global _ACTIVE
    if _ACTIVE is not None:
        return _ACTIVE

    forced = os.getenv("SCEMA_KERNEL", "").strip().lower()
    if forced in {"mojo", "torch", "numpy"}:
        _ACTIVE = forced
        log.info("kernels: %s (forced via SCEMA_KERNEL)", forced)
        return _ACTIVE

    from ..config import CONFIG

    # numpy, not torch: the GPU path loses to the host->device copy at every
    # size this agent reaches. See the module docstring for the measurement.
    _ACTIVE = "mojo" if (CONFIG.use_mojo_kernel and _load_mojo_lib() is not None) else "numpy"
    log.info("kernels: %s", _ACTIVE)
    return _ACTIVE


def topk_cosine(query: np.ndarray, matrix: np.ndarray, k: int = 8) -> Tuple[np.ndarray, np.ndarray]:
    """Return (indices, scores) of the k rows of ``matrix`` closest to ``query``.

    Both are expected L2-normalised, so the dot product *is* cosine similarity.
    """
    if matrix.size == 0 or k <= 0:
        return np.empty(0, dtype=np.int64), np.empty(0, dtype=np.float32)
    if matrix.ndim != 2:
        raise ValueError(f"matrix must be 2-D, got shape {matrix.shape}")
    if query.shape[-1] != matrix.shape[1]:
        raise ValueError(f"dim mismatch: query {query.shape[-1]} vs matrix {matrix.shape[1]}")

    query = np.asarray(query, dtype=np.float32).reshape(-1)
    kernel = active_kernel()

    if kernel == "mojo":
        try:
            return _topk_mojo(query, matrix, k)
        except Exception as exc:  # a broken artifact must not take the agent down
            log.warning("kernels: mojo path failed (%s), falling back to numpy", exc)
    elif kernel == "torch":
        try:
            return _topk_torch(query, matrix, k)
        except Exception as exc:
            log.warning("kernels: torch path failed (%s), falling back to numpy", exc)
    return _topk_numpy(query, matrix, k)
