//! Pure. Zero external crates -- and zero internal ones: this crate's empty
//! `[dependencies]` is what makes ARCHITECTURE.md rule 1 a compile error rather than a
//! convention. Survives a stack rewrite untouched.

/// A concept as the bundle holds it. Raw bytes stay authoritative (README design rules);
/// this is the parsed view used for querying.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Concept {
    pub path: String,
    pub concept_type: String,
    pub title: String,
}

/// Trust tier, derived -- never authored. See ARCHITECTURE.md "The trust invariant".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trust { Verified, Attested, Generated, Unknown }
