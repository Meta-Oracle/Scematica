"""A small neural network in pure Python. No numpy, no framework.

The same position `scematica-nn` takes in Rust, for the same reason: the models here are
tiny — a few thousand parameters over a few thousand samples — and at that size the
dependency costs more than it buys. A user who installs `alchem-link` to check whether an
oracle is stale should not thereby install a tensor library.

What is here is the minimum that trains reliably:

* **He initialisation**, because ReLU halves the variance of every activation and naive
  small-random init makes deep-ish nets start dead.
* **Adam**, because feature scales differ (an interval ratio and a volatility are not
  comparable) and plain SGD needs a per-feature learning rate to cope.
* **Gradient clipping**, because the interval targets are heavy-tailed — one feed that
  went quiet for a day produces a gradient that would otherwise wipe the weights.

Everything is lists of floats. It is slower than numpy by a large constant factor and
entirely fast enough for the data volumes involved.
"""
from __future__ import annotations

import json
import math
import random
from dataclasses import dataclass, field
from typing import Any, Dict, List, Sequence, Tuple

Vector = List[float]
Matrix = List[List[float]]


def he_init(rows: int, cols: int, rng: random.Random) -> Matrix:
    """He normal: std = sqrt(2 / fan_in). The 2 is the ReLU correction."""
    std = math.sqrt(2.0 / max(1, cols))
    return [[rng.gauss(0.0, std) for _ in range(cols)] for _ in range(rows)]


def relu(values: Vector) -> Vector:
    return [v if v > 0.0 else 0.0 for v in values]


def relu_grad(pre: Vector) -> Vector:
    return [1.0 if v > 0.0 else 0.0 for v in pre]


def matvec(matrix: Matrix, vector: Vector, biases: Vector) -> Vector:
    return [
        bias + sum(w * v for w, v in zip(row, vector))
        for row, bias in zip(matrix, biases)
    ]


@dataclass
class Layer:
    """One fully connected layer, with its Adam moments."""
    weights: Matrix
    biases: Vector
    m_w: Matrix = field(default_factory=list)
    v_w: Matrix = field(default_factory=list)
    m_b: Vector = field(default_factory=list)
    v_b: Vector = field(default_factory=list)

    @classmethod
    def new(cls, in_size: int, out_size: int, rng: random.Random) -> "Layer":
        return cls(
            weights=he_init(out_size, in_size, rng),
            biases=[0.0] * out_size,
            m_w=[[0.0] * in_size for _ in range(out_size)],
            v_w=[[0.0] * in_size for _ in range(out_size)],
            m_b=[0.0] * out_size,
            v_b=[0.0] * out_size,
        )

    @property
    def in_size(self) -> int:
        return len(self.weights[0]) if self.weights else 0

    @property
    def out_size(self) -> int:
        return len(self.weights)

    def forward(self, x: Vector) -> Vector:
        return matvec(self.weights, x, self.biases)

    def as_dict(self) -> Dict[str, Any]:
        return {"weights": self.weights, "biases": self.biases}

    @classmethod
    def from_dict(cls, payload: Dict[str, Any]) -> "Layer":
        weights = [list(map(float, row)) for row in payload["weights"]]
        biases = list(map(float, payload["biases"]))
        in_size = len(weights[0]) if weights else 0
        return cls(
            weights=weights,
            biases=biases,
            m_w=[[0.0] * in_size for _ in weights],
            v_w=[[0.0] * in_size for _ in weights],
            m_b=[0.0] * len(biases),
            v_b=[0.0] * len(biases),
        )


class MLP:
    """A ReLU multilayer perceptron with a linear output layer.

    The output stays linear because both heads predict unbounded quantities — a
    normalised interval and a reconstruction — and a ReLU on the output would clamp
    every negative prediction to zero, which is a floor the model cannot then learn
    around.
    """

    def __init__(self, sizes: Sequence[int], seed: int = 7) -> None:
        if len(sizes) < 2:
            raise ValueError("an MLP needs at least an input and an output size")
        rng = random.Random(seed)
        self.sizes = list(sizes)
        self.layers = [
            Layer.new(sizes[i], sizes[i + 1], rng) for i in range(len(sizes) - 1)
        ]
        self.steps = 0

    # ── inference ────────────────────────────────────────────────────────────

    def forward(self, x: Vector) -> Vector:
        activation = list(x)
        last = len(self.layers) - 1
        for index, layer in enumerate(self.layers):
            pre = layer.forward(activation)
            activation = pre if index == last else relu(pre)
        return activation

    def _forward_cache(self, x: Vector) -> Tuple[Vector, List[Vector], List[Vector]]:
        pre_acts: List[Vector] = []
        post_acts: List[Vector] = [list(x)]
        activation = list(x)
        last = len(self.layers) - 1
        for index, layer in enumerate(self.layers):
            pre = layer.forward(activation)
            pre_acts.append(pre)
            activation = pre if index == last else relu(pre)
            post_acts.append(activation)
        return activation, pre_acts, post_acts

    # ── training ─────────────────────────────────────────────────────────────

    def train_batch(
        self,
        batch: Sequence[Tuple[Vector, Vector]],
        lr: float = 1e-3,
        beta1: float = 0.9,
        beta2: float = 0.999,
        eps: float = 1e-8,
        clip: float = 5.0,
    ) -> float:
        """One Adam step over a batch. Returns mean squared error."""
        if not batch:
            return 0.0

        grads_w = [
            [[0.0] * layer.in_size for _ in range(layer.out_size)] for layer in self.layers
        ]
        grads_b = [[0.0] * layer.out_size for layer in self.layers]
        total_loss = 0.0

        for x, target in batch:
            output, pre_acts, post_acts = self._forward_cache(x)
            # dL/dy for mean squared error, averaged over the output dimension.
            delta = [
                2.0 * (out - want) / len(output) for out, want in zip(output, target)
            ]
            total_loss += sum((out - want) ** 2 for out, want in zip(output, target)) / len(output)

            for index in reversed(range(len(self.layers))):
                layer = self.layers[index]
                incoming = post_acts[index]
                for j in range(layer.out_size):
                    grads_b[index][j] += delta[j]
                    row = grads_w[index][j]
                    dj = delta[j]
                    for k in range(layer.in_size):
                        row[k] += dj * incoming[k]

                if index > 0:
                    upstream = [0.0] * layer.in_size
                    for j in range(layer.out_size):
                        dj = delta[j]
                        if dj == 0.0:
                            continue
                        weights = layer.weights[j]
                        for k in range(layer.in_size):
                            upstream[k] += dj * weights[k]
                    gate = relu_grad(pre_acts[index - 1])
                    delta = [u * g for u, g in zip(upstream, gate)]

        scale = 1.0 / len(batch)
        self.steps += 1
        bias1 = 1.0 - beta1 ** self.steps
        bias2 = 1.0 - beta2 ** self.steps

        for index, layer in enumerate(self.layers):
            for j in range(layer.out_size):
                # Interval targets are heavy-tailed: one feed that went quiet for a day
                # yields a gradient orders of magnitude above the rest. Clipping keeps a
                # single outlier from erasing what the other samples taught.
                gb = max(-clip, min(clip, grads_b[index][j] * scale))
                layer.m_b[j] = beta1 * layer.m_b[j] + (1 - beta1) * gb
                layer.v_b[j] = beta2 * layer.v_b[j] + (1 - beta2) * gb * gb
                layer.biases[j] -= lr * (layer.m_b[j] / bias1) / (
                    math.sqrt(layer.v_b[j] / bias2) + eps
                )

                weights = layer.weights[j]
                m_row = layer.m_w[j]
                v_row = layer.v_w[j]
                g_row = grads_w[index][j]
                for k in range(layer.in_size):
                    gw = max(-clip, min(clip, g_row[k] * scale))
                    m_row[k] = beta1 * m_row[k] + (1 - beta1) * gw
                    v_row[k] = beta2 * v_row[k] + (1 - beta2) * gw * gw
                    weights[k] -= lr * (m_row[k] / bias1) / (
                        math.sqrt(v_row[k] / bias2) + eps
                    )

        return total_loss / len(batch)

    # ── persistence ──────────────────────────────────────────────────────────

    def as_dict(self) -> Dict[str, Any]:
        return {
            "sizes": self.sizes,
            "steps": self.steps,
            "layers": [layer.as_dict() for layer in self.layers],
        }

    @classmethod
    def from_dict(cls, payload: Dict[str, Any]) -> "MLP":
        net = cls(payload["sizes"])
        net.layers = [Layer.from_dict(entry) for entry in payload["layers"]]
        net.steps = int(payload.get("steps", 0))
        return net

    def to_json(self) -> str:
        return json.dumps(self.as_dict())

    @classmethod
    def from_json(cls, raw: str) -> "MLP":
        return cls.from_dict(json.loads(raw))

    @property
    def parameters(self) -> int:
        return sum(layer.in_size * layer.out_size + layer.out_size for layer in self.layers)
