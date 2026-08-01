//! Secondary adapter -- one adapter, one crate, one merge unit (hex ADR-001).
//! Imports `okf-ports` ONLY: it cannot reach another adapter, because Cargo will not resolve it.
use okf_ports::{ConceptRepository, Concept};

pub struct FsBundle;

impl ConceptRepository for FsBundle {
    fn list(&self) -> Vec<Concept> { todo!("raw byte IO; index.md and log.md are NOT reserved") }
}
