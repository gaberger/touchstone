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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Trust { Verified, Attested, Generated, #[default] Unknown }

/// One entry in the `verified:` list. Used by the lint use case.
#[derive(Debug, Clone, Default)]
pub struct VerifiedEntry {
    /// Actor identifier, e.g. `human:gary`. None when the `by:` key is absent (a
    /// lint violation per the OKF spec).
    pub by: Option<String>,
}

/// Full parsed view of a concept. Raw bytes are authoritative; these fields are derived
/// for querying and use-case coordination. Produced by the ConceptParser port.
#[derive(Debug, Clone, Default)]
pub struct ParsedConcept {
    pub path: String,
    pub concept_type: String,
    pub title: String,
    pub description: String,
    pub body: String,
    pub tags: Vec<String>,
    pub trust: Trust,
    /// Status string; spec default is "stable" when absent.
    pub status: String,
    /// Raw file bytes -- the authoritative value; never modified by any use case.
    pub raw: Vec<u8>,
    /// Non-conformance error, if any (missing/empty `type`, invalid YAML, …).
    pub error: Option<String>,
    /// If `Some(reason)`, this file must be skipped by FormatBundle. The reason
    /// explains why (anchors, aliases, merge keys, block scalars, …).
    pub format_skip_reason: Option<String>,
    /// `verified:` list entries, for the duplicate-principal and missing-`by` lint rules.
    pub verified_entries: Vec<VerifiedEntry>,
    /// True when any `sources:` entry is missing its required `resource:` field.
    pub has_source_missing_resource: bool,
    /// True when `[[wikilink]]` syntax appears anywhere in the raw text.
    pub has_wikilinks: bool,
}

impl ParsedConcept {
    pub fn conformant(&self) -> bool {
        self.error.is_none() && !self.concept_type.trim().is_empty()
    }

    pub fn as_concept(&self) -> Concept {
        Concept {
            path: self.path.clone(),
            concept_type: self.concept_type.clone(),
            title: self.title.clone(),
        }
    }
}
