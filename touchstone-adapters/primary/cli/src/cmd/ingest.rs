//! `touchstone ingest <file>...` — put source documents into `raw/`.
//!
//! The missing half of the pattern this project is shaped after: `raw/` immutable, concepts
//! compiled from it, every concept citing what it read. Until now a bundle could only hold
//! knowledge somebody had already written in OKF, which is nobody's starting position.
//!
//! Ingest does not parse, convert or summarise. Whatever went in comes back out byte-identical,
//! because the raw layer is what everything else is checked against.

use crate::args::IngestArgs;
use touchstone_ports::{ConceptRepository, ConceptSink};
use touchstone_usecases::ingest_raw;

pub fn run<F>(args: &IngestArgs, files: &F) -> i32
where
    F: ConceptRepository + ConceptSink,
{
    let mut sources = Vec::new();
    for f in &args.files {
        match std::fs::read(f) {
            Ok(bytes) => sources.push((f.clone(), bytes)),
            Err(e) => {
                eprintln!("cannot read {f}: {e}");
                return 1;
            }
        }
    }

    let report = ingest_raw(&sources, files, files);
    for path in &report.ingested {
        println!("ingested {path}");
    }
    for (path, why) in &report.skipped {
        println!("skipped {path} -- {why}");
    }
    if !report.ingested.is_empty() {
        println!(
            "\n{} ingested. Nothing cites them yet -- `touchstone unprocessed` is the work queue.",
            report.ingested.len()
        );
    }
    if report.ingested.is_empty() && !report.skipped.is_empty() { 1 } else { 0 }
}
