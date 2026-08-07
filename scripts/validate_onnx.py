"""Prove the exported ONNX model computes what scematica-nn computes.

    cargo run -p scematica-nn --bin scema-onnx -- --reference onnx-ref.json
    python scripts/validate_onnx.py

An exported model that loads cleanly but computes something subtly different is worse
than no export: it fails silently, confidently, and only in production. The exporter is
hand-written protobuf, so "it loaded" proves the *encoding* and nothing about the
*semantics*. This asserts the semantics.

Three checks, in increasing strength:

1. **Structure** — `onnx.checker` validates the ModelProto against the spec: shapes,
   opset, node wiring, initializer types. Catches a malformed field or a dangling edge.
2. **Executability** — onnxruntime builds an inference session. Catches a graph that is
   well-formed but not runnable, such as an op-version mismatch.
3. **Numerics** — run every reference input and compare elementwise against the Q-values
   Rust produced. This is the one that matters. It also checks `argmax` agreement
   separately, because the *action* is what the agent acts on: two Q-vectors can differ
   within tolerance and still pick different moves, and that would be a real bug even
   though the numbers look close.

Tolerance is set for the f64 → f32 narrowing the export performs, not chosen to make the
test pass — see TOLERANCE below.
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
DEFAULT_MODEL = REPO_ROOT / "scematica-dqn.onnx"
DEFAULT_REFERENCE = REPO_ROOT / "onnx-ref.json"

#: Absolute tolerance on a Q-value.
#:
#: The weights are stored as f32 (~7 significant decimal digits) while Rust computes in
#: f64, and the error compounds across three layers of accumulation. Q-values here run to
#: roughly ±40, so 1e-3 absolute is about five significant figures of agreement — tight
#: enough that a wrong transpose, a missing bias or a mis-broadcast mean cannot hide
#: inside it, loose enough not to trip on representation error.
TOLERANCE = 1e-3


def _configure_output() -> None:
    """Survive a Windows console on a legacy codepage.

    This script prints deltas and arrows; a cp1252 stdout raises UnicodeEncodeError
    mid-report and turns a passing validation into a traceback.
    """
    for stream in (sys.stdout, sys.stderr):
        reconfigure = getattr(stream, "reconfigure", None)
        if reconfigure is not None:
            try:
                reconfigure(encoding="utf-8", errors="replace")
            except (ValueError, OSError):
                pass


def fail(message: str) -> None:
    print(f"  FAIL  {message}")
    sys.exit(1)


def main() -> int:
    _configure_output()
    model_path = Path(sys.argv[1]) if len(sys.argv) > 1 else DEFAULT_MODEL
    reference_path = Path(sys.argv[2]) if len(sys.argv) > 2 else DEFAULT_REFERENCE

    if not model_path.exists():
        fail(f"no model at {model_path} — run `cargo run -p scematica-nn --bin scema-onnx` first")
    if not reference_path.exists():
        fail(f"no reference at {reference_path} — re-run scema-onnx with --reference")

    try:
        import numpy as np
        import onnx
        import onnxruntime as ort
    except ImportError as exc:
        fail(f"needs onnx, onnxruntime and numpy: {exc}")

    print(f"model      {model_path}  ({model_path.stat().st_size:,} bytes)")
    print(f"reference  {reference_path}")

    # ── 1. structure ─────────────────────────────────────────────────────────────
    model = onnx.load(str(model_path))
    onnx.checker.check_model(model)
    graph = model.graph
    ops = [node.op_type for node in graph.node]
    metadata = {entry.key: entry.value for entry in model.metadata_props}

    print(f"\nir_version {model.ir_version}   opset {model.opset_import[0].version}")
    print(f"producer   {model.producer_name} {model.producer_version}")
    print(f"nodes      {len(graph.node)}  ({', '.join(dict.fromkeys(ops))})")
    print(f"inputs     {[i.name for i in graph.input]}")
    print(f"outputs    {[o.name for o in graph.output]}")

    params = sum(
        int(np.prod(init.dims)) for init in graph.initializer
    )
    print(f"parameters {params:,} across {len(graph.initializer)} initializers")
    print("  ok    onnx.checker validated the model")

    for key in ("architecture", "state_dim", "action_dim", "train_steps", "action_labels"):
        if key not in metadata:
            fail(f"metadata is missing {key!r} — the model cannot describe itself")
    print(f"  ok    metadata carries the IO schema and training state")
    print(f"        architecture={metadata['architecture']} "
          f"train_steps={metadata.get('train_steps')} epsilon={metadata.get('epsilon')}")

    features = metadata.get("state_features", "").split(",")
    if len(features) != int(metadata["state_dim"]):
        fail(f"state_features lists {len(features)} names for state_dim={metadata['state_dim']}")
    print(f"  ok    {len(features)} feature names match state_dim")

    # ── 2. executability ─────────────────────────────────────────────────────────
    session = ort.InferenceSession(str(model_path), providers=["CPUExecutionProvider"])
    input_name = session.get_inputs()[0].name
    output_names = [o.name for o in session.get_outputs()]
    print(f"  ok    onnxruntime built a session (outputs: {', '.join(output_names)})")

    # ── 3. numerics ──────────────────────────────────────────────────────────────
    reference = json.loads(reference_path.read_text(encoding="utf-8"))
    inputs = np.asarray(reference["inputs"], dtype=np.float32)
    expected = np.asarray(reference["q_values"], dtype=np.float64)
    labels = reference.get("action_labels", [])

    print(f"\nvectors    {len(inputs)} "
          f"({reference.get('live_states', 0)} from live trades, "
          f"{reference.get('synthetic_states', 0)} synthetic)")

    # One batched call — which also exercises the symbolic batch dimension and the
    # broadcast in the dueling recombination. A per-row loop would pass even if the
    # ReduceMean axis were wrong.
    outputs = session.run(None, {input_name: inputs})
    actual = np.asarray(outputs[0], dtype=np.float64)

    if actual.shape != expected.shape:
        fail(f"shape mismatch: onnx {actual.shape} vs rust {expected.shape}")

    diff = np.abs(actual - expected)
    worst = float(diff.max())
    mean = float(diff.mean())
    print(f"  max abs diff   {worst:.3e}")
    print(f"  mean abs diff  {mean:.3e}")

    if worst > TOLERANCE:
        index = int(np.unravel_index(diff.argmax(), diff.shape)[0])
        print(f"\n  worst row {index}:")
        print(f"    rust  {expected[index]}")
        print(f"    onnx  {actual[index]}")
        fail(f"Q-values diverge by {worst:.3e}, over the {TOLERANCE:.0e} tolerance")
    print(f"  ok    Q-values agree within {TOLERANCE:.0e}")

    # The agent acts on argmax, so equal-within-tolerance is not sufficient on its own.
    rust_actions = expected.argmax(axis=1)
    onnx_actions = actual.argmax(axis=1)
    disagreements = int((rust_actions != onnx_actions).sum())
    if disagreements:
        rows = np.nonzero(rust_actions != onnx_actions)[0][:5]
        for row in rows:
            print(f"    row {row}: rust picks {rust_actions[row]}, onnx picks {onnx_actions[row]}")
        fail(f"{disagreements}/{len(inputs)} inputs select a different action")
    print(f"  ok    argmax action identical on all {len(inputs)} inputs")

    # Single-row inference must match the batched result, or the batch dimension is not
    # actually symbolic the way the graph claims.
    single = session.run(None, {input_name: inputs[:1]})[0]
    if not np.allclose(np.asarray(single, dtype=np.float64), actual[:1], atol=TOLERANCE):
        fail("batch-of-1 disagrees with the batched run — the batch dim is not symbolic")
    print("  ok    batch-of-1 matches the batched run")

    # The dueling value head, when exported, must equal Q − (A − mean A). We cannot see A
    # from outside, but V is constant across actions, so mean(Q) == V exactly.
    if len(output_names) > 1 and "state_value" in output_names:
        value = np.asarray(outputs[output_names.index("state_value")], dtype=np.float64)
        implied = actual.mean(axis=1, keepdims=True)
        value_error = float(np.abs(value - implied).max())
        if value_error > TOLERANCE:
            fail(f"state_value does not equal mean(Q) — off by {value_error:.3e}")
        print(f"  ok    state_value == mean(Q), confirming the dueling identity")

    if labels:
        counts = np.bincount(onnx_actions, minlength=len(labels))
        spread = ", ".join(
            f"{label}={count}" for label, count in zip(labels, counts) if count
        )
        print(f"\naction spread over the reference set: {spread}")

        # Two different properties get confused here, so report both.
        #
        # `I = Var[Q*] / E[Q*]^2` is the intelligence ratio from EQUATIONS.md: whether
        # the valuations depend on the input at all. Below ~1e-4 the network is emitting
        # a constant and has stopped reading its input.
        #
        # A constant argmax is *not* the same thing. A policy can vary its Q-values
        # enormously with the input — reading it perfectly well — while one action stays
        # on top throughout. That is a strong opinion, not a collapse, and calling it one
        # would be exactly the mistake the margin guard made in the other direction.
        q_star = actual.max(axis=1)
        mean_q = float(q_star.mean())
        intelligence = float(q_star.var() / (mean_q**2)) if mean_q else 0.0
        print(f"  intelligence ratio I = Var[Q*]/E[Q*]^2 = {intelligence:.3e} "
              f"(collapse below 1e-4)")
        print(f"  Q* range [{q_star.min():.3f}, {q_star.max():.3f}]")

        if intelligence < 1e-4:
            print("  NOTE  I is below the collapse threshold — the network is returning "
                  "essentially the same valuation regardless of input. See EQUATIONS.md.")
        elif int((counts > 0).sum()) == 1:
            print("  NOTE  one action wins on every reference input, but I is healthy: "
                  "the valuations do track the input, the ranking just does not change. "
                  "Not a collapse — a uniform preference.")

    print(f"\nPASS — the exported graph reproduces scematica-nn's policy.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
