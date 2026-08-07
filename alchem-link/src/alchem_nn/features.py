"""Turn a feed's round history into training data — labelled, for free, by the chain.

The useful property of oracle round history is that it is **self-supervised**. Every
round carries the timestamp of the next one, so "how long until this feed publishes
again?" comes with a ground-truth answer for every historical round, at no labelling
cost. A few hundred `getRoundData` calls yield a few hundred labelled examples.

That is what makes a model here defensible rather than decorative: it is not scoring
something unmeasurable, it is predicting a quantity the chain will confirm.

Features are computed from a trailing window and are all scale-free — ratios against the
feed's own observed cadence rather than absolute seconds. A model trained on Ethereum
(3600s heartbeat) then transfers to Polygon (60s) without retraining, because "we are 40%
of the way through the usual gap" means the same thing on both.

**No feature may look forward.** Every value at round *i* is computed from rounds ≤ *i*
only. Leaking the next interval into the features would produce a model that scores
beautifully offline and predicts nothing live, which is the standard way a time-series
model turns out to be worthless.
"""
from __future__ import annotations

import math
import statistics
from dataclasses import dataclass
from typing import List, Sequence, Tuple

#: Trailing rounds each feature vector summarises.
WINDOW = 6

#: Feature vector width. Asserted in tests against `FEATURE_NAMES`.
FEATURE_DIM = 12

FEATURE_NAMES = (
    "interval_ratio_1",      # most recent interval / window median
    "interval_ratio_2",
    "interval_ratio_3",
    "interval_mean_ratio",   # window mean / window max
    "interval_cv",           # coefficient of variation over the window
    "move_1",                # most recent |price change| scaled
    "move_2",
    "move_mean",
    "move_max",
    "deviation_fraction",    # share of window intervals that beat the ceiling
    "log_median_interval",   # the feed's own timescale, log-compressed
    "trend",                 # are intervals lengthening or shortening
)

#: Percentage moves are compressed by this before scaling — typical deviation
#: thresholds sit near 0.05–0.5%, so a linear scale would push every feature to ~0.
_MOVE_SCALE = 2.0


@dataclass
class Sample:
    """One training example: features, and the interval that actually followed."""
    features: List[float]
    #: Seconds until the next publish, normalised by the window median.
    target: float
    #: The raw interval, kept so the baseline can be scored in real units.
    actual_interval: int
    median_interval: float


def _safe_div(numerator: float, denominator: float, default: float = 0.0) -> float:
    return numerator / denominator if denominator else default


def _clamp01(value: float) -> float:
    return 0.0 if value < 0.0 else (1.0 if value > 1.0 else value)


def build_features(intervals: Sequence[int], moves: Sequence[float]) -> List[float]:
    """Feature vector from a trailing window, newest last.

    ``intervals[i]`` is the gap that ended at round ``i``; ``moves[i]`` is the percentage
    price change across it.
    """
    if not intervals:
        return [0.0] * FEATURE_DIM

    window = list(intervals[-WINDOW:])
    window_moves = list(moves[-WINDOW:])

    median = statistics.median(window) if window else 1.0
    median = median if median > 0 else 1.0
    ceiling = max(window) if window else 1.0
    mean = statistics.mean(window)

    # Ratios against the feed's own median, halved so a typical interval lands near 0.5
    # and a doubled one near 1.0 rather than saturating the clamp.
    recent = [_clamp01(_safe_div(value, median) / 2.0) for value in reversed(window[-3:])]
    while len(recent) < 3:
        recent.append(0.5)

    cv = _safe_div(statistics.pstdev(window), mean) if len(window) > 1 else 0.0

    scaled_moves = [_clamp01(abs(m) / _MOVE_SCALE) for m in reversed(window_moves[-2:])]
    while len(scaled_moves) < 2:
        scaled_moves.append(0.0)

    move_mean = _clamp01(
        _safe_div(sum(abs(m) for m in window_moves), len(window_moves)) / _MOVE_SCALE
    ) if window_moves else 0.0
    move_max = _clamp01(max((abs(m) for m in window_moves), default=0.0) / _MOVE_SCALE)

    # An interval well inside the window's ceiling was triggered by a price move rather
    # than the clock — the same split `alchem_link.cadence` uses.
    deviation_fraction = (
        sum(1 for value in window if value < ceiling * 0.9) / len(window) if window else 0.0
    )

    # log10 seconds, mapped so 1s → 0 and ~28h → 1. Gives the model the feed's absolute
    # timescale without letting a 86400s feed dominate a 60s one numerically.
    log_median = _clamp01(math.log10(max(median, 1.0)) / 5.0)

    if len(window) >= 4:
        half = len(window) // 2
        earlier = statistics.mean(window[:half])
        later = statistics.mean(window[half:])
        trend = _clamp01(_safe_div(later, earlier, 1.0) / 2.0)
    else:
        trend = 0.5

    return [
        recent[0], recent[1], recent[2],
        _clamp01(_safe_div(mean, ceiling)),
        _clamp01(cv),
        scaled_moves[0], scaled_moves[1],
        move_mean, move_max,
        deviation_fraction,
        log_median,
        trend,
    ]


def rounds_to_series(rounds: Sequence) -> Tuple[List[int], List[float]]:
    """Convert `alchem_link.aggregator.Round` objects into intervals and % moves.

    Accepts them in any order and sorts by timestamp; non-positive gaps (duplicate or
    non-monotonic timestamps) are dropped rather than fed in as zeros.
    """
    ordered = sorted(rounds, key=lambda r: r.updated_at)
    intervals: List[int] = []
    moves: List[float] = []
    for previous, following in zip(ordered, ordered[1:]):
        gap = following.updated_at - previous.updated_at
        if gap <= 0:
            continue
        intervals.append(gap)
        moves.append(
            (following.price - previous.price) / abs(previous.price) * 100.0
            if previous.price else 0.0
        )
    return intervals, moves


def build_samples(intervals: Sequence[int], moves: Sequence[float]) -> List[Sample]:
    """Every position with enough history behind it and a known interval ahead.

    The window ends at ``index``; the label is the interval that came *next*, which the
    features have not seen.
    """
    samples: List[Sample] = []
    for index in range(WINDOW, len(intervals)):
        history = intervals[:index]
        history_moves = moves[:index]
        window = history[-WINDOW:]
        median = statistics.median(window) if window else 1.0
        median = median if median > 0 else 1.0

        actual = intervals[index]
        # Target is the ratio to the window median, halved and clamped — the same scale
        # the recent-interval features use, so the model predicts in units it can see.
        target = _clamp01(_safe_div(actual, median) / 2.0)
        samples.append(Sample(
            features=build_features(history, history_moves),
            target=target,
            actual_interval=actual,
            median_interval=median,
        ))
    return samples


def denormalise(prediction: float, median_interval: float) -> float:
    """Turn a model output back into seconds."""
    return _clamp01(prediction) * 2.0 * median_interval
