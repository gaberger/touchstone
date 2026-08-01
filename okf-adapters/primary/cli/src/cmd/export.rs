//! `touchstone export <dir> [--force]` — write raw bytes back out.
//!
//! Raw bytes are authoritative (ARCHITECTURE.md design rule 1): export copies
//! the source file unchanged, so any unknown key or value is preserved byte-
//! identically. T2a (raw round-trip drill) verifies this.

use crate::args::ExportArgs;
use crate::store::CliStore;
use crate::walk::iter_concept_files;
use std::fs;
use std::path::Path;

pub fn run(args: &ExportArgs, bundle: &Path, store: &dyn CliStore) -> i32 {
    let out = Path::new(&args.out);

    if out.exists() {
        if args.force {
            if let Err(e) = fs::remove_dir_all(out) {
                eprintln!("cannot remove {}: {e}", args.out);
                return 1;
            }
        } else {
            eprintln!("destination exists: {} (use --force to overwrite)", args.out);
            return 1;
        }
    }

    if let Err(e) = fs::create_dir_all(out) {
        eprintln!("cannot create {}: {e}", args.out);
        return 1;
    }

    // Copy every indexed concept (raw bytes, not parsed).
    let indexed = store.all_paths();
    let mut n = 0usize;

    for rel in &indexed {
        let src = bundle.join(rel);
        let dst = out.join(rel);
        if let Some(parent) = dst.parent() {
            let _ = fs::create_dir_all(parent);
        }
        match fs::copy(&src, &dst) {
            Ok(_) => n += 1,
            Err(e) => eprintln!("cannot copy {rel}: {e}"),
        }
    }

    // Also copy index.md files (generated artifacts, but part of the bundle).
    for (rel, full) in iter_concept_files(bundle) {
        // iter_concept_files skips index.md; copy index.md separately.
        let _ = (rel, full); // already handled above via indexed paths
    }
    // Walk bundle for index.md files and copy them too (mirrors Python export).
    copy_index_mds(bundle, out);

    println!("exported {n} concepts to {}", args.out);
    0
}

fn copy_index_mds(bundle: &Path, out: &Path) {
    let Ok(entries) = std::fs::read_dir(bundle) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if path.is_dir() {
            // Skip .touchstone and _* directories
            if name_str.starts_with('.') || name_str.starts_with('_') || name_str == "node_modules" {
                continue;
            }
            let dst_dir = out.join(&*name_str);
            let _ = fs::create_dir_all(&dst_dir);
            copy_index_mds(&path, &dst_dir);
        } else if name_str == "index.md" {
            let dst = out.join(&*name_str);
            let _ = fs::copy(&path, &dst);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{BundleStats, CliSearchFilter, IndexRecord, SearchHit};
    use std::collections::{HashMap, HashSet};

    struct FakeStore(Vec<String>);
    impl CliStore for FakeStore {
        fn prev_digests(&self) -> HashMap<String, String> { HashMap::new() }
        fn upsert(&mut self, _: &IndexRecord, _: &HashSet<String>) -> Result<(), String> { Ok(()) }
        fn remove(&mut self, _: &str) -> Result<(), String> { Ok(()) }
        fn reresolve(&mut self) -> Result<(), String> { Ok(()) }
        fn commit(&mut self) -> Result<(), String> { Ok(()) }
        fn broken_link_count(&self) -> usize { 0 }
        fn search(&self, _: &CliSearchFilter) -> Result<Vec<SearchHit>, String> { Ok(vec![]) }
        fn stats(&self) -> BundleStats {
            BundleStats { total: 0, by_type: vec![], by_trust: vec![], by_status: vec![], link_count: 0, broken_link_count: 0 }
        }
        fn all_paths(&self) -> Vec<String> { self.0.clone() }
    }

    #[test]
    fn export_copies_files() {
        let src = tempfile::TempDir::new().unwrap();
        let dst = tempfile::TempDir::new().unwrap();
        fs::write(src.path().join("a.md"), "---\ntype: Note\n---\n").unwrap();
        let store = FakeStore(vec!["a.md".into()]);
        let args = ExportArgs { out: dst.path().to_str().unwrap().to_string(), force: true };
        let rc = run(&args, src.path(), &store);
        assert_eq!(rc, 0);
        assert!(dst.path().join("a.md").exists());
    }

    #[test]
    fn export_no_force_existing_returns_1() {
        let src = tempfile::TempDir::new().unwrap();
        let dst = tempfile::TempDir::new().unwrap();
        let store = FakeStore(vec![]);
        let args = ExportArgs {
            out: dst.path().to_str().unwrap().to_string(),
            force: false,
        };
        let rc = run(&args, src.path(), &store);
        assert_eq!(rc, 1);
    }
}
