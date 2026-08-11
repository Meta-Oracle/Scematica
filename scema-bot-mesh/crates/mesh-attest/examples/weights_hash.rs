//! Commit to the live DQ* checkpoint:
//!   cargo run -p mesh-attest --example weights_hash -- <path>
//!
//! keccak256 over the checkpoint bytes, so the identity registered on chain corresponds to
//! a real artifact rather than a placeholder. Anyone given the same file reproduces it.
use tiny_keccak::{Hasher, Keccak};

fn main() {
    let path = std::env::args().nth(1).expect("usage: weights_hash <checkpoint.json>");
    let bytes = std::fs::read(&path).expect("read checkpoint");
    let mut k = Keccak::v256();
    k.update(&bytes);
    let mut out = [0u8; 32];
    k.finalize(&mut out);
    println!("file   : {path}");
    println!("bytes  : {}", bytes.len());
    print!("keccak : 0x");
    for b in out { print!("{b:02x}"); }
    println!();
}
