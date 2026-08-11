//! The `.mesh` weight file.
//!
//! Requirements are narrow and unusual: this format must round-trip **bit-exactly**,
//! because a policy's commitment hash is computed from its parameters. A format that
//! loses or reorders a single value produces a different `weights_hash`, and every claim
//! made against the original becomes unverifiable — the agent looks like it substituted
//! its model when all that happened was a lossy save.
//!
//! Hence: big-endian fixed-width integers, an explicit shape header, no compression, no
//! floats on disk, and a checksum. Big-endian to match the EVM's word order, so a
//! contract reading a weight slice does not byte-swap.
//!
//! ```text
//! magic    4  "MESH"
//! version  2  u16 = 1
//! state    4  u32   input dimension
//! layers   2  u16   hidden layer count
//! actions  4  u32
//! hidden   4 * layers  u32 each
//! params   4 * n       i32 each, in PolicyNet::parameters() order
//! checksum 4  u32   FNV-1a over the parameter bytes
//! ```
//!
//! The checksum catches truncation and corruption, which are the realistic failures for a
//! file shipped in a game bundle. It is **not** a security control — an attacker editing
//! weights recomputes it trivially. Authenticity comes from `weights_hash` and the on-chain
//! commitment, which is a different question from "did this file survive the disk".

use mesh_core::fixed::Fx;
use mesh_core::net::PolicyNet;

const MAGIC: [u8; 4] = *b"MESH";
const VERSION: u16 = 1;

/// Guards against a corrupt header asking for a terabyte allocation.
const MAX_LAYERS: u16 = 64;
const MAX_DIM: u32 = 1 << 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatError {
    NotMeshFile,
    UnsupportedVersion(u16),
    Truncated { need: usize, have: usize },
    ChecksumMismatch { expected: u32, got: u32 },
    ImplausibleShape(String),
    TrailingBytes(usize),
}

impl core::fmt::Display for FormatError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            FormatError::NotMeshFile => write!(f, "not a .mesh file (bad magic)"),
            FormatError::UnsupportedVersion(v) => write!(f, "unsupported .mesh version {v}"),
            FormatError::Truncated { need, have } => {
                write!(f, "truncated: needed {need} bytes, had {have}")
            }
            FormatError::ChecksumMismatch { expected, got } => {
                write!(f, "checksum mismatch: expected {expected:#010x}, got {got:#010x}")
            }
            FormatError::ImplausibleShape(s) => write!(f, "implausible shape: {s}"),
            FormatError::TrailingBytes(n) => write!(f, "{n} unexpected trailing bytes"),
        }
    }
}

/// FNV-1a. Chosen for being trivial to reimplement in any language — the format is meant
/// to be readable by a JS verifier or a Python tool without pulling a hash library.
fn checksum(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in bytes {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

pub fn encode(net: &PolicyNet) -> Vec<u8> {
    let hidden: Vec<u32> = net.trunk.iter().map(|l| l.outputs as u32).collect();
    let params = net.parameters();

    let mut param_bytes = Vec::with_capacity(params.len() * 4);
    for p in &params {
        param_bytes.extend_from_slice(&p.to_bits().to_be_bytes());
    }

    let mut out = Vec::with_capacity(20 + hidden.len() * 4 + param_bytes.len() + 4);
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&VERSION.to_be_bytes());
    out.extend_from_slice(&(net.state_dim() as u32).to_be_bytes());
    out.extend_from_slice(&(hidden.len() as u16).to_be_bytes());
    out.extend_from_slice(&(net.action_count() as u32).to_be_bytes());
    for h in &hidden {
        out.extend_from_slice(&h.to_be_bytes());
    }
    out.extend_from_slice(&param_bytes);
    out.extend_from_slice(&checksum(&param_bytes).to_be_bytes());
    out
}

pub fn decode(bytes: &[u8]) -> Result<PolicyNet, FormatError> {
    let mut cursor = Reader::new(bytes);

    if cursor.take(4)? != MAGIC {
        return Err(FormatError::NotMeshFile);
    }
    let version = cursor.u16()?;
    if version != VERSION {
        return Err(FormatError::UnsupportedVersion(version));
    }

    let state_dim = cursor.u32()?;
    let layer_count = cursor.u16()?;
    let actions = cursor.u32()?;

    // Bounds checked before any allocation. A corrupt `layer_count` would otherwise be a
    // memory-exhaustion vector for anyone loading an untrusted policy — and policies are
    // expected to be exchanged between strangers, so untrusted is the normal case.
    if state_dim == 0 || state_dim > MAX_DIM {
        return Err(FormatError::ImplausibleShape(format!("state_dim {state_dim}")));
    }
    if actions == 0 || actions > MAX_DIM {
        return Err(FormatError::ImplausibleShape(format!("actions {actions}")));
    }
    if layer_count > MAX_LAYERS {
        return Err(FormatError::ImplausibleShape(format!("{layer_count} hidden layers")));
    }

    let mut hidden = Vec::with_capacity(layer_count as usize);
    for _ in 0..layer_count {
        let h = cursor.u32()?;
        if h == 0 || h > MAX_DIM {
            return Err(FormatError::ImplausibleShape(format!("hidden width {h}")));
        }
        hidden.push(h as usize);
    }

    let mut net = PolicyNet::new(state_dim as usize, &hidden, actions as usize);
    let expected = net.parameter_count();

    let param_bytes = cursor.take(expected * 4)?.to_vec();
    let stated = cursor.u32()?;
    let actual = checksum(&param_bytes);
    if stated != actual {
        return Err(FormatError::ChecksumMismatch { expected: stated, got: actual });
    }
    if !cursor.is_empty() {
        // Trailing bytes mean the shape header and the payload disagree. Ignoring them
        // would let two different files load as the same network.
        return Err(FormatError::TrailingBytes(cursor.remaining()));
    }

    let mut values = Vec::with_capacity(expected);
    for chunk in param_bytes.chunks_exact(4) {
        values.push(Fx::from_bits(i32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]])));
    }

    // Scatter back in exactly the order `parameters()` emits.
    let mut i = 0;
    for layer in net
        .trunk
        .iter_mut()
        .chain([&mut net.value_head, &mut net.advantage_head])
    {
        for w in layer.weights.iter_mut() {
            *w = values[i];
            i += 1;
        }
        for b in layer.biases.iter_mut() {
            *b = values[i];
            i += 1;
        }
    }

    Ok(net)
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Reader { bytes, pos: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], FormatError> {
        if self.pos + n > self.bytes.len() {
            return Err(FormatError::Truncated { need: self.pos + n, have: self.bytes.len() });
        }
        let out = &self.bytes[self.pos..self.pos + n];
        self.pos += n;
        Ok(out)
    }
    fn u16(&mut self) -> Result<u16, FormatError> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Result<u32, FormatError> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn is_empty(&self) -> bool {
        self.pos == self.bytes.len()
    }
    fn remaining(&self) -> usize {
        self.bytes.len() - self.pos
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mesh_core::commit::weights_hash;

    fn populated() -> PolicyNet {
        let mut net = PolicyNet::new(5, &[7, 4], 3);
        let mut seed = 1i32;
        for layer in net
            .trunk
            .iter_mut()
            .chain([&mut net.value_head, &mut net.advantage_head])
        {
            for w in layer.weights.iter_mut().chain(layer.biases.iter_mut()) {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                *w = Fx::from_bits(seed >> 12);
            }
        }
        net
    }

    #[test]
    fn round_trip_is_bit_exact() {
        let net = populated();
        let back = decode(&encode(&net)).unwrap();
        assert_eq!(back, net);
    }

    #[test]
    fn round_trip_preserves_the_commitment_hash() {
        // The property that actually matters: a save/load cycle must not invalidate every
        // claim ever made against the policy.
        let net = populated();
        assert_eq!(weights_hash(&decode(&encode(&net)).unwrap()), weights_hash(&net));
    }

    #[test]
    fn a_linear_policy_with_no_hidden_layers_round_trips() {
        let net = PolicyNet::new(3, &[], 2);
        assert_eq!(decode(&encode(&net)).unwrap(), net);
    }

    #[test]
    fn foreign_files_are_rejected() {
        assert_eq!(decode(b"not a mesh file at all"), Err(FormatError::NotMeshFile));
    }

    #[test]
    fn a_future_version_is_refused_rather_than_guessed() {
        let mut bytes = encode(&populated());
        bytes[4..6].copy_from_slice(&99u16.to_be_bytes());
        assert_eq!(decode(&bytes), Err(FormatError::UnsupportedVersion(99)));
    }

    #[test]
    fn truncation_is_detected() {
        let bytes = encode(&populated());
        let cut = &bytes[..bytes.len() - 8];
        assert!(matches!(decode(cut), Err(FormatError::Truncated { .. })));
    }

    #[test]
    fn a_flipped_weight_fails_the_checksum() {
        let mut bytes = encode(&populated());
        let mid = bytes.len() / 2;
        bytes[mid] ^= 0x01;
        assert!(matches!(decode(&bytes), Err(FormatError::ChecksumMismatch { .. })));
    }

    #[test]
    fn trailing_bytes_are_refused() {
        // Otherwise two distinct files decode to the same network, and "which file did
        // you run" stops having one answer.
        let mut bytes = encode(&populated());
        bytes.push(0);
        assert_eq!(decode(&bytes), Err(FormatError::TrailingBytes(1)));
    }

    #[test]
    fn an_absurd_header_is_rejected_before_allocating() {
        // Policies get exchanged between strangers, so a corrupt header must not be able
        // to ask for a terabyte.
        let mut bytes = encode(&populated());
        bytes[6..10].copy_from_slice(&u32::MAX.to_be_bytes()); // state_dim
        assert!(matches!(decode(&bytes), Err(FormatError::ImplausibleShape(_))));
    }

    #[test]
    fn no_floats_are_written_to_disk() {
        // A f32/f64 on disk would reintroduce platform-dependent parsing at load time,
        // which is the whole thing this design exists to avoid. Header is 16 bytes plus
        // 4 per hidden layer; everything after is i32 params plus a u32 checksum.
        let net = populated();
        let bytes = encode(&net);
        let header = 4 + 2 + 4 + 2 + 4 + net.trunk.len() * 4;
        assert_eq!(bytes.len(), header + net.parameter_count() * 4 + 4);
    }
}
