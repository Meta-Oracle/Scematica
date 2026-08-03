from __future__ import annotations

from typing import Dict, Any, List


def get_recipes() -> List[Dict[str, Any]]:
    """Return a starter set of developer recipes for the Alchemy x Chainlink package."""
    return [
        {
            "id": "oracle-backed-automation",
            "name": "Oracle-backed automation",
            "summary": "Use Chainlink automation plus Alchemy transaction monitoring to trigger and verify onchain actions.",
            "steps": [
                "Watch for a trigger condition with Chainlink automation",
                "Use Alchemy to submit and monitor the transaction",
                "Record execution state back to your application layer",
            ],
            "tags": ["automation", "monitoring", "execution"],
        },
        {
            "id": "real-time-data-pipeline",
            "name": "Real-time data pipeline",
            "summary": "Stream blockchain events through Alchemy and enrich them with Chainlink data feeds.",
            "steps": [
                "Subscribe to event streams via Alchemy websockets",
                "Normalize event payloads into a shared application model",
                "Cross-check values with Chainlink feeds before acting on them",
            ],
            "tags": ["data", "websocket", "feeds"],
        },
        {
            "id": "secure-bridge-experiment",
            "name": "Secure bridge experiment",
            "summary": "Prototype a bridge-like workflow that depends on Alchemy for transaction flow and Chainlink for trust-minimized signals.",
            "steps": [
                "Prepare the source and destination chain context",
                "Use Alchemy to submit and inspect transaction state",
                "Use Chainlink oracles to verify the external state before finalizing",
            ],
            "tags": ["bridge", "cross-chain", "security"],
        },
        {
            "id": "ccip-cross-chain-transfer",
            "name": "CCIP cross-chain transfer",
            "summary": "Use Chainlink CCIP to send tokens or messages cross-chain while Alchemy tracks state on both ends.",
            "steps": [
                "Initiate a CCIP send transaction on the source chain via Alchemy RPC",
                "Monitor the source chain tx confirmation with Alchemy websockets",
                "Poll the destination chain via Alchemy until the CCIP message is executed",
            ],
            "tags": ["ccip", "cross-chain", "monitoring"],
        },
    ]


def get_recipe_by_id(recipe_id: str) -> Dict[str, Any] | None:
    """Fetch a recipe by id if present."""
    for recipe in get_recipes():
        if recipe["id"] == recipe_id:
            return recipe
    return None
