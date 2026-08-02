//! `touchstone new <Type> <Title>` — scaffold a conformant concept.
//!
//! Owns no emission. `capture_concept` builds the request and the `ConceptParser` port emits the
//! frontmatter through a YAML dumper — never string concatenation, because FINDINGS E3d found
//! that hand-crafted frontmatter carrying `: ` inside a value is the single most likely
//! authoring error, and a dumper cannot make it.
//!
//! It also cannot write `verified`. An agent may not assert human verification; the trust tier
//! is derived, not authored.

use crate::args::NewArgs;
use touchstone_ports::{Clock, ConceptParser, ConceptSink};
use touchstone_usecases::{capture_concept, CaptureRequest};

pub fn run<P, W>(args: &NewArgs, parser: &P, sink: &W, clock: &dyn Clock) -> i32
where
    P: ConceptParser,
    W: ConceptSink,
{
    let req = CaptureRequest {
        concept_type: args.concept_type.clone(),
        title: args.title.clone(),
        description: args.description.clone().unwrap_or_default(),
        tags: args.tags.clone(),
        subdir: args.dir.clone(),
        generated_by: args.generated.clone(),
    };
    match capture_concept(&req, parser, sink, clock) {
        Ok(path) => {
            println!("{path}");
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}
