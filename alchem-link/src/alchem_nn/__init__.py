"""alchem-nn — a neural layer for Alchem-Link, trained on what the chain already tells you.

Two models over one idea: a feed's round history is **self-labelling**. Every historical
round records when the next one arrived, so "when will this publish again?" comes with
ground truth for free, from data the toolkit already fetches.

* :class:`CadenceModel` — predicts seconds to the next publish. Sharpens `watch`'s poll
  interval and turns "300s old" into "overdue", which is a different statement.
* :class:`AnomalyModel` — an autoencoder over the same features. Learns a feed's normal
  rhythm and flags it when it stops behaving like itself, without anyone having to
  specify in advance what wrong looks like.

Pure standard library, no numpy — the same position `scematica-nn` takes in Rust, and for
the same reason: at a few thousand parameters the dependency costs more than it buys, and
someone installing this to check an oracle should not thereby install a tensor library.

**Every report carries the trivial baseline.** "Predict the window median" is what anyone
would do without a model, and on a feed whose cadence is dominated by a fixed heartbeat it
is genuinely hard to beat. :meth:`CadenceModel.evaluate` scores both and says which won.
When the model loses, that is the output.
"""

__version__ = "0.1.0"

from .features import (
    FEATURE_DIM,
    FEATURE_NAMES,
    WINDOW,
    Sample,
    build_features,
    build_samples,
    denormalise,
    rounds_to_series,
)
from .model import (
    DEFAULT_ANOMALY_CHECKPOINT,
    DEFAULT_CHECKPOINT,
    AnomalyModel,
    CadenceModel,
    Evaluation,
    TrainingReport,
    samples_from_rounds,
)
from .net import MLP, Layer, he_init, relu

__all__ = [
    "__version__",
    "CadenceModel",
    "AnomalyModel",
    "Evaluation",
    "TrainingReport",
    "samples_from_rounds",
    "DEFAULT_CHECKPOINT",
    "DEFAULT_ANOMALY_CHECKPOINT",
    "Sample",
    "FEATURE_DIM",
    "FEATURE_NAMES",
    "WINDOW",
    "build_features",
    "build_samples",
    "rounds_to_series",
    "denormalise",
    "MLP",
    "Layer",
    "he_init",
    "relu",
]
