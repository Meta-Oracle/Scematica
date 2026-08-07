"""How the two systems actually compose here, stated as commands rather than adjectives.

This module used to return a dict of sentences like "Pull chain state and event streams
into application services". That is true of any RPC provider and tells a developer
nothing they can run.

The map below is keyed on the same four domains, but every entry names the function and
the CLI command that does the thing — and every claim is one this package demonstrates
against a live chain. If an entry cannot point at working code, it does not belong here.
"""
from __future__ import annotations

from typing import Any, Dict


def build_integration_map() -> Dict[str, Any]:
    """Where Alchemy's node access and Chainlink's oracles meet, concretely."""
    return {
        "data_ingestion": {
            "alchemy": "Batched, block-atomic contract reads via Multicall3 — 48 aggregator "
                       "calls in one round trip",
            "chainlink": "Verified price feeds whose staleness is checked against a measured "
                         "heartbeat, not a copied constant",
            "composed": "Every feed on a chain read at one block height, so a comparison "
                        "between them is a comparison at a single moment",
            "code": "feeds.read_all_feeds, multicall.batch_call",
            "commands": ["feeds --live", "verify"],
        },
        "valuation": {
            "alchemy": "Token discovery (alchemy_getTokenBalances) and balances via eth_call",
            "chainlink": "USD prices for the assets held",
            "composed": "A portfolio total in dollars, with explicit coverage — how many "
                        "positions the oracle registry could actually price",
            "code": "enhanced.value_holdings",
            "commands": ["holdings"],
        },
        "execution": {
            "alchemy": "EIP-1559 fee structure from eth_feeHistory: base-fee trend, "
                       "priority-fee percentiles, next-block base fee",
            "chainlink": "The native token's USD price on the same chain",
            "composed": "Transaction cost quoted in dollars instead of gwei, which is the "
                        "only unit that compares across chains",
            "code": "gas.analyse_gas",
            "commands": ["gas"],
        },
        "safety": {
            "alchemy": "Proxy resolution and bound reads against the implementation contract",
            "chainlink": "L2 sequencer uptime feeds and the documented grace period",
            "composed": "A consumer-safety audit covering the failure modes that return "
                        "successfully — stale rounds, carried answers, pinned circuit "
                        "breakers, and a sequencer that is up but not up long enough",
            "code": "safety.audit_feed, sequencer.read_sequencer",
            "commands": ["audit", "sequencer", "generate"],
        },
        "cross_chain": {
            "alchemy": "The same read executed against every chain's endpoint",
            "chainlink": "CCIP routers and chain selectors, with lanes probed via "
                         "isChainSupported",
            "composed": "Basis-point divergence for one pair across ten chains, with stale "
                        "legs excluded from consensus and named as the cause",
            "code": "divergence.compare_pair, ccip.verify_lanes",
            "commands": ["divergence", "ccip"],
        },
    }


def build_package_blueprint() -> Dict[str, Any]:
    """What this package is, measured rather than described."""
    from .ccip import ROUTERS
    from .feeds import feed_count
    from .networks import list_networks
    from .sequencer import SEQUENCER_FEEDS

    networks = list_networks()
    return {
        "project": "Alchem-Link — Alchemy x Chainlink developer toolkit",
        "coverage": {
            "networks": len(networks),
            "feeds": feed_count(),
            "sequencer_uptime_feeds": len(SEQUENCER_FEEDS),
            "ccip_routers": len(ROUTERS),
            "layer2_networks": sum(1 for n in networks if n.layer2),
            "testnets": sum(1 for n in networks if n.testnet),
        },
        "guarantees": [
            "Every registered address was called for description() and decimals(), and is "
            "filed under the pair the contract itself reports",
            "Every heartbeat was measured from round history rather than copied — the "
            "declared-3600s-everywhere default was wrong on most chains",
            "Every CCIP router answers typeAndVersion() as Router 1.2.0, and every lane is "
            "confirmed against the router's own isChainSupported",
            "Function selectors are computed with a bundled Keccak-256, not stored as "
            "constants to be trusted",
        ],
        "dependencies": {
            "runtime": "none — standard library only",
            "tui_only": "textual",
            "note": "Keccak-256 is implemented in-package because hashlib ships SHA3-256, "
                    "which uses a different padding byte and produces a different digest",
        },
        "integration_map": build_integration_map(),
    }
