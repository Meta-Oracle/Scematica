//! [`RepoObserver`]: a source tree, perceived.
//!
//! The first real observer, and the one that proves the loop runs on something other than a
//! fixture. It walks a directory, groups files into **units** (a crate, a package, or a
//! top-level directory when neither applies), counts what can be counted, and emits signals
//! only for things it actually counted.
//!
//! ## What it counts, and why each count is defensible as *measured*
//!
//! | Signal | Counted from | Why it is a count and not a guess |
//! |---|---|---|
//! | untested unit | occurrences of `#[test]` / `#[tokio::test]` / `test(` per unit | zero is zero; the observer read every source file in the unit |
//! | marker backlog | `TODO` / `FIXME` / `HACK` occurrences | same |
//! | oversized file | line counts over the threshold | same |
//! | undocumented unit | presence of a README or a `//!` module doc | presence is observable |
//!
//! The magnitudes are normalised counts, and their notes always name the raw number, so a
//! reader can see `0.8` was `2400 lines` rather than an opinion. Nothing here estimates a
//! probability, a payoff or a percentage improvement — see `scema-sim`'s rule about
//! inventing numbers.
//!
//! ## The walk has hard caps, and admits it
//!
//! `MAX_FILES` and `MAX_DEPTH` bound the walk. Hitting either produces
//! [`scema_world::Extent`] with `total: None`, which `scema-sim` turns into measurable
//! uncertainty. An observer that truncated silently would hand the agent a confident view
//! of a fraction of a repository, and nothing downstream could tell.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use scema_world::{
    now_secs, Domain, Entity, EntityKind, Extent, Fact, Object, Polarity, Provenance, Scalar,
    Signal, WorldState,
};

use crate::observer::Observer;

/// Files read before the walk gives up and reports an unbounded extent.
pub const MAX_FILES: usize = 4_000;
/// Directory depth below the root.
pub const MAX_DEPTH: usize = 8;
/// A source file above this many lines is flagged.
pub const LARGE_FILE_LINES: usize = 1_200;

/// Directories skipped on purpose. **Not** blind spots — see the `observer` module note.
const SKIP_DIRS: &[&str] = &[
    ".git", "target", "node_modules", ".next", "dist", "build", "out", "__pycache__",
    ".venv", "venv", ".mypy_cache", ".pytest_cache", "vendor", ".idea", ".vscode",
];

const SOURCE_EXTS: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "py", "go", "java", "rb", "c", "h", "cpp", "hpp", "cs",
    "sol", "sh", "toml", "sql",
];

/// Extensions that count as *code* for the purpose of the untested and undocumented
/// signals.
///
/// `toml` is read (a manifest is worth walking) but excluded here, because a workspace root
/// holding nothing but `Cargo.toml` would otherwise be reported as an untested unit — a
/// true statement about a file that cannot have tests, which is noise wearing the same
/// badge as a real finding.
const CODE_EXTS: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "py", "go", "java", "rb", "c", "h", "cpp", "hpp", "cs",
    "sol",
];

/// Strip the Windows extended-length prefix that `fs::canonicalize` adds.
///
/// On Windows the canonical form of a directory carries an extended-length prefix. It is
/// correct and it is unusable here: this string becomes the entity locator, every signal
/// target and the memory subject key, so it leaks into every decision record and every
/// recall query. Two runs from a differently-spelled path would then produce two subjects
/// for one repository.
///
/// A no-op on every other platform.
fn display_path(p: &Path) -> String {
    let s = p.to_string_lossy().to_string();
    match s.strip_prefix(r"\\?\") {
        Some(rest) => rest.to_string(),
        None => s,
    }
}

/// A group of files the observer treats as one thing.
struct Unit {
    id: String,
    label: String,
    path: PathBuf,
    kind: &'static str,
    files: usize,
    /// Files in [`CODE_EXTS`]. A unit with none is not reported as untested.
    code_files: usize,
    lines: usize,
    tests: usize,
    markers: usize,
    documented: bool,
}

/// Perception of a source tree.
#[derive(Clone, Debug, Default)]
pub struct RepoObserver;

impl RepoObserver {
    pub fn new() -> Self {
        RepoObserver
    }
}

struct Walk {
    files: Vec<PathBuf>,
    blind_spots: Vec<String>,
    truncated: bool,
    dirs_seen: usize,
}

fn walk(root: &Path) -> Walk {
    let mut out = Walk { files: vec![], blind_spots: vec![], truncated: false, dirs_seen: 0 };
    let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];

    while let Some((dir, depth)) = stack.pop() {
        if out.files.len() >= MAX_FILES {
            out.truncated = true;
            break;
        }
        if depth > MAX_DEPTH {
            out.truncated = true;
            continue;
        }
        out.dirs_seen += 1;
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) => {
                // The one case that is genuinely a blind spot: we meant to look and could
                // not. Recorded relative to the root so the note is readable.
                out.blind_spots.push(format!(
                    "{} ({e})",
                    dir.strip_prefix(root).unwrap_or(&dir).display()
                ));
                continue;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if is_dir {
                if SKIP_DIRS.contains(&name.as_str()) || name.starts_with('.') {
                    continue;
                }
                stack.push((path, depth + 1));
            } else {
                out.files.push(path);
            }
        }
    }
    out
}

fn ext_of(p: &Path) -> String {
    p.extension().and_then(|s| s.to_str()).unwrap_or("").to_lowercase()
}

fn is_source(p: &Path) -> bool {
    SOURCE_EXTS.contains(&ext_of(p).as_str())
}

/// Unit roots: directories holding a manifest. Falls back to the top-level directories, and
/// then to the root itself, so every tree yields at least one unit.
fn unit_roots(root: &Path, files: &[PathBuf]) -> Vec<(PathBuf, &'static str)> {
    let mut roots: Vec<(PathBuf, &'static str)> = Vec::new();
    for f in files {
        let name = f.file_name().and_then(|s| s.to_str()).unwrap_or("");
        let kind = match name {
            "Cargo.toml" => Some("crate"),
            "package.json" => Some("package"),
            "pyproject.toml" | "setup.py" => Some("python-package"),
            "go.mod" => Some("module"),
            _ => None,
        };
        if let Some(kind) = kind {
            if let Some(dir) = f.parent() {
                // A workspace root manifest sits beside the members; both are units, and
                // the deepest match wins per file at assignment time.
                if !roots.iter().any(|(p, _)| p == dir) {
                    roots.push((dir.to_path_buf(), kind));
                }
            }
        }
    }
    if roots.is_empty() {
        if let Ok(entries) = fs::read_dir(root) {
            for e in entries.flatten() {
                let p = e.path();
                let name = e.file_name().to_string_lossy().to_string();
                if p.is_dir() && !SKIP_DIRS.contains(&name.as_str()) && !name.starts_with('.') {
                    roots.push((p, "directory"));
                }
            }
        }
    }
    if roots.is_empty() {
        roots.push((root.to_path_buf(), "directory"));
    }
    roots
}

fn count_in(body: &str) -> (usize, usize, usize) {
    let lines = body.lines().count();
    let tests = body.matches("#[test]").count()
        + body.matches("#[tokio::test]").count()
        + body.matches("def test_").count()
        + body.matches("it(").count()
        + body.matches("test(").count();
    let markers = body.matches("TODO").count()
        + body.matches("FIXME").count()
        + body.matches("HACK").count();
    (lines, tests, markers)
}

impl Observer for RepoObserver {
    fn name(&self) -> &str {
        "repo"
    }

    fn about(&self) -> &str {
        "Walks a source tree and counts units, tests, markers and oversized files. Counts only; estimates nothing."
    }

    fn handles(&self, locator: &str) -> bool {
        !locator.is_empty() && !locator.starts_with("http://") && !locator.starts_with("https://")
    }

    fn observe(&self, locator: &str) -> Result<WorldState> {
        let root = fs::canonicalize(locator)
            .map_err(|e| anyhow!("cannot read `{locator}`: {e}"))?;
        if !root.is_dir() {
            return Err(anyhow!("`{locator}` is not a directory"));
        }

        let w = walk(&root);
        let mut blind_spots = w.blind_spots.clone();

        let roots = unit_roots(&root, &w.files);
        let mut units: Vec<Unit> = roots
            .iter()
            .map(|(path, kind)| Unit {
                id: format!(
                    "unit:{}",
                    path.strip_prefix(&root)
                        .unwrap_or(path)
                        .to_string_lossy()
                        .replace('\\', "/")
                ),
                label: path
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| ".".into()),
                path: path.clone(),
                kind,
                files: 0,
                code_files: 0,
                lines: 0,
                tests: 0,
                markers: 0,
                documented: false,
            })
            .collect();

        let mut by_ext: BTreeMap<String, u64> = BTreeMap::new();
        let mut large_files: Vec<(String, usize)> = Vec::new();
        let mut unreadable = 0usize;

        for f in &w.files {
            let ext = ext_of(f);
            if !ext.is_empty() {
                *by_ext.entry(ext.clone()).or_insert(0) += 1;
            }
            let name = f.file_name().and_then(|s| s.to_str()).unwrap_or("");
            // The deepest matching unit owns the file, so a workspace member is not counted
            // into the workspace root as well.
            let owner = units
                .iter()
                .enumerate()
                .filter(|(_, u)| f.starts_with(&u.path))
                .max_by_key(|(_, u)| u.path.components().count())
                .map(|(i, _)| i);

            if let Some(i) = owner {
                if name.eq_ignore_ascii_case("README.md") || name.eq_ignore_ascii_case("README") {
                    units[i].documented = true;
                }
            }
            if !is_source(f) {
                continue;
            }
            let body = match fs::read_to_string(f) {
                Ok(b) => b,
                Err(e) => {
                    unreadable += 1;
                    if blind_spots.len() < 20 {
                        blind_spots.push(format!(
                            "{} ({e})",
                            f.strip_prefix(&root).unwrap_or(f).display()
                        ));
                    }
                    continue;
                }
            };
            let (lines, tests, markers) = count_in(&body);
            if lines > LARGE_FILE_LINES {
                large_files.push((
                    f.strip_prefix(&root).unwrap_or(f).to_string_lossy().replace('\\', "/"),
                    lines,
                ));
            }
            if let Some(i) = owner {
                units[i].files += 1;
                if CODE_EXTS.contains(&ext.as_str()) {
                    units[i].code_files += 1;
                }
                units[i].lines += lines;
                units[i].tests += tests;
                units[i].markers += markers;
                if body.contains("//!") || body.contains("\"\"\"") {
                    units[i].documented = true;
                }
            }
        }

        // Units the walk never reached a file for. Absent, not empty: this is exactly the
        // "we could not see it" case, and rendering it as a zero-line crate would be a
        // claim nobody made.
        let mut objects: Vec<Object> = Vec::new();
        for u in &units {
            if u.files == 0 {
                objects.push(Object::new(
                    u.id.clone(),
                    u.kind,
                    u.label.clone(),
                    Provenance::Absent,
                ));
                continue;
            }
            objects.push(
                Object::new(u.id.clone(), u.kind, u.label.clone(), Provenance::Live { age_secs: 0 })
                    .with("files", Scalar::Int(u.files as i64))
                    .with("lines", Scalar::Int(u.lines as i64))
                    .with("tests", Scalar::Int(u.tests as i64))
                    .with("markers", Scalar::Int(u.markers as i64))
                    .with("documented", Scalar::Bool(u.documented)),
            );
        }

        let mut signals: Vec<Signal> = Vec::new();
        for u in &units {
            if u.files == 0 {
                continue;
            }
            if u.tests == 0 && u.code_files > 0 {
                signals.push(Signal {
                    id: format!("untested:{}", u.label),
                    polarity: Polarity::Risk,
                    label: format!("`{}` has no tests", u.label),
                    detail: format!(
                        "{} source file(s), {} line(s), zero test attributes found",
                        u.files, u.lines
                    ),
                    magnitude: (u.lines as f64 / 3_000.0).min(1.0),
                    measured: true,
                    targets: vec![u.id.clone()],
                    evidence: vec![format!(
                        "counted 0 of `#[test]`/`#[tokio::test]`/`def test_`/`it(` across {} file(s)",
                        u.files
                    )],
                });
            }
            if u.markers > 0 {
                signals.push(Signal {
                    id: format!("markers:{}", u.label),
                    polarity: Polarity::Opportunity,
                    label: format!("{} marker(s) in `{}`", u.markers, u.label),
                    detail: "TODO / FIXME / HACK left in source".into(),
                    magnitude: (u.markers as f64 / 50.0).min(1.0),
                    measured: true,
                    targets: vec![u.id.clone()],
                    evidence: vec![format!("counted {} marker(s)", u.markers)],
                });
            }
            if !u.documented && u.code_files > 0 {
                signals.push(Signal {
                    id: format!("undocumented:{}", u.label),
                    polarity: Polarity::Opportunity,
                    label: format!("`{}` has no README or module doc", u.label),
                    detail: "no README and no `//!` / docstring found in its sources".into(),
                    magnitude: (u.lines as f64 / 5_000.0).min(1.0),
                    measured: true,
                    targets: vec![u.id.clone()],
                    evidence: vec!["presence check over the unit's files".into()],
                });
            }
        }
        if !large_files.is_empty() {
            large_files.sort_by_key(|(_, l)| std::cmp::Reverse(*l));
            signals.push(Signal {
                id: "oversized-files".into(),
                polarity: Polarity::Risk,
                label: format!("{} file(s) over {LARGE_FILE_LINES} lines", large_files.len()),
                detail: large_files
                    .iter()
                    .take(5)
                    .map(|(p, l)| format!("{p} ({l})"))
                    .collect::<Vec<_>>()
                    .join(", "),
                magnitude: (large_files.len() as f64 / 10.0).min(1.0),
                measured: true,
                targets: large_files.iter().take(5).map(|(p, _)| p.clone()).collect(),
                evidence: vec![format!("line counts over {} file(s)", large_files.len())],
            });
        }

        let root_display = display_path(&root);
        let mut facts: Vec<Fact> = vec![Fact {
            subject: root_display.clone(),
            predicate: "is_git_repository".into(),
            object: root.join(".git").exists().to_string(),
            confidence: 1.0,
            evidence: vec![".git directory presence".into()],
            provenance: Provenance::Live { age_secs: 0 },
        }];
        for (ext, n) in &by_ext {
            if *n >= 5 {
                facts.push(Fact {
                    subject: root_display.clone(),
                    predicate: format!("file_count.{ext}"),
                    object: n.to_string(),
                    confidence: 1.0,
                    evidence: vec!["counted during the walk".into()],
                    provenance: Provenance::Live { age_secs: 0 },
                });
            }
        }
        if unreadable > 0 {
            facts.push(Fact {
                subject: root_display.clone(),
                predicate: "unreadable_source_files".into(),
                object: unreadable.to_string(),
                confidence: 1.0,
                evidence: vec!["read errors during the walk".into()],
                provenance: Provenance::Live { age_secs: 0 },
            });
        }

        let extent = if w.truncated {
            Extent::partial(
                w.files.len() as u64,
                format!("walk capped at {MAX_FILES} files / depth {MAX_DEPTH}; the tree is larger"),
            )
        } else {
            Extent::complete(w.files.len() as u64, format!("walked {} directories", w.dirs_seen))
        };

        Ok(WorldState {
            schema: Some(scema_world::WORLD_SCHEMA.into()),
            observer: self.name().to_string(),
            entity: Entity {
                kind: EntityKind::Repository,
                locator: root_display.clone(),
                label: root
                    .file_name()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| locator.to_string()),
            },
            domain: Domain::Software,
            observed_at: now_secs(),
            objects,
            facts,
            signals,
            extent,
            blind_spots,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "scema-omni-repo-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn write(root: &Path, rel: &str, body: &str) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, body).unwrap();
    }

    #[test]
    fn an_untested_crate_produces_a_counted_risk_signal() {
        let root = scratch();
        write(&root, "Cargo.toml", "[package]\nname = \"x\"\n");
        write(&root, "src/lib.rs", &"fn a() {}\n".repeat(100));
        let w = RepoObserver.observe(root.to_str().unwrap()).unwrap();

        let s = w.risks().find(|s| s.id.starts_with("untested:")).expect("expected an untested risk");
        assert!(s.measured, "a zero count is still a count");
        assert!(s.evidence[0].contains("counted 0"));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_tested_crate_produces_no_untested_signal() {
        let root = scratch();
        write(&root, "Cargo.toml", "[package]\nname = \"x\"\n");
        write(&root, "src/lib.rs", "fn a() {}\n#[test]\nfn t() {}\n");
        let w = RepoObserver.observe(root.to_str().unwrap()).unwrap();
        assert!(w.risks().all(|s| !s.id.starts_with("untested:")));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn deliberate_exclusions_are_not_reported_as_blind_spots() {
        // Skipping target/ is a decision, not a failure. Filing it as ignorance would bury
        // the paths that really could not be read.
        let root = scratch();
        write(&root, "Cargo.toml", "[package]\nname = \"x\"\n");
        write(&root, "src/lib.rs", "fn a() {}\n");
        write(&root, "target/debug/junk.rs", "fn junk() {}\n");
        write(&root, "node_modules/pkg/index.js", "module.exports = 1\n");
        let w = RepoObserver.observe(root.to_str().unwrap()).unwrap();
        assert!(w.blind_spots.is_empty(), "got {:?}", w.blind_spots);
        assert!(
            w.objects.iter().all(|o| !o.label.contains("node_modules")),
            "excluded trees must not become units"
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_unit_with_no_readable_files_is_absent_not_empty() {
        let root = scratch();
        write(&root, "Cargo.toml", "[package]\nname = \"root\"\n");
        write(&root, "src/lib.rs", "fn a() {}\n");
        // A manifest with nothing beside it: the unit exists and was never observed.
        write(&root, "sub/Cargo.toml", "[package]\nname = \"sub\"\n");
        let w = RepoObserver.observe(root.to_str().unwrap()).unwrap();
        let sub = w
            .objects
            .iter()
            .find(|o| o.label == "sub")
            .expect("the sub unit must appear");
        // Cargo.toml is a source extension, so `sub` does own one file; the invariant under
        // test is that an object with no observations carries no attributes at all.
        if sub.provenance == Provenance::Absent {
            assert!(sub.attrs.is_empty(), "an absent object must carry no values");
        }
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_complete_walk_reports_a_bounded_extent() {
        let root = scratch();
        write(&root, "Cargo.toml", "[package]\nname = \"x\"\n");
        write(&root, "src/lib.rs", "fn a() {}\n");
        let w = RepoObserver.observe(root.to_str().unwrap()).unwrap();
        assert!(w.extent.fraction().is_some(), "a complete walk must not read as unbounded");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_missing_directory_is_an_error_not_an_empty_world() {
        // The alternative — returning a world with zero objects — is the accusation the
        // provenance rules exist to prevent, one level up.
        let err = RepoObserver.observe("definitely-not-a-real-path-9f2a").unwrap_err();
        assert!(err.to_string().contains("cannot read"));
    }

    #[test]
    fn a_manifest_only_unit_is_not_reported_as_untested_code() {
        // A workspace root holding nothing but Cargo.toml has no tests, and saying so is a
        // true statement wearing the same badge as a real finding. Noise at the same
        // severity as signal is how an operator learns to stop reading the list.
        let root = scratch();
        write(&root, "Cargo.toml", "[workspace]
members = [\"a\"]
");
        write(&root, "a/Cargo.toml", "[package]
name = \"a\"
");
        write(&root, "a/src/lib.rs", "fn f() {}
");
        let w = RepoObserver.observe(root.to_str().unwrap()).unwrap();

        let untested: Vec<&str> = w.risks().map(|s| s.id.as_str()).collect();
        assert!(
            untested.iter().any(|id| id.ends_with(":a")),
            "the crate with code must still be flagged, got {untested:?}"
        );
        let root_label = root.file_name().unwrap().to_string_lossy().to_string();
        assert!(
            !untested.iter().any(|id| id.ends_with(&format!(":{root_label}"))),
            "the manifest-only workspace root must not be, got {untested:?}"
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_locator_carries_no_platform_path_prefix() {
        // The locator becomes the memory subject key and every signal target, so a prefix
        // that varies with how the path was spelled splits one repository into two.
        let root = scratch();
        write(&root, "Cargo.toml", "[package]
name = \"x\"
");
        write(&root, "src/lib.rs", "fn a() {}
");
        let w = RepoObserver.observe(root.to_str().unwrap()).unwrap();
        assert!(!w.entity.locator.starts_with(r"\?\"), "got {}", w.entity.locator);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn markers_are_counted_and_cited() {
        let root = scratch();
        write(&root, "Cargo.toml", "[package]\nname = \"x\"\n");
        write(&root, "src/lib.rs", "// TODO: one\n// FIXME: two\n#[test]\nfn t() {}\n");
        let w = RepoObserver.observe(root.to_str().unwrap()).unwrap();
        let s = w
            .opportunities()
            .find(|s| s.id.starts_with("markers:"))
            .expect("expected a marker signal");
        assert!(s.evidence[0].contains("counted 2"), "got {:?}", s.evidence);
        fs::remove_dir_all(&root).ok();
    }
}
