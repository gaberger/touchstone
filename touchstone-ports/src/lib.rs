//! Traits only. Depends on `touchstone-domain` for value types and nothing else.
//!
//! Domain types are RE-EXPORTED here because ARCHITECTURE.md rule 4 says adapters import
//! `touchstone-ports` only. Without this re-export an adapter would need its own `touchstone-domain`
//! dependency to name a `Concept`, and rule 4 would be false in practice.
pub use touchstone_domain::{Concept, ParsedConcept, Trust, VerifiedEntry};

/// Pure domain operations, re-exported for the same reason as the value types: rule 4a says a
/// secondary adapter imports `touchstone-ports` and nothing else, so without this an adapter wanting
/// to split a frontmatter block would either need its own `touchstone-domain` dependency (making the
/// rule false in practice) or would write a fourth copy of the function — which is exactly what
/// had already happened three times over.
pub use touchstone_domain::{
    extract_links, fnv64, normalize_path, relative_path, render_index, resolve_link, slugify,
    split_frontmatter,
};

// ── Minimal existing ports (unchanged) ───────────────────────────────────────

pub trait FrontmatterParser { fn parse(&self, raw: &[u8]) -> Result<Concept, String>; }
/// Enumerates the concept files in a bundle. **Deliberately does not parse them.**
///
/// This returned `Vec<Concept>` until the parsed fields turned out to be dead: every use case
/// read only `.path`, while the filesystem adapter — obliged to produce a `concept_type` and
/// `title` it had no parser for — carried a hand-rolled frontmatter scanner to invent them.
/// Two parsers over the same bytes is the exact divergence `ConceptParser` exists as a port to
/// prevent, and this one was load-bearing on nothing.
///
/// Paths are bundle-relative, slash-separated, and sorted, so every walk is deterministic —
/// which is what makes the T1 byte-identical rebuild reproducible rather than lucky.
pub trait ConceptRepository { fn paths(&self) -> Vec<String>; }
pub trait SearchIndex { fn search(&self, q: &str) -> Vec<Concept>; }
pub trait VersionControl { fn attest(&self, path: &str) -> Result<(), String>; }
pub trait SyncEngine { fn merge(&self, path: &str) -> Result<(), String>; }
pub trait Embedder { fn embed(&self, text: &str) -> Vec<f32>; }
pub trait Clock { fn now_iso8601(&self) -> String; }

// ── Extended ports for the use-case layer ────────────────────────────────────

/// Full-fidelity parser for use-case coordination. Returns the complete derived
/// view including body, tags, trust tier, status, lint fields, and a copy of
/// the raw bytes. The adapter is responsible for YAML parsing and for detecting
/// risky constructs (anchors, aliases, merge keys, block scalars) that make safe
/// reformatting impossible.
pub trait ConceptParser {
    fn parse(&self, path: &str, raw: &[u8]) -> ParsedConcept;

    /// Emit canonical frontmatter bytes for `parsed`, or `None` when the concept
    /// contains constructs that cannot be safely re-emitted. Callers MUST re-parse
    /// the result and verify it matches the original field values (T2e requirement).
    fn canonicalize(&self, parsed: &ParsedConcept) -> Option<Vec<u8>>;

    /// Emit a fresh concept file from `req`. The adapter controls YAML emission so
    /// the use-case layer stays YAML-library-free.
    fn emit_new(&self, req: &NewConceptRequest) -> Vec<u8>;
}

/// Parameters for a freshly scaffolded concept (CaptureConcept use case).
#[derive(Debug, Clone, Default)]
pub struct NewConceptRequest {
    pub concept_type: String,
    pub title: String,
    pub description: String,
    pub tags: Vec<String>,
    /// Agent identifier, e.g. `capture/claude-opus-5`. When `Some`, the emitted
    /// concept carries a `generated:` block. When `None`, no `generated:` key.
    pub generated_by: Option<String>,
    /// ISO 8601 timestamp from the Clock port.
    pub now_iso8601: String,
    /// 12-hex-character identifier.
    pub id: String,
}

/// Raw byte access by path. Separate from ConceptRepository so adapters can be
/// composed simply and in-memory fakes remain trivial.
pub trait RawStore {
    /// Raw bytes of the named concept, or `None` if the path is not found.
    fn raw_bytes(&self, path: &str) -> Option<Vec<u8>>;
}

/// Write-side for concept files. Used by FormatBundle (non-check mode) and CaptureConcept.
pub trait ConceptSink {
    fn write(&self, path: &str, raw: &[u8]) -> Result<(), String>;
    fn exists(&self, path: &str) -> bool;
}

/// Write-side for the search index. Used by IndexBundle.
pub trait IndexPopulator {
    fn upsert(
        &mut self,
        path: &str,
        concept_type: &str,
        title: &str,
        description: &str,
        body: &str,
        tags: &[String],
        trust: Trust,
        status: &str,
    ) -> Result<(), String>;
    fn remove(&mut self, path: &str) -> Result<(), String>;
}

/// Structured search query — mirrors the Python `cmd_search` filter options.
#[derive(Debug, Clone, Default)]
pub struct SearchQuery {
    pub text: String,
    pub concept_type: Option<String>,
    pub tag: Option<String>,
    pub status: Option<String>,
    pub trust: Option<Trust>,
    /// Maximum concepts to return. 0 → implementation default (10).
    pub limit: usize,
    /// Whether to expand direct hits by one graph hop (ARCHITECTURE.md pipeline).
    pub expand: bool,
}

/// How a hit was reached: directly by BM25 or via a one-hop graph link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchVia { Direct, Link }

/// A ranked concept returned by `FilteredSearch`.
///
/// Carries the display fields directly rather than wrapping a `Concept`. There were two
/// competing hit shapes — this one and a richer struct inside the CLI adapter — because the
/// minimal `Concept` lacks the `description` and `trust` every result listing needs, so the
/// adapter grew its own. A primary adapter defining the shape its index must return is
/// backwards: the port is the contract, and both the CLI and the MCP surface render the same
/// hits.
#[derive(Debug, Clone)]
pub struct SearchHit {
    pub path: String,
    pub concept_type: String,
    pub title: String,
    pub description: String,
    pub trust: Trust,
    pub via: SearchVia,
}

// ── The index, write side ────────────────────────────────────────────────────

/// Everything the index stores for one concept.
///
/// `fm_json` and `digest` are why this is richer than `ParsedConcept`'s query view: the first
/// is the verbatim frontmatter projection T2c and T2d assert against, the second is the
/// content fingerprint that makes indexing incremental.
#[derive(Debug, Clone, Default)]
pub struct IndexRecord {
    pub path: String,
    pub concept_type: String,
    pub title: String,
    pub description: String,
    pub body: String,
    pub tags: Vec<String>,
    pub trust: Trust,
    pub status: String,
    pub stale_after: Option<String>,
    /// Raw frontmatter serialised as JSON — the authoritative view for T2c.
    pub fm_json: String,
    pub conformant: bool,
    pub error: Option<String>,
    /// Content fingerprint, for incremental indexing.
    pub digest: String,
    /// Bundle-relative link targets resolved from body links.
    pub links: Vec<String>,
}

/// Aggregate counts for `stats`.
#[derive(Debug, Clone, Default)]
pub struct BundleStats {
    pub total: usize,
    pub by_type: Vec<(String, usize)>,
    pub by_trust: Vec<(String, usize)>,
    pub by_status: Vec<(String, usize)>,
    pub link_count: usize,
    pub broken_link_count: usize,
}

/// The derived index: write, query, and summarise.
///
/// This trait lived in the CLI adapter as `CliStore`, implemented by a `FullStore` hand-rolled
/// over raw rusqlite inside the composition root — while `touchstone-sqlite-index`, the adapter
/// whose entire job this is, sat unreachable. It belongs here, so that every driving adapter
/// gets the same index rather than the one its author happened to build.
pub trait BundleIndex {
    /// Bundle-relative path → content fingerprint, for skipping unchanged concepts.
    fn prev_digests(&self) -> std::collections::HashMap<String, String>;

    /// Insert or replace a concept. `known_paths` is the full current path set, so link
    /// targets can be marked resolved or broken as they are written.
    fn upsert(
        &mut self,
        rec: &IndexRecord,
        known_paths: &std::collections::HashSet<String>,
    ) -> Result<(), String>;

    fn remove(&mut self, path: &str) -> Result<(), String>;

    /// Re-resolve every edge target. A concept added late can resolve a link written earlier,
    /// so this runs once after the walk rather than per-upsert.
    fn reresolve(&mut self) -> Result<(), String>;

    fn commit(&mut self) -> Result<(), String>;

    /// Unresolved links. Broken links are legal per spec — they represent not-yet-written
    /// knowledge — so this is a reported statistic, never an error.
    fn broken_link_count(&self) -> usize;

    fn search(&self, q: &SearchQuery) -> Result<Vec<SearchHit>, String>;

    fn stats(&self) -> BundleStats;

    /// Every path currently in the index, so the indexer can delete what has left the bundle.
    fn all_paths(&self) -> Vec<String>;
}

/// Structured search with prefilter. Implemented by adapters holding a full-text index.
/// The basic `SearchIndex` trait remains for callers that only need plain text search.
pub trait FilteredSearch {
    fn search_filtered(&self, q: &SearchQuery) -> Vec<SearchHit>;
}

// ── Blanket impls for owned/boxed port objects ───────────────────────────────

/// A boxed sink is a sink.
///
/// The composition root builds an export destination only after arguments are parsed, so it
/// hands the command a `Box<dyn ConceptSink>`. Without this the use cases — generic over
/// `W: ConceptSink` — could not accept one, and the command would have to reach for the
/// filesystem itself, which rule 5 forbids and which is how duplicate implementations start.
impl<T: ConceptSink + ?Sized> ConceptSink for Box<T> {
    fn write(&self, path: &str, raw: &[u8]) -> Result<(), String> {
        (**self).write(path, raw)
    }
    fn exists(&self, path: &str) -> bool {
        (**self).exists(path)
    }
}
