//! `touchstone unprocessed` — raw documents no concept cites yet.
//!
//! The agent's work queue, and it is DERIVED rather than tracked: a raw document is processed
//! exactly when some concept names it in `sources`. No state file, nothing to fall out of sync,
//! and deleting the derived plane changes nothing — the same rule the index follows.

use touchstone_ports::{ConceptParser, ConceptRepository, RawStore};
use touchstone_usecases::unprocessed_raw;

pub fn run<F, P>(files: &F, parser: &P) -> i32
where
    F: ConceptRepository + RawStore,
    P: ConceptParser,
{
    let pending = unprocessed_raw(files, parser);
    let total = files.raw_paths().len();

    if total == 0 {
        println!("no raw documents. `touchstone ingest <file>` adds source material.");
        return 0;
    }
    for p in &pending {
        println!("{p}");
    }
    println!("\n{} of {} raw documents uncited", pending.len(), total);
    if !pending.is_empty() {
        println!(
            "Compile them into concepts that cite them in `sources`, then they leave this list."
        );
    }
    0
}
