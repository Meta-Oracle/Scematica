//! **A succinct, transparent proof-of-inference backend** — the real zk step behind
//! Primitive I, the piece that lets a dispute window collapse toward zero.
//!
//! [`crate::zkbond::ReexecutionProofSystem`] is *optimistic*: to refute a bad
//! inference a challenger must hold the model, re-run the **entire** forward pass,
//! and catch the lie. That is why the dispute window `W` has to be long — refutation
//! is slow and needs the (possibly private) weights. This module replaces "re-run
//! everything" with "check a **succinct proof**": the prover commits to its whole
//! execution trace and answers a handful of Fiat-Shamir challenges, and a verifier
//! confirms the output is a correct forward pass by checking only those few spots —
//! **without re-executing the network**. A valid proof resolves a bond
//! deterministically ([`crate::coordinator::DisputeCoordinator::resolve_via_oracle`]
//! → [`crate::settlement::FinalizeReason::ProofVerified`]), which is precisely how a
//! real zk backend shrinks `W`.
//!
//! ## What it is, honestly
//!
//! This is a **transparent, hash-based interactive-oracle proof** made
//! non-interactive by Fiat-Shamir — the same soundness core as a zk-STARK, minus the
//! low-degree extension that buys full succinctness and minus the zero-knowledge
//! hiding. Concretely:
//!
//! 1. **Commit.** The prover lays its trace — input, every weight, every bias, every
//!    neuron activation — into one Merkle tree over a fixed-point ([`SCALE`])
//!    quantisation, and publishes the root. The trace is *self-consistent under
//!    quantisation*: each layer is computed from the **dequantised** previous layer,
//!    so a verifier reproduces every value bit-for-bit (no float-rounding drift).
//! 2. **Challenge.** Fiat-Shamir derives `num_queries` random neurons from the root.
//! 3. **Open.** The prover authenticates, against the root: the full input, the full
//!    output, and — for each challenged neuron — its activation, bias, weight row,
//!    and the previous-layer activations it consumed.
//! 4. **Verify.** The verifier checks the Merkle openings, re-pins the input/output
//!    boundary, and recomputes *only the challenged neurons*. Work is
//!    `O(num_queries · fan_in)` — sublinear in the network's parameter count.
//!
//! **Soundness.** Input and output are fully pinned, so forging the output forces the
//! prover to corrupt at least one neuron on every input→output path. A corrupted
//! neuron is caught whenever a challenge lands on it: with `N` computed neurons and
//! `q` queries, a single corruption survives with probability `≤ (1 − 1/N)^q`. Raise
//! `num_queries` to drive that arbitrarily low — the cost/soundness dial an
//! optimistic window can't offer.
//!
//! **Not** in scope (what a SNARK backend — risc0 / SP1 / halo2 — adds behind this
//! same role): zero-knowledge *hiding* of the weights, a proof size independent of
//! the trace, and cryptographic binding to a registered model commitment. Those slot
//! in without touching [`crate::coordinator`] — this module is the transparent,
//! trusted-setup-free reference that already makes refutation succinct.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Fixed-point scale for deterministic float→integer quantisation (6 decimals).
pub const SCALE: f64 = 1_000_000.0;

const DOMAIN_LEAF: &[u8] = b"scemadex.zkbackend.v1.leaf";
const DOMAIN_NODE: &[u8] = b"scemadex.zkbackend.v1.node";
const DOMAIN_FS: &[u8] = b"scemadex.zkbackend.v1.fiat-shamir";

fn q(x: f64) -> i64 {
    (x * SCALE).round() as i64
}

fn dq(i: i64) -> f64 {
    i as f64 / SCALE
}

fn hash(parts: &[&[u8]]) -> [u8; 32] {
    let mut h = Sha256::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}

fn leaf_hash(index: u64, value: i64) -> [u8; 32] {
    hash(&[DOMAIN_LEAF, &index.to_le_bytes(), &value.to_le_bytes()])
}

/// One dense layer, weights in `[out][in]` (row-major) order.
#[derive(Clone, Debug)]
pub struct DenseLayer {
    pub weights: Vec<Vec<f64>>,
    pub biases: Vec<f64>,
    /// Apply a ReLU after the affine map (hidden layers); `false` for a linear head.
    pub relu: bool,
}

impl DenseLayer {
    pub fn new(weights: Vec<Vec<f64>>, biases: Vec<f64>, relu: bool) -> Self {
        Self { weights, biases, relu }
    }

    fn out_dim(&self) -> usize {
        self.biases.len()
    }
}

/// The public shape of a layer, carried in the proof so a verifier can recompute the
/// trace layout and activation rule without holding the weights.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LayerShape {
    pub out_dim: usize,
    pub relu: bool,
}

/// A plain multilayer perceptron whose forward pass can be proven succinctly.
///
/// Layer `l` consumes the previous activation (the input for `l == 0`) and produces
/// `out_dim` activations. Weight rows must match the incoming width.
#[derive(Clone, Debug)]
pub struct TracedMlp {
    pub layers: Vec<DenseLayer>,
    pub input_dim: usize,
}

impl TracedMlp {
    pub fn new(input_dim: usize, layers: Vec<DenseLayer>) -> Self {
        Self { layers, input_dim }
    }

    fn shape(&self) -> Vec<LayerShape> {
        self.layers
            .iter()
            .map(|l| LayerShape { out_dim: l.out_dim(), relu: l.relu })
            .collect()
    }
}

/// How much soundness to buy: more queries → exponentially smaller odds a corrupted
/// neuron escapes, at linear verifier cost.
#[derive(Clone, Copy, Debug)]
pub struct SpotCheckConfig {
    pub num_queries: usize,
}

impl Default for SpotCheckConfig {
    fn default() -> Self {
        Self { num_queries: 16 }
    }
}

/// A single authenticated trace cell: its position, quantised value, and the Merkle
/// path proving it belongs to the committed root.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenedCell {
    pub index: u64,
    pub value: i64,
    pub path: Vec<[u8; 32]>,
}

/// A succinct proof that `output` is the correct forward pass of a committed MLP on
/// an input the verifier supplies. Verified by [`verify`](SpotCheckProof::verify).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpotCheckProof {
    pub root: [u8; 32],
    pub num_leaves: u64,
    pub input_dim: usize,
    pub shape: Vec<LayerShape>,
    /// The number of Fiat-Shamir neuron challenges this proof answers. The
    /// verifier re-derives *exactly this many* challenges from the root. Because the
    /// Fiat-Shamir sample is prefix-nested (deriving `k+1` yields the same first `k`
    /// plus one more), a proof built with more queries strictly subsumes a smaller
    /// one — so a verifier can safely reject a proof whose `num_queries` is below its
    /// required soundness floor (see [`verify_with_min`](SpotCheckProof::verify_with_min)).
    pub num_queries: usize,
    /// The claimed network output (dequantised); pinned to the committed trace.
    pub output: Vec<f64>,
    /// Authenticated cells: full input + full output boundary, plus every cell the
    /// challenged neurons consume. Deduplicated.
    pub cells: Vec<OpenedCell>,
}

/// Layout of the flat trace cell vector, shared by prover and verifier so cell
/// indices line up exactly. Order: `[input] [w,b,act]₀ [w,b,act]₁ …`.
struct Layout {
    input_dim: usize,
    fan_in: Vec<usize>,
    out: Vec<usize>,
    weight_off: Vec<usize>,
    bias_off: Vec<usize>,
    act_off: Vec<usize>,
    total: usize,
}

impl Layout {
    fn new(input_dim: usize, shape: &[LayerShape]) -> Self {
        let mut fan_in = Vec::with_capacity(shape.len());
        let mut out = Vec::with_capacity(shape.len());
        let mut weight_off = Vec::with_capacity(shape.len());
        let mut bias_off = Vec::with_capacity(shape.len());
        let mut act_off = Vec::with_capacity(shape.len());
        let mut cursor = input_dim; // input cells occupy [0, input_dim)
        let mut prev_width = input_dim;
        for s in shape {
            let fi = prev_width;
            fan_in.push(fi);
            out.push(s.out_dim);
            weight_off.push(cursor);
            cursor += s.out_dim * fi;
            bias_off.push(cursor);
            cursor += s.out_dim;
            act_off.push(cursor);
            cursor += s.out_dim;
            prev_width = s.out_dim;
        }
        Self {
            input_dim,
            fan_in,
            out,
            weight_off,
            bias_off,
            act_off,
            total: cursor,
        }
    }

    fn input_cell(&self, i: usize) -> usize {
        i
    }
    fn weight_cell(&self, l: usize, o: usize, i: usize) -> usize {
        self.weight_off[l] + o * self.fan_in[l] + i
    }
    fn bias_cell(&self, l: usize, o: usize) -> usize {
        self.bias_off[l] + o
    }
    fn act_cell(&self, l: usize, o: usize) -> usize {
        self.act_off[l] + o
    }
    /// Cell holding the `i`-th input to layer `l` (the input vector for `l == 0`).
    fn prev_act_cell(&self, l: usize, i: usize) -> usize {
        if l == 0 {
            self.input_cell(i)
        } else {
            self.act_cell(l - 1, i)
        }
    }
    /// Flattened `(layer, neuron)` list — the Fiat-Shamir challenge domain.
    fn computed_neurons(&self) -> Vec<(usize, usize)> {
        let mut v = Vec::new();
        for (l, &o) in self.out.iter().enumerate() {
            for j in 0..o {
                v.push((l, j));
            }
        }
        v
    }
}

/// A Merkle tree over the quantised trace cells, keeping every level so any leaf can
/// be opened. Odd levels duplicate the last node (a standard, unambiguous rule the
/// verifier mirrors via `num_leaves`).
struct MerkleTree {
    levels: Vec<Vec<[u8; 32]>>,
}

impl MerkleTree {
    fn build(cells: &[i64]) -> Self {
        let leaves: Vec<[u8; 32]> = cells
            .iter()
            .enumerate()
            .map(|(i, &v)| leaf_hash(i as u64, v))
            .collect();
        let mut levels = vec![leaves];
        while levels.last().unwrap().len() > 1 {
            let cur = levels.last().unwrap();
            let mut next = Vec::with_capacity(cur.len().div_ceil(2));
            let mut i = 0;
            while i < cur.len() {
                let left = cur[i];
                let right = if i + 1 < cur.len() { cur[i + 1] } else { cur[i] };
                next.push(hash(&[DOMAIN_NODE, &left, &right]));
                i += 2;
            }
            levels.push(next);
        }
        Self { levels }
    }

    fn root(&self) -> [u8; 32] {
        *self.levels.last().unwrap().last().unwrap()
    }

    /// Sibling hashes from leaf to root for `index`.
    fn open(&self, index: usize) -> Vec<[u8; 32]> {
        let mut path = Vec::with_capacity(self.levels.len());
        let mut idx = index;
        for level in &self.levels[..self.levels.len() - 1] {
            let sib = if idx.is_multiple_of(2) {
                if idx + 1 < level.len() { idx + 1 } else { idx } // duplicated last
            } else {
                idx - 1
            };
            path.push(level[sib]);
            idx /= 2;
        }
        path
    }
}

/// Recompute a root from a leaf and its path, mirroring the build rule. `num_leaves`
/// disambiguates the duplicated-last-node case at every level.
fn merkle_root_from(index: u64, value: i64, path: &[[u8; 32]], num_leaves: u64) -> [u8; 32] {
    let mut node = leaf_hash(index, value);
    let mut idx = index;
    let mut level_len = num_leaves;
    for sib in path {
        let (left, right) = if idx.is_multiple_of(2) {
            // Even index: sibling is the right node, or `node` itself if it's the
            // duplicated last leaf of an odd-length level.
            let is_last_dup = idx + 1 == level_len;
            (node, if is_last_dup { node } else { *sib })
        } else {
            (*sib, node)
        };
        node = hash(&[DOMAIN_NODE, &left, &right]);
        idx /= 2;
        level_len = level_len.div_ceil(2);
    }
    node
}

/// Fiat-Shamir: derive `num_queries` distinct neuron indices in `[0, domain)` from
/// the commitment root (and leaf count). Deterministic and verifier-reproducible.
fn fiat_shamir(root: &[u8; 32], num_leaves: u64, num_queries: usize, domain: usize) -> Vec<usize> {
    let mut out = Vec::new();
    if domain == 0 {
        return out;
    }
    let want = num_queries.min(domain);
    let mut seen = std::collections::HashSet::new();
    let mut counter: u64 = 0;
    while out.len() < want {
        let h = hash(&[DOMAIN_FS, root, &num_leaves.to_le_bytes(), &counter.to_le_bytes()]);
        let mut word = [0u8; 8];
        word.copy_from_slice(&h[..8]);
        let idx = (u64::from_le_bytes(word) % domain as u64) as usize;
        if seen.insert(idx) {
            out.push(idx);
        }
        counter += 1;
    }
    out.sort_unstable();
    out
}

/// Prove that `model` maps `input` to its forward output, succinctly.
///
/// Returns the proof plus the dequantised output (also embedded in the proof). Panics
/// only on a structurally invalid model (a weight row whose width disagrees with the
/// incoming activation) — a programming error, not attacker input.
pub fn prove(model: &TracedMlp, input: &[f64], config: SpotCheckConfig) -> SpotCheckProof {
    assert_eq!(input.len(), model.input_dim, "input width must match model");
    let shape = model.shape();
    let layout = Layout::new(model.input_dim, &shape);

    // Build the self-consistent-under-quantisation trace into the flat cell vector.
    let mut cells = vec![0i64; layout.total];

    // Input cells, and the dequantised input activations feeding layer 0.
    let mut prev: Vec<f64> = Vec::with_capacity(input.len());
    for (i, &x) in input.iter().enumerate() {
        let qi = q(x);
        cells[layout.input_cell(i)] = qi;
        prev.push(dq(qi));
    }

    for (l, layer) in model.layers.iter().enumerate() {
        assert_eq!(layer.weights.len(), layout.out[l], "layer {l} out width");
        let mut acts = Vec::with_capacity(layout.out[l]);
        for (o, row) in layer.weights.iter().enumerate() {
            assert_eq!(row.len(), layout.fan_in[l], "layer {l} neuron {o} fan-in");
            let mut acc = 0.0;
            for (i, &w) in row.iter().enumerate() {
                let wq = dq(q(w));
                cells[layout.weight_cell(l, o, i)] = q(w);
                acc += wq * prev[i];
            }
            let bq = dq(q(layer.biases[o]));
            cells[layout.bias_cell(l, o)] = q(layer.biases[o]);
            acc += bq;
            if layer.relu && acc < 0.0 {
                acc = 0.0;
            }
            let aq = q(acc);
            cells[layout.act_cell(l, o)] = aq;
            acts.push(dq(aq));
        }
        prev = acts;
    }

    let tree = MerkleTree::build(&cells);
    let root = tree.root();
    let num_leaves = cells.len() as u64;
    let output = prev.clone(); // last layer's dequantised activations

    // Which cells to open: the full input + output boundary, plus everything the
    // Fiat-Shamir-challenged neurons consume.
    let mut needed: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();
    for i in 0..layout.input_dim {
        needed.insert(layout.input_cell(i));
    }
    let last = shape.len() - 1;
    for o in 0..layout.out[last] {
        needed.insert(layout.act_cell(last, o));
    }
    let neurons = layout.computed_neurons();
    let num_queries = config.num_queries.min(neurons.len());
    for &nid in &fiat_shamir(&root, num_leaves, num_queries, neurons.len()) {
        let (l, o) = neurons[nid];
        needed.insert(layout.act_cell(l, o));
        needed.insert(layout.bias_cell(l, o));
        for i in 0..layout.fan_in[l] {
            needed.insert(layout.weight_cell(l, o, i));
            needed.insert(layout.prev_act_cell(l, i));
        }
    }

    let opened = needed
        .into_iter()
        .map(|idx| OpenedCell {
            index: idx as u64,
            value: cells[idx],
            path: tree.open(idx),
        })
        .collect();

    SpotCheckProof {
        root,
        num_leaves,
        input_dim: model.input_dim,
        shape,
        num_queries,
        output,
        cells: opened,
    }
}

impl SpotCheckProof {
    /// Verify succinctly that this proof binds `input` to [`self.output`](Self::output)
    /// as a correct MLP forward pass. Does **not** re-run the network — it checks the
    /// Merkle openings, re-pins the boundary, and recomputes only the Fiat-Shamir
    /// neurons. `false` on any inconsistency (bad path, boundary mismatch, or a
    /// neuron whose committed activation disagrees with its inputs).
    pub fn verify(&self, input: &[f64]) -> bool {
        self.verify_with_min(input, 0)
    }

    /// Like [`verify`](Self::verify) but rejects a proof whose [`num_queries`](Self::num_queries)
    /// is below `min_queries` — a caller's soundness floor. A relying party (e.g. the
    /// settlement facilitator) sets this so a prover can't submit a
    /// boundary-only, zero-spot-check proof and slip a corrupted interior past it.
    pub fn verify_with_min(&self, input: &[f64], min_queries: usize) -> bool {
        if input.len() != self.input_dim || self.shape.is_empty() {
            return false;
        }
        if self.num_queries < min_queries {
            return false;
        }
        let layout = Layout::new(self.input_dim, &self.shape);
        if layout.total as u64 != self.num_leaves {
            return false;
        }

        // Authenticate every opened cell against the root, into a lookup map.
        let mut map = std::collections::HashMap::with_capacity(self.cells.len());
        for c in &self.cells {
            if c.index >= self.num_leaves {
                return false;
            }
            if merkle_root_from(c.index, c.value, &c.path, self.num_leaves) != self.root {
                return false;
            }
            map.insert(c.index as usize, c.value);
        }
        let get = |idx: usize| map.get(&idx).copied();

        // Boundary: input cells must equal the supplied input; output cells must
        // equal the proof's claimed output. Both are fully opened above.
        for (i, &x) in input.iter().enumerate() {
            if get(layout.input_cell(i)) != Some(q(x)) {
                return false;
            }
        }
        let last = self.shape.len() - 1;
        if self.output.len() != layout.out[last] {
            return false;
        }
        for (o, &y) in self.output.iter().enumerate() {
            if get(layout.act_cell(last, o)) != Some(q(y)) {
                return false;
            }
        }

        // Spot-check the Fiat-Shamir neurons: recompute each from its opened inputs.
        let neurons = layout.computed_neurons();
        for &nid in &fiat_shamir(&self.root, self.num_leaves, self.num_queries, neurons.len()) {
            let (l, o) = neurons[nid];
            let bias = match get(layout.bias_cell(l, o)) {
                Some(b) => dq(b),
                None => return false,
            };
            let mut acc = bias;
            for i in 0..layout.fan_in[l] {
                let (w, a) = match (get(layout.weight_cell(l, o, i)), get(layout.prev_act_cell(l, i))) {
                    (Some(w), Some(a)) => (dq(w), dq(a)),
                    _ => return false,
                };
                acc += w * a;
            }
            if self.shape[l].relu && acc < 0.0 {
                acc = 0.0;
            }
            if get(layout.act_cell(l, o)) != Some(q(acc)) {
                return false;
            }
        }
        true
    }

    /// The Fiat-Shamir query count this proof answers (its soundness parameter).
    pub fn queries(&self) -> usize {
        self.num_queries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A small 3→4→2 net: one ReLU hidden layer, one linear head.
    fn net() -> TracedMlp {
        TracedMlp::new(
            3,
            vec![
                DenseLayer::new(
                    vec![
                        vec![0.5, -0.2, 0.1],
                        vec![-0.3, 0.4, 0.2],
                        vec![0.1, 0.1, -0.5],
                        vec![0.2, -0.1, 0.3],
                    ],
                    vec![0.05, -0.05, 0.0, 0.1],
                    true,
                ),
                DenseLayer::new(
                    vec![vec![0.3, -0.2, 0.5, 0.1], vec![-0.1, 0.4, -0.3, 0.2]],
                    vec![0.0, 0.01],
                    false,
                ),
            ],
        )
    }

    fn cfg() -> SpotCheckConfig {
        // Query every neuron so the tests deterministically exercise all constraints.
        SpotCheckConfig { num_queries: 64 }
    }

    #[test]
    fn honest_proof_verifies() {
        let m = net();
        let input = [1.0, -2.0, 0.5];
        let proof = prove(&m, &input, cfg());
        assert!(proof.verify(&input));
    }

    #[test]
    fn proof_output_matches_a_direct_forward_pass() {
        let m = net();
        let input = [0.7, 0.3, -0.4];
        let proof = prove(&m, &input, cfg());
        // Reproduce the committed forward pass exactly (same quantise/dequantise rule).
        let mut prev: Vec<f64> = input.iter().map(|&x| dq(q(x))).collect();
        for layer in &m.layers {
            let mut acts = Vec::new();
            for (o, row) in layer.weights.iter().enumerate() {
                let mut acc = dq(q(layer.biases[o]));
                for (i, &w) in row.iter().enumerate() {
                    acc += dq(q(w)) * prev[i];
                }
                if layer.relu && acc < 0.0 {
                    acc = 0.0;
                }
                acts.push(dq(q(acc)));
            }
            prev = acts;
        }
        assert_eq!(proof.output, prev);
    }

    #[test]
    fn verify_fails_on_a_different_input() {
        let m = net();
        let proof = prove(&m, &[1.0, -2.0, 0.5], cfg());
        assert!(!proof.verify(&[1.0, -2.0, 0.6]));
    }

    #[test]
    fn tampering_the_claimed_output_is_rejected() {
        let m = net();
        let input = [1.0, -2.0, 0.5];
        let mut proof = prove(&m, &input, cfg());
        proof.output[0] += 1.0; // claim a different inference
        assert!(!proof.verify(&input));
    }

    #[test]
    fn tampering_a_committed_cell_breaks_the_merkle_path() {
        let m = net();
        let input = [1.0, -2.0, 0.5];
        let mut proof = prove(&m, &input, cfg());
        // Flip an opened weight value without a valid path → authentication fails.
        if let Some(c) = proof.cells.iter_mut().find(|c| c.value != 0) {
            c.value = c.value.wrapping_add(1);
        }
        assert!(!proof.verify(&input));
    }

    #[test]
    fn forging_a_neuron_consistently_is_caught_by_a_spot_check() {
        // Corrupt an interior activation AND its Merkle leaf so the path still
        // authenticates, but the neuron no longer recomputes from its inputs. A query
        // landing on it must catch the inconsistency.
        let m = net();
        let input = [1.0, -2.0, 0.5];
        let proof = prove(&m, &input, cfg());
        let layout = Layout::new(proof.input_dim, &proof.shape);
        let target = layout.act_cell(0, 0); // first hidden neuron's activation
        // Rebuild the full cell vector, corrupt one activation, recommit, reopen.
        let mut cells = vec![0i64; layout.total];
        for c in &proof.cells {
            cells[c.index as usize] = c.value;
        }
        // Fill any unopened cells by reproving into a scratch tree is overkill; the
        // spot-check only needs the corrupted neuron's neighborhood, all opened at
        // num_queries=64. Corrupt in place and rebuild a tree over these cells.
        cells[target] += 500_000; // +0.5 — inconsistent with its weight row
        let tree = MerkleTree::build(&cells);
        let mut forged = proof.clone();
        forged.root = tree.root();
        for c in forged.cells.iter_mut() {
            c.value = cells[c.index as usize];
            c.path = tree.open(c.index as usize);
        }
        // Output boundary still equals the (unchanged) last-layer cells, input too;
        // but the corrupted hidden neuron fails its recomputation check.
        assert!(!forged.verify(&input));
    }

    #[test]
    fn merkle_open_and_verify_round_trip_including_odd_levels() {
        // 5 leaves forces duplicated-last-node handling at two levels.
        let cells: Vec<i64> = vec![10, 20, 30, 40, 50];
        let tree = MerkleTree::build(&cells);
        let root = tree.root();
        for (i, &v) in cells.iter().enumerate() {
            let path = tree.open(i);
            assert_eq!(merkle_root_from(i as u64, v, &path, cells.len() as u64), root);
        }
    }
}
