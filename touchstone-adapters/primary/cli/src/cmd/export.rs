//! `touchstone export <dir>` — write raw bytes back out.
//!
//! Byte-exact by construction: `export_bundle` copies `raw_bytes` straight to the sink, so no
//! serializer sits in the write path where it could drop an unknown key. That is what makes the
//! portability claim structural rather than merely tested (T2a).
//!
//! The destination sink is built by the composition root, because this crate cannot name a
//! filesystem adapter (rule 5) and the destination is not known until arguments are parsed.

use crate::args::ExportArgs;
use std::path::Path;
use touchstone_ports::{ConceptRepository, ConceptSink, RawStore};
use touchstone_usecases::export_bundle;

pub fn run<F>(args: &ExportArgs, files: &F, make_sink: &dyn Fn(&Path) -> Box<dyn ConceptSink>) -> i32
where
    F: ConceptRepository + RawStore,
{
    let out = Path::new(&args.out);
    if out.exists() && !args.force {
        eprintln!("{} exists; pass --force to overwrite", args.out);
        return 1;
    }
    let sink = make_sink(out);
    match export_bundle(files, &sink) {
        Ok(stats) => {
            println!("exported {} concepts to {}", stats.count, args.out);
            0
        }
        Err(e) => {
            eprintln!("export failed: {e}");
            1
        }
    }
}
