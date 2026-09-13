"""Memory and online-training behaviour.

    python cortex/tests/test_store_train.py
"""
from __future__ import annotations

import shutil
import sys
import tempfile
from pathlib import Path

import numpy as np
import torch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from scema_cortex.config import CortexConfig  # noqa: E402
from scema_cortex.embed import HashingEmbedder  # noqa: E402
from scema_cortex.model import build_model, features_to_vector  # noqa: E402
from scema_cortex.store import MemoryStore  # noqa: E402
from scema_cortex.train import FeedbackEvent, ReplayBuffer, Trainer  # noqa: E402

DIM = 128
EMB = HashingEmbedder(DIM)


def _vec(text: str) -> np.ndarray:
    return EMB.encode([text])[0]


# --------------------------------------------------------------------- store


def test_add_and_recall_finds_the_related_memory():
    store = MemoryStore(dim=DIM)
    store.add("grok shipped live X search today", _vec("grok shipped live X search today"), surface="sense")
    store.add("the cat sat on the mat", _vec("the cat sat on the mat"), surface="cli")
    store.add("xai released a new grok model", _vec("xai released a new grok model"), surface="sense")

    hits = store.recall(_vec("grok live search release"), k=2)
    assert len(hits) == 2
    assert "grok" in hits[0].record.text, f"unrelated memory ranked first: {hits[0].record.text}"
    assert hits[0].score >= hits[1].score


def test_recall_respects_surface_and_kind_filters():
    store = MemoryStore(dim=DIM)
    store.add("draft about grok", _vec("draft about grok"), surface="twitter", kind="draft")
    store.add("posted about grok", _vec("posted about grok"), surface="twitter", kind="posted")
    store.add("telegram chat about grok", _vec("telegram chat about grok"), surface="telegram")

    only_drafts = store.recall(_vec("grok"), k=5, kinds=["draft"])
    assert len(only_drafts) == 1 and only_drafts[0].record.kind == "draft"

    only_tg = store.recall(_vec("grok"), k=5, surfaces=["telegram"])
    assert len(only_tg) == 1 and only_tg[0].record.surface == "telegram"


def test_recency_decay_reorders_equally_similar_memories():
    store = MemoryStore(dim=DIM)
    old = store.add("identical text", _vec("identical text"))
    new = store.add("identical text", _vec("identical text"))
    # Backdate the first by a week.
    old.created_at -= 7 * 24 * 3600

    hits = store.recall(_vec("identical text"), k=2, half_life_hours=24.0)
    assert hits[0].record.id == new.id, "stale memory outranked the fresh one"
    assert hits[0].weighted > hits[1].weighted


def test_novelty_collapses_once_something_is_remembered():
    store = MemoryStore(dim=DIM)
    assert store.novelty(_vec("anything")) == 1.0, "empty memory must be fully novel"

    text = "grok now reads the timeline in real time"
    store.add(text, _vec(text))
    assert store.novelty(_vec(text)) < 0.01, "an exact repeat must not read as novel"
    assert store.novelty(_vec("unrelated thoughts about gardening")) > 0.5


def test_dim_mismatch_is_rejected_not_reshaped():
    store = MemoryStore(dim=DIM)
    try:
        store.add("bad", np.zeros(DIM + 1, dtype=np.float32))
    except ValueError as exc:
        assert "dim" in str(exc)
    else:
        raise AssertionError("a wrong-dim vector must raise")


def test_save_and_load_round_trip():
    tmp = Path(tempfile.mkdtemp())
    try:
        path = tmp / "memory.npz"
        store = MemoryStore(dim=DIM, path=path)
        for i in range(50):
            store.add(f"memory number {i}", _vec(f"memory number {i}"), kind="observation")
        store.save()

        revived = MemoryStore.load(dim=DIM, path=path)
        assert len(revived) == 50, f"expected 50 records, got {len(revived)}"
        hits = revived.recall(_vec("memory number 42"), k=1)
        assert hits and "42" in hits[0].record.text
        # The append-only audit log should exist alongside the snapshot.
        assert path.with_suffix(".jsonl").exists()
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def test_load_refuses_a_dim_mismatched_checkpoint():
    tmp = Path(tempfile.mkdtemp())
    try:
        path = tmp / "memory.npz"
        store = MemoryStore(dim=DIM, path=path)
        store.add("hello", _vec("hello"))
        store.save()
        # Pretend the embedding backend changed underneath us.
        revived = MemoryStore.load(dim=DIM * 2, path=path)
        assert len(revived) == 0, "mismatched memory must not be silently reshaped"
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def test_growth_keeps_matrix_contiguous_and_correct():
    store = MemoryStore(dim=DIM)
    for i in range(600):  # forces several doublings past the initial 256
        store.add(f"item {i}", _vec(f"item {i}"))
    assert len(store) == 600
    assert store.vectors.shape == (600, DIM)
    assert store.vectors.flags["C_CONTIGUOUS"], "kernel requires contiguous rows"
    norms = np.linalg.norm(store.vectors, axis=1)
    assert np.allclose(norms, 1.0, atol=1e-5), "rows must stay unit-normalised"


# --------------------------------------------------------------------- replay


def test_balanced_sample_fixes_a_lopsided_label_distribution():
    buf = ReplayBuffer(1000)
    for i in range(95):
        buf.add(FeedbackEvent(_vec(f"approved {i}"), features_to_vector(None), taste=1.0))
    for i in range(5):
        buf.add(FeedbackEvent(_vec(f"rejected {i}"), features_to_vector(None), taste=0.0))

    counts = buf.label_counts()
    assert counts["taste_positive"] == 95 and counts["taste_negative"] == 5

    batch = buf.balanced_taste_sample(32)
    pos = sum(1 for e in batch if e.taste >= 0.5)
    neg = len(batch) - pos
    assert pos == neg, f"balanced sample was {pos}/{neg}"


def test_buffer_evicts_oldest_at_capacity():
    buf = ReplayBuffer(10)
    for i in range(25):
        buf.add(FeedbackEvent(_vec(str(i)), features_to_vector(None), taste=1.0, ref=str(i)))
    assert len(buf) == 10
    refs = {e.ref for e in buf._items}
    assert "0" not in refs and "24" in refs, "eviction must drop the oldest"


def test_unlabelled_event_is_rejected():
    trainer = Trainer(build_model(DIM), cfg=CortexConfig(data_dir=Path(tempfile.mkdtemp())))
    out = trainer.record(FeedbackEvent(_vec("nothing"), features_to_vector(None)))
    assert out["accepted"] is False


def test_autotrain_fires_on_the_configured_cadence():
    tmp = Path(tempfile.mkdtemp())
    try:
        cfg = CortexConfig(data_dir=tmp, train_every=8, min_replay_to_train=16, steps_per_trigger=2)
        trainer = Trainer(build_model(DIM), cfg=cfg)

        results = []
        for i in range(32):
            label = 1.0 if i % 2 == 0 else 0.0
            results.append(
                trainer.record(
                    FeedbackEvent(_vec(f"draft {i}"), features_to_vector({"novelty": 0.8}), taste=label)
                )
            )

        trained = [r for r in results if r.get("trained")]
        assert trained, "autotrain never fired"
        assert trainer.total_steps > 0
        # No step before the buffer reached min_replay_to_train.
        first_train_at = next(i for i, r in enumerate(results) if r.get("trained"))
        assert first_train_at >= 15, f"trained too early at event {first_train_at}"
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def test_checkpoint_round_trip_preserves_predictions():
    tmp = Path(tempfile.mkdtemp())
    try:
        cfg = CortexConfig(data_dir=tmp)
        trainer = Trainer(build_model(DIM), cfg=cfg)
        for i in range(40):
            trainer.record(
                FeedbackEvent(_vec(f"x {i}"), features_to_vector(None), taste=float(i % 2)),
                autotrain=False,
            )
        trainer.train(10)
        assert trainer.save() is not None

        probe_emb = torch.from_numpy(_vec("probe text")[None, :]).to(trainer.device)
        probe_feat = torch.from_numpy(features_to_vector(None)[None, :]).to(trainer.device)
        before = float(trainer.model.score(probe_emb, probe_feat)["taste"][0])

        revived = Trainer(build_model(DIM), cfg=cfg)
        assert revived.load() is True
        after = float(revived.model.score(probe_emb, probe_feat)["taste"][0])
        assert abs(before - after) < 1e-5, f"predictions drifted across save/load: {before} vs {after}"
        assert revived.total_steps == trainer.total_steps
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def test_load_refuses_checkpoint_from_a_different_embedding_backend():
    tmp = Path(tempfile.mkdtemp())
    try:
        cfg = CortexConfig(data_dir=tmp)
        Trainer(build_model(DIM), cfg=cfg).save()
        mismatched = Trainer(build_model(DIM * 3), cfg=cfg)
        assert mismatched.load() is False, "must refuse a checkpoint of the wrong width"
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def test_operator_taste_is_actually_learned():
    """The claim the whole design rests on: approve/reject changes behaviour.

    Simulates an operator who consistently approves substantive takes and
    rejects hype, then checks the taste head separates held-out examples of
    each -- using only the labels, never the text.
    """
    tmp = Path(tempfile.mkdtemp())
    try:
        torch.manual_seed(0)
        np.random.seed(0)
        cfg = CortexConfig(data_dir=tmp, batch_size=32, lr=1e-3)
        trainer = Trainer(build_model(DIM), cfg=cfg)

        liked = [
            "a concrete benchmark showing the tradeoff",
            "measured latency before and after the change",
            "the failure mode nobody documents",
            "why this approach loses at scale",
            "a reproducible experiment with numbers",
        ]
        disliked = [
            "this changes everything forever",
            "absolutely mind blowing game changer",
            "you wont believe what happens next",
            "the future is here and it is insane",
            "biggest thing since the internet",
        ]

        for _ in range(12):
            for text in liked:
                trainer.record(
                    FeedbackEvent(_vec(text), features_to_vector(None), taste=1.0, source="telegram"),
                    autotrain=False,
                )
            for text in disliked:
                trainer.record(
                    FeedbackEvent(_vec(text), features_to_vector(None), taste=0.0, source="telegram"),
                    autotrain=False,
                )
        trainer.train(200)

        def taste_of(text: str) -> float:
            emb = torch.from_numpy(_vec(text)[None, :]).to(trainer.device)
            feat = torch.from_numpy(features_to_vector(None)[None, :]).to(trainer.device)
            return float(trainer.model.score(emb, feat)["taste"][0])

        held_out_good = taste_of("a measured comparison with the tradeoff documented")
        held_out_bad = taste_of("this insane thing changes everything")
        assert held_out_good > held_out_bad, (
            f"taste head did not generalise: good={held_out_good:.3f} bad={held_out_bad:.3f}"
        )
        return held_out_good, held_out_bad, trainer.total_steps
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def test_context_features_do_not_become_a_shortcut():
    """Regression: the net must read the text, not a correlated side-channel.

    With nn.BatchNorm1d this failed outright. Near-constant context features
    got their noise amplified by the batch standard deviation, and the net
    learned "long post = approved" from `text_len_norm` -- held-out separation
    collapsed from 1.00 to 0.00 while training loss still looked excellent.
    FeatureNorm's variance floor plus feature-block dropout is the fix.

    The training labels here correlate with length by construction, which is
    exactly the trap: the liked examples are longer than the disliked ones.
    """
    tmp = Path(tempfile.mkdtemp())
    try:
        torch.manual_seed(0)
        np.random.seed(0)
        cfg = CortexConfig(data_dir=tmp, batch_size=32, lr=1e-3)
        trainer = Trainer(build_model(DIM), cfg=cfg)

        def feat(text: str) -> np.ndarray:
            return features_to_vector(
                {"novelty": 1.0, "text_len_norm": min(len(text) / 280.0, 2.0)}
            )

        liked = [
            "a concrete benchmark showing the tradeoff",
            "measured latency before and after the change",
            "the failure mode nobody documents",
            "why this approach loses at scale",
            "a reproducible experiment with numbers",
        ]
        disliked = [
            "this changes everything forever",
            "absolutely mind blowing game changer",
            "you wont believe what happens next",
            "the future is here and it is insane",
            "biggest thing since the internet",
        ]
        for _ in range(10):
            for text in liked:
                trainer.record(FeedbackEvent(_vec(text), feat(text), taste=1.0), autotrain=False)
            for text in disliked:
                trainer.record(FeedbackEvent(_vec(text), feat(text), taste=0.0), autotrain=False)
        trainer.train(120)

        def taste_of(text: str) -> float:
            emb = torch.from_numpy(_vec(text)[None, :]).to(trainer.device)
            fts = torch.from_numpy(feat(text)[None, :]).to(trainer.device)
            return float(trainer.model.score(emb, fts)["taste"][0])

        good = taste_of("a measured comparison of the tradeoff with numbers")
        bad = taste_of("this insane thing changes absolutely everything forever")
        assert good - bad > 0.5, (
            f"generalisation collapsed with real features: good={good:.3f} bad={bad:.3f}"
        )

        # Same meaning, 4x the length. Score must be driven by meaning.
        short = taste_of("measured tradeoff")
        long = taste_of("a measured comparison of the tradeoff with numbers " * 4)
        assert abs(short - long) < 0.35, (
            f"length is acting as a shortcut: short={short:.3f} long={long:.3f}"
        )
        return good, bad, abs(short - long)
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def test_feature_norm_does_not_amplify_a_constant_feature():
    """A feature with no spread must contribute ~nothing, not exploded noise."""
    from scema_cortex.model import FeatureNorm

    norm = FeatureNorm(4, var_floor=0.01)
    norm.train()
    # Column 0 is constant, column 1 genuinely varies.
    for _ in range(50):
        batch = torch.stack(
            [
                torch.full((16,), 1.0),
                torch.randn(16) * 3.0,
                torch.zeros(16),
                torch.full((16,), 0.5),
            ],
            dim=1,
        )
        norm(batch)

    norm.eval()
    probe = torch.tensor([[1.0, 0.0, 0.0, 0.5]])
    out = norm(probe)
    assert abs(float(out[0, 0])) < 1.0, f"constant feature amplified to {float(out[0, 0]):.3f}"
    assert abs(float(out[0, 3])) < 1.0, f"constant feature amplified to {float(out[0, 3]):.3f}"

    # And an absurd input must be clamped rather than saturating the trunk.
    wild = norm(torch.tensor([[1.0, 1e6, 0.0, 0.5]]))
    assert abs(float(wild[0, 1])) <= 5.0 + 1e-4, "outlier was not clamped"


def test_scoring_is_deterministic_across_repeated_calls():
    """Dropout must not leak into inference: the same input, the same answer."""
    model = build_model(DIM).to("cuda" if torch.cuda.is_available() else "cpu")
    device = next(model.parameters()).device
    emb = torch.from_numpy(_vec("a stable probe")[None, :]).to(device)
    fts = torch.from_numpy(features_to_vector({"novelty": 0.5})[None, :]).to(device)
    first = float(model.score(emb, fts)["taste"][0])
    for _ in range(5):
        assert float(model.score(emb, fts)["taste"][0]) == first, "scoring is not deterministic"


if __name__ == "__main__":
    import logging

    logging.disable(logging.ERROR)  # the refusal paths log loudly on purpose

    passed = 0
    for name, fn in sorted(globals().items()):
        if not name.startswith("test_") or not callable(fn):
            continue
        result = fn()
        extra = ""
        if name == "test_operator_taste_is_actually_learned" and result:
            good, bad, steps = result
            extra = f"  (held-out good={good:.3f} vs bad={bad:.3f} after {steps} steps)"
        if name == "test_context_features_do_not_become_a_shortcut" and result:
            good, bad, delta = result
            extra = f"  (sep={good - bad:+.3f}, length-shortcut delta={delta:.3f})"
        print(f"  PASS {name}{extra}")
        passed += 1
    print(f"{passed} checks passed")
