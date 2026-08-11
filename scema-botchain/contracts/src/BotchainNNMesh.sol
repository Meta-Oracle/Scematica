// SPDX-License-Identifier: MIT
pragma solidity 0.8.24;

/**
 * @title BotchainNNMesh
 * @notice Anchors neural-agent inference batches on BOT Chain, and adjudicates challenges.
 *
 * Companion to `scema-bot-mesh`. An agent registers a policy by its `weightsHash`, then
 * periodically anchors a Merkle root over a batch of inference claims. Anyone can
 * challenge a specific claim inside a window.
 *
 * # What this contract can and cannot decide — read this first
 *
 * It **can** prove, on-chain and cheaply, that a given claim was part of an anchored
 * batch. That is a Merkle verification: a few keccak256 hashes.
 *
 * It **cannot** re-run the neural forward pass. Even a small policy is billions of gas,
 * so no contract on any chain adjudicates that directly today.
 *
 * So the split is honest about where trust sits:
 *
 * | question                              | decided by      |
 * |---------------------------------------|-----------------|
 * | was this claim in the batch?           | this contract   |
 * | did the policy really produce it?      | an arbiter      |
 *
 * Inclusion is trustless; correctness is delegated. The value is still real — an agent can
 * no longer deny having made a claim, nor swap in a different batch afterwards — but
 * anyone describing this as "on-chain verified AI" would be overstating it. The path to
 * removing the arbiter is an interactive fraud proof or a SNARK, and neither is here.
 *
 * # Merkle construction
 *
 * Must match `mesh-runtime::batch` exactly, and the tests pin it against vectors generated
 * by the Rust implementation:
 *
 *   leaf(i) = keccak256(0x00 ‖ uint32be(i) ‖ digest)
 *   node    = keccak256(0x01 ‖ left ‖ right)
 *
 * Leaves bind their index and odd nodes are promoted rather than duplicated, which removes
 * the CVE-2012-2459 ambiguity where distinct batches share a root. Hashing is **not**
 * commutative — no sorted pairs — because sibling order carries position information.
 */
contract BotchainNNMesh {
    struct Agent {
        address owner;
        /// Where the weights can be fetched. A challenger needs them to dispute anything.
        string uri;
        uint64 registeredAt;
    }

    struct Anchor {
        bytes32 weightsHash;
        uint32 claimCount;
        uint64 challengeDeadline;
        bool disputed;
        bool slashed;
    }

    /// @dev Long enough that a challenger can realistically fetch weights and re-run.
    uint64 public constant MIN_CHALLENGE_WINDOW = 5 minutes;
    uint64 public constant MAX_CHALLENGE_WINDOW = 30 days;

    address public owner;
    mapping(address => bool) public isArbiter;
    mapping(bytes32 => Agent) public agents;
    /// Anchors keyed by root. A root is globally unique in practice — it commits to
    /// index-bound leaves — so it doubles as the anchor id.
    mapping(bytes32 => Anchor) public anchors;

    event AgentRegistered(bytes32 indexed weightsHash, address indexed owner, string uri);
    event AgentTransferred(bytes32 indexed weightsHash, address indexed from, address indexed to);
    event BatchAnchored(
        bytes32 indexed root, bytes32 indexed weightsHash, uint32 claimCount, uint64 challengeDeadline
    );
    event ChallengeOpened(bytes32 indexed root, address indexed challenger, bytes32 claimDigest, uint32 index);
    event ChallengeResolved(bytes32 indexed root, bool slashed, string reason);
    event ArbiterSet(address indexed arbiter, bool allowed);
    event OwnershipTransferred(address indexed from, address indexed to);

    error NotOwner();
    error NotArbiter();
    error NotAgentOwner();
    error AgentExists();
    error UnknownAgent();
    error AnchorExists();
    error UnknownAnchor();
    error WindowTooShort();
    error WindowTooLong();
    error ChallengeClosed(uint64 deadline);
    error AlreadyDisputed();
    error NotDisputed();
    error NoClaims();
    error ProofFailed();
    error IndexOutOfRange();
    error ZeroAddress();
    error ZeroHash();

    modifier onlyOwner() {
        if (msg.sender != owner) revert NotOwner();
        _;
    }

    constructor(address initialOwner) {
        if (initialOwner == address(0)) revert ZeroAddress();
        owner = initialOwner;
        emit OwnershipTransferred(address(0), initialOwner);
    }

    // ── agents ────────────────────────────────────────────────────────────────

    /**
     * @notice Claim a policy by its weights hash.
     * @dev First registration wins. The hash is a commitment to specific weights, so two
     * parties claiming the same hash are claiming the same model — and whoever registers
     * first is the one who published it.
     */
    function registerAgent(bytes32 weightsHash, string calldata uri) external {
        if (weightsHash == bytes32(0)) revert ZeroHash();
        if (agents[weightsHash].owner != address(0)) revert AgentExists();
        agents[weightsHash] = Agent({ owner: msg.sender, uri: uri, registeredAt: uint64(block.timestamp) });
        emit AgentRegistered(weightsHash, msg.sender, uri);
    }

    function transferAgent(bytes32 weightsHash, address to) external {
        if (to == address(0)) revert ZeroAddress();
        Agent storage a = agents[weightsHash];
        if (a.owner == address(0)) revert UnknownAgent();
        if (msg.sender != a.owner) revert NotAgentOwner();
        address from = a.owner;
        a.owner = to;
        emit AgentTransferred(weightsHash, from, to);
    }

    // ── anchoring ─────────────────────────────────────────────────────────────

    /// @notice Commit a batch root. Only the agent's owner may anchor under its hash.
    function anchorBatch(bytes32 weightsHash, bytes32 root, uint32 claimCount, uint64 challengeWindow)
        external
    {
        Agent storage a = agents[weightsHash];
        if (a.owner == address(0)) revert UnknownAgent();
        if (msg.sender != a.owner) revert NotAgentOwner();
        if (root == bytes32(0)) revert ZeroHash();
        // An anchor asserting zero claims commits to nothing while looking like activity.
        if (claimCount == 0) revert NoClaims();
        if (anchors[root].weightsHash != bytes32(0)) revert AnchorExists();
        if (challengeWindow < MIN_CHALLENGE_WINDOW) revert WindowTooShort();
        if (challengeWindow > MAX_CHALLENGE_WINDOW) revert WindowTooLong();

        uint64 deadline = uint64(block.timestamp) + challengeWindow;
        anchors[root] = Anchor({
            weightsHash: weightsHash,
            claimCount: claimCount,
            challengeDeadline: deadline,
            disputed: false,
            slashed: false
        });
        emit BatchAnchored(root, weightsHash, claimCount, deadline);
    }

    // ── challenges ────────────────────────────────────────────────────────────

    /**
     * @notice Open a challenge against one claim in an anchored batch.
     * @param root        The anchored batch root.
     * @param claimDigest The disputed claim's digest.
     * @param index       Its position in the batch, which is bound into its leaf hash.
     * @param siblings    Proof path, leaf-to-root.
     * @param siblingIsLeft Per step: does the sibling sit on the left?
     *
     * @dev The proof is checked here so a challenge cannot be opened against a claim that
     * was never in the batch. That is the part the contract can decide by itself — whether
     * the claim's *content* is wrong is left to an arbiter, because re-running the network
     * is not affordable on any chain.
     */
    function challenge(
        bytes32 root,
        bytes32 claimDigest,
        uint32 index,
        bytes32[] calldata siblings,
        bool[] calldata siblingIsLeft
    ) external {
        Anchor storage anc = anchors[root];
        if (anc.weightsHash == bytes32(0)) revert UnknownAnchor();
        if (anc.disputed) revert AlreadyDisputed();
        if (index >= anc.claimCount) revert IndexOutOfRange();
        // MIN_CHALLENGE_WINDOW dwarfs any drift a proposer can introduce; see the same
        // reasoning in ScemaBondEscrow.
        // forge-lint: disable-next-line(block-timestamp)
        if (block.timestamp > anc.challengeDeadline) revert ChallengeClosed(anc.challengeDeadline);
        if (!verifyInclusion(root, claimDigest, index, siblings, siblingIsLeft)) revert ProofFailed();

        anc.disputed = true;
        emit ChallengeOpened(root, msg.sender, claimDigest, index);
    }

    /**
     * @notice Arbiter's ruling on an open challenge.
     * @dev Deliberately narrow: it records a verdict and emits it. It does **not** move
     * funds. Slashing lives in `ScemaBondEscrow`, which holds the bond and has its own
     * arbiter set — keeping the registry unable to transfer value means a compromised
     * arbiter here cannot drain anything, only mislabel a batch.
     */
    function resolveChallenge(bytes32 root, bool slashed, string calldata reason) external {
        if (!isArbiter[msg.sender]) revert NotArbiter();
        Anchor storage anc = anchors[root];
        if (anc.weightsHash == bytes32(0)) revert UnknownAnchor();
        if (!anc.disputed) revert NotDisputed();

        anc.slashed = slashed;
        emit ChallengeResolved(root, slashed, reason);
    }

    /// @notice True once the window has closed with no successful challenge.
    function isFinal(bytes32 root) external view returns (bool) {
        Anchor storage anc = anchors[root];
        if (anc.weightsHash == bytes32(0)) return false;
        // forge-lint: disable-next-line(block-timestamp)
        return !anc.slashed && block.timestamp > anc.challengeDeadline;
    }

    // ── Merkle ────────────────────────────────────────────────────────────────

    /// @notice Recompute a root from a claim and its path. Mirrors `mesh-runtime::batch`.
    function verifyInclusion(
        bytes32 root,
        bytes32 claimDigest,
        uint32 index,
        bytes32[] calldata siblings,
        bool[] calldata siblingIsLeft
    ) public pure returns (bool) {
        if (siblings.length != siblingIsLeft.length) return false;

        bytes32 node = leafHash(index, claimDigest);
        for (uint256 i = 0; i < siblings.length; ++i) {
            node = siblingIsLeft[i] ? nodeHash(siblings[i], node) : nodeHash(node, siblings[i]);
        }
        return node == root;
    }

    /// `keccak256(0x00 ‖ uint32be(index) ‖ digest)` — the index binding is what stops a
    /// valid proof for one position being replayed at another.
    function leafHash(uint32 index, bytes32 digest) public pure returns (bytes32) {
        return keccak256(abi.encodePacked(bytes1(0x00), index, digest));
    }

    /// `keccak256(0x01 ‖ left ‖ right)` — tagged so an internal preimage can never be
    /// presented as a leaf, and ordered because position matters.
    function nodeHash(bytes32 left, bytes32 right) public pure returns (bytes32) {
        return keccak256(abi.encodePacked(bytes1(0x01), left, right));
    }

    // ── administration ────────────────────────────────────────────────────────

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
}
