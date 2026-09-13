"""Measure the retrieval hot path instead of guessing about it.

    python -m scema_cortex.bench            # default sweep
    python -m scema_cortex.bench --n 50000  # one size

Reports per-call latency for every available kernel and checks that they agree,
so the Mojo path can never be adopted on the strength of speed alone.
"""
from __future__ import annotations

import argparse
import os
import time
from typing import Callable, Dict, List, Tuple

import numpy as np


def _timeit(fn: Callable[[], object], repeats: int, warmup: int = 3) -> float:
    """Median seconds per call. Median, not mean: one OS hiccup should not
    decide which kernel looks faster."""
    for _ in range(warmup):
        fn()
    samples: List[float] = []
    for _ in range(repeats):
        start = time.perf_counter()
        fn()
        samples.append(time.perf_counter() - start)
    return float(np.median(samples))


def _kernels() -> Dict[str, Callable]:
    from .kernels import _load_mojo_lib, _topk_mojo, _topk_numpy, _topk_torch

    available: Dict[str, Callable] = {"numpy": _topk_numpy}
    try:
        import torch

        if torch.cuda.is_available():
            available["torch"] = _topk_torch
    except Exception:
        pass
    if _load_mojo_lib() is not None:
        available["mojo"] = _topk_mojo
    return available


def _random_memory(n: int, d: int, seed: int = 0) -> Tuple[np.ndarray, np.ndarray]:
    rng = np.random.default_rng(seed)
    matrix = rng.standard_normal((n, d), dtype=np.float32)
    matrix /= np.linalg.norm(matrix, axis=1, keepdims=True)
    query = rng.standard_normal(d, dtype=np.float32)
    query /= np.linalg.norm(query)
    return matrix, query


def run(sizes: List[int], dim: int, k: int, repeats: int) -> int:
    kernels = _kernels()
    print(f"kernels available: {', '.join(sorted(kernels))}")
    print(f"dim={dim} k={k} repeats={repeats}\n")
    print(f"{'n':>9}  " + "  ".join(f"{name:>12}" for name in sorted(kernels)) + "   agree")
    print("-" * (11 + 14 * len(kernels) + 8))

    failures = 0
    for n in sizes:
        matrix, query = _random_memory(n, dim)
        timings: Dict[str, float] = {}
        results: Dict[str, np.ndarray] = {}

        for name, fn in sorted(kernels.items()):
            timings[name] = _timeit(lambda f=fn: f(query, matrix, k), repeats)
            idx, score = fn(query, matrix, k)
            results[name] = score

        # Compare scores rather than indices: ties in random data can order
        # differently between kernels without either being wrong.
        ref_name = "numpy"
        ref = np.sort(results[ref_name])[::-1]
        agree = True
        for name, score in results.items():
            if name == ref_name:
                continue
            if not np.allclose(np.sort(score)[::-1], ref, atol=1e-4):
                agree = False
                failures += 1
                print(f"  MISMATCH {name} vs {ref_name} at n={n}")

        row = f"{n:>9,}  " + "  ".join(
            f"{timings[name] * 1e3:>9.3f}ms" for name in sorted(kernels)
        )
        print(row + f"   {'yes' if agree else 'NO'}")

    if len(kernels) > 1:
        print()
        biggest = sizes[-1]
        matrix, query = _random_memory(biggest, dim)
        base = _timeit(lambda: kernels["numpy"](query, matrix, k), repeats)
        for name in sorted(kernels):
            if name == "numpy":
                continue
            other = _timeit(lambda f=kernels[name]: f(query, matrix, k), repeats)
            ratio = base / other if other > 0 else float("inf")
            verdict = "faster" if ratio > 1 else "SLOWER"
            print(f"at n={biggest:,}: {name} is {ratio:.2f}x {verdict} than numpy")

    return failures


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--n", type=int, action="append", help="memory size (repeatable)")
    parser.add_argument("--dim", type=int, default=int(os.getenv("SCEMA_EMBED_DIM", "384")))
    parser.add_argument("--k", type=int, default=8)
    parser.add_argument("--repeats", type=int, default=15)
    args = parser.parse_args()

    sizes = args.n or [1_000, 10_000, 100_000]
    failures = run(sorted(sizes), args.dim, args.k, args.repeats)
    if failures:
        print(f"\n{failures} kernel disagreement(s) -- do not trust the fast path")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
