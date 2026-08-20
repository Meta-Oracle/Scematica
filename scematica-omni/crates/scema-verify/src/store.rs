//! Where decision records live on disk.
//!
//! One JSON file per record under `<root>/decisions/`, named by id. Not a JSONL stream,
//! which is the convention the bot uses for trades: a trade event is small and append-only,
//! whereas a record embeds a whole world state and is looked up by id far more often than
//! it is scanned. A directory of files is also the format an operator can hand to somebody
//! — `scema verify` on a single file is the point.
//!
//! Writes go to `<file>.tmp` and then rename, the same convention as every state file in
//! the bot workspace, so a reader never sees a half-written record.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};

use crate::record::DecisionRecord;

/// A directory of decision records.
pub struct RecordStore {
    root: PathBuf,
}

impl RecordStore {
    /// Records live under `<root>/decisions/`. The directory is created on first write,
    /// not here — constructing a store must not have a side effect on a machine where the
    /// operator only meant to read.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        RecordStore { root: root.into() }
    }

    fn dir(&self) -> PathBuf {
        self.root.join("decisions")
    }

    pub fn path_for(&self, id: &str) -> PathBuf {
        self.dir().join(format!("{id}.json"))
    }

    pub fn save(&self, record: &DecisionRecord) -> Result<PathBuf> {
        let dir = self.dir();
        fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        let path = self.path_for(&record.id);
        let tmp = path.with_extension("json.tmp");
        let body = serde_json::to_string_pretty(record)?;
        fs::write(&tmp, body).with_context(|| format!("writing {}", tmp.display()))?;
        fs::rename(&tmp, &path).with_context(|| format!("renaming into {}", path.display()))?;
        Ok(path)
    }

    pub fn load_path(path: &Path) -> Result<DecisionRecord> {
        let body = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_str(&body).with_context(|| format!("parsing {}", path.display()))
    }

    /// Load by id or by any unique prefix of one.
    ///
    /// An ambiguous prefix is an error naming the candidates, never a silent pick of the
    /// first match — the whole value of an id is that it refers to one record.
    pub fn load(&self, id_or_prefix: &str) -> Result<DecisionRecord> {
        let exact = self.path_for(id_or_prefix);
        if exact.exists() {
            return Self::load_path(&exact);
        }
        let matches: Vec<String> = self
            .ids()?
            .into_iter()
            .filter(|id| id.starts_with(id_or_prefix))
            .collect();
        match matches.len() {
            0 => Err(anyhow!(
                "no decision record matching `{id_or_prefix}` under {}",
                self.dir().display()
            )),
            1 => Self::load_path(&self.path_for(&matches[0])),
            _ => Err(anyhow!(
                "`{id_or_prefix}` matches {} records: {}",
                matches.len(),
                matches.join(", ")
            )),
        }
    }

    /// Every record id, newest first by file modification time.
    ///
    /// Ordered by mtime rather than by the `at` field inside each record so that a listing
    /// still works when a record is unreadable — an id that cannot be parsed should still
    /// appear in `scema explain --list`, since "there is a broken record here" is exactly
    /// what the operator needs to know.
    pub fn ids(&self) -> Result<Vec<String>> {
        let dir = self.dir();
        if !dir.exists() {
            return Ok(vec![]);
        }
        let mut entries: Vec<(std::time::SystemTime, String)> = Vec::new();
        for e in fs::read_dir(&dir).with_context(|| format!("listing {}", dir.display()))? {
            let e = e?;
            let path = e.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let mtime = e.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
            entries.push((mtime, stem.to_string()));
        }
        entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        Ok(entries.into_iter().map(|(_, id)| id).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "scema-omni-store-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn dummy(id: &str) -> DecisionRecord {
        let mut r = crate::record::tests_support::sample();
        r.id = id.to_string();
        r
    }

    #[test]
    fn a_saved_record_round_trips() {
        let dir = tmpdir();
        let store = RecordStore::new(&dir);
        let r = dummy("aabbccdd");
        store.save(&r).unwrap();
        assert_eq!(store.load("aabbccdd").unwrap().id, "aabbccdd");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_unique_prefix_resolves_and_an_ambiguous_one_errors() {
        let dir = tmpdir();
        let store = RecordStore::new(&dir);
        store.save(&dummy("aa000000")).unwrap();
        store.save(&dummy("aa111111")).unwrap();
        store.save(&dummy("bb000000")).unwrap();

        assert_eq!(store.load("bb").unwrap().id, "bb000000");
        let err = store.load("aa").unwrap_err().to_string();
        assert!(err.contains("matches 2 records"), "got {err}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn listing_an_absent_store_is_empty_not_an_error() {
        // A machine that has never run the agent must not look like a broken one.
        let store = RecordStore::new(tmpdir().join("never-created"));
        assert!(store.ids().unwrap().is_empty());
    }
}
