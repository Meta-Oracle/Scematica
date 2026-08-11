// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

interface IERC20Minimal {
    function balanceOf(address account) external view returns (uint256);
}

/**
 * @title ScemaBondEscrow
 * @notice Conviction bonds for the ScemaDEX rail, settled on BOT Chain.
 *
 * An agent stakes a bond behind a claim. If nobody disputes it within the window, the
 * agent takes it back. If an arbiter rules against it inside the window, the bond is
 * slashed to the beneficiary. This is the EVM counterpart to the dispute-window state
 * machine in `docs/settlement-v2-design.md`.
 *
 * # Design constraints that are load-bearing
 *
 * **The window is checked against `block.timestamp`, and validators can nudge that.**
 * On a 0.67s chain a few seconds of drift is many blocks, so the window must be long
 * enough that timestamp manipulation cannot meaningfully shorten it — `MIN_DISPUTE_WINDOW`
 * exists for that reason, not as a UX preference.
 *
 * **Bond amounts are recorded from the measured balance delta**, not from the caller's
 * claimed `amount`. A fee-on-transfer token delivers less than it says, and trusting the
 * argument would let someone post a bond of 100 that is really worth 90 — and later
 * withdraw 100, taking the difference from another bond in the same contract.
 *
 * **Slashing pays a beneficiary fixed at post time.** An arbiter who could also choose
 * the recipient could slash to themselves, which turns a dispute role into a licence to
 * take any bond in the contract.
 */
contract ScemaBondEscrow {
    enum State {
        None,
        Active,
        Released,
        Slashed
    }

    struct Bond {
        address poster;
        address token;
        /// Actual amount received, which is not necessarily what the poster passed in.
        uint256 amount;
        /// Who receives the bond if it is slashed. Fixed at post time.
        address beneficiary;
        uint64 disputeDeadline;
        State state;
    }

    /// @dev Short windows are not meaningfully enforceable against timestamp drift.
    uint64 public constant MIN_DISPUTE_WINDOW = 5 minutes;
    /// @dev A ceiling so a bond cannot be locked for a practically infinite period.
    uint64 public constant MAX_DISPUTE_WINDOW = 30 days;

    address public owner;
    /// Addresses permitted to slash an active bond.
    mapping(address => bool) public isArbiter;
    mapping(bytes32 => Bond) public bonds;

    event BondPosted(
        bytes32 indexed id,
        address indexed poster,
        address indexed token,
        uint256 amount,
        address beneficiary,
        uint64 disputeDeadline
    );
    event BondReleased(bytes32 indexed id, address indexed to, uint256 amount);
    event BondSlashed(bytes32 indexed id, address indexed beneficiary, uint256 amount, string reason);
    event ArbiterSet(address indexed arbiter, bool allowed);
    event OwnershipTransferred(address indexed from, address indexed to);

    error NotOwner();
    error NotArbiter();
    error NotPoster();
    error BondExists();
    error BondNotActive();
    error WindowTooShort();
    error WindowTooLong();
    error StillDisputable(uint64 deadline);
    error WindowClosed(uint64 deadline);
    error NothingReceived();
    error ZeroAddress();
    error TransferFailed();

    modifier onlyOwner() {
        if (msg.sender != owner) revert NotOwner();
        _;
    }

    constructor(address initialOwner) {
        if (initialOwner == address(0)) revert ZeroAddress();
        owner = initialOwner;
        emit OwnershipTransferred(address(0), initialOwner);
    }

    /**
     * @notice Stake a bond. The caller must have approved `amount` of `token` first.
     * @dev The recorded amount is `balanceAfter - balanceBefore`, so a fee-on-transfer
     * token is accounted at what actually arrived. Recording the requested figure instead
     * would let a bond over-report itself and be released against another bond's funds.
     */
    function postBond(
        bytes32 id,
        address token,
        uint256 amount,
        address beneficiary,
        uint64 disputeWindow
    ) external {
        if (bonds[id].state != State.None) revert BondExists();
        if (beneficiary == address(0) || token == address(0)) revert ZeroAddress();
        if (disputeWindow < MIN_DISPUTE_WINDOW) revert WindowTooShort();
        if (disputeWindow > MAX_DISPUTE_WINDOW) revert WindowTooLong();

        uint256 before = IERC20Minimal(token).balanceOf(address(this));
        // transferFrom(address,address,uint256)
        _erc20Call(token, abi.encodeWithSelector(0x23b872dd, msg.sender, address(this), amount));
        uint256 received = IERC20Minimal(token).balanceOf(address(this)) - before;
        if (received == 0) revert NothingReceived();

        uint64 deadline = uint64(block.timestamp) + disputeWindow;
        bonds[id] = Bond({
            poster: msg.sender,
            token: token,
            amount: received,
            beneficiary: beneficiary,
            disputeDeadline: deadline,
            state: State.Active
        });

        emit BondPosted(id, msg.sender, token, received, beneficiary, deadline);
    }

    /// @notice Reclaim a bond once its dispute window has passed undisputed.
    function release(bytes32 id) external {
        Bond storage b = bonds[id];
        if (b.state != State.Active) revert BondNotActive();
        if (msg.sender != b.poster) revert NotPoster();
        // Validator drift is real and accounted for: MIN_DISPUTE_WINDOW (5 minutes) is
        // orders of magnitude larger than the seconds a proposer can shift, so nudging
        // the timestamp cannot open a window that should still be closed.
        // forge-lint: disable-next-line(block-timestamp)
        if (block.timestamp <= b.disputeDeadline) revert StillDisputable(b.disputeDeadline);

        // State first, transfer second. The token is arbitrary and its `transfer` can
        // re-enter; marking Released up front makes a second withdrawal impossible
        // regardless of what the token does.
        b.state = State.Released;
        uint256 amount = b.amount;
        _erc20Call(b.token, abi.encodeWithSelector(0xa9059cbb, b.poster, amount));
        emit BondReleased(id, b.poster, amount);
    }

    /**
     * @notice Slash an active bond to its beneficiary.
     * @dev Only inside the window. An arbiter who could slash afterwards would make the
     * deadline meaningless and every released bond retroactively unsafe.
     */
    function slash(bytes32 id, string calldata reason) external {
        if (!isArbiter[msg.sender]) revert NotArbiter();
        Bond storage b = bonds[id];
        if (b.state != State.Active) revert BondNotActive();
        // Same reasoning as `release`, and erring here is the safe direction: drift can
        // only make a slash *fail*, never succeed against a bond already released.
        // forge-lint: disable-next-line(block-timestamp)
        if (block.timestamp > b.disputeDeadline) revert WindowClosed(b.disputeDeadline);

        b.state = State.Slashed;
        uint256 amount = b.amount;
        address beneficiary = b.beneficiary;
        _erc20Call(b.token, abi.encodeWithSelector(0xa9059cbb, beneficiary, amount));
        emit BondSlashed(id, beneficiary, amount, reason);
    }

    function setArbiter(address arbiter, bool allowed) external onlyOwner {
        if (arbiter == address(0)) revert ZeroAddress();
        isArbiter[arbiter] = allowed;
        emit ArbiterSet(arbiter, allowed);
    }

    function transferOwnership(address newOwner) external onlyOwner {
        if (newOwner == address(0)) revert ZeroAddress();
        address previous = owner;
        owner = newOwner;
        emit OwnershipTransferred(previous, newOwner);
    }

    function getBond(bytes32 id) external view returns (Bond memory) {
        return bonds[id];
    }

    /// @dev Tolerates tokens that return nothing from transfer/transferFrom. See the
    /// same note in ScemaArbExecutor — BOT Chain carries a "Tether USD".
    function _erc20Call(address token, bytes memory data) private {
        (bool ok, bytes memory ret) = token.call(data);
        if (!ok || (ret.length != 0 && !abi.decode(ret, (bool)))) revert TransferFailed();
    }
}
