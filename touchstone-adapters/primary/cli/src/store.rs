//! Additional traits the CLI adapter needs beyond `touchstone-ports`.
//!
//! These are defined here (not in `touchstone-ports`) because they are CLI-specific:
//! they include the full concept record for indexing, structured search filters,
//! and aggregate statistics that the minimal port traits do not expose.
//!
//! The composition root (`touchstone-cli`) is responsible for implementing `CliStore`
//! by wiring the concrete adapters. Adapters never import each other (rule 5),
//! so the composition root is the only place that can bridge them.

use std::collections::{HashMap, HashSet};

/// Full concept record for index upsert operations.
/// Every field the index needs, parsed from the raw concept file.
pub struct IndexRecord {
    pub path: String,
    pub concept_type: String,
    pub title: String,
    pub description: String,
    pub body: String,
    pub tags: Vec<String>,
    /// "human" | "machine" | "unattributed"
    pub trust: String,
    /// "draft" | "stable" | "deprecated"
    pub status: String,
    pub stale_after: Option<String>,
    /// Raw frontmatter serialised as JSON (the authoritative view for T2c).
    pub fm_json: String,
    pub conformant: bool,
    pub error: Option<String>,
    /// Content fingerprint for incremental indexing.
    pub digest: String,
    /// Bundle-relative link targets (resolved from body links).
    pub links: Vec<String>,
}

/// Structured prefilter for the search command.
pub struct CliSearchFilter {
    pub query: String,
    pub concept_type: Option<String>,
    pub tag: Option<String>,
    pub status: Option<String>,
    pub trust: Option<String>,
    pub limit: usize,
    pub expand: bool,
}

/// One search result — richer than the domain `Concept` view.
#[derive(Clone, Debug)]
pub struct SearchHit {
    pub path: String,
    pub title: String,
    pub description: String,
    pub concept_type: String,
    /// "human" | "machine" | "unattributed"
    pub trust: String,
    /// "direct" | "link"  (direct BM25 hit vs graph-expanded)
    pub via: String,
}

/// Summary statistics for the `stats` command.
pub struct BundleStats {
    pub total: usize,
    pub by_type: Vec<(String, usize)>,
    pub by_trust: Vec<(String, usize)>,
    pub by_status: Vec<(String, usize)>,
    pub link_count: usize,
    pub broken_link_count: usize,
}

/// The index interface the CLI adapter needs.
///
/// Implemented by the composition root (`touchstone-cli`), which wires the concrete
/// storage adapter. The CLI adapter only knows this trait — it never names
/// `SqliteIndex` or any other concrete type.
pub trait CliStore {
    /// Map from bundle-relative path → content fingerprint.
    /// Used to skip unchanged concepts on incremental index.
    fn prev_digests(&self) -> HashMap<String, String>;

    /// Insert or replace a concept in the store.
    /// `known_paths` is the full set of current concept paths (for edge resolution).
    fn upsert(&mut self, rec: &IndexRecord, known_paths: &HashSet<String>)
        -> Result<(), String>;

    /// Remove a concept by path.
    fn remove(&mut self, path: &str) -> Result<(), String>;

    /// Re-resolve all edge targets after an index update.
    fn reresolve(&mut self) -> Result<(), String>;

    /// Commit pending changes.
    fn commit(&mut self) -> Result<(), String>;

    /// Count unresolved (broken) links in the current index.
    fn broken_link_count(&self) -> usize;

    /// Filtered full-text search (structured prefilter → BM25 → optional expansion).
    fn search(&self, filter: &CliSearchFilter) -> Result<Vec<SearchHit>, String>;

    /// Bundle summary statistics for the `stats` command.
    fn stats(&self) -> BundleStats;

    /// List all concept paths currently in the index.
    fn all_paths(&self) -> Vec<String>;
}
