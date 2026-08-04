"""Alchem-Link — a practical Alchemy x Chainlink developer toolkit.

Two halves that work together:

* **Live** — read Chainlink price feeds and Alchemy-served chain state with no
  dependencies beyond the standard library. ``read_feed``, ``diagnose``, ``RpcClient``.
* **Reference** — the integration map and recipes that explain how the two systems fit
  together. ``build_integration_map``, ``get_recipes``.
"""

__version__ = "0.3.0"

from .abi import decode_string, scale, to_int, to_uint, words
from .alchemy import summarize_alchemy_capabilities
from .chainlink import summarize_chainlink_capabilities
from .core import build_package_blueprint
from .feeds import (
    FEEDS,
    Feed,
    FeedReading,
    decode_reading,
    get_feed,
    list_feeds,
    read_all_feeds,
    read_feed,
    verify_registry,
)
from .health import Check, Diagnosis, diagnose
from .integration import build_integration_map
from .networks import (
    DEFAULT_NETWORK,
    NETWORKS,
    Endpoint,
    Network,
    get_network,
    list_networks,
    resolve_endpoint,
)
from .recipes import get_recipe_by_id, get_recipes
from .rpc import RpcClient, RpcError, RpcTransportError, client_for, gwei

__all__ = [
    "__version__",
    # live: feeds
    "Feed",
    "FeedReading",
    "FEEDS",
    "read_feed",
    "read_all_feeds",
    "list_feeds",
    "get_feed",
    "decode_reading",
    "verify_registry",
    # live: rpc + networks
    "RpcClient",
    "RpcError",
    "RpcTransportError",
    "client_for",
    "gwei",
    "Network",
    "NETWORKS",
    "DEFAULT_NETWORK",
    "Endpoint",
    "get_network",
    "list_networks",
    "resolve_endpoint",
    # live: diagnostics
    "diagnose",
    "Diagnosis",
    "Check",
    # abi helpers
    "words",
    "to_uint",
    "to_int",
    "decode_string",
    "scale",
    # reference
    "build_package_blueprint",
    "summarize_alchemy_capabilities",
    "summarize_chainlink_capabilities",
    "build_integration_map",
    "get_recipes",
    "get_recipe_by_id",
]
