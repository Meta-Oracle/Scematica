"""L2 sequencer uptime feeds — the check almost nobody writes.

On an L2, a Chainlink price feed can be perfectly fresh and still unsafe. If the
sequencer goes down, the feed stops updating while the *market* keeps moving. When the
sequencer comes back, every transaction queued during the outage executes at once,
against a price that spent the outage frozen. Liquidations fire on prices that no longer
exist, and there is no way to see it from ``latestRoundData()`` — the answer looks
normal, because it is the last normal answer.

Chainlink publishes a separate uptime feed per L2 for exactly this, and its consumer
documentation requires two checks, not one:

* ``answer == 0`` — the sequencer is up. ``1`` means down.
* ``block.timestamp - startedAt > GRACE_PERIOD`` — it has been up *long enough*. The
  moment after a restart is the most dangerous one, not the safest.

The second check is the one that gets skipped. A contract that tests only ``answer == 0``
reopens exactly during the queue flush it was written to survive.

Addresses below were read from their live contracts: each reports
``L2 Sequencer Uptime Status Feed`` as its own ``description()``.
"""
from __future__ import annotations

import time
from dataclasses import dataclass
from typing import Any, Dict, List, Optional

from .multicall import Call, batch_call
from .rpc import RpcClient, client_for

#: Chainlink's documented recommendation. It is a floor, not a ceiling — a protocol with
#: slower liquidations should hold off longer.
GRACE_PERIOD_SECS = 3600

_ROUND_RETURNS = ["uint80", "int256", "uint256", "uint256", "uint80"]

#: network key → uptime feed address. Verified live; each answers to
#: ``description() == "L2 Sequencer Uptime Status Feed"``.
SEQUENCER_FEEDS: Dict[str, str] = {
    "arbitrum": "0xFdB631F5EE196F0ed6FAa767959853A9F217697D",
    "optimism": "0x371EAD81c9102C9BF4874A9075FFFf170F2Ee389",
    "base": "0xBCF85224fc0756B9Fa45aA7892530B47e10b6433",
}

#: Networks where a price consumer *must* also check the sequencer. Kept separate from
#: SEQUENCER_FEEDS: an L2 with no uptime feed registered here is still an L2, and that
#: gap is itself worth reporting rather than silently passing.
L2_NETWORKS = frozenset({"arbitrum", "optimism", "base", "scroll", "zksync", "linea", "metis"})


def is_l2(network: str) -> bool:
    return network.lower() in L2_NETWORKS


@dataclass
class SequencerStatus:
    """The uptime feed's verdict, with the grace period already applied."""
    network: str
    address: str
    up: bool
    started_at: int
    #: How long the current up/down state has held.
    since_secs: int
    grace_period_secs: int = GRACE_PERIOD_SECS
    error: str = ""

    @property
    def in_grace_period(self) -> bool:
        """Up, but not yet long enough to trust prices."""
        return self.up and self.since_secs <= self.grace_period_secs

    @property
    def ok(self) -> bool:
        """Safe to consume a price on this chain right now."""
        return not self.error and self.up and not self.in_grace_period

    @property
    def state(self) -> str:
        if self.error:
            return "UNKNOWN"
        if not self.up:
            return "DOWN"
        return "GRACE" if self.in_grace_period else "UP"

    @property
    def detail(self) -> str:
        if self.error:
            return self.error
        if not self.up:
            return f"sequencer has been DOWN for {self.since_secs}s — do not consume prices"
        if self.in_grace_period:
            remaining = self.grace_period_secs - self.since_secs
            return (
                f"sequencer came back {self.since_secs}s ago; "
                f"{remaining}s of the {self.grace_period_secs}s grace period remain"
            )
        return f"up for {self.since_secs}s, past the {self.grace_period_secs}s grace period"

    def as_dict(self) -> Dict[str, Any]:
        return {
            "network": self.network,
            "address": self.address,
            "up": self.up,
            "state": self.state,
            "started_at": self.started_at,
            "since_secs": self.since_secs,
            "grace_period_secs": self.grace_period_secs,
            "in_grace_period": self.in_grace_period,
            "ok": self.ok,
            "detail": self.detail,
            "error": self.error,
        }


def list_sequencer_feeds() -> List[Dict[str, str]]:
    return [{"network": net, "address": addr} for net, addr in sorted(SEQUENCER_FEEDS.items())]


def read_sequencer(
    network: str,
    client: Optional[RpcClient] = None,
    rpc_url: Optional[str] = None,
    grace_period_secs: int = GRACE_PERIOD_SECS,
    now: Optional[int] = None,
) -> Optional[SequencerStatus]:
    """Read the uptime feed for ``network``, or ``None`` if that chain has none.

    ``None`` means "not applicable" on L1 and "no feed registered" on an L2 — callers on
    an L2 should treat it as an unchecked risk, not a pass. :func:`is_l2` separates the
    two cases.
    """
    key = network.lower()
    address = SEQUENCER_FEEDS.get(key)
    if address is None:
        return None

    rpc = client or client_for(network=key, rpc_url=rpc_url)
    report = batch_call(rpc, [Call(address, "latestRoundData()", (), _ROUND_RETURNS, "round")])
    result = report.by_label("round")

    if result is None or not result.success:
        return SequencerStatus(
            network=key,
            address=address,
            up=False,
            started_at=0,
            since_secs=0,
            grace_period_secs=grace_period_secs,
            error=f"could not read the uptime feed: {result.error if result else 'no result'}",
        )

    _, answer, started_at, _, _ = result.values
    current = int(time.time()) if now is None else now

    if started_at == 0:
        # Documented case: the feed is in an invalid round and its answer means nothing.
        return SequencerStatus(
            network=key,
            address=address,
            up=False,
            started_at=0,
            since_secs=0,
            grace_period_secs=grace_period_secs,
            error="uptime feed reports startedAt == 0 (invalid round) — status is unknown",
        )

    return SequencerStatus(
        network=key,
        address=address,
        up=(answer == 0),
        started_at=started_at,
        since_secs=max(0, current - started_at),
        grace_period_secs=grace_period_secs,
    )
