// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

/// @notice Minimal ERC-20 surface. Return values are deliberately not declared on
/// `approve`/`transfer` — see `_call` below for why.
interface IERC20 {
    function balanceOf(address account) external view returns (uint256);
}

/**
 * @title ScemaArbExecutor
 * @notice Atomic cross-DEX arbitrage with a profit-or-revert guarantee.
 *
 * The Solidity counterpart to `programs/scematica-swap` on Solana: the trade either ends
 * with more of the input token than it started with, or the whole transaction reverts and
 * costs only gas.
 *
 * # Why the calls are generic
 *
 * This does not hardcode a router ABI, and that is a deliberate response to what the
 * chain actually looks like. BOT Chain has two venues: a V3-style `SwapRouter` and
 * `CASwapRouter`, and the latter **reverts on `WETH()`** — so it is not a stock Uniswap-V2
 * router and its ABI cannot be assumed. A contract written against a guessed interface
 * deploys fine and then reverts on every real call, which is an expensive way to discover
 * a mismatch.
 *
 * Instead the caller supplies encoded `(target, data)` calls. The bot resolves the real
 * ABIs off-chain, where being wrong is free. What this contract enforces is the part that
 * must not be left to the caller: **the balance invariant**.
 *
 * # Threat model
 *
 * Generic calls are dangerous precisely because they are generic. Two constraints contain
 * that, and neither is optional:
 *
 * - **Targets are whitelisted by the owner.** Without this, an executor key could point a
 *   call at any contract — including the tokens this contract has approved — and drain it.
 *   The whitelist is what makes `data` untrusted-but-harmless.
 * - **No `delegatecall`, ever.** Every call is a plain `call`. A `delegatecall` to an
 *   attacker-chosen target rewrites this contract's own storage, which would hand over
 *   ownership in one transaction.
 *
 * Funds live in this contract rather than being pulled from an EOA per trade. A stolen
 * executor key can then only move value through whitelisted venues under the profit
 * check — it cannot transfer the balance out. Only the owner can sweep.
 */
contract ScemaArbExecutor {
    struct Call {
        address target;
        bytes data;
    }

    address public owner;
    address public pendingOwner;

    /// Keys permitted to run trades. Hot, and assumed to be compromisable.
    mapping(address => bool) public isExecutor;

    /// Contracts a trade may call. Cold, owner-only.
    mapping(address => bool) public isAllowedTarget;

    uint256 private _entered;

    event OwnershipTransferStarted(address indexed from, address indexed to);
    event OwnershipTransferred(address indexed from, address indexed to);
    event ExecutorSet(address indexed executor, bool allowed);
    event TargetSet(address indexed target, bool allowed);
    event ApprovalSet(address indexed token, address indexed spender, uint256 amount);
    event ArbExecuted(address indexed token, uint256 balanceBefore, uint256 profit, uint256 calls);
    event Swept(address indexed token, address indexed to, uint256 amount);

    error NotOwner();
    error NotExecutor();
    error Reentrancy();
    error TargetNotAllowed(address target);
    error CallFailed(uint256 index, bytes returndata);
    error NoProfit(uint256 balanceBefore, uint256 balanceAfter, uint256 minProfit);
    error ZeroAddress();
    error NoCalls();

    modifier onlyOwner() {
        if (msg.sender != owner) revert NotOwner();
        _;
    }

    modifier onlyExecutor() {
        if (!isExecutor[msg.sender]) revert NotExecutor();
        _;
    }

    modifier nonReentrant() {
        if (_entered == 1) revert Reentrancy();
        _entered = 1;
        _;
        _entered = 0;
    }

    constructor(address initialOwner) {
        if (initialOwner == address(0)) revert ZeroAddress();
        owner = initialOwner;
        emit OwnershipTransferred(address(0), initialOwner);
    }

    // ── trading ───────────────────────────────────────────────────────────────

    /**
     * @notice Run a sequence of calls and keep the result only if it made money.
     * @param token     The token whose balance must increase. Profit is denominated in it.
     * @param minProfit Minimum increase required, in `token` units.
     * @param calls     Whitelisted targets and pre-encoded calldata, executed in order.
     *
     * @dev The check is on **this contract's own balance**, measured before and after.
     * That is what makes it unforgeable by the calldata: a call sequence can lie about
     * anything it returns, but it cannot lie about the balance this contract holds. A
     * return-value-based check would be trusting the venue to report its own honesty.
     *
     * `minProfit` should cover gas, or a "profitable" trade still loses money overall.
     * Gas cannot be measured in `token` units on-chain, so the caller sets that floor.
     */
    function execute(address token, uint256 minProfit, Call[] calldata calls)
        external
        onlyExecutor
        nonReentrant
        returns (uint256 profit)
    {
        if (calls.length == 0) revert NoCalls();

        uint256 balanceBefore = IERC20(token).balanceOf(address(this));

        for (uint256 i = 0; i < calls.length; ++i) {
            address target = calls[i].target;
            if (!isAllowedTarget[target]) revert TargetNotAllowed(target);

            // Plain `call`. Never `delegatecall` — that would execute an arbitrary
            // target's code against this contract's storage.
            (bool ok, bytes memory ret) = target.call(calls[i].data);
            if (!ok) revert CallFailed(i, ret);
        }

        uint256 balanceAfter = IERC20(token).balanceOf(address(this));
        if (balanceAfter < balanceBefore + minProfit) {
            revert NoProfit(balanceBefore, balanceAfter, minProfit);
        }

        profit = balanceAfter - balanceBefore;
        emit ArbExecuted(token, balanceBefore, profit, calls.length);
    }

    // ── administration ────────────────────────────────────────────────────────

    function setExecutor(address executor, bool allowed) external onlyOwner {
        if (executor == address(0)) revert ZeroAddress();
        isExecutor[executor] = allowed;
        emit ExecutorSet(executor, allowed);
    }

    function setTarget(address target, bool allowed) external onlyOwner {
        if (target == address(0)) revert ZeroAddress();
        isAllowedTarget[target] = allowed;
        emit TargetSet(target, allowed);
    }

    /**
     * @notice Approve a venue to spend a token held here.
     * @dev Owner-only and explicit. Approvals are the standing risk in this design — an
     * approval outlives the trade that needed it — so granting one is a cold-key decision,
     * never something an executor can do for itself.
     */
    function setApproval(address token, address spender, uint256 amount) external onlyOwner {
        if (!isAllowedTarget[spender]) revert TargetNotAllowed(spender);
        _call(token, abi.encodeWithSelector(0x095ea7b3, spender, amount)); // approve(address,uint256)
        emit ApprovalSet(token, spender, amount);
    }

    /// @notice Withdraw tokens. Owner only — an executor key cannot move funds out.
    function sweep(address token, address to, uint256 amount) external onlyOwner {
        if (to == address(0)) revert ZeroAddress();
        _call(token, abi.encodeWithSelector(0xa9059cbb, to, amount)); // transfer(address,uint256)
        emit Swept(token, to, amount);
    }

    /// @notice Withdraw native BOT.
    function sweepNative(address payable to, uint256 amount) external onlyOwner {
        if (to == address(0)) revert ZeroAddress();
        (bool ok,) = to.call{ value: amount }("");
        if (!ok) revert CallFailed(0, "");
        emit Swept(address(0), to, amount);
    }

    /// @dev Two-step, so a typo in an address cannot orphan the contract permanently.
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

    receive() external payable { }

    /**
     * @dev ERC-20 calls that tolerate a missing return value.
     *
     * Several widely-deployed tokens — Tether being the canonical case, and BOT Chain has
     * a "Tether USD" — return nothing from `approve`/`transfer` instead of a bool. Calling
     * them through a `IERC20` interface that declares `returns (bool)` reverts on the ABI
     * decode even though the transfer succeeded. Accepting empty returndata, and requiring
     * `true` only when data is present, handles both shapes.
     */
    function _call(address token, bytes memory data) private {
        (bool ok, bytes memory ret) = token.call(data);
        if (!ok || (ret.length != 0 && !abi.decode(ret, (bool)))) {
            revert CallFailed(0, ret);
        }
    }
}
