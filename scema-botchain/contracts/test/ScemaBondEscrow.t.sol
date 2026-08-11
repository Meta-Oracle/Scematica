// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

import { Test } from "forge-std/Test.sol";
import { ScemaBondEscrow } from "../src/ScemaBondEscrow.sol";

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

    function transferFrom(address from, address to, uint256 amount) external virtual returns (bool) {
        allowance[from][msg.sender] -= amount;
        balanceOf[from] -= amount;
        balanceOf[to] += amount;
        return true;
    }
}

/// Delivers 1% less than requested. Accounting the requested figure would over-credit.
contract FeeToken is MockToken {
    function transferFrom(address from, address to, uint256 amount) external override returns (bool) {
        uint256 fee = amount / 100;
        allowance[from][msg.sender] -= amount;
        balanceOf[from] -= amount;
        balanceOf[to] += amount - fee;
        return true;
    }
}

contract ScemaBondEscrowTest is Test {
    ScemaBondEscrow internal escrow;
    MockToken internal token;

    address internal owner = address(0xA11CE);
    address internal agent = address(0xA6E7);
    address internal arbiter = address(0xA781);
    address internal beneficiary = address(0xBE7E);

    bytes32 internal constant ID = keccak256("bond-1");
    uint64 internal constant WINDOW = 1 hours;

    function setUp() public {
        token = new MockToken();
        escrow = new ScemaBondEscrow(owner);

        vm.prank(owner);
        escrow.setArbiter(arbiter, true);

        token.mint(agent, 10_000);
        vm.prank(agent);
        token.approve(address(escrow), type(uint256).max);
    }

    function _post() internal {
        vm.prank(agent);
        escrow.postBond(ID, address(token), 1_000, beneficiary, WINDOW);
    }

    function test_postAndReleaseAfterWindow() public {
        _post();
        assertEq(token.balanceOf(address(escrow)), 1_000);

        vm.warp(block.timestamp + WINDOW + 1);
        vm.prank(agent);
        escrow.release(ID);

        assertEq(token.balanceOf(agent), 10_000);
        assertEq(uint8(escrow.getBond(ID).state), uint8(ScemaBondEscrow.State.Released));
    }

    function test_cannotReleaseDuringWindow() public {
        _post();
        vm.warp(block.timestamp + WINDOW - 1);
        vm.prank(agent);
        vm.expectRevert();
        escrow.release(ID);
    }

    function test_arbiterSlashesToBeneficiary() public {
        _post();
        vm.prank(arbiter);
        escrow.slash(ID, "bad inference");

        assertEq(token.balanceOf(beneficiary), 1_000);
        assertEq(uint8(escrow.getBond(ID).state), uint8(ScemaBondEscrow.State.Slashed));
    }

    function test_slashIsRefusedAfterTheWindow() public {
        // If an arbiter could slash after the deadline, the deadline would mean nothing
        // and every released bond would be retroactively unsafe.
        _post();
        vm.warp(block.timestamp + WINDOW + 1);
        vm.prank(arbiter);
        vm.expectRevert();
        escrow.slash(ID, "too late");
    }

    function test_onlyArbiterMaySlash() public {
        _post();
        vm.prank(agent);
        vm.expectRevert(ScemaBondEscrow.NotArbiter.selector);
        escrow.slash(ID, "self-serving");
    }

    function test_onlyPosterMayRelease() public {
        _post();
        vm.warp(block.timestamp + WINDOW + 1);
        vm.prank(address(0xBAD));
        vm.expectRevert(ScemaBondEscrow.NotPoster.selector);
        escrow.release(ID);
    }

    function test_cannotReleaseTwice() public {
        _post();
        vm.warp(block.timestamp + WINDOW + 1);
        vm.startPrank(agent);
        escrow.release(ID);
        vm.expectRevert(ScemaBondEscrow.BondNotActive.selector);
        escrow.release(ID);
        vm.stopPrank();
    }

    function test_cannotSlashAReleasedBond() public {
        _post();
        vm.warp(block.timestamp + WINDOW + 1);
        vm.prank(agent);
        escrow.release(ID);

        vm.prank(arbiter);
        vm.expectRevert(ScemaBondEscrow.BondNotActive.selector);
        escrow.slash(ID, "after the fact");
    }

    function test_idCannotBeReused() public {
        _post();
        vm.prank(agent);
        vm.expectRevert(ScemaBondEscrow.BondExists.selector);
        escrow.postBond(ID, address(token), 500, beneficiary, WINDOW);
    }

    function test_windowBoundsAreEnforced() public {
        vm.startPrank(agent);
        vm.expectRevert(ScemaBondEscrow.WindowTooShort.selector);
        escrow.postBond(keccak256("a"), address(token), 100, beneficiary, 1 seconds);
        vm.expectRevert(ScemaBondEscrow.WindowTooLong.selector);
        escrow.postBond(keccak256("b"), address(token), 100, beneficiary, 60 days);
        vm.stopPrank();
    }

    /// A fee-on-transfer token delivers less than requested. Recording the requested
    /// figure would let this bond be released against another bond's funds.
    function test_feeOnTransferIsAccountedAtWhatArrived() public {
        FeeToken fee = new FeeToken();
        fee.mint(agent, 10_000);
        vm.startPrank(agent);
        fee.approve(address(escrow), type(uint256).max);
        escrow.postBond(keccak256("fee"), address(fee), 1_000, beneficiary, WINDOW);
        vm.stopPrank();

        assertEq(escrow.getBond(keccak256("fee")).amount, 990, "must record what arrived, not what was asked for");
        assertEq(fee.balanceOf(address(escrow)), 990);
    }

    function testFuzz_releaseReturnsExactlyWhatWasPosted(uint96 amount, uint32 extra) public {
        vm.assume(amount > 0 && amount <= 10_000);
        token.mint(agent, amount);

        bytes32 id = keccak256(abi.encode(amount, extra));
        vm.prank(agent);
        escrow.postBond(id, address(token), amount, beneficiary, WINDOW);

        uint256 balanceAfterPost = token.balanceOf(agent);
        vm.warp(block.timestamp + WINDOW + 1 + uint256(extra));
        vm.prank(agent);
        escrow.release(id);

        assertEq(token.balanceOf(agent), balanceAfterPost + amount);
    }
}
