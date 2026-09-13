"""Shape, gradient and masking checks for TasteNet.

Runnable with pytest, or directly: python cortex/tests/test_model.py
"""
from __future__ import annotations

import sys
from pathlib import Path

import numpy as np
import torch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from scema_cortex.model import (  # noqa: E402
    HEADS,
    N_FEATURES,
    build_model,
    count_params,
    features_to_vector,
    masked_losses,
)

DEVICE = "cuda" if torch.cuda.is_available() else "cpu"
EMBED_DIM = 256


def _batch(n: int):
    emb = torch.randn(n, EMBED_DIM, device=DEVICE)
    emb = emb / emb.norm(dim=1, keepdim=True)
    feats = torch.from_numpy(
        np.stack([features_to_vector({"log_likes": 3.2, "age_hours": 1.0}) for _ in range(n)])
    ).to(DEVICE)
    return emb, feats


def test_features_to_vector_handles_partial_and_garbage():
    vec = features_to_vector({"log_likes": 2.0, "nonexistent": 9.0, "sentiment": float("nan")})
    assert vec.shape == (N_FEATURES,)
    assert np.isfinite(vec).all(), "NaN must be filtered, not propagated"
    assert vec.sum() == 2.0, "only the known, finite feature should be set"
    assert features_to_vector(None).sum() == 0.0


def test_score_returns_calibrated_ranges():
    model = build_model(EMBED_DIM).to(DEVICE)
    emb, feats = _batch(5)
    out = model.score(emb, feats)
    assert set(out) == set(HEADS)
    for head in ("salience", "taste"):
        vals = out[head]
        assert vals.shape == (5,)
        assert bool(((vals >= 0) & (vals <= 1)).all()), f"{head} must be a probability"
    assert bool((out["resonance"] >= 0).all()), "engagement cannot be negative"


def test_single_row_scoring_does_not_crash_feature_norm():
    model = build_model(EMBED_DIM).to(DEVICE)
    emb, feats = _batch(1)
    out = model.score(emb, feats)
    assert out["taste"].shape == (1,)


def test_masked_loss_trains_only_labelled_heads():
    model = build_model(EMBED_DIM).to(DEVICE)
    emb, feats = _batch(5)
    model.train()
    logits = model(emb, feats)

    zeros = torch.zeros(5, device=DEVICE)
    targets = {
        "salience": zeros,
        "taste": torch.tensor([1.0, 0.0, 0.0, 0.0, 0.0], device=DEVICE),
        "resonance": torch.tensor([0.0, 0.0, 0.0, 0.0, 42.0], device=DEVICE),
    }
    masks = {
        "salience": torch.zeros(5, dtype=torch.bool, device=DEVICE),
        "taste": torch.tensor([1, 1, 0, 0, 0], dtype=torch.bool, device=DEVICE),
        "resonance": torch.tensor([0, 0, 0, 0, 1], dtype=torch.bool, device=DEVICE),
    }
    loss, parts = masked_losses(logits, targets, masks)
    loss.backward()

    assert "taste" in parts and "resonance" in parts
    assert "salience" not in parts, "unlabelled head must contribute no loss"
    grad_sum = sum(float(p.grad.norm()) for p in model.parameters() if p.grad is not None)
    assert grad_sum > 0, "shared trunk must receive gradient from sparse labels"


def test_empty_mask_is_a_noop_not_a_crash():
    model = build_model(EMBED_DIM).to(DEVICE)
    emb, feats = _batch(3)
    logits = model(emb, feats)
    zeros = torch.zeros(3, device=DEVICE)
    none = torch.zeros(3, dtype=torch.bool, device=DEVICE)
    loss, parts = masked_losses(
        logits,
        {h: zeros for h in HEADS},
        {h: none for h in HEADS},
    )
    assert parts.get("skipped") == 1.0
    assert float(loss) == 0.0


def test_learns_a_separable_taste_signal():
    """End-to-end sanity: the net must fit an obvious pattern quickly.

    Two clusters, opposite taste labels. If this cannot be driven down, the
    trunk/head/optimiser wiring is broken.
    """
    torch.manual_seed(0)
    model = build_model(EMBED_DIM).to(DEVICE)
    opt = torch.optim.AdamW(model.parameters(), lr=3e-4)

    good = torch.randn(1, EMBED_DIM, device=DEVICE)
    bad = torch.randn(1, EMBED_DIM, device=DEVICE)
    n = 32
    emb = torch.cat([good.repeat(n // 2, 1), bad.repeat(n // 2, 1)])
    emb = emb + 0.05 * torch.randn_like(emb)
    emb = emb / emb.norm(dim=1, keepdim=True)
    feats = torch.zeros(n, N_FEATURES, device=DEVICE)
    taste = torch.cat([torch.ones(n // 2, device=DEVICE), torch.zeros(n // 2, device=DEVICE)])
    masks = {
        "salience": torch.zeros(n, dtype=torch.bool, device=DEVICE),
        "taste": torch.ones(n, dtype=torch.bool, device=DEVICE),
        "resonance": torch.zeros(n, dtype=torch.bool, device=DEVICE),
    }
    targets = {"salience": taste, "taste": taste, "resonance": taste}

    model.train()
    first = last = None
    for step in range(150):
        opt.zero_grad()
        loss, parts = masked_losses(model(emb, feats), targets, masks)
        loss.backward()
        opt.step()
        if step == 0:
            first = parts["taste"]
        last = parts["taste"]

    assert last < first * 0.5, f"taste loss did not fall: {first:.4f} -> {last:.4f}"

    probs = model.score(emb, feats)["taste"]
    assert probs[: n // 2].mean() > probs[n // 2 :].mean(), "clusters not separated"
    return first, last


if __name__ == "__main__":
    print(f"device={DEVICE}")
    model = build_model(EMBED_DIM)
    print(f"params={count_params(model):,} features={N_FEATURES} heads={HEADS}")

    passed = 0
    for name, fn in sorted(globals().items()):
        if not name.startswith("test_") or not callable(fn):
            continue
        result = fn()
        extra = ""
        if name == "test_learns_a_separable_taste_signal" and result:
            extra = f"  (taste loss {result[0]:.4f} -> {result[1]:.4f})"
        print(f"  PASS {name}{extra}")
        passed += 1
    print(f"{passed} checks passed")
