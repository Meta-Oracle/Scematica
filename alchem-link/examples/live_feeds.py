"""Read live Chainlink feeds and refuse to use stale ones.

Runs with no API key against the public fallback endpoint. Set ALCHEMY_API_KEY for
real rate limits.

    python examples/live_feeds.py
    python examples/live_feeds.py arbitrum
"""
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "src"))

from alchem_link import diagnose, read_all_feeds, read_feed, resolve_endpoint


def guarded_price(pair: str, network: str) -> float:
    """The pattern worth copying: never let a stale answer reach your logic."""
    reading = read_feed(pair, network=network)
    if reading.stale:
        raise RuntimeError(
            f"{reading.pair} last published {reading.age_secs}s ago, "
            f"past its {reading.heartbeat_secs}s heartbeat — refusing to use it"
        )
    if reading.answer_raw <= 0:
        raise RuntimeError(f"{reading.pair} reported a non-positive answer")
    return reading.price


def main() -> int:
    network = sys.argv[1] if len(sys.argv) > 1 else "ethereum"

    endpoint = resolve_endpoint(network=network)
    print(f"network   {network}")
    print(f"endpoint  {endpoint.redacted()}  ({endpoint.source})")

    report = diagnose(network=network)
    print(f"ready     {report.ok}")
    print("")

    print("All registered feeds:")
    for reading in read_all_feeds(network=network):
        marker = " " if not reading.stale else "!"
        print(f" {marker} {reading.pair:<10} {reading.price:>16,.4f}  {reading.status:<7} {reading.age_secs:>7}s old")
    print("")

    try:
        price = guarded_price("ETH/USD", network)
        print(f"Guarded ETH/USD read: {price:,.2f}")
    except (RuntimeError, KeyError) as exc:
        print(f"Guarded read refused: {exc}")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
