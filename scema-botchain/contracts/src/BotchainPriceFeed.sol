// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

/// @notice The interface every EVM DeFi contract already speaks. Implementing it exactly
/// is the whole adoption strategy: an integrator points at this address and their existing
/// Chainlink-consuming code works unmodified.
interface AggregatorV3Interface {
    function decimals() external view returns (uint8);
    function description() external view returns (string memory);
    function version() external view returns (uint256);
    function getRoundData(uint80 _roundId)
        external
        view
        returns (uint80 roundId, int256 answer, uint256 startedAt, uint256 updatedAt, uint80 answeredInRound);
    function latestRoundData()
        external
        view
        returns (uint80 roundId, int256 answer, uint256 startedAt, uint256 updatedAt, uint80 answeredInRound);
}

/**
 * @title BotchainPriceFeed
 * @notice A median-aggregated price feed for BOT Chain, Chainlink-interface compatible.
 *
 * BOT Chain has no oracle, and without one there is no lending, no perps, no stablecoin,
 * no liquidations — every financial primitive is downstream of "what is this worth".
 *
 * # Security model
 *
 * **Median of a reporter set, never a single signer.** One reporter is one key away from an
 * arbitrary price, and an arbitrary price on a lending market is a drained pool.
 *
 * **A pivotal reporter is bounded, not eliminated.** Submissions are public in the mempool,
 * so whoever submits last sees the others and can choose where the median lands. With an
 * honest majority that choice is confined to the range of honest submissions — an attacker
 * can pick *which* honest price wins, never invent one outside them. That bound is the real
 * guarantee here and is asserted in the tests. Removing it entirely needs off-chain
 * aggregation with signature threshold (Chainlink OCR's design); it is not achievable by a
 * contract that accepts submissions one transaction at a time.
 *
 * **Submissions expire.** A price sitting in a half-assembled round is not evidence about
 * the present. Without expiry, two reporters submitting now and a third submitting hours
 * later would produce a median blending fresh and stale data, and the resulting round would
 * be stamped with the *late* timestamp — so it would look fresh while being anything but.
 *
 * **Answer bounds are opt-in and meant to be wide.** They exist to catch a fat-finger or a
 * decimals mistake, not to express a market view. Bounds tight enough to bind during a real
 * crash are how the Venus/LUNA incident happened: the feed pinned at its floor while the
 * asset kept falling, and the protocol lent against a price that no longer existed. Rejecting
 * an out-of-bounds *submission* (rather than clamping the answer) means a genuine excursion
 * stalls the feed into visible staleness instead of silently reporting a fiction.
 *
 * # What this does not solve
 *
 * A price is only as honest as its reporters. On a chain with ~2 swaps per 50 transactions a
 * DEX-derived price is cheap to move, so reporters should source externally and use on-chain
 * prices as a sanity check, never as the source.
 */
contract BotchainPriceFeed is AggregatorV3Interface {
    struct Round {
        int256 answer;
        uint64 startedAt;
        uint64 updatedAt;
        uint16 submissionCount;
    }

    struct Submission {
        int256 price;
        uint64 at;
    }

    /// @dev Tolerance over the nominal heartbeat before a feed is called stale. 1500 = 15%.
    uint16 public constant STALENESS_TOLERANCE_BPS = 1500;
    /// @dev Bounded so the median sort and the purge loop cannot become gas problems.
    uint8 public constant MAX_REPORTERS = 31;

    uint8 public immutable override decimals;
    string public override description;
    uint256 public constant override version = 1;

    /// Nominal seconds between updates. Measured per feed, not a shared default.
    uint32 public heartbeat;
    /// How long a submission may wait for quorum before it is discarded as stale.
    uint32 public maxSubmissionAge;
    /// Submissions required before a round finalises.
    uint8 public quorum;

    /// Sanity bounds on a submission. Both zero disables the check.
    int256 public minAnswer;
    int256 public maxAnswer;

    address public owner;
    address public pendingOwner;
    address[] public reporters;
    mapping(address => bool) public isReporter;

    uint80 public latestRound;
    mapping(uint80 => Round) private _rounds;

    mapping(address => Submission) private _pending;
    mapping(address => bool) private _hasPending;
    address[] private _submitters;

    event ReporterSet(address indexed reporter, bool allowed);
    event QuorumSet(uint8 quorum);
    event HeartbeatSet(uint32 heartbeat);
    event MaxSubmissionAgeSet(uint32 maxSubmissionAge);
    event AnswerBoundsSet(int256 minAnswer, int256 maxAnswer);
    event Submitted(address indexed reporter, int256 price, uint80 indexed round);
    event SubmissionExpired(address indexed reporter, uint64 submittedAt);
    event AnswerUpdated(int256 indexed answer, uint80 indexed roundId, uint256 updatedAt, uint16 submissions);
    event OwnershipTransferStarted(address indexed from, address indexed to);
    event OwnershipTransferred(address indexed from, address indexed to);

    error NotOwner();
    error NotReporter();
    error AlreadySubmitted();
    error QuorumTooHigh();
    error QuorumZero();
    error TooManyReporters();
    error NoData();
    error StaleAnswer(uint256 updatedAt, uint256 maxAge);
    error NonPositiveAnswer(int256 answer);
    error OutOfBounds(int256 answer, int256 min, int256 max);
    error InvalidBounds();
    error ZeroAddress();
    error ZeroAge();

    modifier onlyOwner() {
        if (msg.sender != owner) revert NotOwner();
        _;
    }

    constructor(
        address initialOwner,
        uint8 feedDecimals,
        string memory feedDescription,
        uint32 feedHeartbeat,
        uint8 initialQuorum
    ) {
        if (initialOwner == address(0)) revert ZeroAddress();
        if (initialQuorum == 0) revert QuorumZero();
        owner = initialOwner;
        decimals = feedDecimals;
        description = feedDescription;
        heartbeat = feedHeartbeat;
        quorum = initialQuorum;
        // Defaults to the heartbeat: a submission that has waited longer than the feed's
        // whole update interval is describing a different market than the one being priced.
        maxSubmissionAge = feedHeartbeat;
        emit OwnershipTransferred(address(0), initialOwner);
    }

    // ── reporting ─────────────────────────────────────────────────────────────

    /**
     * @notice Submit a price for the round currently being assembled.
     * @dev Expired submissions are purged first, so quorum is always counted over
     * submissions that are simultaneously live.
     */
    function submit(int256 price) external {
        if (!isReporter[msg.sender]) revert NotReporter();
        if (price <= 0) revert NonPositiveAnswer(price);
        if ((minAnswer != 0 || maxAnswer != 0) && (price < minAnswer || price > maxAnswer)) {
            revert OutOfBounds(price, minAnswer, maxAnswer);
        }

        _purgeExpired();
        if (_hasPending[msg.sender]) revert AlreadySubmitted();

        _pending[msg.sender] = Submission({ price: price, at: uint64(block.timestamp) });
        _hasPending[msg.sender] = true;
        _submitters.push(msg.sender);

        emit Submitted(msg.sender, price, latestRound + 1);

        if (_submitters.length >= quorum) {
            _finalize();
        }
    }

    /**
     * @dev Drop submissions older than `maxSubmissionAge`.
     *
     * Without this a round could blend a price from hours ago with one from this block and
     * stamp the result with the *current* time — a stale answer wearing a fresh timestamp,
     * which defeats every staleness check downstream. Iterating backwards makes the
     * swap-and-pop removal safe, since a swapped-in element lands at an already-visited index.
     */
    function _purgeExpired() private {
        uint256 i = _submitters.length;
        while (i > 0) {
            --i;
            address who = _submitters[i];
            // forge-lint: disable-next-line(block-timestamp)
            if (block.timestamp > _pending[who].at + maxSubmissionAge) {
                emit SubmissionExpired(who, _pending[who].at);
                _clearPending(who, i);
            }
        }
    }

    function _clearPending(address who, uint256 index) private {
        delete _hasPending[who];
        delete _pending[who];
        uint256 last = _submitters.length - 1;
        if (index != last) {
            _submitters[index] = _submitters[last];
        }
        _submitters.pop();
    }

    function _finalize() private {
        uint256 n = _submitters.length;
        int256[] memory values = new int256[](n);
        uint64 oldest = type(uint64).max;
        for (uint256 i = 0; i < n; ++i) {
            Submission storage s = _pending[_submitters[i]];
            values[i] = s.price;
            if (s.at < oldest) oldest = s.at;
        }

        // Insertion sort. n is capped at MAX_REPORTERS, so this is cheap and — unlike a
        // clever algorithm — obviously correct, which matters for something pricing debt.
        for (uint256 i = 1; i < n; ++i) {
            int256 key = values[i];
            uint256 j = i;
            while (j > 0 && values[j - 1] > key) {
                values[j] = values[j - 1];
                --j;
            }
            values[j] = key;
        }

        int256 median;
        if (n % 2 == 1) {
            median = values[n / 2];
        } else {
            // Halve each before summing so two large prices cannot overflow en route to
            // their own average.
            median = values[n / 2 - 1] / 2 + values[n / 2] / 2;
        }

        uint80 roundId = ++latestRound;
        _rounds[roundId] = Round({
            answer: median,
            // `startedAt` is the *oldest* contributing submission, not the finalising
            // block. Reporting the latter would let a round assembled over several minutes
            // claim to be an instantaneous observation.
            startedAt: oldest,
            updatedAt: uint64(block.timestamp),
            // `n` is `_submitters.length`, bounded by `reporters.length <= MAX_REPORTERS`
            // (31), so this cannot truncate. The bound is enforced in `setReporter`.
            // forge-lint: disable-next-line(unsafe-typecast)
            submissionCount: uint16(n)
        });

        for (uint256 i = 0; i < n; ++i) {
            delete _hasPending[_submitters[i]];
            delete _pending[_submitters[i]];
        }
        delete _submitters;

        // Same bound as above: n <= MAX_REPORTERS.
        // forge-lint: disable-next-line(unsafe-typecast)
        emit AnswerUpdated(median, roundId, block.timestamp, uint16(n));
    }

    // ── reads ─────────────────────────────────────────────────────────────────

    /// @inheritdoc AggregatorV3Interface
    /// @dev Spec-compliant: returns the last round whether or not it is stale, because
    /// existing integrations depend on that shape. Use `latestAnswerChecked` for safety.
    function latestRoundData()
        external
        view
        override
        returns (uint80 roundId, int256 answer, uint256 startedAt, uint256 updatedAt, uint80 answeredInRound)
    {
        return getRoundData(latestRound);
    }

    /// @inheritdoc AggregatorV3Interface
    function getRoundData(uint80 _roundId)
        public
        view
        override
        returns (uint80 roundId, int256 answer, uint256 startedAt, uint256 updatedAt, uint80 answeredInRound)
    {
        Round storage r = _rounds[_roundId];
        if (r.updatedAt == 0) revert NoData();
        return (_roundId, r.answer, r.startedAt, r.updatedAt, _roundId);
    }

    /**
     * @notice The price, or a revert if it is too old to use.
     * @dev Chainlink's interface hands back `updatedAt` and trusts the caller to check it;
     * forgetting is the most common oracle failure in production, and it fails *silently* —
     * the consumer prices a liquidation off yesterday's number. Here the check is not optional.
     */
    function latestAnswerChecked() external view returns (int256 answer, uint256 updatedAt) {
        Round storage r = _rounds[latestRound];
        if (r.updatedAt == 0) revert NoData();

        uint256 maxAge = maxAnswerAge();
        // Drift is seconds; maxAge is at minimum a heartbeat plus 15%. Not exploitable.
        // forge-lint: disable-next-line(block-timestamp)
        if (block.timestamp > r.updatedAt + maxAge) revert StaleAnswer(r.updatedAt, maxAge);
        return (r.answer, r.updatedAt);
    }

    /// @notice Heartbeat plus tolerance. Beyond this the feed is stale.
    function maxAnswerAge() public view returns (uint256) {
        return (uint256(heartbeat) * (10_000 + STALENESS_TOLERANCE_BPS)) / 10_000;
    }

    /// @notice Non-reverting staleness check, for UIs that want to show a warning.
    function isStale() external view returns (bool) {
        Round storage r = _rounds[latestRound];
        if (r.updatedAt == 0) return true;
        // forge-lint: disable-next-line(block-timestamp)
        return block.timestamp > r.updatedAt + maxAnswerAge();
    }

    function reporterCount() external view returns (uint256) {
        return reporters.length;
    }

    /// @notice Submissions currently waiting for quorum, **including** any now expired.
    /// Expiry is applied on the next `submit`; this is the raw count.
    function pendingSubmissions() external view returns (uint256) {
        return _submitters.length;
    }

    // ── administration ────────────────────────────────────────────────────────

    /**
     * @notice Add or remove a reporter.
     * @dev Removing one also drops any submission it has pending. Leaving it would let a
     * revoked reporter's price still count toward a median — the removal would look
     * effective while the key retained influence over the next round.
     */
    function setReporter(address reporter, bool allowed) external onlyOwner {
        if (reporter == address(0)) revert ZeroAddress();

        if (allowed && !isReporter[reporter]) {
            if (reporters.length >= MAX_REPORTERS) revert TooManyReporters();
            reporters.push(reporter);
        } else if (!allowed && isReporter[reporter]) {
            uint256 n = reporters.length;
            for (uint256 i = 0; i < n; ++i) {
                if (reporters[i] == reporter) {
                    reporters[i] = reporters[n - 1];
                    reporters.pop();
                    break;
                }
            }
            if (_hasPending[reporter]) {
                uint256 m = _submitters.length;
                for (uint256 i = 0; i < m; ++i) {
                    if (_submitters[i] == reporter) {
                        _clearPending(reporter, i);
                        break;
                    }
                }
            }
            // Quorum must not be left above the reporter count, or the feed deadlocks and
            // goes silently stale — an outage with no visible cause.
            if (quorum > reporters.length && reporters.length > 0) {
                quorum = uint8(reporters.length);
                emit QuorumSet(quorum);
            }
        }
        isReporter[reporter] = allowed;
        emit ReporterSet(reporter, allowed);
    }

    /// @dev Finalises immediately if the new quorum is already satisfied, so lowering it
    /// cannot leave a round hanging until some unrelated reporter happens to submit.
    function setQuorum(uint8 newQuorum) external onlyOwner {
        if (newQuorum == 0) revert QuorumZero();
        if (newQuorum > reporters.length) revert QuorumTooHigh();
        quorum = newQuorum;
        emit QuorumSet(newQuorum);
        if (_submitters.length >= newQuorum) {
            _finalize();
        }
    }

    function setHeartbeat(uint32 newHeartbeat) external onlyOwner {
        heartbeat = newHeartbeat;
        emit HeartbeatSet(newHeartbeat);
    }

    function setMaxSubmissionAge(uint32 newAge) external onlyOwner {
        if (newAge == 0) revert ZeroAge();
        maxSubmissionAge = newAge;
        emit MaxSubmissionAgeSet(newAge);
    }

    /**
     * @notice Sanity bounds for submissions. Both zero disables the check.
     * @dev Set these **wide**. They are for catching a decimals mistake, not for expressing
     * a view on the market — bounds tight enough to bind during a real crash reproduce the
     * Venus/LUNA failure, where the feed pinned at its floor while the asset kept falling.
     */
    function setAnswerBounds(int256 newMin, int256 newMax) external onlyOwner {
        if (newMin != 0 || newMax != 0) {
            if (newMin <= 0 || newMax <= newMin) revert InvalidBounds();
        }
        minAnswer = newMin;
        maxAnswer = newMax;
        emit AnswerBoundsSet(newMin, newMax);
    }

    /// @dev Two-step, matching ScemaArbExecutor: a typo'd address cannot orphan a feed that
    /// lending markets depend on.
    function transferOwnership(address newOwner) external onlyOwner {
        if (newOwner == address(0)) revert ZeroAddress();
        pendingOwner = newOwner;
        emit OwnershipTransferStarted(owner, newOwner);
    }

    function acceptOwnership() external {
        if (msg.sender != pendingOwner) revert NotOwner();
        address previous = owner;
        owner = pendingOwner;
        pendingOwner = address(0);
        emit OwnershipTransferred(previous, owner);
    }
}
