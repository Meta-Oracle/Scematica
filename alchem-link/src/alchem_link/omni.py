"""An oracle network, described in Scematica Omni's vocabulary.

``alchem-link`` answers *what do these feeds currently say, and can they be believed*.
Omni answers *given a world, which of these branches is worth taking*. Until now nothing
joined them: this toolkit could tell an operator that three feeds were stale and the
sequencer had been up for four minutes, and the operator had to be the one who decided
what to do about it.

This module emits a ``scema_world::WorldState`` — as a plain dict, ready for
``json.dumps`` — so that a set of Chainlink aggregators becomes an environment omni can
reason over::

    $ alchem-link omni -n base | scema simulate "is this safe to price against" --path -

Four producers now sit on that wire format: ``RepoObserver`` in Rust, ``perceive.js`` in
the browser extension, ``scematica_mesh::omni`` in the bot workspace, and this. Only the
first is written in a language omni's crates can link, which is the entire reason the
contract is JSON rather than a trait.

Why hand-built rather than a binding
------------------------------------
There is no Rust in this package and there is not going to be. ``alchem-link`` is
stdlib-only by policy — no ``requests``, no ``web3``, a bundled Keccak because ``hashlib``
ships SHA3-256 with different padding — and a compiled dependency would end that. The
wire format is a JSON shape, so a producer needs nothing but ``dict``.

What keeps it honest is not a type. It is :func:`_check` below, which mirrors the
validation omni's own ``ImportObserver`` applies, so this package fails its own tests
rather than producing something the consumer rejects at run time.

Counts only
-----------
``scema-sim`` scores a real expected gain only from a signal whose ``measured`` flag is
true, so that flag is a claim that somebody counted something. Every signal here is a
count of feeds in a particular state — stale, mislabelled, reporting a non-positive
answer, carried over from an earlier round. Nothing estimates a severity, a probability
or an "oracle health score". A number like that invented here would be a hallucination
with a decimal point on it, laundered into a decision record that a third party can
verify but cannot second-guess.

What could not be read is a blind spot, not a zero
--------------------------------------------------
A feed whose aggregator did not answer is *absent*: no price, no age, no verdict. It goes
into ``blind_spots``, which ``scema-sim`` turns into measured uncertainty — so an agent
reasoning about a partly-readable oracle set is less confident and can say so with a
number. Rendering it as a price of ``0`` would be the same error as rendering an
unreadable vault balance as zero, and it is the error this whole toolkit exists to avoid.
"""
from __future__ import annotations

import time
from typing import Any, Dict, List, Optional, Sequence

from .feeds import FeedReading, list_feeds
from .networks import DEFAULT_NETWORK, get_network

#: Recorded in ``WorldState.observer``. Omni stamps it ``imported:alchem-link`` on the way
#: in, so a decision record can never claim a world that arrived down a pipe was observed
#: locally.
OBSERVER = "alchem-link"

#: The world-contract version this producer writes. Must track
#: ``scema_world::WORLD_SCHEMA``; ``scema check`` reports a mismatch in either direction.
WORLD_SCHEMA = "scema.world/1"

#: Signals emitted before the list is capped. A network has tens of feeds, not thousands,
#: so this is generous — but a cap that is never hit still has to be declared. An unbounded
#: producer is one whose output size is somebody else's problem.
MAX_SIGNALS = 64

#: Blind spots listed before truncation. Truncation is *stated*, never silent.
MAX_BLIND_SPOTS = 40


def _unit(value: float) -> float:
    """Clamp into ``[0, 1]``.

    Omni's importer refuses a magnitude outside the unit interval and is right to: an
    out-of-range magnitude would dominate a ranking through arithmetic rather than through
    importance. Clamping here means the producer takes responsibility for its own scale.
    """
    try:
        v = float(value)
    except (TypeError, ValueError):
        return 0.0
    if v != v or v in (float("inf"), float("-inf")):  # NaN / inf
        return 0.0
    return max(0.0, min(1.0, v))


def _scalar_num(value: float) -> Dict[str, Any]:
    """``scema_world::Scalar::Num``, externally tagged as ``{t, v}``."""
    return {"t": "num", "v": float(value)}


def _scalar_int(value: int) -> Dict[str, Any]:
    return {"t": "int", "v": int(value)}


def _scalar_text(value: str) -> Dict[str, Any]:
    return {"t": "text", "v": str(value)}


def _scalar_bool(value: bool) -> Dict[str, Any]:
    return {"t": "bool", "v": bool(value)}


def _signal(
    ident: str,
    polarity: str,
    label: str,
    detail: str,
    magnitude: float,
    targets: Sequence[str],
    evidence: Sequence[str],
) -> Dict[str, Any]:
    return {
        "id": ident,
        "polarity": polarity,
        "label": label,
        "detail": detail,
        "magnitude": _unit(magnitude),
        # Always true here, and every caller supplies an evidence line naming what was
        # counted. Omni's importer refuses a `measured` signal that cites nothing, which is
        # exactly the check this producer wants to be held to.
        "measured": True,
        "targets": list(targets),
        "evidence": list(evidence),
    }


def _provenance(reading: FeedReading) -> Dict[str, Any]:
    """Whether this reading can be believed, asked before the value is reported.

    ``Stale`` is deliberately not folded into ``Live``. A price that was true an hour ago
    looks exactly like one that is true now, and that resemblance is the entire hazard —
    the same reason this toolkit prints ``STALE`` rather than dimming a row.
    """
    if reading.stale:
        return {
            "kind": "stale",
            "age_secs": max(0, int(reading.age_secs)),
            "budget_secs": max(1, int(reading.heartbeat_secs)),
        }
    return {"kind": "live", "age_secs": max(0, int(reading.age_secs))}


def _object(reading: FeedReading) -> Dict[str, Any]:
    """One aggregator, as an omni object."""
    attrs: Dict[str, Any] = {
        "price": _scalar_num(reading.price),
        "decimals": _scalar_int(reading.decimals),
        "age_secs": _scalar_int(reading.age_secs),
        "heartbeat_secs": _scalar_int(reading.heartbeat_secs),
        # False marks a conservative bound rather than a measurement, and it must survive
        # into the record: an agent told a heartbeat is 3600 when nobody measured it will
        # call a feed fresh that its publisher considers late.
        "heartbeat_measured": _scalar_bool(reading.heartbeat_measured),
        "status": _scalar_text(reading.status),
        "round_id": _scalar_int(reading.round_id),
        "carried_over": _scalar_bool(reading.carried_over),
        "address": _scalar_text(reading.address),
        "description": _scalar_text(reading.description),
    }
    return {
        "id": f"feed:{reading.pair}",
        "kind": "aggregator",
        "label": reading.pair,
        "attrs": attrs,
        "provenance": _provenance(reading),
    }


def world(
    readings: Sequence[FeedReading],
    network: str = DEFAULT_NETWORK,
    unreadable: Optional[Sequence[str]] = None,
    sequencer: Optional[Dict[str, Any]] = None,
    now: Optional[int] = None,
) -> Dict[str, Any]:
    """Describe a network's oracle feeds as a ``WorldState``.

    ``readings`` is what came back. ``unreadable`` is the pairs that did not — passed
    separately and on purpose: :func:`alchem_link.feeds.read_all_feeds` reports a failed
    aggregator *by omission*, and an observer that inferred "not in the list" as "does not
    exist" would silently shrink the world every time an RPC hiccuped.
    """
    net = get_network(network)
    observed_at = int(time.time()) if now is None else int(now)
    registered = list_feeds(network)
    unreadable = list(unreadable or [])

    objects = [_object(r) for r in readings]
    signals: List[Dict[str, Any]] = []
    blind_spots: List[str] = []

    # ── what could not be read, first ────────────────────────────────────────
    for pair in unreadable[:MAX_BLIND_SPOTS]:
        blind_spots.append(
            f"{pair} on {net.label}: the aggregator did not answer — "
            "no price, no age, no verdict"
        )
    if len(unreadable) > MAX_BLIND_SPOTS:
        # A silently truncated list is a wrong count, and the count is the point.
        blind_spots.append(
            f"… {len(unreadable) - MAX_BLIND_SPOTS} further unreadable feed(s) not listed"
        )

    total = max(1, len(registered))

    if unreadable:
        signals.append(
            _signal(
                "unreadable-feeds",
                "risk",
                f"{len(unreadable)} of {len(registered)} registered feed(s) did not answer",
                "These have no price at all. Unreadable is not the same as zero, and not "
                "the same as stale.",
                len(unreadable) / total,
                [f"feed:{p}" for p in unreadable[:10]],
                [
                    f"counted {len(unreadable)} feed(s) absent from a batched read of "
                    f"{len(registered)} registered address(es)"
                ],
            )
        )

    # ── stale, measured against each feed's own heartbeat ────────────────────
    stale = [r for r in readings if r.stale]
    if stale:
        signals.append(
            _signal(
                "stale-feeds",
                "risk",
                f"{len(stale)} feed(s) have not published inside their own heartbeat",
                "The value was true once. It is not true now, and pricing against it is "
                "pricing against history.",
                len(stale) / total,
                [f"feed:{r.pair}" for r in stale[:10]],
                [
                    "counted "
                    + ", ".join(
                        f"{r.pair} {r.age_secs}s > {r.heartbeat_secs}s" for r in stale[:6]
                    )
                ],
            )
        )

    # A heartbeat nobody measured is a conservative bound wearing a measurement's clothes.
    # Counted separately from staleness because the fix is different: one is a feed to stop
    # using, the other is a table entry to go and measure.
    unmeasured = [r for r in readings if not r.heartbeat_measured]
    if unmeasured:
        signals.append(
            _signal(
                "unmeasured-heartbeats",
                "risk",
                f"{len(unmeasured)} feed(s) are judged against an assumed heartbeat",
                "Their staleness verdict is a bound, not a measurement. A feed called fresh "
                "here may be late by its publisher's own schedule.",
                len(unmeasured) / total,
                [f"feed:{r.pair}" for r in unmeasured[:10]],
                [
                    f"counted {len(unmeasured)} reading(s) with heartbeat_measured=false "
                    f"of {len(readings)} read"
                ],
            )
        )

    # ── a non-positive answer is not a low price ─────────────────────────────
    invalid = [r for r in readings if r.answer_raw <= 0]
    if invalid:
        signals.append(
            _signal(
                "non-positive-answers",
                "risk",
                f"{len(invalid)} feed(s) report an answer of zero or below",
                "A Chainlink price feed never legitimately reports this. Treating it as a "
                "price is how a liquidation cascade starts.",
                1.0,
                [f"feed:{r.pair}" for r in invalid[:10]],
                [f"counted {len(invalid)} reading(s) with answer_raw <= 0"],
            )
        )

    # ── an answer carried from an earlier round ──────────────────────────────
    carried = [r for r in readings if r.carried_over]
    if carried:
        signals.append(
            _signal(
                "carried-over-answers",
                "risk",
                f"{len(carried)} feed(s) carried their answer from an earlier round",
                "answeredInRound is behind roundId: the current round did not produce a "
                "fresh value.",
                len(carried) / total,
                [f"feed:{r.pair}" for r in carried[:10]],
                [f"counted {len(carried)} reading(s) with answeredInRound < roundId"],
            )
        )

    # ── the sequencer, on an L2 ──────────────────────────────────────────────
    #
    # Passed in rather than read here: this module takes no RPC client, so it cannot
    # silently make a network call while claiming to be a pure transform.
    if sequencer is not None:
        if sequencer.get("readable") is False:
            blind_spots.append(
                f"the {net.label} sequencer uptime feed could not be read — "
                "whether L2 prices are safe to use is unknown, not fine"
            )
        elif sequencer.get("up") is False:
            signals.append(
                _signal(
                    "sequencer-down",
                    "risk",
                    f"the {net.label} sequencer is reported down",
                    "Every price on this chain is suspect while it is: consumers cannot "
                    "post, so feeds go quiet without going stale.",
                    1.0,
                    [],
                    ["read the sequencer uptime aggregator: answer indicates down"],
                )
            )
        elif sequencer.get("grace_remaining_secs"):
            remaining = int(sequencer["grace_remaining_secs"])
            signals.append(
                _signal(
                    "sequencer-grace-period",
                    "risk",
                    f"the {net.label} sequencer came back {remaining}s ago",
                    "Inside the grace period a feed can read fresh while the value behind "
                    "it was produced before the outage.",
                    _unit(remaining / 3600.0),
                    [],
                    [f"counted {remaining}s remaining of the configured grace window"],
                )
            )

    if len(signals) > MAX_SIGNALS:
        signals = signals[:MAX_SIGNALS]
        blind_spots.append(
            f"signal list capped at {MAX_SIGNALS}; some were not emitted"
        )

    fresh = len(readings) - len(stale)

    # The denominator is the registry, which is a fixed table — but only while the readings
    # are a subset of it. A caller reading addresses that are not registered (an ad-hoc set,
    # a `--address` override) is describing something the registry does not bound, and
    # reporting `observed` over a smaller `total` would claim more than 100% coverage. The
    # honest answer there is that the denominator is unknown, which is exactly what
    # `Extent { total: None }` means and what `scema-sim` turns into measured uncertainty.
    #
    # Caught by `_check` below rather than by review: a producer that validates its own
    # output finds this the first time somebody passes an unregistered feed.
    accounted = len(readings) + len(unreadable)
    bounded = accounted <= len(registered)
    extent_total = len(registered) if bounded else None
    extent_note = (
        f"{len(readings)} of {len(registered)} registered feed(s) answered; "
        f"{fresh} fresh, {len(stale)} stale"
        if bounded
        else (
            f"{len(readings)} feed(s) read, {len(registered)} registered — the read went "
            f"beyond the registry, so the denominator is unknown"
        )
    )

    state = {
        # The contract version. This package cannot import the Rust crate, so declaring the
        # version is the only way an importer can tell a producer written against an older
        # reading of the format from a current one.
        "schema": WORLD_SCHEMA,
        "observer": OBSERVER,
        "entity": {
            # `Service` rather than `Chain`: what is being described is one network's oracle
            # *set*, not the chain itself. A decision about whether to price against these
            # feeds is not a decision about the chain.
            "kind": "service",
            "locator": f"chainlink:{network}",
            "label": f"{net.label} price feeds",
        },
        # `data`, and specifically not `trading`. A specialist declines on the domain it
        # cannot serve, and the bot's Deep Q* net reads pool and position data — asked
        # about an oracle set it would still emit five finite Q-values, correctly shaped
        # and entirely meaningless. Declining is what `domain` is for.
        #
        # It said `unknown` until the vocabulary opened, which was accurate about the
        # decline and useless about everything else: a perceived web page reported the
        # same thing, so nothing downstream could tell an oracle set from a DOM.
        "domain": "data",
        "observed_at": observed_at,
        "objects": objects,
        "facts": [],
        "signals": signals,
        "extent": {
            "observed": len(readings),
            # Known exactly when the readings are a subset of the registry — this is one of
            # the few observers in the project that can honestly claim a bounded extent, and
            # it should, because an unnecessary `null` manufactures uncertainty the same way
            # a missing one manufactures confidence. See the note above for when it is not.
            "total": extent_total,
            "note": extent_note,
        },
        "blind_spots": blind_spots,
    }
    _check(state)
    return state


def _check(state: Dict[str, Any]) -> None:
    """The checks omni's ``ImportObserver`` runs, restated here.

    Kept as an explicit list rather than as a dependency on the Rust crate: the whole
    reason this module emits a hand-built dict is that the two do not link, and reaching
    across would quietly reintroduce the coupling it exists to avoid. This package fails
    its own tests instead of producing something the consumer rejects at run time.
    """
    if state.get("schema") != WORLD_SCHEMA:
        raise ValueError(
            f"a world must declare schema {WORLD_SCHEMA!r}, not {state.get('schema')!r}"
        )
    if not str(state["observer"]).strip():
        raise ValueError("a world with no observer cannot be attributed")
    if not str(state["entity"]["locator"]).strip():
        raise ValueError(
            "a world with no entity locator is a record nobody can re-check"
        )

    seen_signals = set()
    for signal in state["signals"]:
        ident = str(signal["id"]).strip()
        if not ident:
            raise ValueError("a signal has an empty id")
        if ident in seen_signals:
            raise ValueError(
                f"duplicate signal id {ident!r}: --ground could not name it unambiguously"
            )
        seen_signals.add(ident)

        magnitude = signal["magnitude"]
        if not 0.0 <= magnitude <= 1.0:
            raise ValueError(f"signal {ident!r} magnitude {magnitude} outside [0,1]")
        if signal["polarity"] not in ("risk", "opportunity"):
            raise ValueError(f"signal {ident!r} has polarity {signal['polarity']!r}")
        # The one that matters. `measured: true` is a claim that somebody counted
        # something, and it is the claim `scema-sim` relies on to score a real expected
        # gain. Making it with nothing to cite is laundering a guess.
        if signal["measured"] and not signal["evidence"]:
            raise ValueError(f"signal {ident!r} claims measured and cites nothing")

    seen_objects = set()
    for obj in state["objects"]:
        if obj["id"] in seen_objects:
            raise ValueError(f"duplicate object id {obj['id']!r}")
        seen_objects.add(obj["id"])

    extent = state["extent"]
    if extent["total"] is not None and extent["observed"] > extent["total"]:
        raise ValueError(
            "extent numerator exceeds its denominator; an unknown total must be null"
        )


def sequencer_facts(status: Any) -> Optional[Dict[str, Any]]:
    """A :class:`~alchem_link.sequencer.SequencerStatus` in the shape :func:`world` takes.

    ``None`` in, ``None`` out — and that is *not* the same as "up". On an L1 there is no
    sequencer and nothing to say; on an L2 with no registered feed it means the risk was
    never checked. :func:`alchem_link.sequencer.is_l2` is what separates the two, and the
    caller is the one that knows which it is looking at.
    """
    if status is None:
        return None
    if getattr(status, "error", ""):
        # Unreadable, which is neither up nor down. The caller turns this into a blind spot
        # rather than a verdict.
        return {"readable": False, "detail": status.error}
    return {
        "readable": True,
        "up": bool(status.up),
        "grace_remaining_secs": (
            max(0, status.grace_period_secs - status.since_secs)
            if status.in_grace_period
            else 0
        ),
        "since_secs": int(status.since_secs),
    }


def perceive(
    network: str = DEFAULT_NETWORK,
    client: Any = None,
    rpc_url: Optional[str] = None,
    now: Optional[int] = None,
) -> Dict[str, Any]:
    """Read a network and describe it as a ``WorldState``.

    The impure half, kept apart from :func:`world` on purpose: everything that decides what
    a reading *means* is a pure transform with tests that need no network, and this function
    only does the reading. A single function doing both would be untestable without an RPC
    endpoint, and the meaning is the part worth testing.

    A feed that does not answer is reported by omission from :func:`read_all_feeds`, so the
    unreadable set is computed here by difference against the registry rather than inferred
    downstream — an observer that treated "not in the list" as "does not exist" would
    silently shrink the world every time an RPC hiccuped.
    """
    from .feeds import read_all_feeds
    from .rpc import client_for
    from .sequencer import is_l2, read_sequencer

    rpc = client or client_for(network=network, rpc_url=rpc_url)
    readings = read_all_feeds(network=network, client=rpc, now=now)
    answered = {r.pair for r in readings}
    unreadable = [f.pair for f in list_feeds(network) if f.pair not in answered]

    sequencer = None
    if is_l2(network):
        sequencer = sequencer_facts(read_sequencer(network, client=rpc, now=now))
        if sequencer is None:
            # An L2 with no registered uptime feed. Unchecked, not fine.
            sequencer = {
                "readable": False,
                "detail": "no sequencer uptime feed is registered for this L2",
            }

    return world(
        readings,
        network=network,
        unreadable=unreadable,
        sequencer=sequencer,
        now=now,
    )


# ── the same feeds, over a window ────────────────────────────────────────────
#
# :func:`world` answers "what do these feeds say right now". That is the question an
# operator asks, and it is not the question an agent about to price against them should
# ask, because a feed can be perfectly fresh at the instant you look at it and have been
# absent for the four hours before. A snapshot cannot distinguish a steady oracle from one
# that publishes in bursts, and the second is the one that hurts.
#
# So this is the same network, described over a span of time: one object per feed carrying
# what its history actually showed, and signals counting the feeds in each bad state. It
# reuses the object ids of :func:`world` (``feed:<pair>``) and the same entity locator, on
# purpose — an agent should see one subject observed two ways, not two subjects.
#
# Everything below obeys the same rule as the rest of this module, and it bites harder here
# because statistics are so easy to fabricate: **an unmeasured statistic is an absent
# attribute, never a zero.** A volatility of 0.0 is a claim that the price did not move. A
# feed with two prints has no volatility at all, and the two must not look alike.

#: Points below which a window's statistics are too thin to lean on.
#:
#: Not a hard floor — the statistics are still emitted, because they are what was measured.
#: It drives a *signal*, so an agent can see that the numbers it is reading rest on four
#: observations and discount them itself. Choosing to hide them here would be this module
#: deciding on the agent's behalf.
MIN_SAMPLES = 8

#: Divergence between the last print and the window's TWAP, in basis points, past which a
#: feed is counted. 100 bps is a percent: large enough not to fire on ordinary drift,
#: small enough to catch a feed whose spot answer has run away from its own average.
TWAP_DIVERGENCE_BPS = 100.0

#: Peak-to-trough fall within the window, as a percentage, past which a feed is counted.
DRAWDOWN_PCT = 10.0

#: How much longer than its own heartbeat a feed's *median* publish interval may run before
#: it is counted as gapping. 1.5x rather than 1.0x because the measured ceilings already run
#: a percent or two over the configured interval — the same reason `STALENESS_TOLERANCE`
#: exists — and a signal that fires every window trains people to ignore it.
GAP_FACTOR = 1.5


def _stat_attrs(stats: "Stats") -> Dict[str, Any]:
    """The measured statistics, and only those.

    A ``None`` in :class:`~alchem_link.analytics.Stats` means the window could not support
    that computation — two points have no volatility, a zero low has no percentage range.
    Those keys are **omitted**, not defaulted, because an omni object with no attribute is
    read as unmeasured and an attribute of ``0.0`` is read as an observation.
    """
    attrs: Dict[str, Any] = {
        "samples": _scalar_int(stats.samples),
        "span_secs": _scalar_int(stats.span_secs),
    }
    optional = (
        ("max_drawdown_pct", stats.max_drawdown_pct),
        ("largest_move_bps", stats.largest_move_bps),
        ("first", stats.first),
        ("last", stats.last),
        ("low", stats.low),
        ("high", stats.high),
        ("mean", stats.mean),
        ("median", stats.median),
        ("twap", stats.twap),
        ("change_pct", stats.change_pct),
        ("range_pct", stats.range_pct),
        ("volatility_annual", stats.volatility_annual),
        ("volatility_period", stats.volatility_period),
        ("median_interval_secs", stats.median_interval_secs),
        ("twap_divergence_bps", stats.twap_divergence_bps),
    )
    for key, value in optional:
        if value is not None:
            attrs[key] = _scalar_num(float(value))
    return attrs


def _window_provenance(last_seen: int, heartbeat: int, observed_at: int) -> Dict[str, Any]:
    """Whether the *end* of the window can be believed as the present.

    A window always describes the past; what varies is whether it reaches up to now. A feed
    whose last print is inside its heartbeat is live and its history is current. One whose
    last print is older is stale — and the history is still perfectly good evidence about
    the span it covers, which is why the value is kept and labelled rather than dropped.
    """
    age = max(0, observed_at - last_seen)
    if age > heartbeat:
        return {"kind": "stale", "age_secs": age, "budget_secs": max(1, heartbeat)}
    return {"kind": "live", "age_secs": age}


def windowed_world(
    histories: Sequence["Series"],
    network: str = DEFAULT_NETWORK,
    window_secs: int = 6 * 3600,
    unreadable: Optional[Sequence[str]] = None,
    truncated: Optional[Sequence[str]] = None,
    now: Optional[int] = None,
) -> Dict[str, Any]:
    """Describe a network's feeds over a time window as a ``WorldState``.

    ``histories`` is one :class:`~alchem_link.analytics.Series` per feed that answered.
    ``unreadable`` names the feeds no history could be fetched for at all, and ``truncated``
    names those whose history was cut short by a block-range cap — two different kinds of
    ignorance that must not be collapsed. The first is a blind spot; the second is a window
    that is honestly shorter than requested, and the object still carries its real
    ``span_secs``.

    Pure. :func:`perceive_window` does the reading.
    """
    from .analytics import summarise

    net = get_network(network)
    observed_at = int(time.time()) if now is None else int(now)
    registered = list_feeds(network)
    heartbeats = {f.pair: int(getattr(f, "heartbeat_secs", 0) or 0) for f in registered}
    unreadable = list(unreadable or [])
    truncated = set(truncated or [])
    total = max(1, len(registered))

    # An empty series is not a history. Keeping it would produce an object whose every
    # statistic is absent, which reads as "we looked and learned nothing" — true, but it
    # belongs in blind_spots where sim turns it into uncertainty, not in objects where it
    # pads the extent numerator.
    usable = [s for s in histories if len(s) > 0]
    empty = [s.pair for s in histories if len(s) == 0]

    objects: List[Dict[str, Any]] = []
    summaries: List[Any] = []
    live_at_end: Dict[str, bool] = {}
    for series in usable:
        stats = summarise(series)
        summaries.append((series, stats))
        heartbeat = heartbeats.get(series.pair, 0) or window_secs
        provenance = _window_provenance(
            series.points[-1].timestamp, heartbeat, observed_at
        )
        live_at_end[series.pair] = provenance["kind"] == "live"
        objects.append(
            {
                "id": f"feed:{series.pair}",
                "kind": "aggregator",
                "label": series.pair,
                "attrs": _stat_attrs(stats),
                "provenance": provenance,
            }
        )

    # Missing is computed by **difference against the registry**, never taken from the
    # caller. `perceive` learned this the same way: a feed absent from the input is absent,
    # and an observer that reported only what it was told about would silently shrink the
    # world every time a caller passed a short list. `unreadable` and `empty` then only
    # refine *why* a feed is missing — the node refused, or the feed did not publish — and
    # both are ignorance, neither is a zero.
    covered = {s.pair for s in usable}
    reasons = {p: "the node did not answer" for p in unreadable}
    reasons.update({p: "the feed published nothing" for p in empty})
    missing = [f.pair for f in registered if f.pair not in covered]
    # A caller may name something the registry does not carry. Dropping it would hide a
    # genuine failure behind a bookkeeping mismatch, so it is appended rather than ignored.
    missing += [p for p in reasons if p not in covered and p not in missing]

    blind_spots: List[str] = []
    for pair in missing[:MAX_BLIND_SPOTS]:
        why = reasons.get(pair, "it was not read")
        blind_spots.append(
            f"{pair} on {net.label}: no history over the last {window_secs}s — {why}; "
            "no prints, no cadence, no volatility"
        )
    if len(missing) > MAX_BLIND_SPOTS:
        blind_spots.append(
            f"… {len(missing) - MAX_BLIND_SPOTS} further feed(s) without history not listed"
        )

    signals: List[Dict[str, Any]] = []

    if missing:
        signals.append(
            _signal(
                "feeds-without-history",
                "risk",
                f"{len(missing)} of {len(registered)} feed(s) produced no history",
                "Nothing is known about how these behaved over the window. That is not the "
                "same as behaving well.",
                len(missing) / total,
                [f"feed:{p}" for p in missing[:10]],
                [
                    f"counted {len(missing)} feed(s) with zero points over {window_secs}s "
                    f"against {len(registered)} registered address(es)"
                ],
            )
        )

    short = [s.pair for s, _ in summaries if s.pair in truncated]
    if short:
        signals.append(
            _signal(
                "truncated-window",
                "risk",
                f"{len(short)} feed(s) have a shorter history than was asked for",
                "The block scan hit its cap. Their statistics are real and describe less "
                "time than the rest — comparing them like for like overstates the shorter "
                "window's stability.",
                len(short) / total,
                [f"feed:{p}" for p in short[:10]],
                [f"counted {len(short)} feed(s) whose log scan was capped"],
            )
        )

    thin = [s.pair for s, st in summaries if st.samples < MIN_SAMPLES]
    if thin:
        signals.append(
            _signal(
                "thin-history",
                "risk",
                f"{len(thin)} feed(s) published fewer than {MIN_SAMPLES} times in the window",
                "Their statistics are computed and reported, because that is what was "
                "measured — but a volatility over four points is not a volatility.",
                len(thin) / total,
                [f"feed:{p}" for p in thin[:10]],
                [
                    f"counted {len(thin)} feed(s) with fewer than {MIN_SAMPLES} points "
                    f"over {window_secs}s"
                ],
            )
        )

    gapping = [
        s.pair
        for s, st in summaries
        if st.median_interval_secs is not None
        and heartbeats.get(s.pair, 0) > 0
        and st.median_interval_secs > heartbeats[s.pair] * GAP_FACTOR
    ]
    if gapping:
        signals.append(
            _signal(
                "publish-gaps",
                "risk",
                f"{len(gapping)} feed(s) published slower than their own heartbeat",
                "Fresh at the moment you look and absent for hours between. A snapshot "
                "cannot see this, which is the reason this window exists.",
                len(gapping) / total,
                [f"feed:{p}" for p in gapping[:10]],
                [
                    f"counted {len(gapping)} feed(s) whose median publish interval exceeded "
                    f"{GAP_FACTOR}x their measured heartbeat"
                ],
            )
        )

    diverged = [
        s.pair
        for s, st in summaries
        if st.twap_divergence_bps is not None
        and abs(st.twap_divergence_bps) >= TWAP_DIVERGENCE_BPS
    ]
    if diverged:
        signals.append(
            _signal(
                "twap-divergence",
                "risk",
                f"{len(diverged)} feed(s) sit more than {TWAP_DIVERGENCE_BPS:.0f} bps from "
                "their own window TWAP",
                "Exactly the condition a TWAP-priced protocol is protected against and a "
                "spot-priced one is not.",
                len(diverged) / total,
                [f"feed:{p}" for p in diverged[:10]],
                [
                    f"counted {len(diverged)} feed(s) with |last - twap| / twap >= "
                    f"{TWAP_DIVERGENCE_BPS:.0f} bps"
                ],
            )
        )

    fell = [
        s.pair
        for s, st in summaries
        if st.max_drawdown_pct is not None and st.max_drawdown_pct >= DRAWDOWN_PCT
    ]
    if fell:
        signals.append(
            _signal(
                "window-drawdown",
                "risk",
                f"{len(fell)} feed(s) fell at least {DRAWDOWN_PCT:.0f}% peak to trough",
                "A price move, not a fault. It is a risk to a position, not to the oracle, "
                "and it is counted separately for that reason.",
                len(fell) / total,
                [f"feed:{p}" for p in fell[:10]],
                [f"counted {len(fell)} feed(s) with max drawdown >= {DRAWDOWN_PCT:.0f}%"],
            )
        )

    troubled = set(thin) | set(gapping) | set(diverged) | set(short)
    steady = [
        s.pair for s, _ in summaries if s.pair not in troubled and live_at_end.get(s.pair)
    ]
    if steady:
        signals.append(
            _signal(
                "steady-feeds",
                "opportunity",
                f"{len(steady)} feed(s) published on cadence throughout the window",
                "Counted, not scored. This says they behaved, not that they are correct — "
                "no oracle's own history can establish that.",
                len(steady) / total,
                [f"feed:{p}" for p in steady[:10]],
                [
                    f"counted {len(steady)} feed(s) live at the window end with no gap, "
                    "no truncation, no thin sample and no TWAP divergence"
                ],
            )
        )

    signals = signals[:MAX_SIGNALS]

    state = {
        "schema": WORLD_SCHEMA,
        "observer": OBSERVER,
        "entity": {
            "kind": "service",
            # The same locator as the snapshot world, deliberately. This is one subject
            # observed two ways, and giving the window its own locator would split a
            # network's memory in half — an agent would never connect "BTC/USD was stale
            # then" with "BTC/USD gaps".
            "locator": f"chainlink:{network}",
            "label": f"{net.label} price feeds over {window_secs}s",
        },
        "domain": "data",
        "observed_at": observed_at,
        "objects": objects,
        "facts": [],
        "signals": signals,
        "extent": {
            "observed": len(objects),
            "total": len(registered) if len(objects) <= len(registered) else None,
            "note": (
                f"{len(objects)} of {len(registered)} registered feed(s) produced history "
                f"over {window_secs}s; {len(missing)} produced none"
            ),
        },
        "blind_spots": blind_spots,
    }
    _check(state)
    return state


def perceive_window(
    network: str = DEFAULT_NETWORK,
    hours: float = 6.0,
    client: Any = None,
    rpc_url: Optional[str] = None,
    now: Optional[int] = None,
) -> Dict[str, Any]:
    """Read a window of history from a network and describe it as a ``WorldState``.

    The impure half of :func:`windowed_world`, split for the same reason :func:`perceive` is
    split from :func:`world`: everything that decides what a history *means* stays testable
    without an RPC endpoint, and the meaning is the part worth testing.

    A feed whose log scan raises is reported as unreadable rather than as an empty history.
    Those are different claims — "the node would not answer" and "the feed did not publish"
    — and only the second is a fact about the oracle.
    """
    from .analytics import Series, Point
    from .logs import answer_updates
    from .rpc import client_for

    rpc = client or client_for(network=network, rpc_url=rpc_url)
    window_secs = int(hours * 3600)

    histories: List[Any] = []
    unreadable: List[str] = []
    for feed in list_feeds(network):
        try:
            updates = answer_updates(
                feed.address, hours=hours, network=network, client=rpc
            )
        except Exception:
            unreadable.append(feed.pair)
            continue
        points = [
            Point(timestamp=int(u.updated_at), price=float(u.price))
            for u in updates
            if getattr(u, "updated_at", None) and getattr(u, "price", None) is not None
        ]
        histories.append(Series(pair=feed.pair, network=network, points=points))

    return windowed_world(
        histories,
        network=network,
        window_secs=window_secs,
        unreadable=unreadable,
        now=now,
    )
