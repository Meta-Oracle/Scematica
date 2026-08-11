//! `Agent` — the surface a game integrates against.
//!
//! Design target: a studio adds an agent without learning anything about chains. Three
//! properties make that possible, and each is a constraint on this file:
//!
//! * **`act` is synchronous, local and allocation-light.** No RPC, no await, no lock. It
//!   is called from a frame loop; anything that can block belongs somewhere else.
//! * **Committing is decoupled from acting.** Claims accumulate in memory and are flushed
//!   in batches by the host, on its own schedule. A stalled node degrades commitments, not
//!   gameplay — a chain outage must never drop frames.
//! * **Recording is optional.** An agent with recording off is a plain policy network with
//!   zero overhead, so the same build ships in single-player where nothing is disputed.

use mesh_core::commit::{weights_hash, Digest, InferenceClaim};
use mesh_core::fixed::Fx;
use mesh_core::net::{MeshError, PolicyNet};

use crate::batch::ClaimBatch;
use crate::format::{self, FormatError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordMode {
    /// Keep every claim. Full auditability, memory grows until flushed.
    All,
    /// Keep only the digest of each claim. Enough to anchor and to prove inclusion, but
    /// the inputs must be reproducible from the host's own replay to challenge one.
    DigestsOnly,
    /// Record nothing. Zero overhead for single-player.
    Off,
}

#[derive(Debug)]
pub enum AgentError {
    Net(MeshError),
    Format(FormatError),
}

impl From<MeshError> for AgentError {
    fn from(e: MeshError) -> Self {
        AgentError::Net(e)
    }
}
impl From<FormatError> for AgentError {
    fn from(e: FormatError) -> Self {
        AgentError::Format(e)
    }
}

/// A policy plus its commitment bookkeeping.
pub struct Agent {
    net: PolicyNet,
    /// Cached because it hashes every parameter — far too expensive to recompute per act,
    /// and the weights cannot change without going through `replace_policy`.
    weights: Digest,
    mode: RecordMode,
    batch: ClaimBatch,
    claims: Vec<InferenceClaim>,
    acts: u64,
}

impl Agent {
    pub fn new(net: PolicyNet, mode: RecordMode) -> Self {
        let weights = weights_hash(&net);
        Agent { net, weights, mode, batch: ClaimBatch::new(), claims: Vec::new(), acts: 0 }
    }

    /// Load a `.mesh` file.
    pub fn load(bytes: &[u8], mode: RecordMode) -> Result<Self, AgentError> {
        Ok(Agent::new(format::decode(bytes)?, mode))
    }

    pub fn save(&self) -> Vec<u8> {
        format::encode(&self.net)
    }

    /// The commitment identifying this policy. Stable for the agent's lifetime.
    pub fn weights_hash(&self) -> Digest {
        self.weights
    }

    pub fn policy(&self) -> &PolicyNet {
        &self.net
    }

    pub fn acts(&self) -> u64 {
        self.acts
    }

    /// Swap in a different policy, rehashing and **clearing pending claims**.
    ///
    /// Clearing is not tidiness. A batch mixing claims from two policies anchors under one
    /// root while its leaves reference different `weights_hash` values, so a challenger
    /// re-running "the" policy would find honest claims failing. Flush before replacing.
    pub fn replace_policy(&mut self, net: PolicyNet) {
        self.net = net;
        self.weights = weights_hash(&self.net);
        self.batch.clear();
        self.claims.clear();
    }

    /// Choose an action. The hot path.
    pub fn act(&mut self, observation: &[Fx]) -> Result<usize, AgentError> {
        self.acts += 1;

        match self.mode {
            RecordMode::Off => Ok(self.net.act(observation)?),
            RecordMode::DigestsOnly => {
                let claim = InferenceClaim::produce(&self.net, observation)?;
                let action = claim.action;
                self.batch.push(claim.digest());
                Ok(action)
            }
            RecordMode::All => {
                let claim = InferenceClaim::produce(&self.net, observation)?;
                let action = claim.action;
                self.batch.push(claim.digest());
                self.claims.push(claim);
                Ok(action)
            }
        }
    }

    /// Q-values without recording anything — for debug overlays and tooling.
    pub fn inspect(&self, observation: &[Fx]) -> Result<Vec<Fx>, AgentError> {
        Ok(self.net.q_values(observation)?)
    }

    pub fn pending(&self) -> usize {
        self.batch.len()
    }

    pub fn batch(&self) -> &ClaimBatch {
        &self.batch
    }

    pub fn claims(&self) -> &[InferenceClaim] {
        &self.claims
    }

    /// Seal the pending claims into an anchor and start a fresh batch.
    ///
    /// Returns `None` when nothing is pending, so a host that flushes on a timer does not
    /// anchor empty roots — an empty commitment costs gas and asserts nothing.
    pub fn flush(&mut self) -> Option<Anchor> {
        let root = self.batch.root()?;
        let anchor = Anchor {
            weights: self.weights,
            root,
            claim_count: self.batch.len(),
            claims: core::mem::take(&mut self.claims),
        };
        self.batch.clear();
        Some(anchor)
    }
}

/// A sealed batch: what goes on chain, plus what a challenger needs.
#[derive(Debug, Clone)]
pub struct Anchor {
    /// Which policy produced these. Anchored alongside the root so a challenger knows
    /// which weights to fetch before disputing anything.
    pub weights: Digest,
    /// The 32 bytes committed on BOT Chain.
    pub root: Digest,
    pub claim_count: usize,
    /// Present only under [`RecordMode::All`].
    pub claims: Vec<InferenceClaim>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::batch::verify_proof;
    use mesh_core::commit::Verdict;

    fn net() -> PolicyNet {
        let mut n = PolicyNet::new(3, &[4], 2);
        for (i, w) in n.trunk[0].weights.iter_mut().enumerate() {
            *w = Fx::from_bits(3_000 * (i as i32 % 7 + 1));
        }
        n.advantage_head.weights[1] = Fx::ONE;
        n.value_head.weights[0] = Fx::ONE;
        n
    }

    fn obs(k: i16) -> Vec<Fx> {
        vec![Fx::from_int(k), Fx::from_int(k + 1), Fx::from_int(2)]
    }

    #[test]
    fn acting_is_deterministic_regardless_of_record_mode() {
        // Recording must not change behaviour, or a studio's single-player build and its
        // audited build would diverge — the worst possible place for a discrepancy.
        let mut off = Agent::new(net(), RecordMode::Off);
        let mut all = Agent::new(net(), RecordMode::All);
        for k in 0..8 {
            assert_eq!(off.act(&obs(k)).unwrap(), all.act(&obs(k)).unwrap());
        }
        assert_eq!(off.pending(), 0);
        assert_eq!(all.pending(), 8);
    }

    #[test]
    fn every_claim_in_a_flushed_anchor_proves_against_its_root() {
        let mut agent = Agent::new(net(), RecordMode::All);
        for k in 0..5 {
            agent.act(&obs(k)).unwrap();
        }
        // Take the proofs before flushing clears the batch.
        let proofs: Vec<_> = (0..5).map(|i| agent.batch().proof(i).unwrap()).collect();
        let digests: Vec<_> = agent.batch().digests().to_vec();

        let anchor = agent.flush().unwrap();
        assert_eq!(anchor.claim_count, 5);
        for (i, proof) in proofs.iter().enumerate() {
            assert!(verify_proof(&digests[i], i as u32, proof, &anchor.root));
        }
    }

    #[test]
    fn recorded_claims_verify_against_the_policy() {
        let policy = net();
        let mut agent = Agent::new(policy.clone(), RecordMode::All);
        for k in 0..4 {
            agent.act(&obs(k)).unwrap();
        }
        let anchor = agent.flush().unwrap();
        assert_eq!(anchor.weights, weights_hash(&policy));
        for claim in &anchor.claims {
            assert_eq!(claim.verify(&policy), Verdict::Valid);
        }
    }

    #[test]
    fn flushing_nothing_anchors_nothing() {
        let mut agent = Agent::new(net(), RecordMode::All);
        assert!(agent.flush().is_none());
    }

    #[test]
    fn flush_resets_the_batch() {
        let mut agent = Agent::new(net(), RecordMode::DigestsOnly);
        agent.act(&obs(1)).unwrap();
        let first = agent.flush().unwrap();
        agent.act(&obs(2)).unwrap();
        let second = agent.flush().unwrap();
        assert_eq!(second.claim_count, 1, "the second batch must not carry the first");
        assert_ne!(first.root, second.root);
    }

    #[test]
    fn digests_only_still_anchors_but_keeps_no_inputs() {
        let mut agent = Agent::new(net(), RecordMode::DigestsOnly);
        agent.act(&obs(3)).unwrap();
        let anchor = agent.flush().unwrap();
        assert_eq!(anchor.claim_count, 1);
        assert!(anchor.claims.is_empty());
    }

    #[test]
    fn replacing_the_policy_drops_claims_made_under_the_old_one() {
        // Otherwise one root would cover leaves referencing two different weight hashes,
        // and honest claims would fail adjudication.
        let mut agent = Agent::new(net(), RecordMode::All);
        agent.act(&obs(1)).unwrap();
        assert_eq!(agent.pending(), 1);

        let mut other = net();
        other.value_head.biases[0] = Fx::ONE;
        let before = agent.weights_hash();
        agent.replace_policy(other);

        assert_eq!(agent.pending(), 0);
        assert_ne!(agent.weights_hash(), before);
    }

    #[test]
    fn save_load_preserves_behaviour_and_identity() {
        let mut original = Agent::new(net(), RecordMode::Off);
        let bytes = original.save();
        let mut restored = Agent::load(&bytes, RecordMode::Off).unwrap();

        assert_eq!(restored.weights_hash(), original.weights_hash());
        for k in 0..6 {
            assert_eq!(restored.act(&obs(k)).unwrap(), original.act(&obs(k)).unwrap());
        }
    }

    #[test]
    fn inspect_does_not_record() {
        // `inspect` takes &self — the immutable binding here is itself the assertion that
        // a debug overlay cannot mutate commitment state.
        let agent = Agent::new(net(), RecordMode::All);
        agent.inspect(&obs(1)).unwrap();
        assert_eq!(agent.pending(), 0, "a debug overlay must not create commitments");
    }

    #[test]
    fn a_bad_observation_shape_is_an_error_not_a_panic() {
        let mut agent = Agent::new(net(), RecordMode::All);
        assert!(matches!(agent.act(&[Fx::ONE]), Err(AgentError::Net(_))));
    }
}
