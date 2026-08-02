//! `touchstone ingest <file>...` — put source documents into `raw/`.
//!
//! The missing half of the pattern this project is shaped after: `raw/` immutable, concepts
//! compiled from it, every concept citing what it read. Until now a bundle could only hold
//! knowledge somebody had already written in OKF, which is nobody's starting position.
//!
//! Ingest does not parse, convert or summarise. Whatever went in comes back out byte-identical,
//! because the raw layer is what everything else is checked against.

use crate::args::IngestArgs;
use touchstone_ports::{ConceptRepository, ConceptSink, RawStore};
use touchstone_usecases::ingest_raw;

/// Collect files, descending into directories.
///
/// Bulk loading is the normal case, not the exception: nobody's notes are a single file, and an
/// ingest command that only took one at a time left the real corpus unreachable -- which is the
/// same reason A3 was untestable before the raw layer existed.
fn collect(path: &std::path::Path, out: &mut Vec<(String, Vec<u8>)>, errs: &mut Vec<String>) {
    if path.is_dir() {
        let Ok(entries) = std::fs::read_dir(path) else {
            errs.push(format!("cannot read directory {}", path.display()));
            return;
        };
        let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        entries.sort();
        for e in entries {
            // Skip dotfiles: .DS_Store, .obsidian/, .git/ are not source material.
            if e.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')) {
                continue;
            }
            collect(&e, out, errs);
        }
        return;
    }
    match std::fs::read(path) {
        Ok(bytes) => out.push((path.to_string_lossy().into_owned(), bytes)),
        Err(e) => errs.push(format!("cannot read {}: {e}", path.display())),
    }
}

pub fn run<F>(args: &IngestArgs, files: &F) -> i32
where
    F: ConceptRepository + ConceptSink + RawStore,
{
    let mut sources = Vec::new();
    let mut errs = Vec::new();
    for f in &args.files {
        collect(std::path::Path::new(f), &mut sources, &mut errs);
    }
    for e in &errs {
        eprintln!("{e}");
    }
    if sources.is_empty() {
        eprintln!("nothing to ingest");
        return 1;
    }

    let report = ingest_raw(&sources, files, files);
    // A bulk load can be thousands of files; listing every one buries the summary that matters.
    for path in report.ingested.iter().take(20) {
        println!("ingested {path}");
    }
    if report.ingested.len() > 20 {
        println!("... and {} more", report.ingested.len() - 20);
    }
    for (path, why) in report.skipped.iter().take(10) {
        println!("skipped {path} -- {why}");
    }
    if report.skipped.len() > 10 {
        println!("... and {} more skipped", report.skipped.len() - 10);
    }
    if !report.ingested.is_empty() {
        println!(
            "\n{} ingested. Nothing cites them yet -- `touchstone unprocessed` is the work queue.",
            report.ingested.len()
        );
    }
    if report.ingested.is_empty() && !report.skipped.is_empty() { 1 } else { 0 }
}
