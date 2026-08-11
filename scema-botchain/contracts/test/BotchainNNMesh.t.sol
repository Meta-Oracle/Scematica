// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

import { Test } from "forge-std/Test.sol";
import { BotchainNNMesh } from "../src/BotchainNNMesh.sol";

contract BotchainNNMeshTest is Test {
    BotchainNNMesh internal reg;

    address internal owner = address(0xA11CE);
    address internal agentOwner = address(0xA6E7);
    address internal arbiter = address(0xA781);
    address internal challenger = address(0xC4A1);

    bytes32 internal constant WEIGHTS = keccak256("policy-v1");
    uint64 internal constant WINDOW = 1 hours;

    /// Claim digests used by the Rust vector generator: 32 bytes of 0x01, 0x02, …
    function d(uint8 b) internal pure returns (bytes32 out) {
        for (uint256 i = 0; i < 32; ++i) {
            out |= bytes32(uint256(b)) << (8 * i);
        }
    }

    function setUp() public {
        reg = new BotchainNNMesh(owner);
        vm.prank(owner);
        reg.setArbiter(arbiter, true);
        vm.prank(agentOwner);
        reg.registerAgent(WEIGHTS, "ipfs://policy-v1");
    }

    // ── cross-implementation parity ───────────────────────────────────────────
    //
    // These constants are printed by `mesh-runtime`'s `print_vectors_for_solidity` test.
    // Two implementations of one hash tree is the likeliest place for this design to break
    // silently — an honest agent whose proof fails on-chain looks like a fraud — so the
    // Rust output is pinned here rather than assumed compatible.

    function test_parity_leafHashMatchesRust() public view {
        // n=1 root from Rust: the tree of one leaf *is* that leaf.
        assertEq(
            reg.leafHash(0, d(1)),
            0xf1c4ffe1e1ca24e75f5216f2937db3e6ca99ba5db2a351e7a66d241e379fb97d
        );
    }

    function test_parity_twoClaimRootMatchesRust() public view {
        bytes32 root = 0x89be21cf0de60939586f4ae3764ab69883d088ae49bf40bb3c61b3d2b3c5b67f;
        assertEq(reg.nodeHash(reg.leafHash(0, d(1)), reg.leafHash(1, d(2))), root);
    }

    function test_parity_inclusionProofsFromRustVerify() public view {
        // n=2, claim 0: sibling on the right.
        bytes32 root2 = 0x89be21cf0de60939586f4ae3764ab69883d088ae49bf40bb3c61b3d2b3c5b67f;
        bytes32[] memory sib = new bytes32[](1);
        bool[] memory left = new bool[](1);
        sib[0] = 0x200279f85f26391d0ee14064013ccecbdd602fec44d307ffc763afe17d72075f;
        left[0] = false;
        assertTrue(reg.verifyInclusion(root2, d(1), 0, sib, left), "n=2 i=0");

        // n=2, claim 1: sibling on the left.
        sib[0] = 0xf1c4ffe1e1ca24e75f5216f2937db3e6ca99ba5db2a351e7a66d241e379fb97d;
        left[0] = true;
        assertTrue(reg.verifyInclusion(root2, d(2), 1, sib, left), "n=2 i=1");
    }

    function test_parity_promotedOddNodeMatchesRust() public view {
        // n=3, claim 2 is promoted through the odd level and needs only one step.
        bytes32 root3 = 0x30f564153d8a82eaf8b6d680365bf825da81f03a6d7c36250eab090fbd355045;
        bytes32[] memory sib = new bytes32[](1);
        bool[] memory left = new bool[](1);
        sib[0] = 0x89be21cf0de60939586f4ae3764ab69883d088ae49bf40bb3c61b3d2b3c5b67f;
        left[0] = true;
        assertTrue(reg.verifyInclusion(root3, d(3), 2, sib, left), "n=3 i=2 promoted");
    }

    function test_parity_deepPathMatchesRust() public view {
        // n=5, claim 2: mixed left/right path, three steps.
        bytes32 root5 = 0x2ea07927b8a11be06c10d82b6de900f8bda76fd3c274f96a3b884b841f80400c;
        bytes32[] memory sib = new bytes32[](3);
        bool[] memory left = new bool[](3);
        sib[0] = 0x19f197ac8dae0b00add39e6048ee77bd43cdac404d6fd5d9a903a10f0051bcdb;
        left[0] = false;
        sib[1] = 0x89be21cf0de60939586f4ae3764ab69883d088ae49bf40bb3c61b3d2b3c5b67f;
        left[1] = true;
        sib[2] = 0x6b406931f6a40c2432d4d49475c3681bd640214ea787399d53471b95865f5023;
        left[2] = false;
        assertTrue(reg.verifyInclusion(root5, d(3), 2, sib, left), "n=5 i=2");
    }

    function test_aClaimCannotBeReplayedAtAnotherIndex() public view {
        bytes32 root2 = 0x89be21cf0de60939586f4ae3764ab69883d088ae49bf40bb3c61b3d2b3c5b67f;
        bytes32[] memory sib = new bytes32[](1);
        bool[] memory left = new bool[](1);
        sib[0] = 0x200279f85f26391d0ee14064013ccecbdd602fec44d307ffc763afe17d72075f;
        left[0] = false;
        // Same proof, wrong index — must fail, because the index is inside the leaf hash.
        assertFalse(reg.verifyInclusion(root2, d(1), 1, sib, left));
    }

    function test_mismatchedProofArraysAreRejected() public view {
        bytes32[] memory sib = new bytes32[](2);
        bool[] memory left = new bool[](1);
        assertFalse(reg.verifyInclusion(bytes32(uint256(1)), d(1), 0, sib, left));
    }

    // ── registry behaviour ────────────────────────────────────────────────────

    function _anchor(bytes32 root) internal {
        vm.prank(agentOwner);
        reg.anchorBatch(WEIGHTS, root, 5, WINDOW);
    }

    function test_onlyAgentOwnerMayAnchor() public {
        vm.prank(challenger);
        vm.expectRevert(BotchainNNMesh.NotAgentOwner.selector);
        reg.anchorBatch(WEIGHTS, keccak256("r"), 5, WINDOW);
    }

    function test_cannotAnchorForAnUnknownAgent() public {
        vm.prank(agentOwner);
        vm.expectRevert(BotchainNNMesh.UnknownAgent.selector);
        reg.anchorBatch(keccak256("nope"), keccak256("r"), 5, WINDOW);
    }

    function test_emptyBatchIsRefused() public {
        // An anchor asserting zero claims looks like activity while committing to nothing.
        vm.prank(agentOwner);
        vm.expectRevert(BotchainNNMesh.NoClaims.selector);
        reg.anchorBatch(WEIGHTS, keccak256("r"), 0, WINDOW);
    }

    function test_aRootCannotBeAnchoredTwice() public {
        bytes32 root = keccak256("r");
        _anchor(root);
        vm.prank(agentOwner);
        vm.expectRevert(BotchainNNMesh.AnchorExists.selector);
        reg.anchorBatch(WEIGHTS, root, 5, WINDOW);
    }

    function test_registrationIsFirstComeFirstServed() public {
        vm.prank(challenger);
        vm.expectRevert(BotchainNNMesh.AgentExists.selector);
        reg.registerAgent(WEIGHTS, "ipfs://impostor");
    }

    function test_challengeRequiresAValidInclusionProof() public {
        bytes32 root = keccak256("r");
        _anchor(root);
        bytes32[] memory sib = new bytes32[](0);
        bool[] memory left = new bool[](0);

        vm.prank(challenger);
        vm.expectRevert(BotchainNNMesh.ProofFailed.selector);
        reg.challenge(root, d(9), 0, sib, left);
    }

    function test_challengeIndexMustBeInsideTheBatch() public {
        bytes32 root = keccak256("r");
        _anchor(root);
        bytes32[] memory sib = new bytes32[](0);
        bool[] memory left = new bool[](0);

        vm.prank(challenger);
        vm.expectRevert(BotchainNNMesh.IndexOutOfRange.selector);
        reg.challenge(root, d(1), 99, sib, left);
    }

    function test_challengeWindowCloses() public {
        // A single-claim batch: the leaf is the root, so the proof is empty and valid.
        bytes32 root = reg.leafHash(0, d(1));
        vm.prank(agentOwner);
        reg.anchorBatch(WEIGHTS, root, 1, WINDOW);

        vm.warp(block.timestamp + WINDOW + 1);
        bytes32[] memory sib = new bytes32[](0);
        bool[] memory left = new bool[](0);
        vm.prank(challenger);
        vm.expectRevert();
        reg.challenge(root, d(1), 0, sib, left);
    }

    function test_fullChallengeAndResolutionPath() public {
        bytes32 root = reg.leafHash(0, d(1));
        vm.prank(agentOwner);
        reg.anchorBatch(WEIGHTS, root, 1, WINDOW);

        bytes32[] memory sib = new bytes32[](0);
        bool[] memory left = new bool[](0);
        vm.prank(challenger);
        reg.challenge(root, d(1), 0, sib, left);

        vm.prank(arbiter);
        reg.resolveChallenge(root, true, "output mismatch on replay");

        (,,, bool disputed, bool slashed) = reg.anchors(root);
        assertTrue(disputed);
        assertTrue(slashed);
        assertFalse(reg.isFinal(root), "a slashed batch never becomes final");
    }

    function test_onlyArbiterMayResolve() public {
        bytes32 root = reg.leafHash(0, d(1));
        vm.prank(agentOwner);
        reg.anchorBatch(WEIGHTS, root, 1, WINDOW);
        bytes32[] memory sib = new bytes32[](0);
        bool[] memory left = new bool[](0);
        vm.prank(challenger);
        reg.challenge(root, d(1), 0, sib, left);

        vm.prank(agentOwner);
        vm.expectRevert(BotchainNNMesh.NotArbiter.selector);
        reg.resolveChallenge(root, false, "self-serving");
    }

    function test_unchallengedBatchBecomesFinal() public {
        bytes32 root = keccak256("r");
        _anchor(root);
        assertFalse(reg.isFinal(root), "not final inside the window");
        vm.warp(block.timestamp + WINDOW + 1);
        assertTrue(reg.isFinal(root));
    }

    function test_windowBoundsAreEnforced() public {
        vm.startPrank(agentOwner);
        vm.expectRevert(BotchainNNMesh.WindowTooShort.selector);
        reg.anchorBatch(WEIGHTS, keccak256("a"), 1, 1 seconds);
        vm.expectRevert(BotchainNNMesh.WindowTooLong.selector);
        reg.anchorBatch(WEIGHTS, keccak256("b"), 1, 60 days);
        vm.stopPrank();
    }

    function test_agentTransferMovesAnchoringRights() public {
        vm.prank(agentOwner);
        reg.transferAgent(WEIGHTS, challenger);

        vm.prank(agentOwner);
        vm.expectRevert(BotchainNNMesh.NotAgentOwner.selector);
        reg.anchorBatch(WEIGHTS, keccak256("r"), 1, WINDOW);

        vm.prank(challenger);
        reg.anchorBatch(WEIGHTS, keccak256("r"), 1, WINDOW);
    }
}
