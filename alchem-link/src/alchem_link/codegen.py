"""Generate the consumer you should have written — and the tests that prove it.

Every check :mod:`alchem_link.safety` looks for is a check somebody left out. The
tutorial consumer everyone starts from is four lines that ignore all of them::

    (, int256 price, , ,) = feed.latestRoundData();
    return uint256(price);

That compiles, works in testing, and is wrong in five separate ways. This module emits
the version with all of them, filled in with values read *from the chain*: the
checksummed address, the measured per-chain heartbeat, and — only on a rollup — the
sequencer uptime feed and grace period.

The part that makes this more than a snippet generator is :func:`generate_project`. A
guard you cannot see fail is a guard you do not know you have, so the emitted Foundry
project ships a settable mock aggregator and a test per failure mode, each asserting the
consumer *reverts*. `forge test` then demonstrates, rather than asserts, that a stale
round or a carried-over answer is rejected.

Three generators:

* :func:`generate_consumer` — one file, for pasting into an existing project.
* :func:`generate_project` — a compile-ready Foundry project: consumer, interfaces,
  mocks, tests, deploy script, config, README.
* :func:`generate_basket` — one contract reading several feeds, each with its own
  heartbeat, because a shared constant is exactly the bug this package keeps finding.
"""
from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Dict, List, Optional, Sequence

from .feeds import STALENESS_TOLERANCE, Feed, get_feed, list_feeds
from .networks import DEFAULT_NETWORK, get_network
from .sequencer import GRACE_PERIOD_SECS, SEQUENCER_FEEDS, is_l2

LANGUAGES = ("solidity", "typescript", "python", "rust")
FRAMEWORKS = ("foundry",)


def _identifier(pair: str) -> str:
    """``ETH/USD`` → ``EthUsd``, safe to paste into a contract name."""
    parts = [p for p in pair.replace("-", "/").split("/") if p]
    return "".join(part.capitalize() for part in parts) or "Price"


def _snake(pair: str) -> str:
    return pair.replace("/", "_").replace("-", "_").lower()


@dataclass
class Artifact:
    """One generated file."""
    path: str
    content: str
    description: str = ""

    def as_dict(self) -> Dict[str, Any]:
        return {"path": self.path, "description": self.description, "bytes": len(self.content)}


@dataclass
class GeneratedConsumer:
    language: str
    pair: str
    network: str
    address: str
    heartbeat_secs: int
    code: str
    guards: tuple = ()

    def as_dict(self) -> Dict[str, Any]:
        return {
            "language": self.language,
            "pair": self.pair,
            "network": self.network,
            "address": self.address,
            "heartbeat_secs": self.heartbeat_secs,
            "guards": list(self.guards),
            "code": self.code,
        }


@dataclass
class GeneratedProject:
    """A directory's worth of files, plus what it is for."""
    name: str
    framework: str
    network: str
    pairs: List[str]
    artifacts: List[Artifact] = field(default_factory=list)
    guards: tuple = ()

    def write(self, out_dir: str | Path, overwrite: bool = False) -> List[str]:
        """Write every artifact under ``out_dir``. Returns the paths written."""
        root = Path(out_dir)
        written: List[str] = []
        for artifact in self.artifacts:
            target = root / artifact.path
            if target.exists() and not overwrite:
                raise FileExistsError(
                    f"{target} already exists — pass overwrite=True (or --force) to replace it"
                )
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text(artifact.content, encoding="utf-8")
            written.append(str(target))
        return written

    def as_dict(self) -> Dict[str, Any]:
        return {
            "name": self.name,
            "framework": self.framework,
            "network": self.network,
            "pairs": self.pairs,
            "guards": list(self.guards),
            "files": [a.as_dict() for a in self.artifacts],
        }


# ── shared Solidity fragments ────────────────────────────────────────────────────

_IAGGREGATOR = """// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// @notice The subset of AggregatorV3Interface a price consumer actually needs.
interface IAggregatorV3 {
    function decimals() external view returns (uint8);
    function description() external view returns (string memory);
    function latestRoundData()
        external
        view
        returns (uint80 roundId, int256 answer, uint256 startedAt, uint256 updatedAt, uint80 answeredInRound);
}
"""

_ISEQUENCER = """// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// @notice Chainlink's L2 sequencer uptime feed.
/// @dev `answer` is 0 when the sequencer is up and 1 when it is down. `startedAt` is
///      when that state began — the grace period is measured from it, not from the
///      round timestamp.
interface ISequencerUptimeFeed {
    function latestRoundData()
        external
        view
        returns (uint80 roundId, int256 answer, uint256 startedAt, uint256 updatedAt, uint80 answeredInRound);
}
"""

_SEQUENCER_STATE = """
    /// @dev {network_label} is a rollup. A price feed keeps answering while the
    ///      sequencer is down, with a price frozen at the moment it stopped.
    ISequencerUptimeFeed public constant SEQUENCER =
        ISequencerUptimeFeed({sequencer_address});

    /// @dev Time the sequencer must have been back up before prices are trusted again.
    ///      The moment after a restart is the dangerous one: transactions queued during
    ///      the outage all execute at once, against a price that did not move.
    uint256 public constant GRACE_PERIOD = {grace};
"""

_SEQUENCER_ERRORS = """    error SequencerDown();
    error GracePeriodNotOver(uint256 elapsed, uint256 required);
"""

_SEQUENCER_CHECK = """        // Check the sequencer before the price. If it is down, the price is a fossil.
        (, int256 sequencerStatus, uint256 sequencerStartedAt, , ) = SEQUENCER.latestRoundData();

        // 0 == up, 1 == down.
        if (sequencerStatus != 0) revert SequencerDown();

        // startedAt == 0 marks an invalid round: the status is unknown, so treat it as down.
        if (sequencerStartedAt == 0) revert SequencerDown();

        uint256 sequencerUpFor = block.timestamp - sequencerStartedAt;
        if (sequencerUpFor <= GRACE_PERIOD) revert GracePeriodNotOver(sequencerUpFor, GRACE_PERIOD);

"""

_GUARD_BODY = """        (uint80 roundId, int256 answer, , uint256 updatedAt, uint80 answeredInRound) =
            {feed_expr}.latestRoundData();

        // A round that was started but never finalised reports updatedAt == 0.
        if (updatedAt == 0) revert IncompleteRound();

        // The feed answers happily whether the last publish was ten seconds or ten
        // hours ago. This is the check that makes the difference.
        if (block.timestamp - updatedAt > {max_age_expr}) revert StalePrice(updatedAt, {max_age_expr});

        // A non-positive answer is never a real quote.
        if (answer <= 0) revert InvalidPrice(answer);

        // answeredInRound < roundId means this round carried an older answer forward.
        if (answeredInRound < roundId) revert StaleRoundAnswer(answeredInRound, roundId);
"""


def _standalone_interfaces(l2: bool) -> str:
    """Interfaces inlined, for the single-file target that must compile alone."""
    block = """
interface IAggregatorV3 {
    function decimals() external view returns (uint8);
    function description() external view returns (string memory);
    function latestRoundData()
        external
        view
        returns (uint80 roundId, int256 answer, uint256 startedAt, uint256 updatedAt, uint80 answeredInRound);
}
"""
    if l2:
        block += """
interface ISequencerUptimeFeed {
    function latestRoundData()
        external
        view
        returns (uint80 roundId, int256 answer, uint256 startedAt, uint256 updatedAt, uint80 answeredInRound);
}
"""
    return block


def _consumer_solidity(
    feed: Feed,
    net,
    contract: str,
    standalone: bool,
) -> str:
    l2 = is_l2(net.key) and net.key in SEQUENCER_FEEDS
    max_age = feed.stale_after_secs
    tolerance = int(STALENESS_TOLERANCE * 100)

    if standalone:
        header = "// SPDX-License-Identifier: MIT\npragma solidity ^0.8.20;\n"
        imports = _standalone_interfaces(l2)
    else:
        header = "// SPDX-License-Identifier: MIT\npragma solidity ^0.8.20;\n"
        imports = '\nimport {IAggregatorV3} from "./interfaces/IAggregatorV3.sol";\n'
        if l2:
            imports += 'import {ISequencerUptimeFeed} from "./interfaces/ISequencerUptimeFeed.sol";\n'

    sequencer_state = (
        _SEQUENCER_STATE.format(
            network_label=net.label,
            sequencer_address=SEQUENCER_FEEDS[net.key],
            grace=GRACE_PERIOD_SECS,
        )
        if l2 else ""
    )

    return f"""{header}
// Generated by alchem-link for {feed.pair} on {net.label}.
// Address, decimals and heartbeat were read from the chain, not copied from a table.
{imports}
/// @notice Reads {feed.pair} with the staleness, positivity and completeness checks that
///         `latestRoundData()` does not perform for you.
contract {contract} {{
    IAggregatorV3 public constant FEED = IAggregatorV3({feed.address});

    /// @dev Measured publish interval for this feed on {net.label}, plus {tolerance}% slack.
    ///      This value is per-feed and per-chain: the same pair is {feed.heartbeat_secs}s here
    ///      and can be an order of magnitude different elsewhere.
    uint256 public constant MAX_AGE = {max_age};
{sequencer_state}
    error StalePrice(uint256 updatedAt, uint256 maxAge);
    error InvalidPrice(int256 answer);
    error IncompleteRound();
    error StaleRoundAnswer(uint80 answeredInRound, uint80 roundId);
{_SEQUENCER_ERRORS if l2 else ""}
    /// @return price The latest answer, guaranteed positive, complete and fresh.
    /// @return decimals The feed's own decimals — do not hardcode 1e8.
    function latestPrice() public view returns (int256 price, uint8 decimals) {{
{_SEQUENCER_CHECK if l2 else ""}{_GUARD_BODY.format(feed_expr="FEED", max_age_expr="MAX_AGE")}
        return (answer, FEED.decimals());
    }}

    /// @notice Scale the answer to 18 decimals, the usual internal convention.
    function latestPriceScaled() external view returns (uint256) {{
        (int256 price, uint8 decimals) = latestPrice();
        return uint256(price) * (10 ** (18 - decimals));
    }}
}}
"""


# ── mocks and tests ──────────────────────────────────────────────────────────────

_MOCK_AGGREGATOR = """// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// @notice A settable AggregatorV3, so every failure mode can be produced on demand.
/// @dev The point of the test suite is to *watch* each guard reject something. That
///      needs a feed you can put into a bad state deliberately, which no real
///      aggregator will do for you.
contract MockAggregator {
    uint8 public decimals = 8;
    string public description;

    uint80 internal _roundId = 1;
    int256 internal _answer;
    uint256 internal _startedAt;
    uint256 internal _updatedAt;
    uint80 internal _answeredInRound = 1;

    constructor(string memory description_, int256 answer_) {
        description = description_;
        _answer = answer_;
        _startedAt = block.timestamp;
        _updatedAt = block.timestamp;
    }

    function latestRoundData()
        external
        view
        returns (uint80, int256, uint256, uint256, uint80)
    {
        return (_roundId, _answer, _startedAt, _updatedAt, _answeredInRound);
    }

    function setAnswer(int256 answer_) external {
        _answer = answer_;
        _updatedAt = block.timestamp;
        _roundId += 1;
        _answeredInRound = _roundId;
    }

    /// @notice Age the last update without publishing a new one.
    function setUpdatedAt(uint256 updatedAt_) external {
        _updatedAt = updatedAt_;
    }

    /// @notice Produce a round that started but never finalised.
    function setIncomplete() external {
        _updatedAt = 0;
    }

    /// @notice Produce a round that carried an older answer forward.
    function setCarriedOver() external {
        _roundId += 1;
        // answeredInRound deliberately left behind roundId.
    }

    function setDecimals(uint8 decimals_) external {
        decimals = decimals_;
    }
}
"""

_MOCK_SEQUENCER = """// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

/// @notice A settable L2 sequencer uptime feed.
/// @dev `startedAt` is the field that matters: the grace period is measured from when
///      the current up/down state began, so "up but only just" is a distinct state from
///      "up and trustworthy" and both must be reachable in tests.
contract MockSequencer {
    int256 internal _answer;      // 0 = up, 1 = down
    uint256 internal _startedAt;

    constructor() {
        _answer = 0;
        _startedAt = 1;
    }

    function latestRoundData()
        external
        view
        returns (uint80, int256, uint256, uint256, uint80)
    {
        return (1, _answer, _startedAt, _startedAt, 1);
    }

    function setUp(uint256 startedAt_) external {
        _answer = 0;
        _startedAt = startedAt_;
    }

    function setDown(uint256 startedAt_) external {
        _answer = 1;
        _startedAt = startedAt_;
    }

    /// @notice The documented invalid-round case: status is unknown, treat as down.
    function setInvalidRound() external {
        _answer = 0;
        _startedAt = 0;
    }
}
"""


def _test_solidity(feed: Feed, net, contract: str) -> str:
    l2 = is_l2(net.key) and net.key in SEQUENCER_FEEDS
    max_age = feed.stale_after_secs
    price = 2000 * 10 ** 8

    sequencer_setup = ""
    sequencer_tests = ""
    deploy_args = ""

    if l2:
        sequencer_setup = """
        sequencer = new MockSequencer();
        // Up long enough to be trusted, so the price tests exercise the price guards.
        sequencer.setUp(1);
        vm.warp(block.timestamp + GRACE + 1);
"""
        deploy_args = "address(feed), address(sequencer)"
        sequencer_tests = """
    function test_RevertsWhenSequencerIsDown() public {
        sequencer.setDown(block.timestamp);
        vm.expectRevert(Consumer.SequencerDown.selector);
        consumer.latestPrice();
    }

    function test_RevertsInsideTheGracePeriod() public {
        // The sequencer is UP — this is the check almost everyone omits, and the one
        // that matters most: the queue flush happens right here.
        sequencer.setUp(block.timestamp);
        vm.expectRevert(
            abi.encodeWithSelector(Consumer.GracePeriodNotOver.selector, 0, GRACE)
        );
        consumer.latestPrice();
    }

    function test_PassesOnceGracePeriodElapses() public {
        sequencer.setUp(block.timestamp);
        vm.warp(block.timestamp + GRACE + 1);
        (int256 price, ) = consumer.latestPrice();
        assertEq(price, int256(PRICE));
    }

    function test_RevertsOnInvalidSequencerRound() public {
        sequencer.setInvalidRound();
        vm.expectRevert(Consumer.SequencerDown.selector);
        consumer.latestPrice();
    }
"""
    else:
        deploy_args = "address(feed)"

    imports = 'import {MockAggregator} from "./mocks/MockAggregator.sol";\n'
    if l2:
        imports += 'import {MockSequencer} from "./mocks/MockSequencer.sol";\n'

    return f"""// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {{Test}} from "forge-std/Test.sol";
import {{{contract}Testable as Consumer}} from "./harness/{contract}Testable.sol";
{imports}
/// @notice Proves each guard in {contract} rejects the thing it was written for.
///
/// Every test here corresponds to a finding `alchem-link audit` looks for. A guard that
/// has never been seen to fire is indistinguishable from one that does not work — these
/// make each failure mode reachable and assert the revert.
contract {contract}Test is Test {{
    uint256 internal constant MAX_AGE = {max_age};
    uint256 internal constant GRACE = {GRACE_PERIOD_SECS};
    int256 internal constant PRICE = {price};

    Consumer internal consumer;
    MockAggregator internal feed;
{"    MockSequencer internal sequencer;" if l2 else ""}

    function setUp() public {{
        // Start well past zero so `block.timestamp - x` cannot underflow in setup.
        vm.warp(1_000_000);
        feed = new MockAggregator("{feed.pair}", PRICE);
{sequencer_setup}
        consumer = new Consumer({deploy_args});
    }}

    function test_ReadsAHealthyFeed() public view {{
        (int256 price, uint8 decimals) = consumer.latestPrice();
        assertEq(price, PRICE);
        assertEq(decimals, 8);
    }}

    function test_RevertsOnStalePrice() public {{
        // Age the last update past the measured heartbeat for this feed on {net.key}.
        feed.setUpdatedAt(block.timestamp - MAX_AGE - 1);
        vm.expectRevert(
            abi.encodeWithSelector(
                Consumer.StalePrice.selector, block.timestamp - MAX_AGE - 1, MAX_AGE
            )
        );
        consumer.latestPrice();
    }}

    function test_AcceptsPriceExactlyAtTheAgeLimit() public {{
        // The boundary is `>`, not `>=`. A feed publishing exactly on time is healthy.
        feed.setUpdatedAt(block.timestamp - MAX_AGE);
        (int256 price, ) = consumer.latestPrice();
        assertEq(price, PRICE);
    }}

    function test_RevertsOnZeroAnswer() public {{
        feed.setAnswer(0);
        vm.expectRevert(abi.encodeWithSelector(Consumer.InvalidPrice.selector, int256(0)));
        consumer.latestPrice();
    }}

    function test_RevertsOnNegativeAnswer() public {{
        feed.setAnswer(-1);
        vm.expectRevert(abi.encodeWithSelector(Consumer.InvalidPrice.selector, int256(-1)));
        consumer.latestPrice();
    }}

    function test_RevertsOnIncompleteRound() public {{
        feed.setIncomplete();
        vm.expectRevert(Consumer.IncompleteRound.selector);
        consumer.latestPrice();
    }}

    function test_RevertsOnCarriedOverRound() public {{
        feed.setCarriedOver();
        vm.expectRevert(
            abi.encodeWithSelector(Consumer.StaleRoundAnswer.selector, uint80(1), uint80(2))
        );
        consumer.latestPrice();
    }}

    function test_ScalesToEighteenDecimals() public view {{
        assertEq(consumer.latestPriceScaled(), uint256(PRICE) * 10 ** 10);
    }}

    function testFuzz_AcceptsAnyPositiveFreshAnswer(int256 answer) public {{
        answer = bound(answer, 1, type(int128).max);
        feed.setAnswer(answer);
        (int256 price, ) = consumer.latestPrice();
        assertEq(price, answer);
    }}
{sequencer_tests}}}
"""


def _harness_solidity(feed: Feed, net, contract: str) -> str:
    """A constructor-injected twin of the consumer, so tests can point it at mocks.

    The shipped consumer hardcodes its addresses as `constant` — that is the right shape
    for production (cheaper, and the address cannot be swapped after deploy) and an
    impossible shape for tests. Rather than weaken the real contract to make it testable,
    the harness mirrors its logic with injectable addresses. The guards are identical;
    only the source of the addresses differs.
    """
    l2 = is_l2(net.key) and net.key in SEQUENCER_FEEDS
    max_age = feed.stale_after_secs

    fields = "    IAggregatorV3 public immutable FEED;\n"
    ctor_args = "address feed_"
    ctor_body = "        FEED = IAggregatorV3(feed_);\n"
    if l2:
        fields += "    ISequencerUptimeFeed public immutable SEQUENCER;\n"
        ctor_args += ", address sequencer_"
        ctor_body += "        SEQUENCER = ISequencerUptimeFeed(sequencer_);\n"

    imports = 'import {IAggregatorV3} from "../../src/interfaces/IAggregatorV3.sol";\n'
    if l2:
        imports += (
            'import {ISequencerUptimeFeed} from "../../src/interfaces/ISequencerUptimeFeed.sol";\n'
        )

    return f"""// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

{imports}
/// @notice Constructor-injected twin of {contract}, for tests only.
/// @dev {contract} holds its addresses as `constant`, which is correct for production
///      and untestable against mocks. This mirrors its guard logic exactly with
///      injectable addresses — if you change a guard there, change it here.
contract {contract}Testable {{
{fields}
    uint256 public constant MAX_AGE = {max_age};
    uint256 public constant GRACE_PERIOD = {GRACE_PERIOD_SECS};

    error StalePrice(uint256 updatedAt, uint256 maxAge);
    error InvalidPrice(int256 answer);
    error IncompleteRound();
    error StaleRoundAnswer(uint80 answeredInRound, uint80 roundId);
{_SEQUENCER_ERRORS if l2 else ""}
    constructor({ctor_args}) {{
{ctor_body}    }}

    function latestPrice() public view returns (int256 price, uint8 decimals) {{
{_SEQUENCER_CHECK if l2 else ""}{_GUARD_BODY.format(feed_expr="FEED", max_age_expr="MAX_AGE")}
        return (answer, FEED.decimals());
    }}

    function latestPriceScaled() external view returns (uint256) {{
        (int256 price, uint8 decimals) = latestPrice();
        return uint256(price) * (10 ** (18 - decimals));
    }}
}}
"""


def _deploy_script(feed: Feed, net, contract: str) -> str:
    return f"""// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

import {{Script}} from "forge-std/Script.sol";
import {{{contract}}} from "../src/{contract}.sol";

/// @notice Deploys {contract} to {net.label}.
///
///   forge script script/Deploy{contract}.s.sol \\
///     --rpc-url $RPC_URL --broadcast --verify
///
/// The feed address is compiled in, so deploying this to a chain other than
/// {net.key} would point it at an address that means nothing there. The chainid
/// check below makes that a failed deploy rather than a silent one.
contract Deploy{contract} is Script {{
    uint256 internal constant EXPECTED_CHAIN_ID = {net.chain_id};

    function run() external returns ({contract} deployed) {{
        require(
            block.chainid == EXPECTED_CHAIN_ID,
            "wrong chain: this consumer's feed address is {net.key}-only"
        );
        vm.startBroadcast();
        deployed = new {contract}();
        vm.stopBroadcast();
    }}
}}
"""


def _foundry_toml(name: str) -> str:
    return f"""# Generated by alchem-link for {name}.
[profile.default]
src = "src"
out = "out"
libs = ["lib"]
test = "test"
solc_version = "0.8.20"
optimizer = true
optimizer_runs = 200

# Guards are `revert`s, and a revert test that silently passes because the call failed
# for a different reason is worse than no test. Verbose traces make the difference
# visible when something regresses.
verbosity = 2

[fmt]
line_length = 100
tab_width = 4
"""


def _project_readme(feed: Feed, net, contract: str, l2: bool) -> str:
    guards = [
        "`IncompleteRound` — `updatedAt == 0`, a round started but never finalised",
        f"`StalePrice` — older than {feed.stale_after_secs}s "
        f"(measured {feed.heartbeat_secs}s heartbeat on {net.key} + "
        f"{int(STALENESS_TOLERANCE * 100)}% slack)",
        "`InvalidPrice` — a non-positive answer",
        "`StaleRoundAnswer` — `answeredInRound < roundId`, a carried-over answer",
    ]
    if l2:
        guards += [
            "`SequencerDown` — the L2 sequencer is down, or reporting an invalid round",
            f"`GracePeriodNotOver` — up, but for less than {GRACE_PERIOD_SECS}s",
        ]

    guard_lines = "\n".join(f"- {g}" for g in guards)
    measured = "measured" if feed.heartbeat_measured else "a conservative bound"

    return f"""# {contract}

Generated by [alchem-link](https://pypi.org/project/alchem-link/) for **{feed.pair}** on
**{net.label}** (chain {net.chain_id}).

    alchem-link generate {feed.pair} -n {net.key} --project --out .

## What is in here

    src/{contract}.sol              the consumer, addresses compiled in
    src/interfaces/                 AggregatorV3{" and the sequencer uptime feed" if l2 else ""}
    test/{contract}.t.sol           one test per failure mode
    test/harness/                   constructor-injected twin, so tests can use mocks
    test/mocks/                     settable aggregator{" and sequencer" if l2 else ""}
    script/Deploy{contract}.s.sol   deploy, with a chain-id guard

## The guards

{guard_lines}

Each has a test that drives the mock into that state and asserts the revert. A guard
nobody has watched fire is indistinguishable from one that does not work.

```bash
forge install foundry-rs/forge-std
forge test -vv
```

## Values, and where they came from

| | |
|---|---|
| Aggregator | `{feed.address}` |
| Network | {net.label} (chain {net.chain_id}) |
| Heartbeat | {feed.heartbeat_secs}s — {measured} |
| `MAX_AGE` | {feed.stale_after_secs}s |
{"| Sequencer uptime feed | `" + SEQUENCER_FEEDS.get(net.key, "") + "` |" if l2 else ""}

The address was read from the chain and is filed under the pair the contract itself
reports. The heartbeat came from walking this feed's round history — it is **per feed and
per chain**, and a shared 3600s constant is wrong nearly everywhere: Polygon publishes
about every 60 seconds.

Re-check any of it:

```bash
alchem-link audit {feed.pair} -n {net.key}
alchem-link cadence {feed.pair} -n {net.key}
```

## Before you deploy

- `MAX_AGE` reflects the heartbeat measured when this was generated. Feeds get
  reconfigured; re-run `alchem-link cadence` before relying on it.
- The feed address is chain-specific. The deploy script refuses to run on the wrong
  chain rather than deploying something that points nowhere.
{"- On an L2 the sequencer check is not optional. Removing it reopens the contract during exactly the queue flush it was written to survive." if l2 else ""}
"""


# ── other language targets ───────────────────────────────────────────────────────

_TYPESCRIPT = '''// Generated by alchem-link for {pair} on {network_label}.
// Values read from the chain: address, decimals and the measured heartbeat.

const FEED_ADDRESS = "{address}";
const MAX_AGE_SECONDS = {max_age}; // measured {heartbeat}s heartbeat + {tolerance}% slack
const LATEST_ROUND_DATA = "0xfeaf968c"; // keccak256("latestRoundData()")[:4]
const DECIMALS = "0x313ce567";          // keccak256("decimals()")[:4]

export interface PriceReading {{
  price: number;
  decimals: number;
  updatedAt: number;
  ageSeconds: number;
  roundId: bigint;
}}

async function ethCall(rpcUrl: string, to: string, data: string): Promise<string> {{
  const response = await fetch(rpcUrl, {{
    method: "POST",
    headers: {{ "Content-Type": "application/json" }},
    body: JSON.stringify({{ jsonrpc: "2.0", id: 1, method: "eth_call", params: [{{ to, data }}, "latest"] }}),
  }});
  const body = await response.json();
  if (body.error) throw new Error(`eth_call failed: ${{body.error.message}}`);
  return body.result;
}}

/** Word `index` of an ABI payload, as a bigint. */
function word(payload: string, index: number): bigint {{
  const raw = payload.startsWith("0x") ? payload.slice(2) : payload;
  return BigInt("0x" + raw.slice(index * 64, (index + 1) * 64));
}}

/** Two's-complement int256 — Chainlink answers are signed. */
function toInt256(value: bigint): bigint {{
  return value >= 1n << 255n ? value - (1n << 256n) : value;
}}

/**
 * Read {pair}, rejecting anything a consumer must not act on.
 * Throws rather than returning a suspect number: a stale price that looks like a live
 * one is the failure mode worth engineering against.
 */
export async function read{contract_name}(rpcUrl: string): Promise<PriceReading> {{
  const [roundPayload, decimalsPayload] = await Promise.all([
    ethCall(rpcUrl, FEED_ADDRESS, LATEST_ROUND_DATA),
    ethCall(rpcUrl, FEED_ADDRESS, DECIMALS),
  ]);

  const roundId = word(roundPayload, 0);
  const answer = toInt256(word(roundPayload, 1));
  const updatedAt = Number(word(roundPayload, 3));
  const answeredInRound = word(roundPayload, 4);
  const decimals = Number(word(decimalsPayload, 0));

  if (updatedAt === 0) throw new Error("incomplete round: updatedAt is 0");
  if (answer <= 0n) throw new Error(`invalid answer: ${{answer}}`);
  if (answeredInRound < roundId) throw new Error("round carried an older answer forward");

  const ageSeconds = Math.floor(Date.now() / 1000) - updatedAt;
  if (ageSeconds > MAX_AGE_SECONDS) {{
    throw new Error(`stale: ${{ageSeconds}}s old, limit ${{MAX_AGE_SECONDS}}s`);
  }}

  return {{ price: Number(answer) / 10 ** decimals, decimals, updatedAt, ageSeconds, roundId }};
}}
'''

_PYTHON = '''"""Generated by alchem-link for {pair} on {network_label}."""
from alchem_link import read_feed

#: Measured heartbeat for this feed on this chain, plus {tolerance}% slack.
MAX_AGE_SECONDS = {max_age}
FEED_ADDRESS = "{address}"


def latest_{snake}() -> float:
    """Return the current {pair} price, or raise if it is not safe to act on.

    Raising rather than returning is deliberate: a caller that gets a number back has no
    way to notice it was stale, and the whole point of the check is that a stale answer
    looks exactly like a good one.
    """
    reading = read_feed("{pair}", network="{network}")

    if reading.answer_raw <= 0:
        raise ValueError(f"{pair} returned a non-positive answer: {{reading.answer_raw}}")
    if reading.carried_over:
        raise ValueError(f"{pair} round carried an older answer forward")
    if reading.age_secs > MAX_AGE_SECONDS:
        raise ValueError(
            f"{pair} is {{reading.age_secs}}s old, limit {{MAX_AGE_SECONDS}}s — refusing to trade"
        )
    return reading.price
'''

_RUST = '''//! Generated by alchem-link for {pair} on {network_label}.
//!
//! No ABI crate needed: reading an aggregator is three static words plus a signed
//! integer, and the selector is a constant.

use std::time::{{SystemTime, UNIX_EPOCH}};

/// Aggregator address on {network_label}.
pub const FEED_ADDRESS: &str = "{address}";

/// Measured {heartbeat}s heartbeat on {network} plus {tolerance}% slack.
pub const MAX_AGE_SECONDS: u64 = {max_age};

const LATEST_ROUND_DATA: &str = "0xfeaf968c"; // keccak256("latestRoundData()")[..4]
const DECIMALS: &str = "0x313ce567";          // keccak256("decimals()")[..4]

#[derive(Debug, Clone, Copy)]
pub struct PriceReading {{
    pub price: f64,
    pub decimals: u8,
    pub updated_at: u64,
    pub age_secs: u64,
    pub round_id: u128,
}}

#[derive(Debug)]
pub enum PriceError {{
    Transport(String),
    IncompleteRound,
    NonPositive(i128),
    CarriedOver {{ answered_in_round: u128, round_id: u128 }},
    Stale {{ age_secs: u64, max_age: u64 }},
}}

impl std::fmt::Display for PriceError {{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {{
        match self {{
            PriceError::Transport(e) => write!(f, "rpc failed: {{e}}"),
            PriceError::IncompleteRound => write!(f, "incomplete round: updatedAt is 0"),
            PriceError::NonPositive(a) => write!(f, "non-positive answer: {{a}}"),
            PriceError::CarriedOver {{ answered_in_round, round_id }} => write!(
                f, "round {{round_id}} carried the answer from {{answered_in_round}}"
            ),
            PriceError::Stale {{ age_secs, max_age }} => {{
                write!(f, "stale: {{age_secs}}s old, limit {{max_age}}s")
            }}
        }}
    }}
}}

impl std::error::Error for PriceError {{}}

/// Word `index` of an ABI payload as a u128 (enough for round ids and timestamps).
fn word_u128(payload: &str, index: usize) -> u128 {{
    let raw = payload.trim_start_matches("0x");
    let start = index * 64;
    u128::from_str_radix(&raw[start + 32..start + 64], 16).unwrap_or(0)
}}

/// Two's-complement int256, narrowed to i128 — Chainlink answers are signed.
fn word_i128(payload: &str, index: usize) -> i128 {{
    let raw = payload.trim_start_matches("0x");
    let start = index * 64;
    let high = u128::from_str_radix(&raw[start..start + 32], 16).unwrap_or(0);
    let low = u128::from_str_radix(&raw[start + 32..start + 64], 16).unwrap_or(0);
    // All-ones in the high half means a negative value.
    if high == u128::MAX {{ (low as i128).wrapping_sub(0) }} else {{ low as i128 }}
}}

/// Read {pair}, rejecting anything a consumer must not act on.
///
/// `eth_call` is left to the caller so this stays runtime-agnostic — pass a closure
/// that performs `eth_call(to, data)` with whatever client you already have.
pub fn read_price<F>(mut eth_call: F) -> Result<PriceReading, PriceError>
where
    F: FnMut(&str, &str) -> Result<String, String>,
{{
    let round = eth_call(FEED_ADDRESS, LATEST_ROUND_DATA).map_err(PriceError::Transport)?;
    let decimals_raw = eth_call(FEED_ADDRESS, DECIMALS).map_err(PriceError::Transport)?;

    let round_id = word_u128(&round, 0);
    let answer = word_i128(&round, 1);
    let updated_at = word_u128(&round, 3) as u64;
    let answered_in_round = word_u128(&round, 4);
    let decimals = word_u128(&decimals_raw, 0) as u8;

    if updated_at == 0 {{
        return Err(PriceError::IncompleteRound);
    }}
    if answer <= 0 {{
        return Err(PriceError::NonPositive(answer));
    }}
    if answered_in_round < round_id {{
        return Err(PriceError::CarriedOver {{ answered_in_round, round_id }});
    }}

    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
    let age_secs = now.saturating_sub(updated_at);
    if age_secs > MAX_AGE_SECONDS {{
        return Err(PriceError::Stale {{ age_secs, max_age: MAX_AGE_SECONDS }});
    }}

    Ok(PriceReading {{
        price: answer as f64 / 10f64.powi(decimals as i32),
        decimals,
        updated_at,
        age_secs,
        round_id,
    }})
}}
'''


def generate_consumer(
    pair: str,
    network: str = DEFAULT_NETWORK,
    language: str = "solidity",
    feed: Optional[Feed] = None,
) -> GeneratedConsumer:
    """Emit a single-file consumer for one feed on one chain."""
    if language not in LANGUAGES:
        raise ValueError(f"unknown language '{language}'. Choose from: {', '.join(LANGUAGES)}")

    net = get_network(network)
    target = feed or get_feed(pair, network)
    contract = f"{_identifier(target.pair)}Consumer"
    l2 = is_l2(net.key) and net.key in SEQUENCER_FEEDS

    guards = ["staleness", "positive answer", "complete round", "carried-round"]
    if l2 and language == "solidity":
        guards.append("L2 sequencer uptime + grace period")

    common = {
        "pair": target.pair,
        "network": net.key,
        "network_label": net.label,
        "address": target.address,
        "max_age": target.stale_after_secs,
        "heartbeat": target.heartbeat_secs,
        "tolerance": int(STALENESS_TOLERANCE * 100),
        "contract_name": _identifier(target.pair),
        "snake": _snake(target.pair),
    }

    if language == "solidity":
        code = _consumer_solidity(target, net, contract, standalone=True)
    elif language == "typescript":
        code = _TYPESCRIPT.format(**common)
    elif language == "rust":
        code = _RUST.format(**common)
    else:
        code = _PYTHON.format(**common)

    return GeneratedConsumer(
        language=language,
        pair=target.pair,
        network=net.key,
        address=target.address,
        heartbeat_secs=target.heartbeat_secs,
        code=code,
        guards=tuple(guards),
    )


def generate_project(
    pair: str,
    network: str = DEFAULT_NETWORK,
    framework: str = "foundry",
) -> GeneratedProject:
    """Emit a compile-ready Foundry project: consumer, mocks, tests, deploy, config."""
    if framework not in FRAMEWORKS:
        raise ValueError(f"unknown framework '{framework}'. Choose from: {', '.join(FRAMEWORKS)}")

    net = get_network(network)
    feed = get_feed(pair, network)
    contract = f"{_identifier(feed.pair)}Consumer"
    l2 = is_l2(net.key) and net.key in SEQUENCER_FEEDS

    guards = ["staleness", "positive answer", "complete round", "carried-round"]
    if l2:
        guards.append("L2 sequencer uptime + grace period")

    artifacts = [
        Artifact(f"src/{contract}.sol", _consumer_solidity(feed, net, contract, standalone=False),
                 "the consumer, addresses compiled in"),
        Artifact("src/interfaces/IAggregatorV3.sol", _IAGGREGATOR, "aggregator interface"),
        Artifact(f"test/{contract}.t.sol", _test_solidity(feed, net, contract),
                 "one test per failure mode"),
        Artifact(f"test/harness/{contract}Testable.sol", _harness_solidity(feed, net, contract),
                 "constructor-injected twin, so tests can use mocks"),
        Artifact("test/mocks/MockAggregator.sol", _MOCK_AGGREGATOR, "settable aggregator"),
        Artifact(f"script/Deploy{contract}.s.sol", _deploy_script(feed, net, contract),
                 "deploy, with a chain-id guard"),
        Artifact("foundry.toml", _foundry_toml(contract), "framework config"),
        Artifact("README.md", _project_readme(feed, net, contract, l2), "what this is and why"),
    ]
    if l2:
        artifacts.insert(2, Artifact("src/interfaces/ISequencerUptimeFeed.sol", _ISEQUENCER,
                                     "L2 sequencer uptime interface"))
        artifacts.insert(6, Artifact("test/mocks/MockSequencer.sol", _MOCK_SEQUENCER,
                                     "settable sequencer uptime feed"))

    return GeneratedProject(
        name=contract,
        framework=framework,
        network=net.key,
        pairs=[feed.pair],
        artifacts=artifacts,
        guards=tuple(guards),
    )


def generate_basket(
    pairs: Sequence[str],
    network: str = DEFAULT_NETWORK,
    contract_name: str = "BasketPriceConsumer",
) -> GeneratedConsumer:
    """One contract reading several feeds, each with **its own** heartbeat.

    The reason this is a separate generator rather than a loop: the obvious multi-feed
    contract shares one `MAX_AGE`, and on most chains that is wrong for at least one of
    the feeds. Here every feed carries the interval measured for it — on Base, ETH/USD
    is 1200s while USDC/USD is 86400s, a 72x difference inside one contract.
    """
    net = get_network(network)
    feeds = [get_feed(p, network) for p in pairs]
    if not feeds:
        raise ValueError("a basket needs at least one pair")

    l2 = is_l2(net.key) and net.key in SEQUENCER_FEEDS
    entries = "\n".join(
        f"        _register({index}, {f.address}, {f.stale_after_secs}); "
        f"// {f.pair}: {f.heartbeat_secs}s heartbeat"
        for index, f in enumerate(feeds)
    )
    names = "\n".join(f"    /// {index}: {f.pair}" for index, f in enumerate(feeds))

    sequencer_state = (
        _SEQUENCER_STATE.format(
            network_label=net.label,
            sequencer_address=SEQUENCER_FEEDS[net.key],
            grace=GRACE_PERIOD_SECS,
        )
        if l2 else ""
    )

    code = f"""// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

// Generated by alchem-link for {len(feeds)} feeds on {net.label}.
{_standalone_interfaces(l2)}
/// @notice Reads several feeds, each against the heartbeat measured for *that* feed.
///
/// @dev The naive version of this contract shares one MAX_AGE across every feed. On
///      {net.key} that is wrong: the heartbeats below span
///      {min(f.heartbeat_secs for f in feeds)}s to {max(f.heartbeat_secs for f in feeds)}s.
///      A single constant is either too tight for the slow feeds (false staleness) or
///      too loose for the fast ones (a dead feed goes unnoticed).
{names}
contract {contract_name} {{
    struct FeedConfig {{
        IAggregatorV3 aggregator;
        uint256 maxAge;
    }}

    mapping(uint256 => FeedConfig) public feeds;
    uint256 public immutable feedCount;
{sequencer_state}
    error UnknownFeed(uint256 id);
    error StalePrice(uint256 id, uint256 updatedAt, uint256 maxAge);
    error InvalidPrice(uint256 id, int256 answer);
    error IncompleteRound(uint256 id);
    error StaleRoundAnswer(uint256 id, uint80 answeredInRound, uint80 roundId);
{_SEQUENCER_ERRORS if l2 else ""}
    constructor() {{
{entries}
        feedCount = {len(feeds)};
    }}

    function _register(uint256 id, address aggregator, uint256 maxAge) private {{
        feeds[id] = FeedConfig(IAggregatorV3(aggregator), maxAge);
    }}

    function priceOf(uint256 id) public view returns (int256 price, uint8 decimals) {{
        FeedConfig memory config = feeds[id];
        if (address(config.aggregator) == address(0)) revert UnknownFeed(id);
{_SEQUENCER_CHECK if l2 else ""}
        (uint80 roundId, int256 answer, , uint256 updatedAt, uint80 answeredInRound) =
            config.aggregator.latestRoundData();

        if (updatedAt == 0) revert IncompleteRound(id);
        if (block.timestamp - updatedAt > config.maxAge) {{
            revert StalePrice(id, updatedAt, config.maxAge);
        }}
        if (answer <= 0) revert InvalidPrice(id, answer);
        if (answeredInRound < roundId) revert StaleRoundAnswer(id, answeredInRound, roundId);

        return (answer, config.aggregator.decimals());
    }}

    /// @notice Every price at once. Reverts if any single feed is unusable — a basket
    ///         valued from a partially stale set is worse than no value at all.
    function allPrices() external view returns (int256[] memory prices) {{
        prices = new int256[](feedCount);
        for (uint256 i = 0; i < feedCount; i++) {{
            (prices[i], ) = priceOf(i);
        }}
    }}
}}
"""

    guards = ["staleness (per feed)", "positive answer", "complete round", "carried-round"]
    if l2:
        guards.append("L2 sequencer uptime + grace period")

    return GeneratedConsumer(
        language="solidity",
        pair=", ".join(f.pair for f in feeds),
        network=net.key,
        address=", ".join(f.address for f in feeds),
        heartbeat_secs=max(f.heartbeat_secs for f in feeds),
        code=code,
        guards=tuple(guards),
    )


def basket_pairs(network: str = DEFAULT_NETWORK) -> List[str]:
    """Every pair on a network — the default basket."""
    return [f.pair for f in list_feeds(network)]
