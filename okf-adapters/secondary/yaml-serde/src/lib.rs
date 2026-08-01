//! Secondary adapter -- one adapter, one crate, one merge unit (hex ADR-001).
//! Imports `okf-ports` ONLY: it cannot reach another adapter, because Cargo will not resolve it.
use okf_ports::{FrontmatterParser, Concept};

pub struct YamlSerde;

impl FrontmatterParser for YamlSerde {
    fn parse(&self, _raw: &[u8]) -> Result<Concept, String> { todo!("E3a: temporal values must stay ISO 8601 strings") }
}
