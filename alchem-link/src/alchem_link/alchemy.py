from __future__ import annotations

from typing import Dict, Any


def summarize_alchemy_capabilities() -> Dict[str, Any]:
    """Summarize the core Alchemy developer capabilities relevant to this package."""
    return {
        "rpc": "High-throughput node access for transaction submission and chain state reads",
        "websocket": "Real-time event subscriptions for application and monitoring workflows",
        "apis": [
            "Enhanced APIs for token, NFT, and transaction intelligence",
            "Debug and trace endpoints for smarter development and incident handling",
        ],
        "developer_value": "Makes blockchain integration feel practical, observable, and fast to iterate on",
    }
