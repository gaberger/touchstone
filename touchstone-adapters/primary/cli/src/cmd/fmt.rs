//! `touchstone fmt [--check]` — canonicalize frontmatter.
//!
//! Refuses any file it cannot safely reproduce (anchors, block scalars, merge
//! keys). Any rewrite that does not survive re-parse unchanged is also refused.
//! This is the safety predicate from FINDINGS.md E3b.

use crate::args::FmtArgs;
use crate::parse::parse_concept;
use crate::walk::iter_concept_files;
use std::fs;
use std::path::Path;

pub fn run(args: &FmtArgs, bundle: &Path) -> i32 {
    let files = iter_concept_files(bundle);
    let mut changed: Vec<String> = Vec::new();
    let mut skipped: Vec<(String, String)> = Vec::new();

    for (rel, full) in &files {
        let raw = match fs::read(full) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let c = parse_concept(rel, raw);

        if let Some(reason) = c.formattable() {
            if !c.fm.is_empty() {
                skipped.push((rel.clone(), reason));
            }
            continue;
        }

        let new_text = c.format();

        // Safety: re-parse the rewrite and verify values are unchanged (T2e).
        let c2 = parse_concept(rel, new_text.as_bytes().to_vec());
        if c2.fm != c.fm {
            skipped.push((rel.clone(), "reserialization would change values".to_string()));
            continue;
        }

        let old_text = std::str::from_utf8(&c.raw).unwrap_or("").to_string();
        if new_text != old_text {
            changed.push(rel.clone());
            if !args.check {
                let _ = fs::write(full, new_text.as_bytes());
            }
        }
    }

    for (r, why) in &skipped {
        println!("skipped: {r} -- {why}");
    }
    for r in &changed {
        if args.check {
            println!("would reformat: {r}");
        } else {
            println!("formatted: {r}");
        }
    }
    let n = changed.len();
    let s = skipped.len();
    if args.check {
        println!("{n} file(s) need formatting, {s} skipped");
    } else {
        println!("{n} file(s) formatted, {s} skipped");
    }

    if args.check && !changed.is_empty() { 1 } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_check_unformatted_returns_1() {
        let tmp = tempfile::TempDir::new().unwrap();
        // Write a concept with non-canonical key order
        let raw = "---\ntitle: Hello\ntype: Note\n---\n\nBody.\n";
        fs::write(tmp.path().join("x.md"), raw).unwrap();
        let args = FmtArgs { check: true };
        let rc = run(&args, tmp.path());
        assert_eq!(rc, 1);
    }

    #[test]
    fn fmt_already_canonical_returns_0() {
        let tmp = tempfile::TempDir::new().unwrap();
        // Write a concept that is already canonical (type before title).
        // After fmt, it should be unchanged.
        let raw = b"---\ntype: Note\ntitle: Hello\n---\n\nBody.\n";
        let c = parse_concept("x.md", raw.to_vec());
        let canonical = c.format();
        fs::write(tmp.path().join("x.md"), &canonical).unwrap();
        let args = FmtArgs { check: true };
        let rc = run(&args, tmp.path());
        assert_eq!(rc, 0);
    }

    #[test]
    fn fmt_skips_risky_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let raw = "---\ntype: Note\nscript: |\n  echo hi\n---\n\nBody.\n";
        fs::write(tmp.path().join("risky.md"), raw).unwrap();
        let args = FmtArgs { check: false };
        let rc = run(&args, tmp.path());
        // No crash, risky file is skipped
        assert_eq!(rc, 0);
        // File unchanged
        assert_eq!(fs::read_to_string(tmp.path().join("risky.md")).unwrap(), raw);
    }
}
