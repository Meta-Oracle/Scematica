"""A tool-calling agent whose every number comes off a chain.

The single design rule: **the model never produces a price.** It chooses which of this
package's read functions to call and how to phrase the result; the numbers come from
`eth_call`. A language model asked "what is ETH worth" will happily emit a plausible
figure from training data, and a plausible-but-invented oracle price is precisely the
failure this whole toolkit exists to prevent. So:

* Every tool is a real function in this package. There is no "answer from memory" tool.
* The system prompt forbids stating any figure that did not come from a tool result.
* Every tool call is recorded in :class:`AgentTurn` and shown in the shell, so the user
  can re-run the same command and check.
* When no tool applies, the honest answer is "I can't read that", not a guess.

The tool surface is deliberately small and read-only. Nothing here can spend gas, sign,
or write a file.
"""
from __future__ import annotations

import json
from dataclasses import dataclass, field
from typing import Any, Callable, Dict, List, Optional

from .cadence import profile_feed
from .ccip import ROUTERS, verify_lanes
from .divergence import common_pairs, compare_pair
from .feeds import FEEDS, feed_count, list_feeds, read_all_feeds, read_feed
from .gas import analyse_gas
from .health import diagnose
from .llm import Completion, LlmClient, Message, NoProviderConfigured
from .networks import list_networks
from .safety import audit_feed, audit_network
from .sequencer import SEQUENCER_FEEDS, read_sequencer

MAX_TOOL_ROUNDS = 6

#: Hard cap on one tool result, in characters.
#:
#: Free tiers meter tokens per minute — Groq's is 12,000 — and a raw `audit_network`
#: dump is thousands of tokens of `remedy` prose the model does not need to answer the
#: question. Every tool below therefore returns a *compacted* view rather than its full
#: dataclass. That was not an optimisation: sending the full dicts hit a 429 on the
#: second question of a two-question session. Less noise also measurably improves the
#: answers.
MAX_TOOL_RESULT_CHARS = 3000

SYSTEM_PROMPT = """You are the assistant inside Alchem-Link, a command-line toolkit that \
reads Chainlink oracles and EVM chain state.

ABSOLUTE RULE: never state a price, age, heartbeat, gas figure, address, or any other \
number unless it came back from a tool call in this conversation. You have no reliable \
knowledge of current prices. If you cannot get a number from a tool, say so plainly \
instead of estimating. Inventing an oracle price is the exact failure this tool exists \
to prevent.

Guidance:
- Prefer one targeted tool call over several broad ones.
- `audit_feed` is the right tool for "is this safe to use" questions. It checks \
staleness, non-positive answers, incomplete and carried-over rounds, circuit-breaker \
bounds, and the L2 sequencer.
- Staleness is judged per feed against its MEASURED heartbeat, which differs by chain: \
Polygon publishes about every 60s, Base and Optimism about every 1200s, Ethereum 3600s.
- On L2s (base, arbitrum, optimism, scroll, linea) a price read is only safe if the \
sequencer is up AND has been up past its grace period.
- When a feed is stale or an audit finds something, lead with that.
- Be concise. This is a terminal. A few sentences, no headings, no markdown tables.
- Quote figures with their units and say which network they came from.
- Do not name the tools you called or say "the function returned". The caller already \
sees every call. State what you found."""


@dataclass
class ToolCall:
    """One executed tool call, kept so the user can verify the answer."""
    name: str
    arguments: Dict[str, Any]
    ok: bool
    result: Any = None
    error: str = ""

    @property
    def summary(self) -> str:
        args = ", ".join(f"{k}={v!r}" for k, v in self.arguments.items())
        return f"{self.name}({args})"


@dataclass
class AgentTurn:
    """What the agent did and said for one user message."""
    reply: str
    tool_calls: List[ToolCall] = field(default_factory=list)
    rounds: int = 0
    model: str = ""
    error: str = ""

    @property
    def ok(self) -> bool:
        return not self.error


# ── tool implementations ─────────────────────────────────────────────────────────
#
# Each returns plain JSON-able data. Exceptions are caught by the dispatcher and handed
# back to the model as an error string, so a bad argument becomes a retry rather than a
# crashed session.


def _reading(reading) -> Dict[str, Any]:
    """The fields that answer a price question, and nothing else."""
    return {
        "pair": reading.pair,
        "network": reading.network,
        "description": reading.description,
        "price": round(reading.price, 8),
        "status": reading.status,
        "age_secs": reading.age_secs,
        "heartbeat_secs": reading.heartbeat_secs,
        "stale": reading.stale,
        **({"note": reading.note} if reading.note else {}),
        **({"carried_over": True} if reading.carried_over else {}),
    }


def _tool_read_feed(pair: str, network: str = "ethereum") -> Dict[str, Any]:
    return _reading(read_feed(pair, network=network))


def _tool_read_all_feeds(network: str = "ethereum") -> List[Dict[str, Any]]:
    return [_reading(r) for r in read_all_feeds(network=network)]


def _audit(audit) -> Dict[str, Any]:
    """Findings without the `remedy` prose — the model explains, it does not recite."""
    return {
        "pair": audit.pair,
        "network": audit.network,
        "price": audit.price,
        "worst": audit.worst,
        "safe_to_consume": audit.safe_to_consume,
        "findings": [
            {"code": f.code, "severity": f.severity, "detail": f.detail}
            for f in audit.sorted_findings
            # `info` findings record that a check ran and passed. Useful in the CLI,
            # pure token cost here.
            if f.severity != "info"
        ],
    }


def _tool_audit_feed(pair: str, network: str = "ethereum") -> Dict[str, Any]:
    return _audit(audit_feed(pair, network=network))


def _tool_audit_network(network: str = "ethereum") -> List[Dict[str, Any]]:
    return [
        {k: v for k, v in _audit(a).items() if k != "network"}
        for a in audit_network(network=network)
    ]


def _tool_compare_across_chains(pair: str) -> Dict[str, Any]:
    report = compare_pair(pair)
    return {
        "pair": report.pair,
        "consensus": report.consensus,
        "spread_bps": round(report.spread_bps, 2),
        "verdict": report.verdict,
        "legs": [
            {
                "network": leg.network,
                "price": round(leg.price, 8),
                "deviation_bps": round(leg.deviation_bps, 1),
                "age_secs": leg.age_secs,
                "stale": leg.stale,
                **({"error": leg.error} if leg.error else {}),
            }
            for leg in report.legs
        ],
    }


def _tool_feed_cadence(pair: str, network: str = "ethereum", rounds: int = 40) -> Dict[str, Any]:
    profile = profile_feed(pair, network=network, rounds=min(max(rounds, 5), 120))
    return {
        "pair": profile.pair,
        "network": profile.network,
        "declared_heartbeat": profile.declared_heartbeat,
        "observed_heartbeat": profile.observed_heartbeat,
        "heartbeat_verdict": profile.heartbeat_verdict,
        "observed_ceiling_secs": profile.observed_ceiling_secs,
        "median_interval": profile.median_interval,
        "heartbeat_triggered": profile.heartbeat_triggered,
        "deviation_triggered": profile.deviation_triggered,
        "inferred_deviation_pct": profile.inferred_deviation_pct,
        "samples": profile.samples,
    }


def _tool_sequencer_status(network: str) -> Dict[str, Any]:
    status = read_sequencer(network)
    if status is None:
        return {
            "network": network,
            "applicable": False,
            "detail": f"{network} has no registered sequencer uptime feed "
                      f"(registered: {', '.join(sorted(SEQUENCER_FEEDS))})",
        }
    return {**status.as_dict(), "applicable": True}


def _tool_gas(network: str = "ethereum") -> Dict[str, Any]:
    report = analyse_gas(network=network)
    return {
        "network": report.network,
        "base_fee_gwei": round(report.base_fee_wei / 1e9, 4),
        "next_base_fee_gwei": round(report.next_base_fee_wei / 1e9, 4),
        "trend": report.trend,
        "congestion": round(report.congestion, 3),
        "native_symbol": report.native_symbol,
        "native_usd": report.native_usd,
        "tiers": [
            {
                "label": tier.label,
                "tip_gwei": round(tier.priority_fee_wei / 1e9, 4),
                "transfer_usd": (
                    round(tier.cost_wei(21_000) / 1e18 * report.native_usd, 4)
                    if report.native_usd else None
                ),
                "swap_usd": (
                    round(tier.cost_wei(180_000) / 1e18 * report.native_usd, 4)
                    if report.native_usd else None
                ),
            }
            for tier in report.tiers
        ],
    }


def _tool_ccip_lanes(network: str) -> Dict[str, Any]:
    if network.lower() not in ROUTERS:
        return {
            "error": f"no verified CCIP router for {network}. "
                     f"Verified: {', '.join(sorted(ROUTERS))}"
        }
    lanes = verify_lanes(network)
    return {
        "network": network.lower(),
        "router": ROUTERS[network.lower()],
        "open": [lane.destination for lane in lanes if lane.supported],
        "closed": [lane.destination for lane in lanes if lane.supported is False],
        "selectors": {lane.destination: lane.destination_selector for lane in lanes},
    }


def _tool_doctor(network: str = "ethereum") -> Dict[str, Any]:
    return diagnose(network=network).as_dict()


def _tool_registry(network: str = "") -> Dict[str, Any]:
    """Offline: what this toolkit knows about, so the model stops guessing pair names."""
    if network:
        return {
            "network": network,
            "feeds": [
                {
                    "pair": f.pair,
                    "address": f.address,
                    "heartbeat_secs": f.heartbeat_secs,
                    "heartbeat_measured": f.heartbeat_measured,
                    "note": f.note,
                }
                for f in list_feeds(network)
            ],
        }
    return {
        "total_feeds": feed_count(),
        "networks": [
            {
                "key": n.key,
                "chain_id": n.chain_id,
                "layer2": n.layer2,
                "testnet": n.testnet,
                "feeds": sorted(FEEDS.get(n.key, {})),
            }
            for n in list_networks()
        ],
        "multi_chain_pairs": common_pairs(),
    }


TOOL_IMPLS: Dict[str, Callable[..., Any]] = {
    "read_feed": _tool_read_feed,
    "read_all_feeds": _tool_read_all_feeds,
    "audit_feed": _tool_audit_feed,
    "audit_network": _tool_audit_network,
    "compare_across_chains": _tool_compare_across_chains,
    "feed_cadence": _tool_feed_cadence,
    "sequencer_status": _tool_sequencer_status,
    "gas": _tool_gas,
    "ccip_lanes": _tool_ccip_lanes,
    "doctor": _tool_doctor,
    "registry": _tool_registry,
}


def _schema(name: str, description: str, properties: Dict[str, Any], required: List[str]):
    return {
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": {
                "type": "object",
                "properties": properties,
                "required": required,
            },
        },
    }


_PAIR = {"type": "string", "description": "Feed pair, e.g. ETH/USD"}
_NETWORK = {
    "type": "string",
    "description": "Network key: ethereum, sepolia, base, arbitrum, optimism, polygon, "
                   "avalanche, bnb, gnosis, scroll, linea",
}

TOOL_SCHEMAS: List[Dict[str, Any]] = [
    _schema(
        "read_feed",
        "Read one Chainlink price feed live. Returns price, age, heartbeat and a "
        "FRESH/STALE/INVALID status. Use for any 'what is X worth' question.",
        {"pair": _PAIR, "network": _NETWORK},
        ["pair"],
    ),
    _schema(
        "read_all_feeds",
        "Read every registered feed on one network in a single batched call. Use when "
        "asked about a whole chain rather than one pair.",
        {"network": _NETWORK},
        [],
    ),
    _schema(
        "audit_feed",
        "Run every oracle consumer-safety check against one feed: staleness, "
        "non-positive answers, incomplete rounds, carried-over rounds, circuit-breaker "
        "bounds, decimals, and the L2 sequencer. Use for 'is it safe / can I trust it'.",
        {"pair": _PAIR, "network": _NETWORK},
        ["pair"],
    ),
    _schema(
        "audit_network",
        "Audit every feed on a network, worst first.",
        {"network": _NETWORK},
        [],
    ),
    _schema(
        "compare_across_chains",
        "Read one pair on every chain that carries it and report divergence in basis "
        "points, excluding stale legs from consensus. Use for cross-chain questions.",
        {"pair": _PAIR},
        ["pair"],
    ),
    _schema(
        "feed_cadence",
        "Measure a feed's real publish behaviour from its round history: observed "
        "heartbeat, deviation threshold, and whether the declared heartbeat matches.",
        {"pair": _PAIR, "network": _NETWORK,
         "rounds": {"type": "integer", "description": "Rounds of history, 5-120"}},
        ["pair"],
    ),
    _schema(
        "sequencer_status",
        "Read an L2's Chainlink sequencer uptime feed, with the grace period applied. "
        "Only base, arbitrum and optimism have registered uptime feeds.",
        {"network": _NETWORK},
        ["network"],
    ),
    _schema(
        "gas",
        "EIP-1559 fee tiers for a network, priced in USD via that chain's own Chainlink "
        "native-token feed.",
        {"network": _NETWORK},
        [],
    ),
    _schema(
        "ccip_lanes",
        "CCIP router, chain selectors, and which destination lanes the router reports "
        "as open.",
        {"network": _NETWORK},
        ["network"],
    ),
    _schema(
        "doctor",
        "End-to-end readiness check for a network: endpoint, chain id, gas, a feed read.",
        {"network": _NETWORK},
        [],
    ),
    _schema(
        "registry",
        "What this toolkit knows offline: networks, which pairs each carries, "
        "heartbeats. Call this first when unsure whether a pair exists.",
        {"network": {"type": "string", "description": "Optional: one network's feeds only"}},
        [],
    ),
]


def _truncate(payload: str) -> str:
    """Cap a tool result, saying so rather than silently cutting mid-JSON."""
    if len(payload) <= MAX_TOOL_RESULT_CHARS:
        return payload
    return (
        payload[:MAX_TOOL_RESULT_CHARS]
        + f'... [truncated at {MAX_TOOL_RESULT_CHARS} chars — '
          'ask about a specific network or pair for the full picture]'
    )


def execute_tool(name: str, arguments: Dict[str, Any]) -> ToolCall:
    """Run one tool, never raising — failures go back to the model as text."""
    impl = TOOL_IMPLS.get(name)
    if impl is None:
        return ToolCall(name=name, arguments=arguments, ok=False,
                        error=f"unknown tool '{name}'")
    try:
        return ToolCall(name=name, arguments=arguments, ok=True, result=impl(**arguments))
    except TypeError as exc:
        return ToolCall(name=name, arguments=arguments, ok=False,
                        error=f"bad arguments: {exc}")
    except Exception as exc:
        return ToolCall(name=name, arguments=arguments, ok=False,
                        error=f"{type(exc).__name__}: {exc}")


class Agent:
    """Chat over the toolkit, with tool results as the only source of numbers."""

    def __init__(
        self,
        client: Optional[LlmClient] = None,
        system_prompt: str = SYSTEM_PROMPT,
        max_rounds: int = MAX_TOOL_ROUNDS,
        network: str = "ethereum",
    ) -> None:
        self.client = client or LlmClient()
        self.max_rounds = max_rounds
        self.network = network
        self.history: List[Message] = [Message(role="system", content=system_prompt)]

    @property
    def label(self) -> str:
        return self.client.label

    def reset(self) -> None:
        self.history = self.history[:1]

    def _default_network_note(self) -> str:
        return f"(The user's current network context is '{self.network}'. " \
               f"Use it when they do not name one.)"

    def ask(self, prompt: str, on_tool: Optional[Callable[[ToolCall], None]] = None) -> AgentTurn:
        """One user turn, running tools until the model stops asking for them."""
        self.history.append(
            Message(role="user", content=f"{prompt}\n\n{self._default_network_note()}")
        )
        turn = AgentTurn(reply="", model=self.client.label)

        for round_index in range(self.max_rounds):
            turn.rounds = round_index + 1
            try:
                completion: Completion = self.client.chat(self.history, tools=TOOL_SCHEMAS)
            except Exception as exc:
                turn.error = str(exc)
                turn.reply = f"LLM request failed: {exc}"
                return turn

            if not completion.wants_tools:
                self.history.append(Message(role="assistant", content=completion.content))
                turn.reply = completion.content or "(no reply)"
                return turn

            self.history.append(Message(
                role="assistant",
                content=completion.content,
                tool_calls=completion.tool_calls,
            ))

            for raw in completion.tool_calls:
                function = raw.get("function") or {}
                name = function.get("name", "")
                try:
                    arguments = json.loads(function.get("arguments") or "{}")
                    if not isinstance(arguments, dict):
                        arguments = {}
                except json.JSONDecodeError:
                    arguments = {}

                call = execute_tool(name, arguments)
                turn.tool_calls.append(call)
                if on_tool is not None:
                    on_tool(call)

                payload = (
                    _truncate(json.dumps(call.result, default=str))
                    if call.ok
                    else json.dumps({"error": call.error})
                )
                self.history.append(Message(
                    role="tool",
                    content=payload,
                    tool_call_id=raw.get("id", ""),
                    name=name,
                ))

        turn.reply = (
            f"Stopped after {self.max_rounds} tool rounds without a final answer. "
            "Try a narrower question."
        )
        turn.error = "tool round limit reached"
        return turn


def build_agent(network: str = "ethereum") -> Agent:
    """Construct an agent, or raise :class:`NoProviderConfigured` with setup guidance."""
    return Agent(client=LlmClient(), network=network)


__all__ = [
    "Agent",
    "AgentTurn",
    "ToolCall",
    "TOOL_SCHEMAS",
    "TOOL_IMPLS",
    "SYSTEM_PROMPT",
    "execute_tool",
    "build_agent",
    "NoProviderConfigured",
]
