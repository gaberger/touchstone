//! `touchstone index` — rebuild the derived index and index.md files.
//!
//! Algorithm (mirrors Python `cmd_index`):
//! 1. Walk concept files in the bundle.
//! 2. Compare digests to the previous index; only upsert changed concepts.
//! 3. Remove concepts that no longer exist.
//! 4. Re-resolve edge targets.
//! 5. Commit.
//! 6. Write `index.md` for every directory that contains concepts.

use crate::parse::{parse_concept, RichConcept};
use crate::render::render_index;
use crate::store::{CliStore, IndexRecord};
use crate::walk::iter_concept_files;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::Path;

pub fn run(bundle: &Path, store: &mut dyn CliStore, quiet: bool) -> i32 {
    // 1. Walk and parse all concept files.
    let file_pairs = iter_concept_files(bundle);
    let concepts: Vec<RichConcept> = file_pairs
        .iter()
        .map(|(rel, full)| {
            let raw = fs::read(full).unwrap_or_default();
            parse_concept(rel, raw)
        })
        .collect();

    let known: HashSet<String> = concepts.iter().map(|c| c.path.clone()).collect();

    // 2. Check digests; upsert changed concepts.
    let prev = store.prev_digests();
    let mut new_count = 0usize;
    let mut changed_count = 0usize;

    for c in &concepts {
        let digest = c.digest();
        if prev.get(&c.path).map(|d| d.as_str()) == Some(digest.as_str()) {
            continue; // unchanged
        }
        let rec = concept_to_record(c, digest);
        if let Err(e) = store.upsert(&rec, &known) {
            eprintln!("index error ({}): {e}", c.path);
        }
        if prev.contains_key(&c.path) {
            changed_count += 1;
        } else {
            new_count += 1;
        }
    }

    // 3. Remove deleted concepts.
    let removed: Vec<String> = prev
        .keys()
        .filter(|p| !known.contains(p.as_str()))
        .cloned()
        .collect();
    for p in &removed {
        let _ = store.remove(p);
    }

    // 4. Re-resolve edges and commit.
    let _ = store.reresolve();
    let _ = store.commit();

    // 5. Write index.md files.
    let written = write_indexes(bundle, &concepts);

    // 6. Report.
    let bad: Vec<&RichConcept> = concepts.iter().filter(|c| !c.conformant()).collect();
    let broken = store.broken_link_count();

    if !quiet {
        println!(
            "indexed {} concepts ({} new, {} changed, {} removed)",
            concepts.len(),
            new_count,
            changed_count,
            removed.len()
        );
        println!("index.md files written: {written}");
        println!("broken links: {broken} (legal per spec -- not-yet-written knowledge)");
        if !bad.is_empty() {
            println!("NON-CONFORMANT: {}", bad.len());
            for c in bad.iter().take(10) {
                let err = c.error.as_deref().unwrap_or("?");
                println!("  {}: {err}", c.path);
            }
        }
    }

    0
}

fn concept_to_record(c: &RichConcept, digest: String) -> IndexRecord {
    IndexRecord {
        path: c.path.clone(),
        concept_type: c.concept_type().to_string(),
        title: c.title(),
        description: c.description().to_string(),
        body: c.body.clone(),
        tags: c.tags(),
        trust: c.trust().to_string(),
        status: c.status().to_string(),
        stale_after: c.stale_after(),
        fm_json: c.fm_json(),
        conformant: c.conformant(),
        error: c.error.clone(),
        digest,
        links: c.resolved_links(),
    }
}

/// Write `index.md` for every directory implied by the concept set.
/// Returns the number of `index.md` files written (created or updated).
fn write_indexes(bundle: &Path, concepts: &[RichConcept]) -> usize {
    // Group concepts by their parent directory (bundle-relative).
    let mut by_dir: BTreeMap<String, Vec<&RichConcept>> = BTreeMap::new();
    for c in concepts {
        let dir = if let Some(pos) = c.path.rfind('/') {
            c.path[..pos].to_string()
        } else {
            String::new()
        };
        by_dir.entry(dir).or_default().push(c);
    }

    // Collect all directories (including ancestors with no direct concepts).
    let mut dirs: HashSet<String> = HashSet::new();
    dirs.insert(String::new()); // root always present
    for d in by_dir.keys() {
        let mut parts: Vec<&str> = if d.is_empty() { vec![] } else { d.split('/').collect() };
        while !parts.is_empty() {
            dirs.insert(parts.join("/"));
            parts.pop();
        }
        dirs.insert(String::new());
    }

    let count_under = |d: &str| -> usize {
        by_dir
            .iter()
            .filter(|(k, _)| {
                if d.is_empty() {
                    true
                } else {
                    *k == d || k.starts_with(&format!("{d}/"))
                }
            })
            .map(|(_, v)| v.len())
            .sum()
    };

    let mut n = 0;
    let mut sorted_dirs: Vec<String> = dirs.into_iter().collect();
    sorted_dirs.sort();

    for d in &sorted_dirs {
        // Direct children (sub-directories one level down)
        let depth = if d.is_empty() { 0 } else { d.split('/').count() };
        let mut kids: BTreeMap<String, bool> = BTreeMap::new();
        for other in &sorted_dirs {
            if other == d {
                continue;
            }
            let other_depth = if other.is_empty() { 0 } else { other.split('/').count() };
            if other_depth != depth + 1 {
                continue;
            }
            let is_child = if d.is_empty() {
                !other.contains('/')
            } else {
                other.starts_with(&format!("{d}/")) && !other[d.len() + 1..].contains('/')
            };
            if is_child {
                kids.insert(other.clone(), true);
            }
        }

        let subdirs: Vec<(String, usize)> = kids
            .keys()
            .map(|k| {
                let name = k.rsplit('/').next().unwrap_or(k.as_str()).to_string();
                let cnt = count_under(k);
                (name, cnt)
            })
            .collect();

        let empty_vec: Vec<&RichConcept> = Vec::new();
        let dir_concepts: &[&RichConcept] =
            by_dir.get(d.as_str()).map(|v| v.as_slice()).unwrap_or(&empty_vec);

        let is_root = d.is_empty();
        let text = render_index(d, dir_concepts, &subdirs, is_root);

        let target = if d.is_empty() {
            bundle.join("index.md")
        } else {
            bundle.join(d).join("index.md")
        };

        if let Some(parent) = target.parent() {
            let _ = fs::create_dir_all(parent);
        }

        let old = fs::read_to_string(&target).unwrap_or_default();
        if old != text {
            let _ = fs::write(&target, &text);
        }
        n += 1;
    }

    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn relpath_logic_in_write_indexes() {
        // Just ensure write_indexes doesn't panic on empty input.
        use tempfile::TempDir;
        struct NullStore;
        impl CliStore for NullStore {
            fn prev_digests(&self) -> HashMap<String, String> { HashMap::new() }
            fn upsert(&mut self, _: &IndexRecord, _: &HashSet<String>) -> Result<(), String> { Ok(()) }
            fn remove(&mut self, _: &str) -> Result<(), String> { Ok(()) }
            fn reresolve(&mut self) -> Result<(), String> { Ok(()) }
            fn commit(&mut self) -> Result<(), String> { Ok(()) }
            fn broken_link_count(&self) -> usize { 0 }
            fn search(&self, _: &crate::store::CliSearchFilter) -> Result<Vec<crate::store::SearchHit>, String> { Ok(vec![]) }
            fn stats(&self) -> crate::store::BundleStats {
                crate::store::BundleStats { total: 0, by_type: vec![], by_trust: vec![], by_status: vec![], link_count: 0, broken_link_count: 0 }
            }
            fn all_paths(&self) -> Vec<String> { vec![] }
        }

        let tmp = TempDir::new().unwrap();
        let written = write_indexes(tmp.path(), &[]);
        // Root index.md should be written even with no concepts.
        assert!(written >= 1);
        assert!(tmp.path().join("index.md").exists());
    }
}
