"""sentience CLI — compute S, Ψ and run a short demo loop."""
from __future__ import annotations
import argparse
import sys
from . import (
    CognitiveLoop, CognitiveState, GrowthModel,
    Perception, RationalityInputs, LogicInputs, EthicsInputs,
    SentienceIndex, MasterEquation, Bounded,
)
from .types import Observation


def cmd_compute(args: argparse.Namespace) -> None:
    r = RationalityInputs(args.evidence, args.consistency, args.uncertainty, args.bias)
    l = LogicInputs(args.validity, args.consistency, args.causal, args.formal)
    m = EthicsInputs(args.harm, args.contextual, args.fairness, args.rights)
    d = Perception(args.audio, args.visual, args.sensory, args.integrity)
    s = SentienceIndex.compute(r, l, m, d)
    _, psi = MasterEquation.compute(r, l, m, d,
        agency_ratio=args.agency, meta_ratio=args.meta,
        knowledge_density=args.knowledge, feedback=args.feedback)
    print(f"\n{'='*50}")
    print(f"  Sentience Index  S  = {s.value.value:.6f}")
    print(f"  Integrated Cog  Ψ  = {psi.psi.value:.6f}")
    print(f"  Bottleneck         : {s.bottleneck()}")
    print(f"{'='*50}")
    print(f"  R (rationality)  = {s.rationality.value:.4f}")
    print(f"  L (logic)        = {s.logic.value:.4f}")
    print(f"  M (moral)        = {s.moral.value:.4f}")
    print(f"  D (data)         = {s.data.value:.4f}")
    print(f"{'='*50}\n")


def cmd_demo(args: argparse.Namespace) -> None:
    state = CognitiveState.initial()
    loop = CognitiveLoop(state)
    growth = GrowthModel(capability=1.0, c_max=10.0, alpha=0.3)

    print(f"\n{'='*55}")
    print(f"  {'t':>4}  {'S':>8}  {'Ψ':>8}  {'error':>8}  {'C_t':>8}")
    print(f"{'='*55}")

    predicted = 0.5
    import math, random
    random.seed(42)
    for t in range(1, args.steps + 1):
        obs_val = 0.5 + 0.4 * math.sin(t * 0.5) + random.gauss(0, 0.05)
        obs = Observation(value=obs_val, confidence=Bounded(0.85), timestep=t)
        out = loop.step(obs, predicted, feedback=0.9)
        l_t = out.sentience.logic.value
        c_t = growth.step(l_t, out.sentience.data.value, 0.9)
        predicted = obs_val  # next prediction = last observation
        marker = " ← reassess" if out.reassessment_triggered else ""
        print(f"  {t:>4}  {out.sentience.value.value:>8.4f}  "
              f"{out.psi.value:>8.4f}  {out.error:>8.4f}  {c_t:>8.4f}{marker}")

    print(f"{'='*55}\n")


def main(argv=None) -> None:
    parser = argparse.ArgumentParser(prog="sentience",
        description="Singularity Cognitive Architecture — compute S and Ψ")
    sub = parser.add_subparsers(dest="cmd", required=True)

    # compute sub-command
    cp = sub.add_parser("compute", help="Compute S and Ψ from inputs")
    for name, default in [
        ("--evidence", 0.9), ("--consistency", 0.9), ("--uncertainty", 0.8), ("--bias", 0.05),
        ("--validity", 0.9), ("--causal", 0.85), ("--formal", 0.85),
        ("--harm", 0.95), ("--contextual", 0.85), ("--fairness", 0.9), ("--rights", 0.95),
        ("--audio", 1.0), ("--visual", 1.0), ("--sensory", 1.0), ("--integrity", 1.0),
        ("--agency", 0.85), ("--meta", 0.80), ("--knowledge", 0.5), ("--feedback", 0.9),
    ]:
        cp.add_argument(name, type=float, default=default)
    cp.set_defaults(func=cmd_compute)

    # demo sub-command
    dp = sub.add_parser("demo", help="Run a multi-step cognitive loop demo")
    dp.add_argument("--steps", type=int, default=10)
    dp.set_defaults(func=cmd_demo)

    args = parser.parse_args(argv)
    args.func(args)


if __name__ == "__main__":
    main()
