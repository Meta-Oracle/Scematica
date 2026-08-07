"""EIP-1559 fee intelligence, priced in dollars through the chain's own oracle.

``eth_gasPrice`` returns one number and hides the structure. Since EIP-1559 a fee is two
parts with different dynamics: a **base fee** the protocol sets deterministically from
how full the last block was and then burns, and a **priority fee** you bid to the
proposer. Quoting their sum as "the gas price" makes it impossible to see that the base
fee is climbing while tips are flat, which is the difference between "wait two minutes"
and "bid higher".

``eth_feeHistory`` returns both, per block, with priority fees already bucketed into
percentiles. Two things fall out of it that a single ``gasPrice`` cannot give you:

* **The next block's base fee is not a forecast.** EIP-1559 fixes it as a function of
  the parent block, so the node returns it as fact — ``baseFeePerGas`` has one more entry
  than there are blocks. This module labels it accordingly rather than dressing it up as
  a prediction.
* **Congestion is visible.** ``gasUsedRatio`` above 0.5 means blocks are more than half
  full and the base fee is rising; below, it is falling. The trend matters more than the
  level.

And because this package already reads Chainlink, the estimate comes out in **dollars**:
the native-token feed on the same chain converts wei to USD. That is the whole
Alchemy-plus-Chainlink premise in one function — chain state from the node, valuation
from the oracle — rather than a paragraph claiming the two compose.
"""
from __future__ import annotations

import statistics
from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional

from .feeds import FEEDS, read_feed
from .networks import DEFAULT_NETWORK, get_network
from .rpc import RpcClient, client_for, gwei

#: Percentiles requested from eth_feeHistory: a frugal bid, the median, and a fast one.
DEFAULT_PERCENTILES = [10.0, 50.0, 90.0]

#: Blocks of history to sample. Twenty is enough to see a trend without paying for it.
DEFAULT_BLOCKS = 20

#: Intrinsic gas for a plain native transfer — the floor any fee estimate is measured in.
GAS_TRANSFER = 21_000
#: Rough gas for an ERC-20 transfer and for a typical AMM swap. Order-of-magnitude
#: figures for costing, not a substitute for eth_estimateGas on your actual calldata.
GAS_ERC20_TRANSFER = 65_000
GAS_SWAP = 180_000

#: Which registered feed prices each chain's native token.
NATIVE_FEEDS = {
    "ethereum": "ETH/USD",
    "sepolia": "ETH/USD",
    "base": "ETH/USD",
    "arbitrum": "ETH/USD",
    "optimism": "ETH/USD",
    "scroll": "ETH/USD",
    "linea": "ETH/USD",
    "polygon": "MATIC/USD",
    "avalanche": "AVAX/USD",
    "bnb": "BNB/USD",
    "gnosis": "DAI/USD",  # xDAI is DAI-denominated, so this is the right native price
}


@dataclass
class FeeEstimate:
    """One priority tier: what to bid, and what it costs."""
    label: str
    priority_fee_wei: int
    base_fee_wei: int

    @property
    def max_fee_wei(self) -> int:
        """Base fee doubled plus the tip — headroom for a few blocks of base-fee rise."""
        return self.base_fee_wei * 2 + self.priority_fee_wei

    @property
    def total_wei(self) -> int:
        return self.base_fee_wei + self.priority_fee_wei

    def cost_wei(self, gas: int) -> int:
        return self.total_wei * gas

    def as_dict(self, native_usd: Optional[float] = None) -> Dict[str, Any]:
        payload: Dict[str, Any] = {
            "label": self.label,
            "priority_fee_gwei": round(gwei(self.priority_fee_wei), 4),
            "max_fee_gwei": round(gwei(self.max_fee_wei), 4),
            "total_gwei": round(gwei(self.total_wei), 4),
            "transfer_cost_native": self.cost_wei(GAS_TRANSFER) / 1e18,
        }
        if native_usd:
            payload["transfer_cost_usd"] = self.cost_wei(GAS_TRANSFER) / 1e18 * native_usd
            payload["swap_cost_usd"] = self.cost_wei(GAS_SWAP) / 1e18 * native_usd
        return payload


@dataclass
class GasReport:
    """What the last N blocks say about what a transaction will cost."""
    network: str
    native_symbol: str
    base_fee_wei: int
    next_base_fee_wei: int
    blocks_sampled: int
    gas_used_ratios: List[float] = field(default_factory=list)
    tiers: List[FeeEstimate] = field(default_factory=list)
    native_usd: Optional[float] = None
    native_price_stale: bool = False
    price_error: str = ""

    @property
    def congestion(self) -> float:
        """Mean fullness of the sampled blocks. 0.5 is the EIP-1559 equilibrium."""
        return statistics.mean(self.gas_used_ratios) if self.gas_used_ratios else 0.0

    @property
    def trend(self) -> str:
        """Where the base fee is heading, from the protocol's own adjustment rule."""
        if not self.base_fee_wei:
            return "flat"
        change = (self.next_base_fee_wei - self.base_fee_wei) / self.base_fee_wei
        if change > 0.02:
            return "rising"
        if change < -0.02:
            return "falling"
        return "flat"

    @property
    def trend_detail(self) -> str:
        pct = (
            (self.next_base_fee_wei - self.base_fee_wei) / self.base_fee_wei * 100
            if self.base_fee_wei else 0.0
        )
        return (
            f"blocks {self.congestion * 100:.0f}% full on average; next base fee "
            f"{gwei(self.next_base_fee_wei):.4f} gwei ({pct:+.1f}%)"
        )

    def tier(self, label: str) -> Optional[FeeEstimate]:
        for entry in self.tiers:
            if entry.label == label:
                return entry
        return None

    def as_dict(self) -> Dict[str, Any]:
        return {
            "network": self.network,
            "native_symbol": self.native_symbol,
            "base_fee_gwei": round(gwei(self.base_fee_wei), 4),
            "next_base_fee_gwei": round(gwei(self.next_base_fee_wei), 4),
            "trend": self.trend,
            "trend_detail": self.trend_detail,
            "congestion": round(self.congestion, 4),
            "blocks_sampled": self.blocks_sampled,
            "native_usd": self.native_usd,
            "native_price_stale": self.native_price_stale,
            "price_error": self.price_error,
            "gas_units": {
                "native_transfer": GAS_TRANSFER,
                "erc20_transfer": GAS_ERC20_TRANSFER,
                "swap": GAS_SWAP,
            },
            "tiers": [tier.as_dict(self.native_usd) for tier in self.tiers],
        }


def _int(value: Any) -> int:
    if isinstance(value, int):
        return value
    return int(str(value), 16)


def analyse_gas(
    network: str = DEFAULT_NETWORK,
    blocks: int = DEFAULT_BLOCKS,
    client: Optional[RpcClient] = None,
    rpc_url: Optional[str] = None,
    price_in_usd: bool = True,
) -> GasReport:
    """Sample recent fee history and turn it into actionable tiers.

    Falls back to ``eth_gasPrice`` on chains that do not implement ``eth_feeHistory`` —
    which is most pre-1559 forks. The report then carries a single tier and a flat trend
    rather than pretending to a structure the chain does not have.
    """
    net = get_network(network)
    rpc = client or client_for(network=network, rpc_url=rpc_url)

    report = GasReport(
        network=net.key,
        native_symbol=net.native_symbol,
        base_fee_wei=0,
        next_base_fee_wei=0,
        blocks_sampled=0,
    )

    try:
        history = rpc.fee_history(blocks=blocks, percentiles=DEFAULT_PERCENTILES)
        base_fees = [_int(v) for v in history.get("baseFeePerGas", [])]
        rewards = [[_int(v) for v in row] for row in history.get("reward", []) or []]
        report.gas_used_ratios = [float(r) for r in history.get("gasUsedRatio", []) or []]
    except Exception:
        base_fees, rewards = [], []

    if len(base_fees) >= 2:
        # baseFeePerGas carries one entry more than the number of blocks: the last is the
        # next block's base fee, already determined by the protocol.
        report.base_fee_wei = base_fees[-2]
        report.next_base_fee_wei = base_fees[-1]
        report.blocks_sampled = len(base_fees) - 1
    else:
        # Pre-1559 chain, or a node without feeHistory. One number is all there is.
        price = rpc.gas_price_wei()
        report.base_fee_wei = price
        report.next_base_fee_wei = price

    if rewards:
        columns = list(zip(*rewards))
        labels = ["slow", "standard", "fast"]
        for label, column in zip(labels, columns):
            usable = [v for v in column if v > 0]
            report.tiers.append(FeeEstimate(
                label=label,
                # Median across blocks, not mean: one block with an outlier tip should
                # not move the recommendation.
                priority_fee_wei=int(statistics.median(usable)) if usable else 0,
                base_fee_wei=report.next_base_fee_wei,
            ))
    else:
        report.tiers.append(FeeEstimate("standard", 0, report.next_base_fee_wei))

    if price_in_usd:
        pair = NATIVE_FEEDS.get(net.key)
        if pair and pair in FEEDS.get(net.key, {}):
            try:
                reading = read_feed(pair, network=net.key, client=rpc)
                report.native_usd = reading.price
                report.native_price_stale = reading.stale
            except Exception as exc:
                report.price_error = str(exc)
        else:
            report.price_error = f"no native-token feed registered for {net.key}"

    return report
