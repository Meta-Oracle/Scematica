from __future__ import annotations

from typing import Dict, Any


def summarize_chainlink_capabilities() -> Dict[str, Any]:
    """Summarize the core Chainlink capabilities relevant to this package."""
    return {
        "price_feeds": "Secure off-chain market data for smart contract and application logic",
        "vrf": "Verifiable randomness for games, lotteries, and dynamic systems",
        "automation": "Condition-based execution for maintenance and operational workflows",
        "ccip": "Cross-Chain Interoperability Protocol for secure token and message transfers across chains",
        "developer_value": "Adds trust-minimized, verifiable execution patterns to the stack",
    }
