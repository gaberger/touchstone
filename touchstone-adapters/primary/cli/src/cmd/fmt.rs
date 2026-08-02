//! `touchstone fmt [--check]` — canonicalize frontmatter.
//!
//! Owns no rules. `touchstone_usecases::format_bundle` decides what can be safely rewritten and
//! what must be refused; this prints the result.
//!
//! The safety predicate lives behind the `ConceptParser` port because it is a property of the
//! serializer, not of the command: a file carrying anchors, aliases, merge keys or block scalars
//! cannot be reproduced, so it is skipped rather than risked (FINDINGS E3b).

use std::path::Path;
use touchstone_ports::{ConceptParser, ConceptRepository, ConceptSink, RawStore};
use touchstone_usecases::format_bundle;

pub fn run<F, P>(check: bool, files: &F, parser: &P) -> i32
where
    F: ConceptRepository + RawStore + ConceptSink,
    P: ConceptParser,
{
    let report = format_bundle(files, parser, files, check);

    for (path, why) in &report.skipped {
        println!("skipped: {path} -- {why}");
    }
    for path in &report.changed {
        println!("{}: {path}", if check { "would reformat" } else { "formatted" });
    }
    println!(
        "{} file(s) {}, {} skipped",
        report.changed.len(),
        if check { "need formatting" } else { "formatted" },
        report.skipped.len()
    );

    // `--check` is a gate: non-zero when anything would change, so CI can use it directly.
    if check && !report.changed.is_empty() { 1 } else { 0 }
}
