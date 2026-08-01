//! Secondary adapter -- one adapter, one crate, one merge unit (hex ADR-001).
//! Imports `okf-ports` ONLY: it cannot reach another adapter, because Cargo will not resolve it.
use okf_ports::{Embedder, Concept};

pub struct EmbedLocal;

impl Embedder for EmbedLocal {
    fn embed(&self, _text: &str) -> Vec<f32> { todo!() }
}
