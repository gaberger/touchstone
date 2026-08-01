//! `touchstone lint` — conformance floor plus duplicate checks.
//!
//! Rules from FINDINGS.md E2 / E3d: the duplicate checks are what git's
//! 16% CLEAN_DEFECT rate actually produces; the conformance floor catches the
//! authoring errors `new` is designed to prevent.

use crate::parse::{lint, parse_concept};
use crate::walk::iter_concept_files;
use std::fs;
use std::path::Path;

pub fn run(bundle: &Path) -> i32 {
    let files = iter_concept_files(bundle);
    let mut total = 0usize;

    for (rel, full) in &files {
        let raw = match fs::read(full) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let c = parse_concept(rel, raw);
        let problems = lint(&c);
        if !problems.is_empty() {
            println!("{rel}");
            for p in &problems {
                println!("  - {p}");
            }
            total += problems.len();
        }
    }

    println!("\n{total} problem(s)");
    if total > 0 { 1 } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lint_clean_bundle_returns_0() {
        let tmp = tempfile::TempDir::new().unwrap();
        let raw = "---\ntype: Note\ntitle: Clean\nstatus: stable\ntags: []\n---\n\nBody.\n";
        fs::write(tmp.path().join("clean.md"), raw).unwrap();
        assert_eq!(run(tmp.path()), 0);
    }

    #[test]
    fn lint_bad_concept_returns_1() {
        let tmp = tempfile::TempDir::new().unwrap();
        let raw = "---\ntype: Note\ntags: [a, b, a]\n---\n\nBody.\n";
        fs::write(tmp.path().join("bad.md"), raw).unwrap();
        assert_eq!(run(tmp.path()), 1);
    }
}
