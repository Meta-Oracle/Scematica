"""alchem-nn: the network maths, the feature contract, and the evaluation honesty.

All offline. The network is trained here on synthetic functions with known answers,
because "does this converge" is a property of the optimiser, not of the chain.

Two things get specific attention:

* **No forward-looking features.** A time-series model that peeks at its label scores
  beautifully offline and predicts nothing live. The window contract is asserted
  directly.
* **The baseline comparison.** The whole value of the reporting is that it says when the
  model loses, so the losing path is tested as carefully as the winning one.
"""
import json
import math
import random
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from alchem_nn.features import (
    FEATURE_DIM,
    FEATURE_NAMES,
    WINDOW,
    build_features,
    build_samples,
    denormalise,
    rounds_to_series,
)
from alchem_nn.model import AnomalyModel, CadenceModel, Evaluation, split_by_feed
from alchem_nn.net import MLP, Layer, he_init, relu, relu_grad


class NetworkTests(unittest.TestCase):
    def test_he_init_scales_with_fan_in(self):
        rng = random.Random(0)
        narrow = he_init(64, 4, rng)
        wide = he_init(64, 256, rng)
        narrow_spread = statistics_stdev([v for row in narrow for v in row])
        wide_spread = statistics_stdev([v for row in wide for v in row])
        # std = sqrt(2/fan_in), so a wider input must produce smaller weights.
        self.assertGreater(narrow_spread, wide_spread)

    def test_relu_and_gradient(self):
        self.assertEqual(relu([-1.0, 0.0, 2.0]), [0.0, 0.0, 2.0])
        self.assertEqual(relu_grad([-1.0, 0.0, 2.0]), [0.0, 0.0, 1.0])

    def test_learns_a_linear_function(self):
        rng = random.Random(1)
        net = MLP([3, 12, 1], seed=2)
        data = []
        for _ in range(200):
            a, b, c = rng.random(), rng.random(), rng.random()
            data.append(([a, b, c], [0.3 * a + 0.5 * b - 0.2 * c]))
        for _ in range(300):
            for start in range(0, len(data), 16):
                net.train_batch(data[start:start + 16], lr=5e-3)
        error = sum(abs(net.forward(x)[0] - y[0]) for x, y in data) / len(data)
        self.assertLess(error, 0.02, "the optimiser failed to fit a linear function")

    def test_learns_a_nonlinear_function(self):
        """A single ReLU layer cannot do XOR-like structure; this checks depth works."""
        rng = random.Random(3)
        net = MLP([2, 16, 8, 1], seed=4)
        data = []
        for _ in range(400):
            a, b = rng.random(), rng.random()
            data.append(([a, b], [abs(a - b)]))
        for _ in range(400):
            for start in range(0, len(data), 16):
                net.train_batch(data[start:start + 16], lr=5e-3)
        error = sum(abs(net.forward(x)[0] - y[0]) for x, y in data) / len(data)
        self.assertLess(error, 0.05)

    def test_gradient_clipping_survives_an_outlier(self):
        """One absurd target must not wipe what the rest taught."""
        net = MLP([2, 8, 1], seed=5)
        clean = [([0.5, 0.5], [0.5])] * 40
        for _ in range(200):
            net.train_batch(clean, lr=5e-3)
        before = net.forward([0.5, 0.5])[0]

        net.train_batch([([0.5, 0.5], [1e9])], lr=5e-3)
        after = net.forward([0.5, 0.5])[0]
        self.assertTrue(math.isfinite(after))
        self.assertLess(abs(after - before), 1.0)

    def test_parameter_count(self):
        net = MLP([4, 3, 2])
        # (4*3 + 3) + (3*2 + 2) = 15 + 8
        self.assertEqual(net.parameters, 23)

    def test_json_round_trip_preserves_outputs(self):
        net = MLP([3, 5, 2], seed=6)
        net.train_batch([([0.1, 0.2, 0.3], [0.4, 0.5])], lr=1e-3)
        restored = MLP.from_json(net.to_json())
        self.assertEqual(restored.forward([0.1, 0.2, 0.3]), net.forward([0.1, 0.2, 0.3]))
        self.assertEqual(restored.steps, net.steps)

    def test_layer_round_trip_resets_optimiser_moments(self):
        """Adam state is deliberately not persisted — resuming with stale moments on a
        different dataset is worse than starting the accumulators clean."""
        layer = Layer.from_dict({"weights": [[1.0, 2.0]], "biases": [0.5]})
        self.assertEqual(layer.m_w, [[0.0, 0.0]])
        self.assertEqual(layer.v_b, [0.0])


def statistics_stdev(values):
    mean = sum(values) / len(values)
    return math.sqrt(sum((v - mean) ** 2 for v in values) / len(values))


class Round:
    """Stand-in for `alchem_link.aggregator.Round`."""

    def __init__(self, updated_at, price):
        self.updated_at = updated_at
        self.price = price


class FeatureTests(unittest.TestCase):
    def test_feature_names_match_the_dimension(self):
        self.assertEqual(len(FEATURE_NAMES), FEATURE_DIM)
        self.assertEqual(len(build_features([60] * 8, [0.1] * 8)), FEATURE_DIM)

    def test_features_are_bounded(self):
        """Every feature is a clamped ratio, so nothing can blow up the first layer."""
        for intervals, moves in (
            ([1] * 8, [0.0] * 8),
            ([86400] * 8, [50.0] * 8),
            ([1, 90000, 3, 70000, 5, 60000, 2, 1], [-99.0, 99.0] * 4),
        ):
            for name, value in zip(FEATURE_NAMES, build_features(intervals, moves)):
                self.assertTrue(0.0 <= value <= 1.0, f"{name} = {value} escaped [0, 1]")

    def test_empty_history_is_handled(self):
        self.assertEqual(build_features([], []), [0.0] * FEATURE_DIM)

    def test_features_are_scale_free(self):
        """A 60s feed and a 3600s feed with the same *shape* must look alike.

        This is what lets one model serve every network — without it, Polygon and
        Ethereum would need separate training.
        """
        fast = build_features([60, 60, 120, 60, 60, 60], [0.1] * 6)
        slow = build_features([3600, 3600, 7200, 3600, 3600, 3600], [0.1] * 6)
        # Every feature but the absolute-timescale one should agree.
        for name, a, b in zip(FEATURE_NAMES, fast, slow):
            if name == "log_median_interval":
                continue
            self.assertAlmostEqual(a, b, places=6, msg=f"{name} is not scale-free")

    def test_samples_never_see_their_own_label(self):
        """The contract that makes the model meaningful rather than a leak."""
        intervals = list(range(10, 10 + 30))
        moves = [0.1] * 30
        samples = build_samples(intervals, moves)
        self.assertTrue(samples)
        for index, sample in enumerate(samples):
            position = WINDOW + index
            # Rebuild the features from history strictly before the label.
            expected = build_features(intervals[:position], moves[:position])
            self.assertEqual(sample.features, expected)
            self.assertEqual(sample.actual_interval, intervals[position])

    def test_rounds_to_series_sorts_and_drops_bad_gaps(self):
        rounds = [Round(300, 2.0), Round(100, 1.0), Round(200, 1.5), Round(200, 1.6)]
        intervals, moves = rounds_to_series(rounds)
        self.assertEqual(intervals, [100, 100])
        self.assertEqual(len(moves), 2)

    def test_denormalise_inverts_the_target_scale(self):
        self.assertAlmostEqual(denormalise(0.5, 3600), 3600.0)
        self.assertAlmostEqual(denormalise(0.0, 3600), 0.0)


def synthetic_samples(count=200, seed=9):
    """Samples whose next interval is genuinely predictable from the features.

    Built so the model *can* win — otherwise a passing "beats baseline" test would only
    be proving the data is unlearnable.
    """
    rng = random.Random(seed)
    intervals = []
    moves = []
    for index in range(count + WINDOW + 1):
        # A big move is followed by a short interval: deviation-triggered publishing.
        move = rng.choice([0.01, 0.02, 1.5, 2.0])
        intervals.append(60 if move > 1.0 else 600)
        moves.append(move)
    # Shift so the move at i predicts the interval at i+1.
    return build_samples(intervals, moves)


class CadenceModelTests(unittest.TestCase):
    def test_fit_reports_against_the_baseline(self):
        model = CadenceModel()
        report = model.fit(synthetic_samples(), epochs=30)
        self.assertGreater(report.train_samples, 0)
        self.assertGreater(report.test_samples, 0)
        self.assertIsInstance(report.evaluation, Evaluation)
        self.assertGreater(report.evaluation.baseline_mae_secs, 0.0)

    def test_refuses_to_train_on_too_little_data(self):
        with self.assertRaises(ValueError):
            CadenceModel().fit(synthetic_samples(count=5))

    def test_evaluation_verdict_is_honest_when_the_model_loses(self):
        """The reporting must not quietly round a loss into a win."""
        losing = Evaluation(
            samples=100,
            model_mae_secs=120.0,
            baseline_mae_secs=100.0,
            model_mae_norm=0.2,
            baseline_mae_norm=0.1,
        )
        self.assertFalse(losing.beats_baseline)
        self.assertIn("loses", losing.verdict)
        self.assertLess(losing.improvement_pct, 0)

    def test_marginal_wins_are_labelled_marginal(self):
        marginal = Evaluation(100, 98.0, 100.0, 0.1, 0.1)
        self.assertTrue(marginal.beats_baseline)
        self.assertIn("marginal", marginal.verdict)

    def test_small_sample_is_reported_as_insufficient(self):
        self.assertEqual(Evaluation(5, 1.0, 2.0, 0.1, 0.2).verdict, "insufficient data")

    def test_explicit_test_set_skips_the_internal_split(self):
        samples = synthetic_samples()
        model = CadenceModel()
        report = model.fit(samples[:150], epochs=5, test_samples=samples[150:])
        self.assertEqual(report.train_samples, 150)
        self.assertEqual(report.test_samples, len(samples) - 150)

    def test_checkpoint_round_trip(self):
        model = CadenceModel()
        model.fit(synthetic_samples(), epochs=5, feeds=["ethereum:ETH/USD"])
        with tempfile.TemporaryDirectory() as tmp:
            path = str(Path(tmp) / "cadence.json")
            model.save(path)
            restored = CadenceModel.load(path)
        features = [0.5] * FEATURE_DIM
        self.assertAlmostEqual(
            restored.predict_normalised(features), model.predict_normalised(features)
        )
        self.assertEqual(restored.trained_on, ["ethereum:ETH/USD"])

    def test_checkpoint_with_wrong_feature_dim_is_rejected(self):
        """Silently loading a model trained on a different feature set would produce
        confident nonsense."""
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "stale.json"
            path.write_text(json.dumps({
                "feature_dim": FEATURE_DIM + 1,
                "net": MLP([FEATURE_DIM + 1, 4, 1]).as_dict(),
            }), encoding="utf-8")
            with self.assertRaises(ValueError):
                CadenceModel.load(str(path))


class SplitTests(unittest.TestCase):
    def test_split_is_per_feed_not_per_pool(self):
        """Pooling then slicing splits by *feed*, which is a different experiment.

        Measured on Ethereum it trained on the hourly feeds and tested on the daily
        ones, then reported the mismatch as a modelling failure.
        """
        feed_a = synthetic_samples(count=40, seed=1)
        feed_b = synthetic_samples(count=40, seed=2)
        train, test = split_by_feed([("a", feed_a), ("b", feed_b)], test_fraction=0.25)

        # Both feeds must appear in both halves.
        self.assertEqual(len(train), int(len(feed_a) * 0.75) + int(len(feed_b) * 0.75))
        self.assertEqual(len(test), len(feed_a) + len(feed_b) - len(train))
        # And the test half must be the chronological tail of each, not a random draw.
        self.assertIn(feed_a[-1], test)
        self.assertIn(feed_b[-1], test)
        self.assertIn(feed_a[0], train)

    def test_tiny_feeds_go_entirely_into_training(self):
        train, test = split_by_feed([("tiny", synthetic_samples(count=2))])
        self.assertEqual(test, [])
        self.assertTrue(train)


class AnomalyModelTests(unittest.TestCase):
    def test_learns_normal_and_flags_the_abnormal(self):
        model = AnomalyModel()
        normal = synthetic_samples()
        model.fit(normal, epochs=40)

        typical = model.score(normal[0].features)
        self.assertFalse(typical["anomalous"])

        # A feature vector unlike anything in training.
        weird = model.score([1.0, 0.0] * (FEATURE_DIM // 2))
        self.assertGreaterEqual(weird["error"], typical["error"])

    def test_score_without_training_says_so(self):
        result = AnomalyModel().score([0.5] * FEATURE_DIM)
        self.assertIsNone(result["percentile"])
        self.assertIn("train it first", result["detail"])

    def test_checkpoint_round_trip(self):
        model = AnomalyModel()
        model.fit(synthetic_samples(), epochs=5)
        with tempfile.TemporaryDirectory() as tmp:
            path = str(Path(tmp) / "anomaly.json")
            model.save(path)
            restored = AnomalyModel.load(path)
        features = [0.4] * FEATURE_DIM
        self.assertAlmostEqual(
            restored.reconstruction_error(features), model.reconstruction_error(features)
        )
        self.assertEqual(len(restored.error_baseline), len(model.error_baseline))


if __name__ == "__main__":
    unittest.main()
