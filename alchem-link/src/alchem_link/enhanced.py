"""Alchemy's Enhanced APIs, and the honest line between what needs a key and what does not.

The Enhanced APIs are the reason to point this toolkit at Alchemy rather than any public
node. ``alchemy_getTokenBalances`` answers "what does this address hold?" in one call —
a question standard JSON-RPC cannot answer at all, because there is no way to enumerate
ERC-20 contracts from an address. You would have to scan every ``Transfer`` log ever
emitted. ``alchemy_getAssetTransfers`` does the same for history.

The split this module keeps sharp:

* **Discovery needs Alchemy.** Which tokens an address holds, and what moved when.
  There is no keyless substitute, and this module says so rather than silently
  returning nothing.
* **Valuation does not.** Once you know the token addresses, ``balanceOf``, ``symbol``
  and ``decimals`` are plain ERC-20 calls that work against any node — and pricing them
  is a Chainlink read this package already does. So :func:`value_holdings` works keyless
  against a token list you supply, and only *finding* that list needs the key.

That matters for a toolkit that advertises working without an API key. The keyless path
is not a crippled demo; it is everything except enumeration.
"""
from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional

from .abi import to_checksum_address
from .feeds import FEEDS, read_feed
from .multicall import Call, batch_call
from .networks import ALCHEMY_KEY_ENV, DEFAULT_NETWORK
from .rpc import RpcClient, RpcError, client_for

#: Alchemy caps a single getTokenBalances call; chunk anything larger.
MAX_TOKENS_PER_CALL = 100


class NeedsAlchemyKey(RuntimeError):
    """Raised when a call has no keyless equivalent and no key is configured."""

    def __init__(self, method: str) -> None:
        super().__init__(
            f"{method} is an Alchemy Enhanced API and the current endpoint is not "
            f"authenticated. Set {ALCHEMY_KEY_ENV} (or ALCHEMY_URL) and retry. "
            "Standard JSON-RPC has no equivalent — token holdings cannot be enumerated "
            "from chain state alone."
        )


@dataclass
class TokenBalance:
    """One ERC-20 position, valued where a Chainlink feed exists for it."""
    contract: str
    raw_balance: int
    symbol: str = ""
    name: str = ""
    decimals: int = 18
    price_usd: Optional[float] = None
    price_pair: str = ""
    price_stale: bool = False

    @property
    def balance(self) -> float:
        return self.raw_balance / (10 ** self.decimals)

    @property
    def value_usd(self) -> Optional[float]:
        return None if self.price_usd is None else self.balance * self.price_usd

    def as_dict(self) -> Dict[str, Any]:
        return {
            "contract": self.contract,
            "symbol": self.symbol,
            "name": self.name,
            "decimals": self.decimals,
            "raw_balance": self.raw_balance,
            "balance": self.balance,
            "price_usd": self.price_usd,
            "price_pair": self.price_pair,
            "price_stale": self.price_stale,
            "value_usd": self.value_usd,
        }


@dataclass
class Holdings:
    """An address's positions, and how much of the total could actually be priced."""
    address: str
    network: str
    tokens: List[TokenBalance] = field(default_factory=list)
    native_raw: int = 0
    native_symbol: str = ""
    native_price_usd: Optional[float] = None
    discovered_via: str = "supplied list"
    notes: List[str] = field(default_factory=list)

    @property
    def native_balance(self) -> float:
        return self.native_raw / 1e18

    @property
    def native_value_usd(self) -> Optional[float]:
        if self.native_price_usd is None:
            return None
        return self.native_balance * self.native_price_usd

    @property
    def priced(self) -> List[TokenBalance]:
        return [t for t in self.tokens if t.value_usd is not None]

    @property
    def unpriced(self) -> List[TokenBalance]:
        return [t for t in self.tokens if t.value_usd is None and t.raw_balance > 0]

    @property
    def total_usd(self) -> float:
        total = sum(t.value_usd or 0.0 for t in self.tokens)
        return total + (self.native_value_usd or 0.0)

    @property
    def coverage(self) -> str:
        """How much of the portfolio the registry could price.

        Stated explicitly because an unpriced token is not a zero-value token, and a
        total that quietly omits them is worse than no total.
        """
        held = [t for t in self.tokens if t.raw_balance > 0]
        if not held:
            return "no ERC-20 positions"
        return f"{len(self.priced)}/{len(held)} positions priced by a Chainlink feed"

    def as_dict(self) -> Dict[str, Any]:
        return {
            "address": self.address,
            "network": self.network,
            "discovered_via": self.discovered_via,
            "native_symbol": self.native_symbol,
            "native_balance": self.native_balance,
            "native_value_usd": self.native_value_usd,
            "total_usd": self.total_usd,
            "coverage": self.coverage,
            "tokens": [t.as_dict() for t in self.tokens if t.raw_balance > 0],
            "notes": self.notes,
        }


def is_authenticated(client: RpcClient) -> bool:
    return client.endpoint.is_authenticated


def get_token_balances(
    address: str,
    network: str = DEFAULT_NETWORK,
    client: Optional[RpcClient] = None,
    rpc_url: Optional[str] = None,
) -> List[str]:
    """Discover which ERC-20 contracts ``address`` holds. Requires Alchemy.

    Returns contract addresses with a non-zero balance. Raises :class:`NeedsAlchemyKey`
    rather than returning an empty list, because "holds nothing" and "cannot look" are
    very different answers.
    """
    rpc = client or client_for(network=network, rpc_url=rpc_url)
    if not is_authenticated(rpc):
        raise NeedsAlchemyKey("alchemy_getTokenBalances")

    result = rpc.call("alchemy_getTokenBalances", [address, "erc20"]).result
    balances = (result or {}).get("tokenBalances", []) if isinstance(result, dict) else []
    held: List[str] = []
    for entry in balances:
        raw = entry.get("tokenBalance") or "0x0"
        try:
            if int(raw, 16) > 0:
                held.append(to_checksum_address(entry["contractAddress"]))
        except (ValueError, KeyError):
            continue
    return held


def get_asset_transfers(
    network: str = DEFAULT_NETWORK,
    from_address: Optional[str] = None,
    to_address: Optional[str] = None,
    categories: Optional[List[str]] = None,
    max_count: int = 25,
    client: Optional[RpcClient] = None,
    rpc_url: Optional[str] = None,
) -> List[Dict[str, Any]]:
    """Transfer history for an address. Requires Alchemy."""
    rpc = client or client_for(network=network, rpc_url=rpc_url)
    if not is_authenticated(rpc):
        raise NeedsAlchemyKey("alchemy_getAssetTransfers")

    params: Dict[str, Any] = {
        "fromBlock": "0x0",
        "toBlock": "latest",
        "category": categories or ["external", "erc20"],
        "maxCount": hex(max_count),
        "order": "desc",
        "withMetadata": True,
    }
    if from_address:
        params["fromAddress"] = from_address
    if to_address:
        params["toAddress"] = to_address

    result = rpc.call("alchemy_getAssetTransfers", [params]).result
    return (result or {}).get("transfers", []) if isinstance(result, dict) else []


def _price_for_symbol(symbol: str, network: str, rpc: RpcClient) -> tuple:
    """Find a registered USD feed for a token symbol. Returns (price, pair, stale)."""
    table = FEEDS.get(network.lower(), {})
    for candidate in (f"{symbol.upper()}/USD", f"{symbol.upper().lstrip('W')}/USD"):
        if candidate in table:
            try:
                reading = read_feed(candidate, network=network, client=rpc)
                return reading.price, candidate, reading.stale
            except Exception:
                return None, "", False
    return None, "", False


def value_holdings(
    address: str,
    tokens: Optional[List[str]] = None,
    network: str = DEFAULT_NETWORK,
    client: Optional[RpcClient] = None,
    rpc_url: Optional[str] = None,
    discover: bool = True,
) -> Holdings:
    """Read balances for ``tokens`` and price them through Chainlink.

    With ``discover`` and an Alchemy key, the token list is found automatically. Without
    either, pass ``tokens`` explicitly — the read and the valuation are plain JSON-RPC
    and work against any endpoint.
    """
    from .networks import get_network

    net = get_network(network)
    rpc = client or client_for(network=network, rpc_url=rpc_url)
    holdings = Holdings(
        address=to_checksum_address(address),
        network=net.key,
        native_symbol=net.native_symbol,
    )

    contracts = list(tokens or [])
    if not contracts and discover:
        try:
            contracts = get_token_balances(address, network=net.key, client=rpc)
            holdings.discovered_via = "alchemy_getTokenBalances"
        except NeedsAlchemyKey as exc:
            holdings.notes.append(str(exc))
        except RpcError as exc:
            holdings.notes.append(f"token discovery failed: {exc}")

    # Native balance is one standard call, always available.
    try:
        holdings.native_raw = int(rpc.call("eth_getBalance", [address, "latest"]).result, 16)
    except Exception as exc:
        holdings.notes.append(f"native balance unavailable: {exc}")

    from .gas import NATIVE_FEEDS

    native_pair = NATIVE_FEEDS.get(net.key)
    if native_pair and native_pair in FEEDS.get(net.key, {}):
        try:
            holdings.native_price_usd = read_feed(native_pair, network=net.key, client=rpc).price
        except Exception:
            pass

    if not contracts:
        return holdings

    calls: List[Call] = []
    for contract in contracts:
        calls.append(Call(contract, "balanceOf(address)", (address,), ["uint256"], f"{contract}|bal"))
        calls.append(Call(contract, "symbol()", (), ["string"], f"{contract}|sym"))
        calls.append(Call(contract, "decimals()", (), ["uint8"], f"{contract}|dec"))
        calls.append(Call(contract, "name()", (), ["string"], f"{contract}|name"))

    report = batch_call(rpc, calls)
    for contract in contracts:
        balance = report.by_label(f"{contract}|bal")
        if not balance or not balance.success:
            continue
        symbol_result = report.by_label(f"{contract}|sym")
        decimals_result = report.by_label(f"{contract}|dec")
        name_result = report.by_label(f"{contract}|name")

        symbol = str(symbol_result.one("") if symbol_result else "") or "?"
        token = TokenBalance(
            contract=to_checksum_address(contract),
            raw_balance=int(balance.one(0)),
            symbol=symbol,
            name=str(name_result.one("") if name_result else ""),
            decimals=int(decimals_result.one(18) if decimals_result else 18),
        )
        if token.raw_balance > 0:
            price, pair, stale = _price_for_symbol(symbol, net.key, rpc)
            token.price_usd, token.price_pair, token.price_stale = price, pair, stale
        holdings.tokens.append(token)

    return holdings


def summarize_alchemy_capabilities(
    network: str = DEFAULT_NETWORK,
    client: Optional[RpcClient] = None,
) -> Dict[str, Any]:
    """Report which Alchemy features the *current* endpoint can actually use.

    This replaced a hardcoded dict of marketing prose. The useful question is not what
    Alchemy offers in general — it is whether the endpoint you are pointed at right now
    can answer these calls, which depends on your key.
    """
    rpc = client or client_for(network=network)
    authenticated = is_authenticated(rpc)

    features = [
        {
            "method": "eth_feeHistory",
            "capability": "EIP-1559 base-fee trend and priority-fee percentiles",
            "needs_key": False,
        },
        {
            "method": "eth_call via Multicall3",
            "capability": "batched, block-atomic contract reads",
            "needs_key": False,
        },
        {
            "method": "alchemy_getTokenBalances",
            "capability": "enumerate an address's ERC-20 holdings",
            "needs_key": True,
        },
        {
            "method": "alchemy_getAssetTransfers",
            "capability": "indexed transfer history without log scanning",
            "needs_key": True,
        },
    ]
    for feature in features:
        feature["available"] = authenticated or not feature["needs_key"]

    return {
        "network": rpc.endpoint.network.key,
        "endpoint": rpc.endpoint.redacted(),
        "source": rpc.endpoint.source,
        "authenticated": authenticated,
        "features": features,
        "hint": (
            "" if authenticated
            else f"Set {ALCHEMY_KEY_ENV} to enable the enumeration APIs. Everything else "
                 "works on the keyless endpoint."
        ),
    }
