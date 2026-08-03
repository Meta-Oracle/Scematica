from __future__ import annotations

from typing import Dict, Any


def build_integration_map() -> Dict[str, Any]:
    """Create a high-level integration map between Alchemy and Chainlink patterns."""
    return {
        "data_ingestion": {
            "alchemy": "Pull chain state and event streams into application services",
            "chainlink": "Validate and enrich state with trusted external data",
        },
        "execution": {
            "alchemy": "Submit and monitor transactions with low-friction developer tooling",
            "chainlink": "Trigger secure automation and oracle-driven actions",
        },
        "monitoring": {
            "alchemy": "Observe transaction status and debugging signals",
            "chainlink": "Observe external conditions and operational triggers",
        },
        "cross_chain": {
            "alchemy": "Track and verify transactions across source and destination chains via multi-network RPC",
            "chainlink": "Route messages and tokens securely across chains using CCIP",
        },
    }
