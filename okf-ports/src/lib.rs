//! Traits only. Depends on `okf-domain` for value types and nothing else.
//!
//! Domain types are RE-EXPORTED here because ARCHITECTURE.md rule 4 says adapters import
//! `okf-ports` only. Without this re-export an adapter would need its own `okf-domain`
//! dependency to name a `Concept`, and rule 4 would be false in practice.
pub use okf_domain::{Concept, Trust};

pub trait FrontmatterParser { fn parse(&self, raw: &[u8]) -> Result<Concept, String>; }
pub trait ConceptRepository { fn list(&self) -> Vec<Concept>; }
pub trait SearchIndex { fn search(&self, q: &str) -> Vec<Concept>; }
pub trait VersionControl { fn attest(&self, path: &str) -> Result<(), String>; }
pub trait SyncEngine { fn merge(&self, path: &str) -> Result<(), String>; }
pub trait Embedder { fn embed(&self, text: &str) -> Vec<f32>; }
pub trait Clock { fn now_iso8601(&self) -> String; }
