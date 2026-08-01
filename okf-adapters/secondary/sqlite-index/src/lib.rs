//! Secondary adapter -- one adapter, one crate, one merge unit (hex ADR-001).
//! Imports `okf-ports` ONLY: it cannot reach another adapter, because Cargo will not resolve it.
use okf_ports::{SearchIndex, Concept};

pub struct SqliteIndex;

impl SearchIndex for SqliteIndex {
    fn search(&self, _q: &str) -> Vec<Concept> { todo!() }
}
