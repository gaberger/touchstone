//! Secondary adapter -- one adapter, one crate, one merge unit (hex ADR-001).
//! Imports `okf-ports` ONLY: it cannot reach another adapter, because Cargo will not resolve it.
use okf_ports::{VersionControl, Concept};

pub struct GitAttest;

impl VersionControl for GitAttest {
    fn attest(&self, _path: &str) -> Result<(), String> { todo!() }
}
