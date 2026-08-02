//! Secondary adapter -- one adapter, one crate, one merge unit (hex ADR-001).
//! Imports `touchstone-ports` ONLY: it cannot reach another adapter, because Cargo will not resolve it.
use touchstone_ports::{SyncEngine, Concept};

pub struct CrdtSync;

impl SyncEngine for CrdtSync {
    fn merge(&self, _path: &str) -> Result<(), String> { todo!() }
}
