//! Secondary adapter — the bundle on disk.
//!
//! Imports `touchstone-ports` ONLY (ARCHITECTURE.md rule 4a). Implements three ports:
//! `ConceptRepository` (which files are concepts), `RawStore` (their bytes), and
//! `ConceptSink` (writing bytes back).
//!
//! **This adapter does not parse.** It knows which files are concepts and how to read and write
//! their bytes; what a concept *means* is the parser's job. That separation is not tidiness —
//! this crate used to carry a hand-rolled frontmatter scanner so it could fill in the
//! `concept_type` and `title` the old port signature demanded, which meant two different
//! parsers read the same bytes and could disagree about a concept's type. Those fields were
//! never read by any caller. Both the scanner and the divergence are gone.
//!
//! Raw bytes are authoritative, so nothing here rewrites what it reads: `raw_bytes` hands back
//! the file verbatim and `write` stores exactly what it is given. That is what makes byte-exact
//! export (T2a) a property of the design rather than a test that happens to pass.

use std::fs;
use std::path::{Path, PathBuf};
use touchstone_ports::{CaptureEvent, ConceptRepository, ConceptSink, EventLog, RawStore};

pub struct FsBundle {
    pub root: PathBuf,
}

impl FsBundle {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        FsBundle { root: root.into() }
    }

    /// Absolute path for a bundle-relative concept path.
    pub fn full_path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }
}

// ── Which files are concepts ───────────────────────────────────────────────────

/// Directories the walk never descends into.
///
/// Dotted directories hold derived state (`.touchstone/`) or tooling; underscored ones are
/// working and fixture areas (`_upstream/`, `_work-*`). `node_modules` is excluded because a
/// vendored JS tree can hold thousands of markdown files that are not knowledge.
fn skip_dir(name: &str) -> bool {
    name.starts_with('.') || name.starts_with('_') || name == "node_modules" || name == RAW_DIR
}

/// The immutable source layer. Reserved at the bundle root.
///
/// Excluded from the concept walk deliberately: a markdown file you pasted in from somewhere
/// else is source material, not a concept, and indexing it would put unverified third-party
/// text into the same ranking pool as knowledge you wrote. It becomes knowledge when a concept
/// cites it.
pub const RAW_DIR: &str = "raw";

/// True when a filename is a concept rather than a generated artifact.
///
/// `index.md` is generated and therefore not a concept. **`log.md` IS a concept** — reserving
/// that filename is the E4a defect: the Python oracle reserved it alongside `index.md` and so
/// dropped a legitimate `type: Log` concept from a third-party bundle. The spec reserves no
/// such name, and narrowing the spec's tolerance is the failure mode this project treats as a
/// bug rather than as strictness.
fn is_concept_file(name: &str) -> bool {
    name.ends_with(".md") && name != "index.md"
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = entries.flatten().collect();
    // Sort by filename so the walk is stable across filesystems. readdir order is not
    // guaranteed, and an unstable order would make the generated index.md differ between
    // machines — silently falsifying T1 for everyone but the author.
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();

        if path.is_dir() {
            if !skip_dir(&name) {
                walk(root, &path, out);
            }
        } else if path.is_file() && is_concept_file(&name) {
            if let Ok(rel) = path.strip_prefix(root) {
                out.push(to_slash(rel));
            }
        }
    }
}

fn to_slash(p: &Path) -> String {
    #[cfg(windows)]
    {
        p.to_string_lossy().replace('\\', "/")
    }
    #[cfg(not(windows))]
    {
        p.to_string_lossy().into_owned()
    }
}

/// Find the bundle root by walking up from `start`, looking for `index.md` or `.touchstone`.
/// Falls back to `start` itself, so a bare directory of concepts still works before the first
/// `index` run has produced anything.
pub fn find_bundle(start: &Path) -> PathBuf {
    let p = start.canonicalize().unwrap_or_else(|_| start.to_path_buf());
    for cand in std::iter::once(p.clone()).chain(p.ancestors().skip(1).map(Path::to_path_buf)) {
        if cand.join("index.md").exists() || cand.join(".touchstone").exists() {
            return cand;
        }
    }
    p
}

// ── Ports ──────────────────────────────────────────────────────────────────────

impl ConceptRepository for FsBundle {
    fn paths(&self) -> Vec<String> {
        let mut out = Vec::new();
        walk(&self.root, &self.root, &mut out);
        out.sort();
        out
    }

    fn artifact_paths(&self) -> Vec<String> {
        let mut out = Vec::new();
        walk_artifacts(&self.root, &self.root, &mut out);
        out.sort();
        out
    }

    fn raw_paths(&self) -> Vec<String> {
        let mut out = Vec::new();
        walk_all(&self.root, &self.root.join(RAW_DIR), &mut out);
        out.sort();
        out
    }
}

/// Walk every file, regardless of extension. Used only for `raw/`, where a `.md` is still
/// source material rather than a concept.
fn walk_all(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if !name.starts_with('.') {
                walk_all(root, &path, out);
            }
        } else if path.is_file() {
            if let Ok(rel) = path.strip_prefix(root) {
                out.push(to_slash(rel));
            }
        }
    }
}

/// Walk every non-markdown file, applying the same directory rules as the concept walk.
///
/// Markdown is excluded because it is either a concept (handled by `paths`) or a generated
/// `index.md` (derived, and regenerated rather than copied).
fn walk_artifacts(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            // skip_dir excludes raw/, which raw_paths() reports separately -- otherwise a raw
            // PDF would be both an artifact and a source, and export would write it twice.
            if !skip_dir(&name) {
                walk_artifacts(root, &path, out);
            }
        } else if path.is_file() && !name.ends_with(".md") {
            if let Ok(rel) = path.strip_prefix(root) {
                out.push(to_slash(rel));
            }
        }
    }
}

/// Where the A3 capture log lives, relative to the bundle root.
///
/// Dot-prefixed, so the concept walk and the artifact walk both skip it — the log is not
/// knowledge and must never be indexed as a concept. Outside `.touchstone/` on purpose: that
/// directory is derived and gets deleted, and an experiment log that a rebuild destroys is
/// not an experiment log.
pub const A3_LOG_REL: &str = ".a3/capture.jsonl";

impl EventLog for FsBundle {
    fn record(&self, e: &CaptureEvent) -> Result<(), String> {
        use std::io::Write;
        let path = self.full_path(A3_LOG_REL);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|err| format!("cannot create {}: {err}", parent.display()))?;
        }
        // Hand-rolled JSON: one line, six flat scalar fields, no nesting. Pulling a serialiser
        // into this adapter to emit it would be more dependency than the format deserves.
        let esc = |v: &str| v.replace('\\', "\\\\").replace('"', "\\\"");
        let line = format!(
            "{{\"at\":\"{}\",\"surface\":\"{}\",\"path\":\"{}\",\"type\":\"{}\",\"trust\":\"{}\",\"elapsed_ms\":{}}}\n",
            esc(&e.at), esc(&e.surface), esc(&e.path), esc(&e.concept_type), esc(&e.trust), e.elapsed_ms
        );
        fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut f| f.write_all(line.as_bytes()))
            .map_err(|err| format!("cannot append to {A3_LOG_REL}: {err}"))
    }
}

impl RawStore for FsBundle {
    fn raw_bytes(&self, path: &str) -> Option<Vec<u8>> {
        fs::read(self.full_path(path)).ok()
    }
}

impl ConceptSink for FsBundle {
    fn write(&self, path: &str, raw: &[u8]) -> Result<(), String> {
        let target = self.full_path(path);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        fs::write(&target, raw).map_err(|e| format!("cannot write {path}: {e}"))
    }

    fn exists(&self, path: &str) -> bool {
        self.full_path(path).exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn upstream() -> PathBuf {
        Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../_upstream/acme_retail"))
            .to_path_buf()
    }

    fn tmp_bundle() -> tempfile::TempDir {
        let t = tempfile::TempDir::new().unwrap();
        fs::create_dir_all(t.path().join("notes")).unwrap();
        fs::write(t.path().join("notes/a.md"), b"---\ntype: Note\n---\n\nA.\n").unwrap();
        fs::write(t.path().join("index.md"), b"generated\n").unwrap();
        fs::write(t.path().join("notes/index.md"), b"generated\n").unwrap();
        fs::write(t.path().join("notes/skip.txt"), b"not markdown\n").unwrap();
        t
    }

    #[test]
    fn generated_indexes_are_not_concepts() {
        let t = tmp_bundle();
        let paths = FsBundle::new(t.path()).paths();
        assert!(
            paths.iter().all(|p| !p.ends_with("index.md")),
            "index.md must be excluded: {paths:?}"
        );
        assert_eq!(paths, vec!["notes/a.md".to_string()]);
    }

    #[test]
    fn non_markdown_is_not_a_concept() {
        let t = tmp_bundle();
        assert!(FsBundle::new(t.path()).paths().iter().all(|p| p.ends_with(".md")));
    }

    /// E4a. The one filename this must NOT reserve.
    #[test]
    fn log_md_is_a_concept() {
        let paths = FsBundle::new(upstream()).paths();
        assert!(paths.iter().any(|p| p == "log.md"), "log.md must be indexed (E4a): {paths:?}");
    }

    #[test]
    fn acme_retail_yields_ten_concepts() {
        let paths = FsBundle::new(upstream()).paths();
        assert_eq!(paths.len(), 10, "expected 10; 9 would mean log.md was reserved away");
    }

    #[test]
    fn paths_are_sorted_and_bundle_relative() {
        let paths = FsBundle::new(upstream()).paths();
        let mut sorted = paths.clone();
        sorted.sort();
        assert_eq!(paths, sorted, "walk order must be deterministic or T1 is luck");
        assert!(paths.iter().all(|p| !p.starts_with('/')), "paths must be bundle-relative");
    }

    #[test]
    fn derived_and_underscored_directories_are_skipped() {
        let t = tmp_bundle();
        fs::create_dir_all(t.path().join(".touchstone")).unwrap();
        fs::write(t.path().join(".touchstone/x.md"), b"---\ntype: Note\n---\n").unwrap();
        fs::create_dir_all(t.path().join("_work")).unwrap();
        fs::write(t.path().join("_work/y.md"), b"---\ntype: Note\n---\n").unwrap();
        assert_eq!(FsBundle::new(t.path()).paths(), vec!["notes/a.md".to_string()]);
    }

    #[test]
    fn raw_bytes_round_trip_verbatim() {
        let t = tempfile::TempDir::new().unwrap();
        let b = FsBundle::new(t.path());
        // CRLF and a BOM: the two things a careless reader would normalise away.
        let raw = b"\xef\xbb\xbf---\r\ntype: Note\r\n---\r\n\r\nBody.\r\n";
        b.write("notes/crlf.md", raw).unwrap();
        assert_eq!(b.raw_bytes("notes/crlf.md").as_deref(), Some(&raw[..]));
    }

    #[test]
    fn writing_creates_missing_directories() {
        let t = tempfile::TempDir::new().unwrap();
        let b = FsBundle::new(t.path());
        assert!(!b.exists("deep/nested/x.md"));
        b.write("deep/nested/x.md", b"hi").unwrap();
        assert!(b.exists("deep/nested/x.md"));
    }

    #[test]
    fn missing_bytes_are_none_not_a_panic() {
        let t = tempfile::TempDir::new().unwrap();
        assert!(FsBundle::new(t.path()).raw_bytes("nope.md").is_none());
    }

    #[test]
    fn find_bundle_walks_up_to_a_marked_root() {
        let t = tempfile::TempDir::new().unwrap();
        fs::create_dir_all(t.path().join("notes/deep")).unwrap();
        fs::create_dir_all(t.path().join(".touchstone")).unwrap();
        let found = find_bundle(&t.path().join("notes/deep"));
        assert_eq!(found, t.path().canonicalize().unwrap(), "should walk up to .touchstone");
    }

    /// Documents a sharp edge rather than asserting it is desirable: `index.md` marks a bundle
    /// root, but `touchstone index` generates an `index.md` in *every* directory, so running
    /// from inside a subdirectory resolves that subdirectory as the root and silently operates
    /// on a subset of the bundle.
    ///
    /// Only the root index carries `okf_version`, so a stricter marker is available if this is
    /// ever judged a defect. Pinned here so a future change to `find_bundle` is a deliberate
    /// decision with a failing test attached, not an accident.
    #[test]
    fn a_generated_subdirectory_index_also_looks_like_a_root() {
        let t = tmp_bundle(); // writes both index.md and notes/index.md
        let found = find_bundle(&t.path().join("notes"));
        assert_eq!(
            found,
            t.path().canonicalize().unwrap().join("notes"),
            "current behaviour: the nearest index.md wins, even a generated sub-index"
        );
    }

    #[test]
    fn find_bundle_falls_back_to_the_start() {
        let t = tempfile::TempDir::new().unwrap();
        assert_eq!(find_bundle(t.path()), t.path().canonicalize().unwrap());
    }
}
