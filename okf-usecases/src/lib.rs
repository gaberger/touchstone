//! Compose ports. Never names an adapter -- that is rule 3, enforced by these dependencies.
use okf_ports::{ConceptRepository, SearchIndex};

pub fn index_bundle<R: ConceptRepository>(repo: &R) -> usize { repo.list().len() }
pub fn search_bundle<S: SearchIndex>(idx: &S, q: &str) -> Vec<okf_ports::Concept> { idx.search(q) }
