"""The two models, and the baseline that decides whether they are worth anything.

**CadenceModel** predicts how long until a feed publishes again. Useful concretely: it
sets `watch`'s poll interval, and it turns "this feed is 300s old" into "this feed is
overdue", which are different statements.

**AnomalyModel** is an autoencoder over the same features. It learns to reconstruct a
feed's normal publishing rhythm; when reconstruction error spikes, the feed is behaving
unlike its own history. Cadence shifts precede migrations and incidents, and this catches
them without anyone specifying in advance what "wrong" looks like.

# The baseline rule

Every report here carries the score of a trivial predictor — "the next interval will be
the window median" — alongside the model's. That is not modesty, it is the only way the
number means anything. A model with 12% error sounds good until the constant baseline
scores 11%, at which point the network is a slower way of being worse.

:meth:`CadenceModel.evaluate` returns both and a `beats_baseline` flag, and every caller
reports it. If the model loses, the honest output is that it lost.
"""
from __future__ import annotations

import json
import math
import random
import statistics
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Dict, List, Optional, Sequence, Tuple

from .features import (
    FEATURE_DIM,
    WINDOW,
    Sample,
    build_features,
    build_samples,
    denormalise,
    rounds_to_series,
)
from .net import MLP

#: Where checkpoints go by default, following the project's file-per-artifact convention.
DEFAULT_CHECKPOINT = "alchem-nn-cadence.json"
DEFAULT_ANOMALY_CHECKPOINT = "alchem-nn-anomaly.json"

#: Hidden layer widths. Small on purpose: a few hundred samples per feed cannot support
#: a wide network, and an over-parameterised model memorises the window instead of
#: learning the rhythm.
CADENCE_HIDDEN = (24, 12)
#: Autoencoder bottleneck. Narrower than the input by design — a bottleneck wide enough
#: to copy the input learns the identity function and flags nothing.
ANOMALY_HIDDEN = (10, 4, 10)


@dataclass
class Evaluation:
    """Model score against the trivial baseline, in real units."""
    samples: int
    model_mae_secs: float
    baseline_mae_secs: float
    model_mae_norm: float
    baseline_mae_norm: float

    @property
    def beats_baseline(self) -> bool:
        return self.model_mae_secs < self.baseline_mae_secs

    @property
    def improvement_pct(self) -> float:
        if not self.baseline_mae_secs:
            return 0.0
        return (self.baseline_mae_secs - self.model_mae_secs) / self.baseline_mae_secs * 100.0

    @property
    def verdict(self) -> str:
        if self.samples < 30:
            return "insufficient data"
        if not self.beats_baseline:
            return "loses to the median baseline"
        if self.improvement_pct < 5.0:
            return "marginally better than the median baseline"
        return "beats the median baseline"

    def as_dict(self) -> Dict[str, Any]:
        return {
            "samples": self.samples,
            "model_mae_secs": round(self.model_mae_secs, 2),
            "baseline_mae_secs": round(self.baseline_mae_secs, 2),
            "improvement_pct": round(self.improvement_pct, 2),
            "beats_baseline": self.beats_baseline,
            "verdict": self.verdict,
        }


@dataclass
class TrainingReport:
    epochs: int
    train_samples: int
    test_samples: int
    final_loss: float
    evaluation: Evaluation
    feeds: List[str] = field(default_factory=list)

    def as_dict(self) -> Dict[str, Any]:
        return {
            "epochs": self.epochs,
            "train_samples": self.train_samples,
            "test_samples": self.test_samples,
            "final_loss": round(self.final_loss, 6),
            "feeds": self.feeds,
            "evaluation": self.evaluation.as_dict(),
        }


class CadenceModel:
    """Predicts the seconds until a feed's next publish."""

    def __init__(self, seed: int = 11) -> None:
        self.net = MLP([FEATURE_DIM, *CADENCE_HIDDEN, 1], seed=seed)
        self.trained_on: List[str] = []

    # ── training ─────────────────────────────────────────────────────────────

    def fit(
        self,
        samples: Sequence[Sample],
        epochs: int = 60,
        batch_size: int = 16,
        lr: float = 3e-3,
        test_fraction: float = 0.25,
        seed: int = 3,
        feeds: Optional[List[str]] = None,
        test_samples: Optional[Sequence[Sample]] = None,
    ) -> TrainingReport:
        """Train, holding out a **chronological** tail per feed for evaluation.

        The split is by time, not shuffled. Shuffling a time series lets the model train
        on rounds that came after the ones it is tested on, which inflates the score and
        says nothing about predicting the future.

        When ``test_samples`` is given, ``samples`` is taken as the training set and no
        split happens here. That matters for pooled training: samples from several feeds
        arrive concatenated, each feed's block contiguous, so slicing the tail of the
        pooled list splits *by feed*, not by time — it trains on Ethereum's hourly feeds
        and tests on its daily ones, and reports the resulting mismatch as a modelling
        failure. Callers pooling feeds must split each feed separately and pass both
        halves. :func:`split_by_feed` does that.
        """
        if test_samples is None:
            if len(samples) < 40:
                raise ValueError(
                    f"need at least 40 samples to train and hold out a test set, "
                    f"got {len(samples)}"
                )
            split = int(len(samples) * (1.0 - test_fraction))
            train, test = list(samples[:split]), list(samples[split:])
        else:
            train, test = list(samples), list(test_samples)
            if len(train) < 20:
                raise ValueError(f"need at least 20 training samples, got {len(train)}")

        rng = random.Random(seed)
        loss = 0.0
        for _ in range(epochs):
            order = list(range(len(train)))
            rng.shuffle(order)
            for start in range(0, len(order), batch_size):
                batch = [
                    (train[i].features, [train[i].target])
                    for i in order[start:start + batch_size]
                ]
                loss = self.net.train_batch(batch, lr=lr)

        self.trained_on = feeds or []
        return TrainingReport(
            epochs=epochs,
            train_samples=len(train),
            test_samples=len(test),
            final_loss=loss,
            evaluation=self.evaluate(test),
            feeds=self.trained_on,
        )

    def evaluate(self, samples: Sequence[Sample]) -> Evaluation:
        """Score the model and the median baseline on the same samples, in seconds."""
        if not samples:
            return Evaluation(0, 0.0, 0.0, 0.0, 0.0)

        model_errors: List[float] = []
        baseline_errors: List[float] = []
        model_norm: List[float] = []
        baseline_norm: List[float] = []

        for sample in samples:
            predicted = self.predict_normalised(sample.features)
            predicted_secs = denormalise(predicted, sample.median_interval)
            # The baseline: "the next interval will be the window median". It is the
            # thing anyone would do without a model, and it is hard to beat on a feed
            # whose heartbeat dominates its cadence.
            baseline_secs = sample.median_interval

            model_errors.append(abs(predicted_secs - sample.actual_interval))
            baseline_errors.append(abs(baseline_secs - sample.actual_interval))
            model_norm.append(abs(predicted - sample.target))
            baseline_norm.append(abs(0.5 - sample.target))

        return Evaluation(
            samples=len(samples),
            model_mae_secs=statistics.mean(model_errors),
            baseline_mae_secs=statistics.mean(baseline_errors),
            model_mae_norm=statistics.mean(model_norm),
            baseline_mae_norm=statistics.mean(baseline_norm),
        )

    # ── inference ────────────────────────────────────────────────────────────

    def predict_normalised(self, features: Sequence[float]) -> float:
        return self.net.forward(list(features))[0]

    def predict_seconds(self, intervals: Sequence[int], moves: Sequence[float]) -> float:
        """Predicted seconds to the next publish, from a live history window."""
        window = list(intervals[-WINDOW:])
        median = statistics.median(window) if window else 1.0
        median = median if median > 0 else 1.0
        return denormalise(self.predict_normalised(build_features(intervals, moves)), median)

    # ── persistence ──────────────────────────────────────────────────────────

    def save(self, path: str = DEFAULT_CHECKPOINT) -> None:
        payload = {
            "kind": "alchem-nn-cadence",
            "feature_dim": FEATURE_DIM,
            "window": WINDOW,
            "trained_on": self.trained_on,
            "net": self.net.as_dict(),
        }
        # Same atomic-rename convention as the rest of the project.
        tmp = f"{path}.tmp"
        Path(tmp).write_text(json.dumps(payload), encoding="utf-8")
        Path(tmp).replace(path)

    @classmethod
    def load(cls, path: str = DEFAULT_CHECKPOINT) -> "CadenceModel":
        payload = json.loads(Path(path).read_text(encoding="utf-8"))
        if payload.get("feature_dim") != FEATURE_DIM:
            raise ValueError(
                f"checkpoint was trained on {payload.get('feature_dim')} features, "
                f"this build uses {FEATURE_DIM} — retrain rather than loading it"
            )
        model = cls()
        model.net = MLP.from_dict(payload["net"])
        model.trained_on = list(payload.get("trained_on", []))
        return model


class AnomalyModel:
    """Autoencoder over cadence features; reconstruction error is the anomaly score."""

    def __init__(self, seed: int = 13) -> None:
        self.net = MLP([FEATURE_DIM, *ANOMALY_HIDDEN, FEATURE_DIM], seed=seed)
        #: Reconstruction errors seen in training, for percentile scoring.
        self.error_baseline: List[float] = []
        self.trained_on: List[str] = []

    def fit(
        self,
        samples: Sequence[Sample],
        epochs: int = 80,
        batch_size: int = 16,
        lr: float = 3e-3,
        seed: int = 5,
        feeds: Optional[List[str]] = None,
    ) -> Dict[str, Any]:
        if len(samples) < 20:
            raise ValueError(f"need at least 20 samples, got {len(samples)}")

        rng = random.Random(seed)
        loss = 0.0
        indices = list(range(len(samples)))
        for _ in range(epochs):
            rng.shuffle(indices)
            for start in range(0, len(indices), batch_size):
                batch = [
                    (samples[i].features, samples[i].features)
                    for i in indices[start:start + batch_size]
                ]
                loss = self.net.train_batch(batch, lr=lr)

        self.error_baseline = sorted(self.reconstruction_error(s.features) for s in samples)
        self.trained_on = feeds or []
        return {
            "epochs": epochs,
            "samples": len(samples),
            "final_loss": round(loss, 6),
            "median_error": round(statistics.median(self.error_baseline), 6),
            "p95_error": round(self._percentile_value(0.95), 6),
            "feeds": self.trained_on,
        }

    def reconstruction_error(self, features: Sequence[float]) -> float:
        rebuilt = self.net.forward(list(features))
        return math.sqrt(
            sum((a - b) ** 2 for a, b in zip(rebuilt, features)) / max(1, len(features))
        )

    def _percentile_value(self, fraction: float) -> float:
        if not self.error_baseline:
            return 0.0
        index = min(len(self.error_baseline) - 1, int(fraction * len(self.error_baseline)))
        return self.error_baseline[index]

    def score(self, features: Sequence[float]) -> Dict[str, Any]:
        """Anomaly score as a percentile against the errors seen in training.

        A percentile rather than a raw error, because a raw reconstruction error is
        uninterpretable without knowing the model's typical error — 0.03 could be
        perfectly normal or wildly out.
        """
        error = self.reconstruction_error(features)
        if not self.error_baseline:
            return {"error": error, "percentile": None, "anomalous": False,
                    "detail": "model has no baseline — train it first"}

        below = sum(1 for value in self.error_baseline if value < error)
        percentile = below / len(self.error_baseline) * 100.0
        anomalous = percentile >= 95.0
        return {
            "error": round(error, 6),
            "percentile": round(percentile, 2),
            "anomalous": anomalous,
            "detail": (
                f"reconstruction error is above {percentile:.0f}% of what this model saw "
                "in training" if anomalous else
                f"cadence looks normal ({percentile:.0f}th percentile)"
            ),
        }

    def save(self, path: str = DEFAULT_ANOMALY_CHECKPOINT) -> None:
        payload = {
            "kind": "alchem-nn-anomaly",
            "feature_dim": FEATURE_DIM,
            "trained_on": self.trained_on,
            "error_baseline": self.error_baseline,
            "net": self.net.as_dict(),
        }
        tmp = f"{path}.tmp"
        Path(tmp).write_text(json.dumps(payload), encoding="utf-8")
        Path(tmp).replace(path)

    @classmethod
    def load(cls, path: str = DEFAULT_ANOMALY_CHECKPOINT) -> "AnomalyModel":
        payload = json.loads(Path(path).read_text(encoding="utf-8"))
        model = cls()
        model.net = MLP.from_dict(payload["net"])
        model.error_baseline = list(payload.get("error_baseline", []))
        model.trained_on = list(payload.get("trained_on", []))
        return model


def samples_from_rounds(rounds: Sequence) -> List[Sample]:
    """Round history → training samples."""
    intervals, moves = rounds_to_series(rounds)
    return build_samples(intervals, moves)


def split_by_feed(
    grouped: Sequence[Tuple[str, List[Sample]]],
    test_fraction: float = 0.25,
) -> Tuple[List[Sample], List[Sample]]:
    """Split each feed's samples chronologically, then pool the halves.

    Pooling first and slicing afterwards looks equivalent and is not: each feed's samples
    sit contiguously in the pooled list, so a tail slice hands the model an entirely
    different set of feeds to be tested on than it trained on. Measured on Ethereum that
    turned a fair evaluation into a cross-feed transfer test and cost 18% of the score.

    Splitting per feed asks the question actually intended — given this feed's past, can
    the model predict this feed's future.
    """
    train: List[Sample] = []
    test: List[Sample] = []
    for _name, samples in grouped:
        if len(samples) < 8:
            train.extend(samples)
            continue
        cut = int(len(samples) * (1.0 - test_fraction))
        train.extend(samples[:cut])
        test.extend(samples[cut:])
    return train, test
