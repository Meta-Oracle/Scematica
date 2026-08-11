// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

import { Test } from "forge-std/Test.sol";
import { AggregatorV3Interface, BotchainPriceFeed } from "../src/BotchainPriceFeed.sol";

/// A consumer written against Chainlink. If this compiles and works unmodified, the
/// adoption claim holds.
contract ChainlinkStyleConsumer {
    AggregatorV3Interface public immutable feed;

    constructor(AggregatorV3Interface f) {
        feed = f;
    }

    function price() external view returns (int256) {
        (, int256 answer,,,) = feed.latestRoundData();
        return answer;
    }
}

contract BotchainPriceFeedTest is Test {
    BotchainPriceFeed internal feed;

    address internal owner = address(0xA11CE);
    address[5] internal rep =
        [address(0xE1), address(0xE2), address(0xE3), address(0xE4), address(0xE5)];

    uint32 internal constant HEARTBEAT = 1 hours;

    function setUp() public {
        feed = new BotchainPriceFeed(owner, 8, "BOT / USD", HEARTBEAT, 3);
        vm.startPrank(owner);
        for (uint256 i = 0; i < rep.length; ++i) {
            feed.setReporter(rep[i], true);
        }
        vm.stopPrank();
    }

    function _submit(uint256 i, int256 p) internal {
        vm.prank(rep[i]);
        feed.submit(p);
    }

    // ── the median property ───────────────────────────────────────────────────

    function test_medianOfThreeIgnoresAnOutlier() public {
        // The reason this is a median and not an average: one corrupt reporter must not
        // move the price. An average of these three would be ~333,00 — wildly wrong.
        _submit(0, 100e8);
        _submit(1, 101e8);
        _submit(2, 1_000_000e8); // rogue reporter

        (, int256 answer,,,) = feed.latestRoundData();
        assertEq(answer, 101e8, "median must reject the outlier");
    }

    function test_medianOfFourAveragesTheMiddlePair() public {
        vm.prank(owner);
        feed.setQuorum(4);
        _submit(0, 100e8);
        _submit(1, 200e8);
        _submit(2, 300e8);
        _submit(3, 400e8);

        (, int256 answer,,,) = feed.latestRoundData();
        assertEq(answer, 250e8);
    }

    function test_medianIsIndependentOfSubmissionOrder() public {
        _submit(2, 300e8);
        _submit(0, 100e8);
        _submit(1, 200e8);
        (, int256 answer,,,) = feed.latestRoundData();
        assertEq(answer, 200e8);
    }

    function testFuzz_medianAlwaysLiesBetweenTheExtremes(int64 a, int64 b, int64 c) public {
        vm.assume(a > 0 && b > 0 && c > 0);
        _submit(0, a);
        _submit(1, b);
        _submit(2, c);
        (, int256 answer,,,) = feed.latestRoundData();

        int256 lo = a < b ? (a < c ? a : c) : (b < c ? b : c);
        int256 hi = a > b ? (a > c ? a : c) : (b > c ? b : c);
        assertGe(answer, lo);
        assertLe(answer, hi);
    }

    // ── rounds and quorum ─────────────────────────────────────────────────────

    function test_noRoundFinalisesBeforeQuorum() public {
        _submit(0, 100e8);
        _submit(1, 100e8);
        assertEq(feed.latestRound(), 0);
        assertEq(feed.pendingSubmissions(), 2);

        _submit(2, 100e8);
        assertEq(feed.latestRound(), 1);
        assertEq(feed.pendingSubmissions(), 0, "pending state must reset for the next round");
    }

    function test_aReporterCannotSubmitTwiceInARound() public {
        // Otherwise one reporter reaches quorum alone and the median is theirs.
        _submit(0, 100e8);
        vm.prank(rep[0]);
        vm.expectRevert(BotchainPriceFeed.AlreadySubmitted.selector);
        feed.submit(101e8);
    }

    function test_consecutiveRoundsAreIndependent() public {
        _submit(0, 100e8);
        _submit(1, 100e8);
        _submit(2, 100e8);
        _submit(0, 200e8);
        _submit(1, 200e8);
        _submit(2, 200e8);

        assertEq(feed.latestRound(), 2);
        (, int256 answer,,,) = feed.latestRoundData();
        assertEq(answer, 200e8);
    }

    function test_onlyReportersMaySubmit() public {
        vm.prank(address(0xBAD));
        vm.expectRevert(BotchainPriceFeed.NotReporter.selector);
        feed.submit(100e8);
    }

    function test_nonPositivePricesAreRejected() public {
        // A zero or negative print is a broken reporter, not a market. Admitting one
        // would let a single bad feed drag the median through zero.
        vm.prank(rep[0]);
        vm.expectRevert(abi.encodeWithSelector(BotchainPriceFeed.NonPositiveAnswer.selector, int256(0)));
        feed.submit(0);
    }

    // ── staleness ─────────────────────────────────────────────────────────────

    function test_maxAgeIsHeartbeatPlusTolerance() public view {
        // 15% over nominal, matching alchem-link's rule — real publish intervals overrun
        // slightly, and a feed that flickers stale every cycle gets ignored.
        assertEq(feed.maxAnswerAge(), (uint256(HEARTBEAT) * 11_500) / 10_000);
    }

    function test_checkedReadRevertsOnceStale() public {
        _submit(0, 100e8);
        _submit(1, 100e8);
        _submit(2, 100e8);

        (int256 fresh,) = feed.latestAnswerChecked();
        assertEq(fresh, 100e8);
        assertFalse(feed.isStale());

        vm.warp(block.timestamp + feed.maxAnswerAge() + 1);
        assertTrue(feed.isStale());
        vm.expectRevert();
        feed.latestAnswerChecked();
    }

    function test_standardReadStaysSpecCompliantWhenStale() public {
        // Deliberate: `latestRoundData` must keep returning data so existing Chainlink
        // integrations behave as they do everywhere else. Safety is opt-in via the
        // checked variant, not a surprise revert inside someone's liquidation path.
        _submit(0, 100e8);
        _submit(1, 100e8);
        _submit(2, 100e8);
        vm.warp(block.timestamp + 10 days);

        (, int256 answer,, uint256 updatedAt,) = feed.latestRoundData();
        assertEq(answer, 100e8);
        assertGt(updatedAt, 0);
    }

    function test_readingBeforeAnyRoundReverts() public {
        // Returning 0 would be worse: a consumer would price something at zero.
        vm.expectRevert(BotchainPriceFeed.NoData.selector);
        feed.latestAnswerChecked();
        assertTrue(feed.isStale(), "a feed with no data is stale, not fresh");
    }

    // ── Chainlink compatibility ───────────────────────────────────────────────

    function test_anUnmodifiedChainlinkConsumerWorks() public {
        ChainlinkStyleConsumer consumer = new ChainlinkStyleConsumer(feed);
        _submit(0, 42e8);
        _submit(1, 43e8);
        _submit(2, 44e8);
        assertEq(consumer.price(), 43e8);
    }

    function test_interfaceMetadataIsPopulated() public view {
        assertEq(feed.decimals(), 8);
        assertEq(feed.description(), "BOT / USD");
        assertEq(feed.version(), 1);
    }

    function test_historicalRoundsRemainReadable() public {
        _submit(0, 100e8);
        _submit(1, 100e8);
        _submit(2, 100e8);
        _submit(0, 200e8);
        _submit(1, 200e8);
        _submit(2, 200e8);

        (, int256 first,,,) = feed.getRoundData(1);
        (, int256 second,,,) = feed.getRoundData(2);
        assertEq(first, 100e8);
        assertEq(second, 200e8);
    }

    // ── administration ────────────────────────────────────────────────────────

    function test_quorumCannotExceedReporters() public {
        // A quorum above the reporter count deadlocks the feed and it silently goes
        // stale — an outage with no visible cause.
        vm.prank(owner);
        vm.expectRevert(BotchainPriceFeed.QuorumTooHigh.selector);
        feed.setQuorum(99);
    }

    function test_removingAReporterKeepsTheSetConsistent() public {
        vm.startPrank(owner);
        feed.setReporter(rep[0], false);
        assertEq(feed.reporterCount(), 4);
        assertFalse(feed.isReporter(rep[0]));
        vm.stopPrank();

        vm.prank(rep[0]);
        vm.expectRevert(BotchainPriceFeed.NotReporter.selector);
        feed.submit(100e8);
    }

    function test_onlyOwnerAdministers() public {
        vm.startPrank(address(0xBAD));
        vm.expectRevert(BotchainPriceFeed.NotOwner.selector);
        feed.setReporter(address(0x1), true);
        vm.expectRevert(BotchainPriceFeed.NotOwner.selector);
        feed.setHeartbeat(60);
        vm.stopPrank();
    }
}

/// Security regressions. Each of these was a real defect in the first version, not a
/// hypothetical — they are kept as tests so a refactor cannot quietly reintroduce them.
contract BotchainPriceFeedSecurityTest is Test {
    BotchainPriceFeed internal feed;
    address internal owner = address(0xA11CE);
    address[5] internal rep =
        [address(0xE1), address(0xE2), address(0xE3), address(0xE4), address(0xE5)];
    uint32 internal constant HEARTBEAT = 1 hours;

    function setUp() public {
        feed = new BotchainPriceFeed(owner, 8, "BOT / USD", HEARTBEAT, 3);
        vm.startPrank(owner);
        for (uint256 i = 0; i < rep.length; ++i) {
            feed.setReporter(rep[i], true);
        }
        vm.stopPrank();
    }

    function _submit(uint256 i, int256 p) internal {
        vm.prank(rep[i]);
        feed.submit(p);
    }

    // ── bug 1: a revoked reporter kept influence ──────────────────────────────

    function test_removingAReporterDropsItsPendingSubmission() public {
        _submit(0, 1e8); // an absurd lowball, still pending
        assertEq(feed.pendingSubmissions(), 1);

        vm.prank(owner);
        feed.setReporter(rep[0], false);
        assertEq(feed.pendingSubmissions(), 0, "revoked key must lose its pending price");

        // The round now forms from honest reporters only.
        _submit(1, 100e8);
        _submit(2, 100e8);
        _submit(3, 100e8);
        (, int256 answer,,,) = feed.latestRoundData();
        assertEq(answer, 100e8, "revoked lowball must not reach the median");
    }

    // ── bug 2: stale submissions blended into a fresh-looking round ───────────

    function test_expiredSubmissionsAreNotCountedTowardQuorum() public {
        _submit(0, 1e8);
        _submit(1, 1e8);
        // Both age out before a third arrives.
        vm.warp(block.timestamp + HEARTBEAT + 1);
        _submit(2, 100e8);

        assertEq(feed.latestRound(), 0, "expired submissions must not complete a round");
        assertEq(feed.pendingSubmissions(), 1);
    }

    function test_aRoundReportsTheOldestContributingSubmission() public {
        // startedAt must reflect the oldest input, or a round assembled over minutes
        // claims to be an instantaneous observation.
        _submit(0, 100e8);
        uint256 first = block.timestamp;
        vm.warp(block.timestamp + 60);
        _submit(1, 100e8);
        _submit(2, 100e8);

        (,, uint256 startedAt, uint256 updatedAt,) = feed.latestRoundData();
        assertEq(startedAt, first);
        assertGt(updatedAt, startedAt);
    }

    function test_anExpiredReporterMaySubmitAgain() public {
        _submit(0, 100e8);
        vm.warp(block.timestamp + HEARTBEAT + 1);
        // Not AlreadySubmitted: the old one is gone, so the key is not locked out.
        _submit(0, 200e8);
        assertEq(feed.pendingSubmissions(), 1);
    }

    // ── bug 3: lowering quorum left a round hanging ──────────────────────────

    function test_loweringQuorumFinalisesAnAlreadySatisfiedRound() public {
        _submit(0, 100e8);
        _submit(1, 102e8);
        assertEq(feed.latestRound(), 0);

        vm.prank(owner);
        feed.setQuorum(2);
        assertEq(feed.latestRound(), 1, "must not wait for an unrelated submission");
    }

    function test_removingReportersBelowQuorumRepairsQuorum() public {
        // Otherwise quorum exceeds the reporter count and the feed deadlocks into silent
        // staleness — an outage with no visible cause.
        vm.startPrank(owner);
        feed.setReporter(rep[4], false);
        feed.setReporter(rep[3], false);
        feed.setReporter(rep[2], false);
        vm.stopPrank();
        assertLe(feed.quorum(), feed.reporterCount());
    }

    // ── the manipulation bound ────────────────────────────────────────────────

    function testFuzz_aPivotalReporterCannotEscapeTheHonestRange(int64 attack) public {
        vm.assume(attack > 0);
        // Two honest reporters submit; the attacker sees both and submits last.
        _submit(0, 100e8);
        _submit(1, 110e8);
        _submit(2, attack);

        (, int256 answer,,,) = feed.latestRoundData();
        // The real guarantee: the attacker chooses *where within* the honest range the
        // median lands, and can never push it outside. Eliminating even this needs
        // off-chain threshold aggregation, not a contract taking one tx at a time.
        assertGe(answer, int256(100e8));
        assertLe(answer, int256(110e8));
    }

    // ── answer bounds ─────────────────────────────────────────────────────────

    function test_boundsRejectAFatFingeredDecimalError() public {
        vm.prank(owner);
        feed.setAnswerBounds(1e8, 1000e8);
        vm.prank(rep[0]);
        vm.expectRevert();
        feed.submit(100e18); // 10 decimal places too many
    }

    function test_boundsAreDisabledByDefault() public {
        // Deliberately off at deploy: a bound is a claim about the market, and a wrong one
        // is worse than none.
        assertEq(feed.minAnswer(), 0);
        assertEq(feed.maxAnswer(), 0);
        _submit(0, 1);
        assertEq(feed.pendingSubmissions(), 1);
    }

    function test_invertedBoundsAreRejected() public {
        vm.startPrank(owner);
        vm.expectRevert(BotchainPriceFeed.InvalidBounds.selector);
        feed.setAnswerBounds(1000e8, 1e8);
        vm.stopPrank();
    }

    // ── ownership ─────────────────────────────────────────────────────────────

    function test_ownershipTransferIsTwoStep() public {
        // A feed lending markets depend on must not be orphanable by one typo.
        vm.prank(owner);
        feed.transferOwnership(address(0xBEEF));
        assertEq(feed.owner(), owner);

        vm.prank(address(0xBEEF));
        feed.acceptOwnership();
        assertEq(feed.owner(), address(0xBEEF));
    }

    function test_onlyPendingOwnerMayAccept() public {
        vm.prank(owner);
        feed.transferOwnership(address(0xBEEF));
        vm.prank(address(0xBAD));
        vm.expectRevert(BotchainPriceFeed.NotOwner.selector);
        feed.acceptOwnership();
    }
}
