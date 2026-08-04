"""Live checks: is the endpoint reachable, is it the chain you asked for, is it fast.

`doctor` exists because the three ways this toolkit fails in practice are all silent:
you are on the keyless fallback and getting rate limited, you are pointed at the wrong
chain, or a feed is technically responding but has not published in hours. Each check
below turns one of those into a visible line.
"""
from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional

from .feeds import list_feeds, read_feed
from .networks import ALCHEMY_KEY_ENV, DEFAULT_NETWORK, get_network
from .rpc import RpcClient, client_for, gwei


@dataclass
class Check:
    name: str
    ok: bool
    detail: str
    hint: str = ""

    def as_dict(self) -> Dict[str, Any]:
        payload = {"name": self.name, "ok": self.ok, "detail": self.detail}
        if self.hint:
            payload["hint"] = self.hint
        return payload


@dataclass
class Diagnosis:
    network: str
    endpoint: str
    endpoint_source: str
    checks: List[Check] = field(default_factory=list)

    @property
    def ok(self) -> bool:
        return all(check.ok for check in self.checks)

    def as_dict(self) -> Dict[str, Any]:
        return {
            "network": self.network,
            "endpoint": self.endpoint,
            "endpoint_source": self.endpoint_source,
            "ok": self.ok,
            "checks": [check.as_dict() for check in self.checks],
        }


def diagnose(
    network: str = DEFAULT_NETWORK,
    client: Optional[RpcClient] = None,
    rpc_url: Optional[str] = None,
) -> Diagnosis:
    """Run the end-to-end readiness checks for one network."""
    net = get_network(network)
    rpc = client or client_for(network=network, rpc_url=rpc_url)
    diagnosis = Diagnosis(
        network=net.key,
        endpoint=rpc.endpoint.redacted(),
        endpoint_source=rpc.endpoint.source,
    )

    if rpc.endpoint.is_authenticated:
        diagnosis.checks.append(
            Check("credentials", True, f"using {rpc.endpoint.source}")
        )
    else:
        diagnosis.checks.append(
            Check(
                "credentials",
                True,
                "using the keyless public endpoint",
                hint=f"Set {ALCHEMY_KEY_ENV} for higher rate limits and reliability.",
            )
        )

    try:
        result = rpc.call("eth_blockNumber")
        block = int(result.result, 16)
        diagnosis.checks.append(
            Check("rpc reachable", True, f"block {block:,} in {result.elapsed_ms:.0f} ms")
        )
    except Exception as exc:
        diagnosis.checks.append(
            Check("rpc reachable", False, str(exc), hint="Check the URL, your key, and any proxy.")
        )
        return diagnosis

    try:
        actual = rpc.chain_id()
        matches = actual == net.chain_id
        diagnosis.checks.append(
            Check(
                "chain id",
                matches,
                f"expected {net.chain_id}, got {actual}",
                hint="" if matches else "The endpoint is serving a different chain than the network you asked for.",
            )
        )
    except Exception as exc:
        diagnosis.checks.append(Check("chain id", False, str(exc)))

    try:
        price = rpc.gas_price_wei()
        diagnosis.checks.append(Check("gas price", True, f"{gwei(price):.3f} gwei"))
    except Exception as exc:
        diagnosis.checks.append(Check("gas price", False, str(exc)))

    feeds = list_feeds(net.key)
    if not feeds:
        diagnosis.checks.append(
            Check("feed read", True, "no feeds registered for this network")
        )
        return diagnosis

    probe = feeds[0]
    try:
        reading = read_feed(probe.pair, network=net.key, client=rpc)
        fresh = not reading.stale
        diagnosis.checks.append(
            Check(
                "feed read",
                fresh,
                f"{reading.pair} = {reading.price:,.4f} ({reading.status}, {reading.age_secs}s old)",
                hint="" if fresh else f"Last update exceeds the {reading.heartbeat_secs}s heartbeat — do not trade on it.",
            )
        )
    except Exception as exc:
        diagnosis.checks.append(Check("feed read", False, str(exc)))

    return diagnosis
