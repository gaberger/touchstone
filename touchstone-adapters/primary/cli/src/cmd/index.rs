//! `touchstone index` — rebuild the derived index and every `index.md`.
//!
//! Owns no algorithm. `touchstone_usecases::reindex_bundle` walks, parses, upserts what
//! changed, drops what left, resolves edges and regenerates the index files; this formats the
//! report for a human.
//!
//! That split is not cosmetic. This command used to own the pipeline, and the MCP
//! `touchstone_index` tool consequently did everything *except* regenerate `index.md` — same
//! name, same description, different effect on disk. Two adapters sharing a name rather than an
//! implementation is exactly what parity is meant to prevent.

use std::path::Path;
use touchstone_ports::{BundleIndex, ConceptParser, ConceptRepository, ConceptSink, RawStore};
use touchstone_usecases::reindex_bundle;

pub fn run<F, P>(_bundle: &Path, index: &mut dyn BundleIndex, files: &F, parser: &P, quiet: bool) -> i32
where
    F: ConceptRepository + RawStore + ConceptSink,
    P: ConceptParser,
{
    let r = reindex_bundle(files, parser, index);

    if !quiet {
        println!(
            "indexed {} concepts ({} new, {} changed, {} removed)",
            r.total, r.new, r.changed, r.removed
        );
        println!("index.md files written: {}", r.indexes_written);
        println!(
            "broken links: {} (legal per spec -- not-yet-written knowledge)",
            r.broken_links
        );
        if !r.non_conformant.is_empty() {
            println!("NON-CONFORMANT: {}", r.non_conformant.len());
            for (path, err) in r.non_conformant.iter().take(10) {
                println!("  {path}: {err}");
            }
        }
    }
    0
}
