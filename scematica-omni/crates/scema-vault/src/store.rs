//! The corpus on disk, addressed by commitment.
//!
//! One file per world, named `<commitment>.json`. That is the whole scheme, and it is chosen
//! so the lookup key is the thing the caller already proved an entitlement to — there is no
//! index to fall out of sync and no id to map between.
//!
//! ## The path is built, never joined
//!
//! A commitment is validated as 64 lowercase hex characters *before* it reaches here, and
//! validated again here rather than trusted. `../` in a URL segment is the oldest bug in file
//! serving, and defence at exactly one layer is how it comes back the first time somebody
//! adds a second caller.

use std::path::{Path, PathBuf};

use scema_entitlement::is_digest;

pub struct RecordStore {
    root: PathBuf,
}

impl RecordStore {
    pub fn new(root: impl AsRef<Path>) -> Self {
        RecordStore { root: root.as_ref().to_path_buf() }
    }

    /// The path for a commitment, or `None` if it is not one.
    ///
    /// Re-validates rather than trusting the caller. The check is cheap and the failure it
    /// prevents is reading an arbitrary file off the operator's disk.
    fn path(&self, commitment: &str) -> Option<PathBuf> {
        if !is_digest(commitment) {
            return None;
        }
        Some(self.root.join(format!("{commitment}.json")))
    }

    pub fn has(&self, commitment: &str) -> bool {
        self.path(commitment).is_some_and(|p| p.is_file())
    }

    /// The record text, verbatim.
    ///
    /// **Verbatim matters.** The bytes on disk are what the commitment was taken over, and
    /// re-serialising through `serde_json` would collapse Rust's `0.0` to `0`, moving it from
    /// the FLOAT tag to the INTEGER tag in the canonical encoding and changing the digest.
    /// A holder would then fetch a record that fails its own verification — the single worst
    /// failure available here, because it teaches people the verifier is broken.
    pub fn read(&self, commitment: &str) -> Option<String> {
        std::fs::read_to_string(self.path(commitment)?).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn scratch() -> PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!(
            "scema-vault-store-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn a_stored_record_comes_back_byte_for_byte() {
        // The bytes are what the commitment was taken over. Re-serialising would collapse
        // `0.0` to `0` and the record would fail its own verification.
        let root = scratch();
        let text = "{\"a\": 0.0,\n  \"b\": [1,2]}\n";
        std::fs::write(root.join(format!("{A}.json")), text).unwrap();
        assert_eq!(RecordStore::new(&root).read(A).as_deref(), Some(text));
    }

    #[test]
    fn traversal_is_refused_at_this_layer_too() {
        // Validated once in the router and again here. Defence at exactly one layer is how
        // it comes back the first time somebody adds a second caller.
        let root = scratch();
        let s = RecordStore::new(&root);
        for bad in ["../../etc/passwd", "..", "a/../b", &A.to_uppercase(), "short", ""] {
            assert!(s.path(bad).is_none(), "{bad} produced a path");
            assert!(!s.has(bad));
            assert!(s.read(bad).is_none());
        }
    }

    #[test]
    fn a_missing_record_is_none_rather_than_empty() {
        assert!(RecordStore::new(scratch()).read(A).is_none());
    }
}
