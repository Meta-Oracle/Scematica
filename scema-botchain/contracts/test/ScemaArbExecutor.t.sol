// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

import { Test } from "forge-std/Test.sol";
import { ScemaArbExecutor } from "../src/ScemaArbExecutor.sol";

/// Standard token: returns bool.
contract MockToken {
    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;

    function mint(address to, uint256 amount) external {
        balanceOf[to] += amount;
    }

    function approve(address spender, uint256 amount) external returns (bool) {
        allowance[msg.sender][spender] = amount;
        return true;
    }

    function transfer(address to, uint256 amount) external returns (bool) {
        balanceOf[msg.sender] -= amount;
        balanceOf[to] += amount;
        return true;
    }
}

/// Tether-shaped token: returns **nothing**. BOT Chain carries one of these.
contract NoReturnToken {
    mapping(address => uint256) public balanceOf;

    function mint(address to, uint256 amount) external {
        balanceOf[to] += amount;
    }

    function approve(address, uint256) external { }

    function transfer(address to, uint256 amount) external {
        balanceOf[msg.sender] -= amount;
        balanceOf[to] += amount;
    }
}

/// Stands in for a DEX: mints `gain` of `token` to the caller when poked.
contract MockVenue {
    MockToken public immutable token;

    constructor(MockToken t) {
        token = t;
    }

    function trade(uint256 gain) external {
        token.mint(msg.sender, gain);
    }

    function fail() external pure {
        revert("venue reverted");
    }
}

/// Tries to re-enter `execute` while being called from inside it.
contract Reenterer {
    ScemaArbExecutor public immutable exec;

    constructor(ScemaArbExecutor e) {
        exec = e;
    }

    function attack(address token) external {
        ScemaArbExecutor.Call[] memory calls = new ScemaArbExecutor.Call[](1);
        calls[0] = ScemaArbExecutor.Call({ target: address(this), data: abi.encodeWithSignature("attack(address)", token) });
        exec.execute(token, 0, calls);
    }
}

contract ScemaArbExecutorTest is Test {
    ScemaArbExecutor internal exec;
    MockToken internal token;
    MockVenue internal venue;

    address internal owner = address(0xA11CE);
    address internal bot = address(0xB07);
    address internal outsider = address(0xBAD);

    function setUp() public {
        token = new MockToken();
        venue = new MockVenue(token);

        vm.prank(owner);
        exec = new ScemaArbExecutor(owner);

        vm.startPrank(owner);
        exec.setExecutor(bot, true);
        exec.setTarget(address(venue), true);
        vm.stopPrank();

        token.mint(address(exec), 1_000);
    }

    function _tradeCalls(uint256 gain) internal view returns (ScemaArbExecutor.Call[] memory calls) {
        calls = new ScemaArbExecutor.Call[](1);
        calls[0] = ScemaArbExecutor.Call({
            target: address(venue),
            data: abi.encodeWithSelector(MockVenue.trade.selector, gain)
        });
    }

    // ── the core guarantee ────────────────────────────────────────────────────

    function test_profitableTradeSucceeds() public {
        vm.prank(bot);
        uint256 profit = exec.execute(address(token), 50, _tradeCalls(100));
        assertEq(profit, 100);
        assertEq(token.balanceOf(address(exec)), 1_100);
    }

    function test_unprofitableTradeReverts() public {
        // The whole point: a trade that does not clear minProfit costs gas, not capital.
        vm.prank(bot);
        vm.expectRevert(abi.encodeWithSelector(ScemaArbExecutor.NoProfit.selector, 1_000, 1_010, 50));
        exec.execute(address(token), 50, _tradeCalls(10));
        assertEq(token.balanceOf(address(exec)), 1_000, "balance must be untouched after revert");
    }

    function test_zeroGainRevertsWhenProfitRequired() public {
        vm.prank(bot);
        vm.expectRevert();
        exec.execute(address(token), 1, _tradeCalls(0));
    }

    function testFuzz_profitCheckIsExact(uint96 gain, uint96 minProfit) public {
        vm.prank(bot);
        if (uint256(gain) >= uint256(minProfit)) {
            uint256 profit = exec.execute(address(token), minProfit, _tradeCalls(gain));
            assertEq(profit, gain);
        } else {
            vm.expectRevert();
            exec.execute(address(token), minProfit, _tradeCalls(gain));
        }
    }

    // ── access control ────────────────────────────────────────────────────────

    function test_onlyExecutorMayTrade() public {
        vm.prank(outsider);
        vm.expectRevert(ScemaArbExecutor.NotExecutor.selector);
        exec.execute(address(token), 0, _tradeCalls(100));
    }

    function test_nonWhitelistedTargetIsRejected() public {
        // Without this the executor key could call any contract, including tokens this
        // contract has approved, and drain it.
        MockVenue rogue = new MockVenue(token);
        ScemaArbExecutor.Call[] memory calls = new ScemaArbExecutor.Call[](1);
        calls[0] = ScemaArbExecutor.Call({
            target: address(rogue),
            data: abi.encodeWithSelector(MockVenue.trade.selector, 100)
        });

        vm.prank(bot);
        vm.expectRevert(abi.encodeWithSelector(ScemaArbExecutor.TargetNotAllowed.selector, address(rogue)));
        exec.execute(address(token), 0, calls);
    }

    function test_executorCannotSweep() public {
        // A compromised hot key must not be able to move funds out.
        vm.prank(bot);
        vm.expectRevert(ScemaArbExecutor.NotOwner.selector);
        exec.sweep(address(token), bot, 1_000);
    }

    function test_executorCannotWhitelistOrApprove() public {
        vm.startPrank(bot);
        vm.expectRevert(ScemaArbExecutor.NotOwner.selector);
        exec.setTarget(address(0xdead), true);
        vm.expectRevert(ScemaArbExecutor.NotOwner.selector);
        exec.setApproval(address(token), address(venue), type(uint256).max);
        vm.stopPrank();
    }

    function test_approvalRequiresWhitelistedSpender() public {
        vm.prank(owner);
        vm.expectRevert(abi.encodeWithSelector(ScemaArbExecutor.TargetNotAllowed.selector, address(0xdead)));
        exec.setApproval(address(token), address(0xdead), 1);
    }

    // ── safety properties ─────────────────────────────────────────────────────

    function test_reentrancyIsBlocked() public {
        Reenterer attacker = new Reenterer(exec);
        vm.startPrank(owner);
        exec.setTarget(address(attacker), true);
        exec.setExecutor(address(attacker), true);
        vm.stopPrank();

        vm.expectRevert();
        attacker.attack(address(token));
    }

    function test_failingCallRevertsWholeTrade() public {
        ScemaArbExecutor.Call[] memory calls = new ScemaArbExecutor.Call[](2);
        calls[0] = ScemaArbExecutor.Call({
            target: address(venue),
            data: abi.encodeWithSelector(MockVenue.trade.selector, 500)
        });
        calls[1] = ScemaArbExecutor.Call({ target: address(venue), data: abi.encodeWithSelector(MockVenue.fail.selector) });

        vm.prank(bot);
        vm.expectRevert();
        exec.execute(address(token), 0, calls);
        assertEq(token.balanceOf(address(exec)), 1_000, "a mid-sequence failure must roll back the whole thing");
    }

    function test_emptyCallListReverts() public {
        vm.prank(bot);
        vm.expectRevert(ScemaArbExecutor.NoCalls.selector);
        exec.execute(address(token), 0, new ScemaArbExecutor.Call[](0));
    }

    /// Tokens that return nothing must not break approve/transfer. Tether does this, and
    /// BOT Chain has one; an `IERC20`-typed call would revert on the ABI decode.
    function test_handlesTokensThatReturnNoBool() public {
        NoReturnToken odd = new NoReturnToken();
        odd.mint(address(exec), 500);

        vm.startPrank(owner);
        exec.setApproval(address(odd), address(venue), 100);
        exec.sweep(address(odd), owner, 200);
        vm.stopPrank();

        assertEq(odd.balanceOf(owner), 200);
    }

    // ── ownership ─────────────────────────────────────────────────────────────

    function test_ownershipTransferIsTwoStep() public {
        vm.prank(owner);
        exec.transferOwnership(outsider);
        // Not yet — a typo'd address must not be able to orphan the contract.
        assertEq(exec.owner(), owner);

        vm.prank(outsider);
        exec.acceptOwnership();
        assertEq(exec.owner(), outsider);
    }

    function test_pendingOwnerOnlyMayAccept() public {
        vm.prank(owner);
        exec.transferOwnership(outsider);

        vm.prank(address(0xDEAD));
        vm.expectRevert(ScemaArbExecutor.NotOwner.selector);
        exec.acceptOwnership();
    }
}
