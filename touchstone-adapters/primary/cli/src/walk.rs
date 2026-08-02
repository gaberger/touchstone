//! Concept file walker — mirrors Python's `iter_concept_files`.
//!
//! Walk rules (from Python oracle `touchstone/cli.py`, with E4a correction):
//! - Descend directories, skipping those whose name starts with `.` or `_`,
//!   and `node_modules`.
//! - Accept only `.md` files.
//! - Skip `index.md` (generated). DO NOT skip `log.md` (E4a: it is a concept).

use crate::parse::{parse_concept, RichConcept};
use std::fs;
use std::path::{Path, PathBuf};

/// Walk `root` and return parsed `RichConcept` for every concept file, sorted by path.
pub fn load_all(root: &Path) -> Vec<RichConcept> {
    let mut pairs = Vec::new();
    walk(root, root, &mut pairs);
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    pairs
        .into_iter()
        .map(|(rel, full)| {
            let raw = fs::read(&full).unwrap_or_default();
            parse_concept(&rel, raw)
        })
        .collect()
}

/// Walk `root` and return `(rel_path, full_path)` pairs, in sorted order.
pub fn iter_concept_files(root: &Path) -> Vec<(String, PathBuf)> {
    let mut pairs = Vec::new();
    walk(root, root, &mut pairs);
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    pairs
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if path.is_dir() {
            if skip_dir(&name_str) {
                continue;
            }
            walk(root, &path, out);
        } else if path.is_file() {
            if !name_str.ends_with(".md") || name_str == "index.md" {
                continue;
            }
            let rel = path
                .strip_prefix(root)
                .expect("must be under root")
                .to_slash_lossy();
            out.push((rel, path));
        }
    }
}

fn skip_dir(name: &str) -> bool {
    name.starts_with('.') || name.starts_with('_') || name == "node_modules"
}

trait ToSlashLossy {
    fn to_slash_lossy(&self) -> String;
}

impl ToSlashLossy for Path {
    fn to_slash_lossy(&self) -> String {
        #[cfg(windows)]
        {
            self.to_string_lossy().replace('\\', "/")
        }
        #[cfg(not(windows))]
        {
            self.to_string_lossy().into_owned()
        }
    }
}

/// Find the bundle root by walking up from `start`, looking for `index.md`
/// or `.touchstone`. Falls back to `start` itself if not found.
/// Mirrors Python's `find_bundle()`.
pub fn find_bundle(start: &Path) -> PathBuf {
    let p = match start.canonicalize() {
        Ok(c) => c,
        Err(_) => start.to_path_buf(),
    };
    let candidates: Vec<PathBuf> = std::iter::once(p.clone())
        .chain(p.ancestors().skip(1).map(|a| a.to_path_buf()))
        .collect();
    for cand in candidates {
        if cand.join("index.md").exists() || cand.join(".touchstone").exists() {
            return cand;
        }
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iter_skips_index_md() {
        let root = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../_upstream/acme_retail"
        ));
        let files = iter_concept_files(root);
        assert!(
            files.iter().all(|(rel, _)| !rel.ends_with("index.md")),
            "index.md must be excluded"
        );
    }

    #[test]
    fn iter_includes_log_md() {
        let root = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../_upstream/acme_retail"
        ));
        let files = iter_concept_files(root);
        assert!(
            files.iter().any(|(rel, _)| rel == "log.md"),
            "log.md must be included (E4a)"
        );
    }

    #[test]
    fn iter_sorted_by_path() {
        let root = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../_upstream/acme_retail"
        ));
        let files = iter_concept_files(root);
        let paths: Vec<&str> = files.iter().map(|(r, _)| r.as_str()).collect();
        let mut sorted = paths.clone();
        sorted.sort();
        assert_eq!(paths, sorted);
    }
}
