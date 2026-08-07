"""Replay a consumer's oracle guards against history, and against known failure modes.

:mod:`alchem_link.safety` audits a feed as it is *right now*. This module asks the other
question, which is the one that actually decides whether a protocol survives: **given the
checks my contract performs, what would have happened?**

You describe your consumer's guards once as a :class:`Guard` — max staleness, whether you
reject carried rounds, your own sanity bounds, whether you gate on the L2 sequencer — and
then replay them. Against a real feed's history, to see whether your staleness window
would have rejected rounds the feed legitimately produced. Against the built-in
:data:`SCENARIOS`, to see whether it would have caught the failure modes that have
already cost people money:

``bounded_crash``     the LUNA/Venus shape — price falls through ``minAnswer`` and the
                      feed keeps returning the floor, fresh and well-formed
``frozen_feed``       publishing stops; every read succeeds with an increasingly old answer
``sequencer_outage``  an L2 whose sequencer is down, so the price is a fossil
``carried_rounds``    rounds that finalise with no fresh answer
``flash_spike``       a single-round outlier that immediately reverts
``clock_skew``        a round timestamped in the future

A guard that accepts every observation in ``bounded_crash`` is a guard with a real hole
in it, and this is how you find that out before it matters rather than after. Everything
here is deterministic and offline: no chain, no clock, no randomness.
"""
from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Callable, Dict, List, Optional, Sequence

from .analytics import Series
from .errors import SimulationError

#: One hour. The default nearly every tutorial hardcodes, and therefore the default worth
#: testing people's guards against.
DEFAULT_MAX_AGE = 3600

#: Chainlink's documented L2 grace period after a sequencer comes back up.
DEFAULT_GRACE_SECS = 3600


@dataclass
class Guard:
    """The checks a consumer contract performs before trusting an answer.

    Defaults describe the *typical* integration rather than a good one: a staleness
    window and a positivity check, and nothing else. That is deliberate — the first thing
    most people do with this module is replay the defaults, see what gets through, and
    then turn the other guards on.
    """

    #: Reject an answer older than this. ``0`` disables the check.
    max_age_secs: int = DEFAULT_MAX_AGE
    #: Reject ``answer <= 0``. Almost free and catches a real failure mode.
    require_positive: bool = True
    #: Reject ``answeredInRound < roundId`` — a round that carried an older answer.
    reject_carried: bool = False
    #: Reject ``updatedAt == 0`` — a round that started and never finalised.
    reject_incomplete: bool = True
    #: Consumer-side sanity bounds, in price units. ``None`` disables.
    min_price: Optional[float] = None
    max_price: Optional[float] = None
    #: Reject a move larger than this from the previous accepted answer. The consumer-side
    #: circuit breaker; ``None`` disables.
    max_move_bps: Optional[float] = None
    #: Require the L2 sequencer to be up and past its grace period.
    require_sequencer: bool = False
    grace_secs: int = DEFAULT_GRACE_SECS
    #: Reject an answer timestamped in the future by more than this many seconds.
    max_future_skew_secs: int = 60

    def as_dict(self) -> Dict[str, Any]:
        return {
            "max_age_secs": self.max_age_secs,
            "require_positive": self.require_positive,
            "reject_carried": self.reject_carried,
            "reject_incomplete": self.reject_incomplete,
            "min_price": self.min_price,
            "max_price": self.max_price,
            "max_move_bps": self.max_move_bps,
            "require_sequencer": self.require_sequencer,
            "grace_secs": self.grace_secs,
            "max_future_skew_secs": self.max_future_skew_secs,
        }

    @classmethod
    def strict(cls) -> "Guard":
        """Every check on. What :func:`alchem_link.codegen.generate_consumer` emits."""
        return cls(
            max_age_secs=DEFAULT_MAX_AGE,
            require_positive=True,
            reject_carried=True,
            reject_incomplete=True,
            max_move_bps=2000.0,
            require_sequencer=True,
        )

    @classmethod
    def naive(cls) -> "Guard":
        """``latestRoundData()`` and nothing else — the integration in most tutorials."""
        return cls(max_age_secs=0, require_positive=False, reject_incomplete=False)


@dataclass
class Observation:
    """What a consumer sees at one moment: the round, and the world around it."""

    timestamp: int
    price: float
    updated_at: int
    round_id: int = 1
    answered_in_round: int = 0
    #: Seconds the L2 sequencer has been up, or ``None`` for "not an L2 / not checked".
    #: ``0`` means it is down right now.
    sequencer_up_secs: Optional[int] = None

    def __post_init__(self) -> None:
        if not self.answered_in_round:
            self.answered_in_round = self.round_id

    @property
    def age_secs(self) -> int:
        return self.timestamp - self.updated_at

    @property
    def carried(self) -> bool:
        return self.answered_in_round < self.round_id

    def as_dict(self) -> Dict[str, Any]:
        return {
            "timestamp": self.timestamp,
            "price": self.price,
            "updated_at": self.updated_at,
            "age_secs": self.age_secs,
            "round_id": self.round_id,
            "answered_in_round": self.answered_in_round,
            "carried": self.carried,
            "sequencer_up_secs": self.sequencer_up_secs,
        }


@dataclass
class Verdict:
    """Whether one observation passed, and every reason it did not."""

    observation: Observation
    reasons: List[str] = field(default_factory=list)

    @property
    def accepted(self) -> bool:
        return not self.reasons

    def as_dict(self) -> Dict[str, Any]:
        return {
            "accepted": self.accepted,
            "reasons": self.reasons,
            **self.observation.as_dict(),
        }


def evaluate(guard: Guard, observation: Observation,
             previous: Optional[Observation] = None) -> Verdict:
    """Apply every enabled guard to one observation.

    All failing checks are collected rather than returning on the first, because the
    useful output is "this round fails staleness *and* the bounds check", not whichever
    check happens to run first.
    """
    reasons: List[str] = []

    if guard.reject_incomplete and observation.updated_at == 0:
        reasons.append("INCOMPLETE_ROUND: updatedAt is 0 — the round never finalised")

    if guard.require_positive and observation.price <= 0:
        reasons.append(f"NON_POSITIVE: answer is {observation.price}, never a real quote")

    age = observation.age_secs
    if guard.max_age_secs and age > guard.max_age_secs:
        reasons.append(
            f"STALE: answer is {age}s old, past the {guard.max_age_secs}s window"
        )

    if guard.max_future_skew_secs and age < -guard.max_future_skew_secs:
        reasons.append(f"FUTURE_TIMESTAMP: answer is dated {-age}s in the future")

    if guard.reject_carried and observation.carried:
        reasons.append(
            f"CARRIED_ROUND: answeredInRound {observation.answered_in_round} "
            f"< roundId {observation.round_id}"
        )

    if guard.min_price is not None and observation.price < guard.min_price:
        reasons.append(f"BELOW_MIN: {observation.price} < configured floor {guard.min_price}")
    if guard.max_price is not None and observation.price > guard.max_price:
        reasons.append(f"ABOVE_MAX: {observation.price} > configured ceiling {guard.max_price}")

    if guard.max_move_bps is not None and previous is not None and previous.price > 0:
        move = abs(observation.price - previous.price) / previous.price * 10_000
        if move > guard.max_move_bps:
            reasons.append(
                f"MOVE_LIMIT: {move:.0f} bps move exceeds the {guard.max_move_bps:.0f} bps limit"
            )

    if guard.require_sequencer and observation.sequencer_up_secs is not None:
        if observation.sequencer_up_secs <= 0:
            reasons.append("SEQUENCER_DOWN: the L2 sequencer is not up")
        elif observation.sequencer_up_secs < guard.grace_secs:
            reasons.append(
                f"SEQUENCER_GRACE: up for only {observation.sequencer_up_secs}s of the "
                f"{guard.grace_secs}s grace period"
            )

    return Verdict(observation=observation, reasons=reasons)


@dataclass
class ReplayReport:
    """The outcome of replaying a guard over a sequence of observations."""

    name: str
    guard: Guard
    verdicts: List[Verdict]
    #: What the scenario was designed to test, when it came from :data:`SCENARIOS`.
    expectation: str = ""

    @property
    def accepted(self) -> List[Verdict]:
        return [v for v in self.verdicts if v.accepted]

    @property
    def rejected(self) -> List[Verdict]:
        return [v for v in self.verdicts if not v.accepted]

    @property
    def acceptance_rate(self) -> float:
        return len(self.accepted) / len(self.verdicts) if self.verdicts else 0.0

    @property
    def first_rejection(self) -> Optional[Verdict]:
        return self.rejected[0] if self.rejected else None

    @property
    def longest_rejection_streak(self) -> int:
        """The longest run of consecutive rejections.

        The number that decides whether a guard is usable in production. A guard that
        rejects 2% of rounds scattered about is fine; one that rejects 2% as a single
        forty-minute block is a protocol that halts for forty minutes.
        """
        longest = run = 0
        for verdict in self.verdicts:
            run = run + 1 if not verdict.accepted else 0
            longest = max(longest, run)
        return longest

    @property
    def reason_counts(self) -> Dict[str, int]:
        counts: Dict[str, int] = {}
        for verdict in self.rejected:
            for reason in verdict.reasons:
                code = reason.split(":")[0]
                counts[code] = counts.get(code, 0) + 1
        return dict(sorted(counts.items(), key=lambda kv: -kv[1]))

    @property
    def caught(self) -> bool:
        """True when the guard rejected at least one observation.

        For a failure scenario this is the pass/fail: a guard that accepted every
        observation in ``bounded_crash`` would have consumed the floor price as though it
        were the market.
        """
        return bool(self.rejected)

    @property
    def worst_accepted_price(self) -> Optional[float]:
        """The most extreme price the guard let through.

        The scenario's punchline in one number — what your contract would have used.
        """
        if not self.accepted:
            return None
        first = self.verdicts[0].observation.price
        return max((v.observation.price for v in self.accepted),
                   key=lambda p: abs(p - first))

    def as_dict(self) -> Dict[str, Any]:
        return {
            "name": self.name,
            "expectation": self.expectation,
            "guard": self.guard.as_dict(),
            "observations": len(self.verdicts),
            "accepted": len(self.accepted),
            "rejected": len(self.rejected),
            "acceptance_rate": round(self.acceptance_rate, 4),
            "caught": self.caught,
            "longest_rejection_streak": self.longest_rejection_streak,
            "reason_counts": self.reason_counts,
            "first_rejection": self.first_rejection.as_dict() if self.first_rejection else None,
            "worst_accepted_price": self.worst_accepted_price,
        }


def replay(guard: Guard, observations: Sequence[Observation],
           name: str = "replay", expectation: str = "") -> ReplayReport:
    """Apply ``guard`` to each observation in order.

    ``previous`` for the move-limit check is the last *accepted* observation, not the
    last one seen. That mirrors a real consumer, which stores the price it used — a
    rejected round never becomes the baseline for the next comparison.
    """
    verdicts: List[Verdict] = []
    previous: Optional[Observation] = None
    for observation in observations:
        verdict = evaluate(guard, observation, previous)
        verdicts.append(verdict)
        if verdict.accepted:
            previous = observation
    return ReplayReport(name=name, guard=guard, verdicts=verdicts, expectation=expectation)


def observations_from_series(series: Series, heartbeat_secs: int = DEFAULT_MAX_AGE,
                             sequencer_up_secs: Optional[int] = None) -> List[Observation]:
    """Turn a real price history into observations a guard can be replayed over.

    Each point is evaluated as of its own timestamp plus a full heartbeat — the moment a
    consumer would be reading it just before the next publish, which is the worst case
    that history supports. Evaluating at the publish instant would make every round look
    perfectly fresh and the replay would prove nothing.
    """
    observations: List[Observation] = []
    for index, point in enumerate(series.points, start=1):
        observations.append(Observation(
            timestamp=point.timestamp + heartbeat_secs,
            price=point.price,
            updated_at=point.timestamp,
            round_id=index,
            answered_in_round=index,
            sequencer_up_secs=sequencer_up_secs,
        ))
    return observations


# ── scenarios ────────────────────────────────────────────────────────────────

_BASE_TIME = 1_700_000_000
_BASE_PRICE = 2000.0


def _steady(count: int, interval: int = 600, price: float = _BASE_PRICE) -> List[Observation]:
    return [
        Observation(
            timestamp=_BASE_TIME + i * interval,
            price=price,
            updated_at=_BASE_TIME + i * interval,
            round_id=i + 1,
        )
        for i in range(count)
    ]


def scenario_bounded_crash() -> List[Observation]:
    """The LUNA shape: the price falls through the aggregator's floor and pins there.

    Every observation after the pin is *fresh* — the feed keeps publishing on its
    heartbeat — and every one reports the floor. Nothing about staleness, positivity, or
    round completeness catches this. Only a consumer-side sanity bound or a move limit
    does, which is exactly the lesson.
    """
    floor = 100.0
    prices = [2000.0, 1600.0, 900.0, 400.0, 120.0, floor, floor, floor, floor, floor]
    return [
        Observation(
            timestamp=_BASE_TIME + i * 600,
            price=price,
            updated_at=_BASE_TIME + i * 600,
            round_id=i + 1,
        )
        for i, price in enumerate(prices)
    ]


def scenario_frozen_feed() -> List[Observation]:
    """Publishing stops. Every read succeeds; the answer just gets older."""
    frozen_at = _BASE_TIME
    return [
        Observation(
            timestamp=_BASE_TIME + i * 900,
            price=_BASE_PRICE,
            updated_at=frozen_at,
            round_id=1,
        )
        for i in range(10)
    ]


def scenario_sequencer_outage() -> List[Observation]:
    """An L2 sequencer goes down, comes back, and is inside its grace period.

    The price feed answers throughout — with a price frozen at the moment the sequencer
    stopped. A consumer that does not gate on the uptime feed cannot tell.
    """
    uptimes = [7200, 3600, 0, 0, 0, 60, 600, 1800, 3000, 4000]
    return [
        Observation(
            timestamp=_BASE_TIME + i * 600,
            price=_BASE_PRICE if up else _BASE_PRICE * 0.98,
            # While the sequencer is down the answer stops advancing.
            updated_at=_BASE_TIME + (i * 600 if up else 1200),
            round_id=i + 1,
            sequencer_up_secs=up,
        )
        for i, up in enumerate(uptimes)
    ]


def scenario_carried_rounds() -> List[Observation]:
    """Rounds finalise without producing a fresh answer."""
    out: List[Observation] = []
    for i in range(10):
        carried = 3 <= i <= 6
        out.append(Observation(
            timestamp=_BASE_TIME + i * 600,
            price=_BASE_PRICE,
            updated_at=_BASE_TIME + i * 600,
            round_id=i + 1,
            answered_in_round=3 if carried else i + 1,
        ))
    return out


def scenario_flash_spike() -> List[Observation]:
    """One round prints 40% away and the next round reverts.

    Only a move limit catches this. Staleness, positivity and round completeness all pass
    — the spike is a perfectly well-formed, perfectly fresh, wrong number.
    """
    prices = [2000.0, 2005.0, 1998.0, 2800.0, 2002.0, 1999.0, 2001.0]
    return [
        Observation(
            timestamp=_BASE_TIME + i * 600,
            price=price,
            updated_at=_BASE_TIME + i * 600,
            round_id=i + 1,
        )
        for i, price in enumerate(prices)
    ]


def scenario_incomplete_round() -> List[Observation]:
    """A round that started and never finalised — ``updatedAt`` is 0."""
    out = _steady(6)
    out[3] = Observation(
        timestamp=out[3].timestamp, price=0.0, updated_at=0, round_id=4, answered_in_round=4,
    )
    return out


def scenario_clock_skew() -> List[Observation]:
    """A round timestamped in the future, which makes naive age arithmetic go negative.

    Worth its own scenario because ``block.timestamp - updatedAt`` underflows in a
    Solidity consumer using unsigned arithmetic, and the result is an enormous age that
    either reverts or — with the comparison written the other way — passes trivially.
    """
    out = _steady(6)
    skewed = out[2]
    out[2] = Observation(
        timestamp=skewed.timestamp,
        price=skewed.price,
        updated_at=skewed.timestamp + 7200,
        round_id=3,
    )
    return out


def scenario_healthy() -> List[Observation]:
    """A well-behaved feed. The control: a good guard must accept all of this.

    Without it, "reject everything" would score perfectly on every other scenario.
    """
    prices = [2000.0, 2004.0, 1996.0, 2010.0, 2008.0, 1999.0, 2003.0, 2011.0]
    return [
        Observation(
            timestamp=_BASE_TIME + i * 600,
            price=price,
            updated_at=_BASE_TIME + i * 600,
            round_id=i + 1,
            sequencer_up_secs=86400,
        )
        for i, price in enumerate(prices)
    ]


@dataclass(frozen=True)
class Scenario:
    name: str
    summary: str
    expectation: str
    build: Callable[[], List[Observation]]
    #: True when a correct guard rejects at least one observation. The healthy control
    #: sets this False, which is what stops "reject everything" from scoring well.
    should_catch: bool = True


SCENARIOS: Dict[str, Scenario] = {
    s.name: s for s in [
        Scenario(
            "healthy", "A well-behaved feed publishing on schedule",
            "a good guard accepts every round here",
            scenario_healthy, should_catch=False,
        ),
        Scenario(
            "bounded_crash", "Price falls through minAnswer and the feed pins to the floor",
            "needs a consumer-side price bound or a move limit — staleness will not catch it",
            scenario_bounded_crash,
        ),
        Scenario(
            "frozen_feed", "Publishing stops; reads keep succeeding with an ageing answer",
            "caught by a staleness window sized to the real heartbeat",
            scenario_frozen_feed,
        ),
        Scenario(
            "sequencer_outage", "An L2 sequencer goes down and returns inside its grace period",
            "needs the sequencer uptime gate; the price feed answers throughout",
            scenario_sequencer_outage,
        ),
        Scenario(
            "carried_rounds", "Rounds finalise without a fresh answer",
            "needs answeredInRound >= roundId",
            scenario_carried_rounds,
        ),
        Scenario(
            "flash_spike", "One round prints 40% away and the next reverts",
            "needs a move limit; the spike is fresh, positive and complete",
            scenario_flash_spike,
        ),
        Scenario(
            "incomplete_round", "A round started and never finalised (updatedAt == 0)",
            "needs the updatedAt != 0 check",
            scenario_incomplete_round,
        ),
        Scenario(
            "clock_skew", "A round timestamped in the future",
            "needs signed age arithmetic; unsigned subtraction underflows here",
            scenario_clock_skew,
        ),
    ]
}


def run_scenario(name: str, guard: Optional[Guard] = None) -> ReplayReport:
    """Replay one named scenario. Raises :class:`SimulationError` on an unknown name."""
    scenario = SCENARIOS.get(name)
    if scenario is None:
        raise SimulationError(
            f"unknown scenario '{name}'. Known: {', '.join(sorted(SCENARIOS))}",
            scenario=name,
        )
    return replay(guard or Guard(), scenario.build(), name=scenario.name,
                  expectation=scenario.expectation)


@dataclass
class AuditResult:
    """How one guard fared across every scenario."""

    guard: Guard
    reports: List[ReplayReport]

    @property
    def passed(self) -> List[str]:
        """Scenarios the guard handled correctly — caught the bad, accepted the good."""
        out = []
        for report in self.reports:
            scenario = SCENARIOS[report.name]
            if report.caught == scenario.should_catch:
                out.append(report.name)
        return out

    @property
    def failed(self) -> List[str]:
        return [r.name for r in self.reports if r.name not in self.passed]

    @property
    def score(self) -> float:
        return len(self.passed) / len(self.reports) if self.reports else 0.0

    def as_dict(self) -> Dict[str, Any]:
        return {
            "guard": self.guard.as_dict(),
            "passed": self.passed,
            "failed": self.failed,
            "score": round(self.score, 4),
            "reports": [r.as_dict() for r in self.reports],
        }


def audit_guard(guard: Optional[Guard] = None) -> AuditResult:
    """Replay a guard against every scenario and report which holes it leaves.

    This is the module's headline: one call that answers "which of the known oracle
    failure modes does my integration actually defend against?"
    """
    guard = guard or Guard()
    return AuditResult(
        guard=guard,
        reports=[run_scenario(name, guard) for name in SCENARIOS],
    )
