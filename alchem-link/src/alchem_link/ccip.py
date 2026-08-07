"""CCIP lanes: routers, chain selectors, and whether a lane is actually open.

Cross-Chain Interoperability Protocol addressing does not use chain IDs. It uses its own
64-bit **chain selectors**, and they look nothing like the chain ID you already know —
Ethereum mainnet is chain ID 1 and selector 5009297550715157269. Passing a chain ID where
a selector belongs is the standard first CCIP bug; it compiles, deploys, and reverts.

Both tables below are checkable rather than asserted, which is the point of putting them
here instead of in a document:

* Every router answers ``typeAndVersion()`` with ``Router 1.2.0``. Addresses that did not
  are not in this file — two candidates were dropped during assembly for having no code
  at all.
* Every selector is confirmed against the router's own ``isChainSupported(uint64)``.
  A selector that the router rejects is a lane that does not exist, and
  :func:`verify_lanes` asks rather than assumes.

Chains this package knows about but which have no verified router here — Gnosis, Scroll,
Linea — are absent on purpose. An unverified address in a cross-chain table is worse than
a missing one.
"""
from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Dict, List, Optional

from .multicall import Call, batch_call
from .rpc import RpcClient, client_for

#: network key → CCIP Router. Each verified live as ``Router 1.2.0``.
ROUTERS: Dict[str, str] = {
    "ethereum": "0x80226fc0Ee2b096224EeAc085Bb9a8cba1146f7D",
    "optimism": "0x3206695CaE29952f4b0c22a169725a865bc8Ce0f",
    "arbitrum": "0x141fa059441E0ca23ce184B6A78bafD2A517DdE8",
    "base": "0x881e3A65B4d4a04dD529061dd0071cf975F58bCD",
    "polygon": "0x849c5ED5a80F5B408Dd4969b78c2C8fdf0565Bfe",
    "avalanche": "0xF4c7E640EdA248ef95972845a62bdC74237805dB",
    "bnb": "0x34B03Cb9086d7D758AC55af71584F81A598759FE",
    "sepolia": "0x0BF3dE8c5D3e8A2B34D2BEeB17ABfCeBaf363A59",
}

#: network key → CCIP chain selector. Not the chain ID. Confirmed via isChainSupported.
CHAIN_SELECTORS: Dict[str, int] = {
    "ethereum": 5009297550715157269,
    "optimism": 3734403246176062136,
    "arbitrum": 4949039107694359620,
    "base": 15971525489660198786,
    "polygon": 4051577828743386545,
    "avalanche": 6433500567565415381,
    "bnb": 11344663589394136015,
    "sepolia": 16015286601757825753,
}


@dataclass
class Lane:
    """A directed source → destination pair, and whether the source router allows it."""
    source: str
    destination: str
    router: str
    destination_selector: int
    supported: Optional[bool] = None
    error: str = ""

    def as_dict(self) -> Dict[str, Any]:
        return {
            "source": self.source,
            "destination": self.destination,
            "router": self.router,
            "destination_selector": self.destination_selector,
            "supported": self.supported,
            "error": self.error,
        }


def get_router(network: str) -> Optional[str]:
    return ROUTERS.get(network.lower())


def get_selector(network: str) -> Optional[int]:
    return CHAIN_SELECTORS.get(network.lower())


def ccip_networks() -> List[str]:
    return sorted(ROUTERS)


def list_lanes(source: str) -> List[Lane]:
    """Every lane this table can describe out of ``source``, unverified."""
    key = source.lower()
    router = ROUTERS.get(key)
    if router is None:
        return []
    return [
        Lane(source=key, destination=dest, router=router, destination_selector=selector)
        for dest, selector in sorted(CHAIN_SELECTORS.items())
        if dest != key
    ]


def verify_lanes(
    source: str,
    client: Optional[RpcClient] = None,
    rpc_url: Optional[str] = None,
) -> List[Lane]:
    """Ask the source router which destinations it actually supports.

    One batched call per source chain. A selector the router rejects means the lane is
    closed — which is a real answer, and a much better one than a table asserting it is
    open.
    """
    lanes = list_lanes(source)
    if not lanes:
        return []

    rpc = client or client_for(network=source, rpc_url=rpc_url)
    report = batch_call(rpc, [
        Call(
            lane.router,
            "isChainSupported(uint64)",
            (lane.destination_selector,),
            ["bool"],
            lane.destination,
        )
        for lane in lanes
    ])

    for lane in lanes:
        result = report.by_label(lane.destination)
        if result is None or not result.success:
            lane.error = result.error if result else "no result"
        else:
            lane.supported = bool(result.one(False))
    return lanes


def summarize_chainlink_capabilities() -> Dict[str, Any]:
    """What this package can actually verify about each Chainlink service.

    Replaces a hardcoded dict of prose. Each entry says whether the toolkit reads the
    thing live or merely knows it exists — a distinction the prose version elided.
    """
    from .feeds import feed_count
    from .sequencer import SEQUENCER_FEEDS

    return {
        "price_feeds": {
            "summary": "Off-chain market data with on-chain staleness and bound checks",
            "verified_live": True,
            "detail": f"{feed_count()} feeds registered, each verified against its own description()",
            "commands": ["price", "feeds", "verify", "audit", "cadence", "divergence"],
        },
        "l2_sequencer_uptime": {
            "summary": "Uptime feeds gating price reads on rollups",
            "verified_live": True,
            "detail": f"{len(SEQUENCER_FEEDS)} uptime feeds read, with the grace period applied",
            "commands": ["sequencer", "audit"],
        },
        "ccip": {
            "summary": "Cross-chain token and message transfer",
            "verified_live": True,
            "detail": f"{len(ROUTERS)} routers verified as Router 1.2.0; lanes probed via isChainSupported",
            "commands": ["ccip"],
        },
        "vrf": {
            "summary": "Verifiable randomness",
            "verified_live": False,
            "detail": "Not read by this toolkit — a VRF request is a transaction, and this package is read-only",
            "commands": [],
        },
        "automation": {
            "summary": "Condition-based execution of on-chain jobs",
            "verified_live": False,
            "detail": "Not read by this toolkit — registry upkeep state is out of scope for a read-only reader",
            "commands": [],
        },
    }
