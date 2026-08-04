"""Supported networks and how to reach them.

Every network here carries three things: the Alchemy subdomain used to build an
authenticated endpoint, a keyless public endpoint so the toolkit works before you have
an API key, and the chain id to verify you actually reached the chain you asked for.

Chain ids below were read back from the live endpoints, not copied from a table.
"""
from __future__ import annotations

import os
from dataclasses import dataclass
from typing import Dict, List, Optional

ALCHEMY_KEY_ENV = "ALCHEMY_API_KEY"
ALCHEMY_URL_ENV = "ALCHEMY_URL"


@dataclass(frozen=True)
class Network:
    key: str
    label: str
    chain_id: int
    native_symbol: str
    #: Alchemy hostname prefix — the endpoint is https://<subdomain>.g.alchemy.com/v2/<key>
    alchemy_subdomain: str
    #: Keyless fallback so `alchem-link price ETH/USD` works with zero setup.
    public_rpc: str
    explorer: str

    def alchemy_url(self, api_key: str) -> str:
        return f"https://{self.alchemy_subdomain}.g.alchemy.com/v2/{api_key}"


NETWORKS: Dict[str, Network] = {
    "ethereum": Network(
        key="ethereum",
        label="Ethereum Mainnet",
        chain_id=1,
        native_symbol="ETH",
        alchemy_subdomain="eth-mainnet",
        public_rpc="https://ethereum-rpc.publicnode.com",
        explorer="https://etherscan.io",
    ),
    "sepolia": Network(
        key="sepolia",
        label="Ethereum Sepolia",
        chain_id=11155111,
        native_symbol="ETH",
        alchemy_subdomain="eth-sepolia",
        public_rpc="https://ethereum-sepolia-rpc.publicnode.com",
        explorer="https://sepolia.etherscan.io",
    ),
    "base": Network(
        key="base",
        label="Base Mainnet",
        chain_id=8453,
        native_symbol="ETH",
        alchemy_subdomain="base-mainnet",
        public_rpc="https://base-rpc.publicnode.com",
        explorer="https://basescan.org",
    ),
    "arbitrum": Network(
        key="arbitrum",
        label="Arbitrum One",
        chain_id=42161,
        native_symbol="ETH",
        alchemy_subdomain="arb-mainnet",
        public_rpc="https://arbitrum-one-rpc.publicnode.com",
        explorer="https://arbiscan.io",
    ),
    "optimism": Network(
        key="optimism",
        label="OP Mainnet",
        chain_id=10,
        native_symbol="ETH",
        alchemy_subdomain="opt-mainnet",
        public_rpc="https://optimism-rpc.publicnode.com",
        explorer="https://optimistic.etherscan.io",
    ),
    "polygon": Network(
        key="polygon",
        label="Polygon PoS",
        chain_id=137,
        native_symbol="POL",
        alchemy_subdomain="polygon-mainnet",
        public_rpc="https://polygon-bor-rpc.publicnode.com",
        explorer="https://polygonscan.com",
    ),
}

DEFAULT_NETWORK = "ethereum"


def list_networks() -> List[Network]:
    return list(NETWORKS.values())


def get_network(key: str) -> Network:
    try:
        return NETWORKS[key.lower()]
    except KeyError:
        known = ", ".join(sorted(NETWORKS))
        raise KeyError(f"unknown network '{key}'. Known networks: {known}") from None


@dataclass(frozen=True)
class Endpoint:
    url: str
    #: Where the URL came from — shown by `doctor` so nobody debugs the wrong endpoint.
    source: str
    network: Network

    @property
    def is_authenticated(self) -> bool:
        return self.source in ("--rpc-url", ALCHEMY_URL_ENV, ALCHEMY_KEY_ENV)

    def redacted(self) -> str:
        """URL safe to print: an Alchemy key in the path is replaced with a placeholder."""
        marker = "/v2/"
        idx = self.url.find(marker)
        if idx == -1:
            return self.url
        return self.url[: idx + len(marker)] + "<key>"


def resolve_endpoint(
    network: str = DEFAULT_NETWORK,
    rpc_url: Optional[str] = None,
    env: Optional[Dict[str, str]] = None,
) -> Endpoint:
    """Pick the RPC endpoint to use, most explicit source first.

    1. ``--rpc-url`` / the ``rpc_url`` argument
    2. ``ALCHEMY_URL`` — a full endpoint, for anyone using a non-default host
    3. ``ALCHEMY_API_KEY`` — combined with the network's Alchemy subdomain
    4. the network's keyless public endpoint

    The fallback is what makes the toolkit usable in the first minute. It is rate
    limited and unauthenticated; `doctor` says so rather than letting you discover it
    under load.
    """
    net = get_network(network)
    environ = os.environ if env is None else env

    if rpc_url:
        return Endpoint(url=rpc_url, source="--rpc-url", network=net)

    explicit_url = environ.get(ALCHEMY_URL_ENV, "").strip()
    if explicit_url:
        return Endpoint(url=explicit_url, source=ALCHEMY_URL_ENV, network=net)

    api_key = environ.get(ALCHEMY_KEY_ENV, "").strip()
    if api_key:
        return Endpoint(url=net.alchemy_url(api_key), source=ALCHEMY_KEY_ENV, network=net)

    return Endpoint(url=net.public_rpc, source="public fallback", network=net)
