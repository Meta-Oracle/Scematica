"""An oracle-consumer lint: every way a live, responding feed can still be wrong.

``latestRoundData()`` succeeding tells you almost nothing. It succeeds when the feed has
not published in a day, when the answer is pinned against a circuit breaker, when the
round is a carried-over duplicate, and — on an L2 — when the sequencer has been down for
an hour and the price is a fossil. Each of those has cost real protocols real money, and
none of them raise.

This module runs the checks a careful consumer contract would, against a live feed, and
grades what it finds. The checks, and the incident each one is drawn from:

``BOUNDED_ANSWER``
    An aggregator has ``minAnswer``/``maxAnswer`` circuit breakers and *cannot* report
    outside them. In the LUNA collapse the price fell through the floor, and the feed
    kept returning the floor — fresh, well-formed, and off by orders of magnitude.
    Protocols that treated it as spot were drained. This is the finding this module
    exists for, and it is invisible unless you resolve the proxy and read the bounds off
    the implementation.

``STALE`` / ``NON_POSITIVE`` / ``INCOMPLETE_ROUND`` / ``CARRIED_ROUND``
    The four conditions Chainlink's own consumer guidance says to reject. All four
    return successfully.

``SEQUENCER_DOWN`` / ``SEQUENCER_GRACE`` / ``L2_NO_SEQUENCER_CHECK``
    See :mod:`alchem_link.sequencer`. On an L2 this is not optional.

``DECIMALS_MISMATCH`` / ``DESCRIPTION_MISMATCH``
    Consumers hardcode ``1e8``. A feed with 18 decimals then reads as 1e10 times its
    real value. And an address filed under the wrong pair is a mislabel that survives
    every test you write against your own registry — only the chain can catch it.

Severity is about consequence, not confidence: ``critical`` means acting on this price
right now can lose money.
"""
from __future__ import annotations

import time
from dataclasses import dataclass, field
from typing import Any, Dict, List, Optional

from .aggregator import AggregatorInfo, describe_aggregator
from .feeds import DEFAULT_HEARTBEAT_SECS, Feed, get_feed, list_feeds
from .keccak import is_checksum_address, to_checksum_address
from .networks import DEFAULT_NETWORK, get_network
from .rpc import RpcClient, client_for
from .sequencer import is_l2, read_sequencer

SEVERITIES = ("critical", "high", "medium", "low", "info")
_SEVERITY_RANK = {name: index for index, name in enumerate(SEVERITIES)}


@dataclass
class Finding:
    """One graded problem, with the fix."""
    code: str
    severity: str
    title: str
    detail: str
    remedy: str = ""

    def as_dict(self) -> Dict[str, Any]:
        return {
            "code": self.code,
            "severity": self.severity,
            "title": self.title,
            "detail": self.detail,
            "remedy": self.remedy,
        }


@dataclass
class Audit:
    """The findings for one feed, worst first."""
    pair: str
    network: str
    address: str
    description: str = ""
    price: Optional[float] = None
    findings: List[Finding] = field(default_factory=list)
    info: Optional[AggregatorInfo] = None
    checks_run: int = 0

    def add(self, finding: Finding) -> None:
        self.findings.append(finding)

    @property
    def sorted_findings(self) -> List[Finding]:
        return sorted(self.findings, key=lambda f: _SEVERITY_RANK[f.severity])

    @property
    def worst(self) -> str:
        """Worst severity present, or ``ok`` when nothing above ``info`` was found."""
        blocking = [f for f in self.findings if f.severity != "info"]
        if not blocking:
            return "ok"
        return min(blocking, key=lambda f: _SEVERITY_RANK[f.severity]).severity

    @property
    def safe_to_consume(self) -> bool:
        """False when anything at ``medium`` or worse was found."""
        return all(_SEVERITY_RANK[f.severity] > _SEVERITY_RANK["medium"] for f in self.findings)

    def count(self, severity: str) -> int:
        return sum(1 for f in self.findings if f.severity == severity)

    def as_dict(self) -> Dict[str, Any]:
        return {
            "pair": self.pair,
            "network": self.network,
            "address": self.address,
            "description": self.description,
            "price": self.price,
            "worst": self.worst,
            "safe_to_consume": self.safe_to_consume,
            "checks_run": self.checks_run,
            "findings": [f.as_dict() for f in self.sorted_findings],
        }


def _check_answer(audit: Audit, info: AggregatorInfo, heartbeat: int, now: int) -> None:
    latest = info.latest
    if latest is None:
        audit.add(Finding(
            "UNREADABLE", "critical",
            "latestRoundData() did not return a usable round",
            "; ".join(info.notes) or "the call failed or returned an unexpected shape",
            "Confirm the address is an AggregatorV3Interface proxy on this network.",
        ))
        return

    audit.price = latest.price

    if latest.answer <= 0:
        audit.add(Finding(
            "NON_POSITIVE", "critical",
            "the feed is reporting a non-positive answer",
            f"answer = {latest.answer}. A price of zero or below is never a real quote.",
            "require(answer > 0) before using the value.",
        ))

    if latest.updated_at == 0:
        audit.add(Finding(
            "INCOMPLETE_ROUND", "high",
            "the round has not completed",
            "updatedAt == 0, which marks a round that was started but never finalised.",
            "require(updatedAt != 0).",
        ))
    else:
        age = max(0, now - latest.updated_at)
        if age > heartbeat:
            overdue = age / heartbeat if heartbeat else 0
            audit.add(Finding(
                "STALE", "critical" if overdue >= 3 else "high",
                f"last update is {age}s old against a {heartbeat}s heartbeat",
                f"The feed is {overdue:.1f}x past its expected publish interval. "
                "It answers normally; the answer is simply old.",
                f"require(block.timestamp - updatedAt <= {heartbeat}).",
            ))

    if latest.carried_over:
        audit.add(Finding(
            "CARRIED_ROUND", "medium",
            "this round carries an answer from an earlier round",
            f"answeredInRound ({latest.answered_in_round}) < roundId ({latest.round_id}), "
            "so no fresh answer was produced for this round.",
            "require(answeredInRound >= roundId).",
        ))


def _check_bounds(audit: Audit, info: AggregatorInfo) -> None:
    if info.min_answer is None and info.max_answer is None:
        audit.add(Finding(
            "BOUNDS_UNKNOWN", "low",
            "answer bounds could not be read",
            "The implementation exposes neither minAnswer() nor maxAnswer(), so whether "
            "a circuit breaker exists cannot be determined from chain state.",
            "Check the aggregator's source on the block explorer before relying on it.",
        ))
        return

    floor, ceiling = info.floor_headroom, info.ceiling_headroom

    if not info.bounds_are_binding:
        # Say so out loud. Current Chainlink deployments set minAnswer to 1 and maxAnswer
        # to the int192 extreme, so this check passes on every feed in the registry — and
        # a check that is silent when it passes is indistinguishable from one that never
        # ran. It earns its keep against *custom* AggregatorV3 wrappers, which protocols
        # deploy constantly and which do sometimes carry narrow bounds.
        if floor is not None or ceiling is not None:
            audit.add(Finding(
                "BOUNDS_WIDE", "info",
                "circuit breakers are far from the current price",
                f"the price would have to fall {floor:.3g}x to reach minAnswer"
                if floor is not None else "no lower bound is set",
                "",
            ))
        return

    parts = []
    if floor is not None and floor < 100:
        parts.append(f"the price need only fall {floor:.1f}x to reach minAnswer ({info.min_price:,.6g})")
    if ceiling is not None and ceiling < 100:
        parts.append(f"a {ceiling:.1f}x rise reaches maxAnswer ({info.max_price:,.6g})")

    tightest = min([v for v in (floor, ceiling) if v is not None], default=1e9)
    audit.add(Finding(
        "BOUNDED_ANSWER", "critical" if tightest < 10 else "high",
        "the aggregator's circuit breakers are close to the current price",
        "; ".join(parts) + ". Past a bound the feed reports the bound itself — a value "
        "that stays fresh and well-formed while being arbitrarily wrong. This is the "
        "LUNA failure mode.",
        "Reject answers at or adjacent to minAnswer/maxAnswer rather than trusting them, "
        "and pair the feed with an independent source for tail moves.",
    ))


def _check_registry(
    audit: Audit,
    info: AggregatorInfo,
    expected_pair: Optional[str],
    expected_decimals: Optional[int],
) -> None:
    on_chain = info.description.replace(" ", "").upper()
    audit.description = info.description

    # Deliberately keyed on what the *caller* asserted, not on a registry hit. Auditing
    # an arbitrary address as "ETH/USD" is exactly when a mislabel matters most — there
    # is no registry entry to have caught it earlier.
    if expected_pair and on_chain and on_chain != expected_pair.replace(" ", "").upper():
        audit.add(Finding(
            "DESCRIPTION_MISMATCH", "high",
            "the contract reports a different pair than the one requested",
            f"asked for {expected_pair!r}, the contract says {info.description!r}.",
            "Use the name the contract answers to, or a different address.",
        ))

    if expected_decimals is not None and info.decimals and expected_decimals != info.decimals:
        audit.add(Finding(
            "DECIMALS_MISMATCH", "high",
            "declared decimals do not match the contract",
            f"registry says {expected_decimals}, the contract says {info.decimals}.",
            "Read decimals() rather than hardcoding 1e8.",
        ))

    if info.decimals and info.decimals != 8:
        audit.add(Finding(
            "NON_STANDARD_DECIMALS", "medium",
            f"this feed uses {info.decimals} decimals, not the usual 8",
            "USD pairs are conventionally 8 and ETH pairs 18, and consumers routinely "
            f"hardcode 1e8. Against this feed that is off by 1e{abs(info.decimals - 8)}.",
            "Scale by decimals() read from the contract.",
        ))

    if not is_checksum_address(audit.address):
        audit.add(Finding(
            "ADDRESS_NOT_CHECKSUMMED", "info",
            "the address is not EIP-55 checksummed",
            f"Checksummed form: {to_checksum_address(audit.address)}. Mixed-case addresses "
            "are typo-detecting; all-lowercase ones are not.",
            "Store the checksummed form.",
        ))

    if not info.is_proxy:
        audit.add(Finding(
            "NOT_A_PROXY", "low",
            "this address is an aggregator, not a proxy",
            "It has no aggregator() function, so it is the implementation itself. When "
            "Chainlink migrates the feed this address stops updating rather than "
            "following the migration.",
            "Consume the EACAggregatorProxy address instead.",
        ))


def _check_sequencer(audit: Audit, network: str, client: RpcClient, now: int) -> None:
    if not is_l2(network):
        return

    status = read_sequencer(network, client=client, now=now)
    if status is None:
        audit.add(Finding(
            "L2_NO_SEQUENCER_CHECK", "high",
            "no sequencer uptime feed is registered for this L2",
            "On an L2 a price feed stops updating while the sequencer is down, but keeps "
            "answering with the last price. Without an uptime feed that outage is "
            "invisible to a consumer.",
            "Find this chain's Chainlink uptime feed and gate price reads on it.",
        ))
        return

    if status.error:
        audit.add(Finding(
            "SEQUENCER_UNKNOWN", "high",
            "the sequencer uptime feed could not be read",
            status.detail,
            "Treat an unreadable uptime feed as down.",
        ))
    elif not status.up:
        audit.add(Finding(
            "SEQUENCER_DOWN", "critical",
            "the L2 sequencer is down",
            status.detail + ". The price feed is frozen while the market moves.",
            "Halt price-dependent actions until the sequencer is up and past its grace period.",
        ))
    elif status.in_grace_period:
        audit.add(Finding(
            "SEQUENCER_GRACE", "high",
            "the sequencer restarted recently",
            status.detail + ". Transactions queued during the outage are executing now, "
            "against a price that did not move during it.",
            f"require(block.timestamp - startedAt > {status.grace_period_secs}).",
        ))
    else:
        audit.add(Finding(
            "SEQUENCER_OK", "info",
            "L2 sequencer is up and past its grace period",
            status.detail,
            "Keep the check in the consumer anyway — this is a point-in-time reading.",
        ))


def audit_feed(
    pair: str,
    network: str = DEFAULT_NETWORK,
    client: Optional[RpcClient] = None,
    rpc_url: Optional[str] = None,
    address: Optional[str] = None,
    heartbeat_secs: Optional[int] = None,
    now: Optional[int] = None,
) -> Audit:
    """Run every consumer-safety check against one feed.

    ``pair`` is looked up in the registry unless ``address`` is given, which lets the
    audit run against a feed this package has never heard of.
    """
    get_network(network)
    rpc = client or client_for(network=network, rpc_url=rpc_url)
    current = int(time.time()) if now is None else now

    feed: Optional[Feed] = None
    if address is None:
        feed = get_feed(pair, network)
        address = feed.address
        heartbeat = heartbeat_secs or feed.heartbeat_secs
    else:
        heartbeat = heartbeat_secs or DEFAULT_HEARTBEAT_SECS

    audit = Audit(pair=pair, network=network.lower(), address=address)
    info = describe_aggregator(address, network=network, client=rpc)
    audit.info = info

    _check_answer(audit, info, heartbeat, current)
    _check_bounds(audit, info)
    _check_registry(
        audit,
        info,
        expected_pair=pair if pair and "/" in pair else None,
        expected_decimals=feed.decimals if feed else None,
    )
    _check_sequencer(audit, network, rpc, current)
    # Seven categories, run whether or not they fire — the count is what makes "no
    # findings" mean something.
    audit.checks_run = 7
    return audit


def audit_network(
    network: str = DEFAULT_NETWORK,
    client: Optional[RpcClient] = None,
    rpc_url: Optional[str] = None,
    now: Optional[int] = None,
) -> List[Audit]:
    """Audit every registered feed on a network, worst-graded first."""
    rpc = client or client_for(network=network, rpc_url=rpc_url)
    audits = [
        audit_feed(feed.pair, network=network, client=rpc, now=now)
        for feed in list_feeds(network)
    ]
    return sorted(
        audits,
        key=lambda a: (_SEVERITY_RANK.get(a.worst, len(SEVERITIES)), a.pair),
    )
