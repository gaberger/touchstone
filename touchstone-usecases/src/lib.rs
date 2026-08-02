//! Compose ports. Never names an adapter -- that is ARCHITECTURE.md rule 3, enforced by
//! these `[dependencies]`. External crates (uuid) are permitted; internal touchstone-* adapter
//! crates are not.

use touchstone_ports as ports;
use touchstone_ports::{
    Clock, Concept, ConceptParser, ConceptRepository, ConceptSink, FilteredSearch,
    IndexPopulator, NewConceptRequest, ParsedConcept, RawStore, SearchHit, SearchQuery,
};

// Keep the legacy stub signatures so existing callers compile.
use touchstone_ports::SearchIndex;
pub fn index_bundle<R: ConceptRepository>(repo: &R) -> usize { repo.paths().len() }
pub fn search_bundle<S: SearchIndex>(idx: &S, q: &str) -> Vec<Concept> { idx.search(q) }

// ── helpers ───────────────────────────────────────────────────────────────────

/// Mirror of Python `touchstone.cli.slugify`.
fn slugify(s: &str) -> String {
    let mut out: String = s
        .chars()
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    while out.contains("--") {
        out = out.replace("--", "-");
    }
    let out = out.trim_matches('-');
    if out.is_empty() { "untitled".to_string() } else { out.to_string() }
}

/// Generate a 12-hex-character concept ID using std-only entropy sources.
///
/// Combines monotonic-ish time with thread identity for enough diversity that
/// sequential calls in the same process don't collide. Not cryptographically
/// strong — that is not needed here; the spec only requires uniqueness within a
/// bundle for routing purposes.
fn generate_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    // Mix in the thread id to differentiate concurrent callers.
    let tid_str = format!("{:?}", std::thread::current().id());
    let tid_hash = tid_str
        .bytes()
        .fold(0u64, |acc, b| acc.wrapping_mul(131).wrapping_add(b as u64));
    let mixed = nanos ^ tid_hash;
    // Mask to 48 bits so the hex representation is always exactly 12 characters.
    format!("{:012x}", mixed & 0x0000_ffff_ffff_ffff)
}

// ── lint helpers (pure domain logic) ─────────────────────────────────────────

fn find_duplicates(items: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashMap::<&str, usize>::new();
    for item in items {
        *seen.entry(item.as_str()).or_insert(0) += 1;
    }
    let mut dupes: Vec<String> = seen
        .into_iter()
        .filter(|(_, n)| *n > 1)
        .map(|(s, _)| s.to_string())
        .collect();
    dupes.sort();
    dupes
}

// ── IndexBundle ───────────────────────────────────────────────────────────────

/// Summary returned by `index_bundle_full`.
#[derive(Debug, Default)]
pub struct IndexStats {
    pub total: usize,
    pub errors: Vec<(String, String)>, // (path, error)
}

/// Walk `repo`, parse every concept with `parser`, and populate `writer`.
///
/// Mirrors Python `cmd_index` minus the incremental-digest optimisation and the
/// index.md renderer (both belong at higher layers). This layer is pure coordination
/// over port traits.
pub fn index_bundle_full<R, P, W>(repo: &R, parser: &P, writer: &mut W) -> IndexStats
where
    R: ConceptRepository + RawStore,
    P: ConceptParser,
    W: IndexPopulator,
{
    let concepts = repo.paths();
    let mut stats = IndexStats { total: concepts.len(), ..Default::default() };

    for path in &concepts {
        let raw = match repo.raw_bytes(path) {
            Some(b) => b,
            None => {
                stats.errors.push((path.clone(), "raw bytes not found".to_string()));
                continue;
            }
        };
        let parsed = parser.parse(path, &raw);
        if let Some(ref err) = parsed.error {
            stats.errors.push((path.clone(), err.clone()));
        }
        // Upsert even non-conformant concepts so the index reflects every file.
        if let Err(e) = writer.upsert(
            &parsed.path,
            &parsed.concept_type,
            &parsed.title,
            &parsed.description,
            &parsed.body,
            &parsed.tags,
            parsed.trust,
            &parsed.status,
        ) {
            stats.errors.push((path.clone(), e));
        }
    }
    stats
}

// ── SearchBundle ──────────────────────────────────────────────────────────────

/// Result of `search_bundle_full`.
#[derive(Debug, Default)]
pub struct SearchResult {
    pub hits: Vec<SearchHit>,
}

/// Structured prefilter → index query → ranked hits.
///
/// Graph expansion and trust-rank are delegated to `idx` (the FilteredSearch adapter),
/// matching the ARCHITECTURE.md retrieval pipeline.
pub fn search_bundle_full<S: FilteredSearch>(idx: &S, query: &SearchQuery) -> SearchResult {
    SearchResult { hits: idx.search_filtered(query) }
}

// ── ExportBundle ──────────────────────────────────────────────────────────────

/// Summary returned by `export_bundle`.
#[derive(Debug, Default)]
pub struct ExportStats {
    pub count: usize,
}

/// Write raw bytes for every concept in `repo` to `sink`, byte-exact.
///
/// "Raw bytes are authoritative" (README design rule): export copies the bytes without
/// touching them. This is what T2a asserts.
pub fn export_bundle<R, W>(repo: &R, sink: &W) -> Result<ExportStats, String>
where
    R: ConceptRepository + RawStore,
    W: ConceptSink,
{
    let concepts = repo.paths();
    let mut count = 0;
    for path in &concepts {
        let raw = repo.raw_bytes(path)
            .ok_or_else(|| format!("raw bytes not found for {path}"))?;
        sink.write(path, &raw)?;
        count += 1;
    }
    Ok(ExportStats { count })
}

// ── LintBundle ────────────────────────────────────────────────────────────────

/// A single lint problem.
#[derive(Debug, Clone)]
pub struct LintProblem {
    pub path: String,
    pub message: String,
}

/// Report of all lint problems found.
#[derive(Debug, Default)]
pub struct LintReport {
    pub problems: Vec<LintProblem>,
}

impl LintReport {
    pub fn total(&self) -> usize { self.problems.len() }
    pub fn is_clean(&self) -> bool { self.problems.is_empty() }
}

fn push(report: &mut LintReport, path: &str, msg: impl Into<String>) {
    report.problems.push(LintProblem { path: path.to_string(), message: msg.into() });
}

/// Lint all concepts in `repo` against OKF conformance rules.
///
/// Rules (from Python `okf.lint`, FINDINGS.md E2 & E3d):
/// 1. Parse/conformance error.
/// 2. Duplicate tags.
/// 3. `verified` entries missing required `by`.
/// 4. Duplicate verified principals.
/// 5. Sources entry missing required `resource`.
/// 6. Invalid `status` (must be draft | stable | deprecated).
/// 7. `[[wikilink]]` syntax in body -- not OKF, will not resolve.
pub fn lint_bundle<R, P>(repo: &R, parser: &P) -> LintReport
where
    R: ConceptRepository + RawStore,
    P: ConceptParser,
{
    let mut report = LintReport::default();
    for path in &repo.paths() {
        let raw = match repo.raw_bytes(path) {
            Some(b) => b,
            None => {
                push(&mut report, path, "raw bytes not found");
                continue;
            }
        };
        let parsed = parser.parse(path, &raw);
        lint_one(&parsed, &mut report);
    }
    report
}

pub fn lint_one(c: &ParsedConcept, report: &mut LintReport) {
    let path = &c.path;

    if let Some(ref err) = c.error {
        push(report, path, format!("error: {err}"));
        if c.concept_type.is_empty() {
            return; // can't lint further without a valid parse
        }
    }

    let tag_dupes = find_duplicates(&c.tags);
    if !tag_dupes.is_empty() {
        push(report, path, format!("duplicate tags: {}", tag_dupes.join(", ")));
    }

    let bys: Vec<String> = c.verified_entries.iter().filter_map(|e| e.by.clone()).collect();
    let principal_dupes = find_duplicates(&bys);
    if !principal_dupes.is_empty() {
        push(report, path, format!("duplicate verified principals: {}", principal_dupes.join(", ")));
    }
    for entry in &c.verified_entries {
        if entry.by.is_none() {
            push(report, path, "verified entry missing required `by`");
        }
    }

    if c.has_source_missing_resource {
        push(report, path, "source entry missing required `resource`");
    }

    if !c.status.is_empty()
        && !matches!(c.status.as_str(), "draft" | "stable" | "deprecated")
    {
        push(report, path, format!("status `{}` is not draft|stable|deprecated", c.status));
    }

    if c.has_wikilinks {
        push(report, path, "body contains [[wikilinks]] -- not OKF, will not resolve");
    }
}

// ── FormatBundle ──────────────────────────────────────────────────────────────

/// Report returned by `format_bundle`.
#[derive(Debug, Default)]
pub struct FormatReport {
    /// Paths that were (or would be, in check mode) reformatted.
    pub changed: Vec<String>,
    /// Paths skipped, with the reason each was skipped.
    pub skipped: Vec<(String, String)>,
}

impl FormatReport {
    pub fn needs_formatting(&self) -> bool { !self.changed.is_empty() }
}

/// Canonicalize frontmatter for all formattable concepts.
///
/// Mirrors Python `cmd_fmt` (FINDINGS.md E3b): concepts containing anchors, aliases,
/// merge keys, or block scalars are REFUSED rather than mangled. The adapter's
/// `canonicalize` returns `None` for those; `ParsedConcept.format_skip_reason` records
/// why.
///
/// When `check_only` is true, the sink is never written; `report.changed` still lists
/// what would change.
pub fn format_bundle<R, P, W>(
    repo: &R,
    parser: &P,
    sink: &W,
    check_only: bool,
) -> FormatReport
where
    R: ConceptRepository + RawStore,
    P: ConceptParser,
    W: ConceptSink,
{
    let mut report = FormatReport::default();

    for path in &repo.paths() {
        let raw = match repo.raw_bytes(path) {
            Some(b) => b,
            None => {
                report.skipped.push((path.clone(), "raw bytes not found".to_string()));
                continue;
            }
        };
        let parsed = parser.parse(path, &raw);

        // Skip if the parser says formatting is unsafe.
        if let Some(ref reason) = parsed.format_skip_reason {
            // Only report the skip for files that have some frontmatter (Python behaviour:
            // files with no frontmatter at all are silently skipped).
            if !parsed.concept_type.is_empty() || parsed.error.is_some() {
                report.skipped.push((path.clone(), reason.clone()));
            }
            continue;
        }
        if let Some(ref err) = parsed.error {
            report.skipped.push((path.clone(), err.clone()));
            continue;
        }

        let new_bytes = match parser.canonicalize(&parsed) {
            Some(b) => b,
            None => {
                report.skipped.push((path.clone(), "canonicalize returned None".to_string()));
                continue;
            }
        };

        // T2e: re-parse and verify that canonicalization preserved field values.
        let re_parsed = parser.parse(path, &new_bytes);
        if re_parsed.concept_type != parsed.concept_type
            || re_parsed.title != parsed.title
            || re_parsed.description != parsed.description
            || re_parsed.tags != parsed.tags
            || re_parsed.status != parsed.status
        {
            report.skipped.push((
                path.clone(),
                "reserialization would change values".to_string(),
            ));
            continue;
        }

        // Only count/write if content actually changed.
        if new_bytes != raw {
            report.changed.push(path.clone());
            if !check_only {
                if let Err(e) = sink.write(path, &new_bytes) {
                    report.skipped.push((path.clone(), format!("write failed: {e}")));
                }
            }
        }
    }
    report
}

// ── CaptureConcept ────────────────────────────────────────────────────────────

/// Parameters for scaffolding a new concept. Mirrors Python `cmd_new` options.
#[derive(Debug, Clone, Default)]
pub struct CaptureRequest {
    /// OKF concept type, e.g. `Note`, `Decision`, `Runbook`.
    pub concept_type: String,
    pub title: String,
    pub description: String,
    pub tags: Vec<String>,
    /// Subdirectory override; defaults to `<type.to_lowercase()>s/`.
    pub subdir: Option<String>,
    /// Agent identifier for the `generated:` block. `None` → no generated key.
    pub generated_by: Option<String>,
}

/// Scaffold a new conformant concept and write it to `sink`.
///
/// Returns the bundle-relative path on success, or an error string when the path
/// already exists. Mirrors Python `cmd_new`: slug from title, emit through
/// `ConceptParser.emit_new` (never string concatenation).
pub fn capture_concept<P, W, C>(req: &CaptureRequest, parser: &P, sink: &W, clock: &C)
    -> Result<String, String>
where
    P: ConceptParser,
    W: ConceptSink,
    C: Clock + ?Sized,
{
    let subdir = req
        .subdir
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("{}s", req.concept_type.to_ascii_lowercase()));
    let slug = slugify(&req.title);
    let rel = format!("{subdir}/{slug}.md");

    if sink.exists(&rel) {
        return Err(format!("exists: {rel}"));
    }

    let new_req = NewConceptRequest {
        concept_type: req.concept_type.clone(),
        title: req.title.clone(),
        description: req.description.clone(),
        tags: req.tags.clone(),
        generated_by: req.generated_by.clone(),
        now_iso8601: clock.now_iso8601(),
        id: generate_id(),
    };
    let bytes = parser.emit_new(&new_req);
    sink.write(&rel, &bytes)?;
    Ok(rel)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use touchstone_ports::{
        Concept, ConceptParser, ConceptRepository, ConceptSink, FilteredSearch, IndexPopulator,
        NewConceptRequest, ParsedConcept, RawStore, SearchHit, SearchQuery, SearchVia, Trust,
        VerifiedEntry,
    };
    use std::cell::RefCell;
    use std::collections::HashMap;

    // ── In-memory fakes ───────────────────────────────────────────────────────

    /// In-memory bundle: path → raw bytes. Implements ConceptRepository + RawStore.
    struct FakeBundle(HashMap<String, Vec<u8>>);

    impl FakeBundle {
        fn new(entries: &[(&str, &str)]) -> Self {
            FakeBundle(
                entries.iter().map(|(p, r)| (p.to_string(), r.as_bytes().to_vec())).collect(),
            )
        }
    }

    impl ConceptRepository for FakeBundle {
        fn paths(&self) -> Vec<String> {
            let mut v: Vec<String> = self.0.keys().cloned().collect();
            v.sort();
            v
        }
    }

    impl RawStore for FakeBundle {
        fn raw_bytes(&self, path: &str) -> Option<Vec<u8>> { self.0.get(path).cloned() }
    }

    /// Minimal parser: extracts type, title, description, status, tags from
    /// `key: value` lines. Sufficient for use-case tests without a YAML library.
    struct FakeParser;

    impl FakeParser {
        fn field<'a>(text: &'a str, key: &str) -> Option<&'a str> {
            text.lines()
                .find(|l| l.starts_with(&format!("{key}:")))
                .map(|l| l[key.len() + 1..].trim())
        }
    }

    impl ConceptParser for FakeParser {
        fn parse(&self, path: &str, raw: &[u8]) -> ParsedConcept {
            let text = std::str::from_utf8(raw).unwrap_or("");
            let concept_type = Self::field(text, "type").unwrap_or("").to_string();
            let title       = Self::field(text, "title").unwrap_or("").to_string();
            let description = Self::field(text, "description").unwrap_or("").to_string();
            let status      = Self::field(text, "status").unwrap_or("stable").to_string();
            let tags: Vec<String> = Self::field(text, "tags")
                .map(|v| {
                    let inner = v.trim_matches(|c| c == '[' || c == ']');
                    if inner.is_empty() { vec![] }
                    else { inner.split(',').map(|s| s.trim().to_string()).collect() }
                })
                .unwrap_or_default();
            let trust = if text.contains("human:") { Trust::Verified }
                else if text.contains("generated:") { Trust::Generated }
                else { Trust::Unknown };
            let error = if concept_type.is_empty() {
                Some("missing or empty `type`".to_string())
            } else { None };
            // Detect risky constructs for FormatBundle tests.
            let format_skip_reason = if text.contains("<<:") || text.contains("&id") || text.contains(": |") {
                Some("contains anchors, aliases, merge keys or block scalars".to_string())
            } else {
                error.clone()
            };
            let has_wikilinks = text.contains("[[");
            ParsedConcept {
                path: path.to_string(),
                concept_type,
                title,
                description,
                body: text.to_string(),
                tags,
                trust,
                status,
                raw: raw.to_vec(),
                error,
                format_skip_reason,
                verified_entries: vec![],
                has_source_missing_resource: false,
                has_wikilinks,
                // The fake carries no frontmatter view: these tests exercise use-case
                // coordination, and the real JSON projection is the parser adapter's job.
                frontmatter_json: String::new(),
            }
        }

        fn canonicalize(&self, parsed: &ParsedConcept) -> Option<Vec<u8>> {
            if parsed.format_skip_reason.is_some() { return None; }
            Some(parsed.raw.clone()) // identity: already canonical in the fake
        }

        fn emit_new(&self, req: &NewConceptRequest) -> Vec<u8> {
            let gen = match &req.generated_by {
                Some(by) => format!("generated:\n  by: {by}\n  at: {}\n", req.now_iso8601),
                None => String::new(),
            };
            let tags = if req.tags.is_empty() { "[]".to_string() }
                else { format!("[{}]", req.tags.join(", ")) };
            format!(
                "---\nid: {}\ntype: {}\ntitle: {}\ndescription: {}\ntags: {}\nstatus: draft\n{gen}---\n\n# {}\n",
                req.id, req.concept_type, req.title, req.description, tags, req.title,
            ).into_bytes()
        }
    }

    /// In-memory index writer.
    #[derive(Default)]
    struct FakeIndex {
        records: Vec<(String, String)>, // (path, concept_type)
    }

    impl IndexPopulator for FakeIndex {
        fn upsert(&mut self, path: &str, concept_type: &str, _title: &str,
                  _desc: &str, _body: &str, _tags: &[String], _trust: Trust, _status: &str,
        ) -> Result<(), String> {
            self.records.retain(|(p, _)| p != path);
            self.records.push((path.to_string(), concept_type.to_string()));
            Ok(())
        }
        fn remove(&mut self, path: &str) -> Result<(), String> {
            self.records.retain(|(p, _)| p != path);
            Ok(())
        }
    }

    /// In-memory write sink.
    #[derive(Default)]
    struct FakeSink {
        files: RefCell<HashMap<String, Vec<u8>>>,
    }

    impl ConceptSink for FakeSink {
        fn write(&self, path: &str, raw: &[u8]) -> Result<(), String> {
            self.files.borrow_mut().insert(path.to_string(), raw.to_vec());
            Ok(())
        }
        fn exists(&self, path: &str) -> bool { self.files.borrow().contains_key(path) }
    }

    /// In-memory filtered search with optional type prefilter.
    struct FakeSearch { hits: Vec<SearchHit> }

    impl FilteredSearch for FakeSearch {
        fn search_filtered(&self, q: &SearchQuery) -> Vec<SearchHit> {
            let limit = if q.limit == 0 { 10 } else { q.limit };
            self.hits.iter()
                .filter(|h| q.concept_type.as_deref()
                    .map(|t| h.concept_type == t).unwrap_or(true))
                .take(limit)
                .cloned()
                .collect()
        }
    }

    struct FakeClock(String);
    impl Clock for FakeClock { fn now_iso8601(&self) -> String { self.0.clone() } }

    // ── slugify ───────────────────────────────────────────────────────────────

    #[test]
    fn slugify_basic() { assert_eq!(slugify("Hello World"), "hello-world"); }

    #[test]
    fn slugify_collapses_hyphens() { assert_eq!(slugify("foo--bar"), "foo-bar"); }

    #[test]
    fn slugify_trims_hyphens() { assert_eq!(slugify("--foo--"), "foo"); }

    #[test]
    fn slugify_empty_returns_untitled() {
        assert_eq!(slugify(""), "untitled");
        assert_eq!(slugify("---"), "untitled");
    }

    #[test]
    fn slugify_preserves_alphanumeric() { assert_eq!(slugify("abc123"), "abc123"); }

    // ── IndexBundle ───────────────────────────────────────────────────────────

    #[test]
    fn index_bundle_full_counts_concepts() {
        let bundle = FakeBundle::new(&[
            ("notes/a.md", "---\ntype: Note\ntitle: A\n---\nBody.\n"),
            ("notes/b.md", "---\ntype: Note\ntitle: B\n---\nBody.\n"),
        ]);
        let mut idx = FakeIndex::default();
        let stats = index_bundle_full(&bundle, &FakeParser, &mut idx);
        assert_eq!(stats.total, 2);
        assert!(stats.errors.is_empty());
        assert_eq!(idx.records.len(), 2);
    }

    #[test]
    fn index_bundle_full_upsert_is_idempotent() {
        let bundle = FakeBundle::new(&[
            ("notes/a.md", "---\ntype: Note\ntitle: A\n---\n"),
        ]);
        let mut idx = FakeIndex::default();
        index_bundle_full(&bundle, &FakeParser, &mut idx);
        index_bundle_full(&bundle, &FakeParser, &mut idx);
        assert_eq!(idx.records.len(), 1, "upsert must not duplicate records");
    }

    #[test]
    fn index_bundle_full_records_parse_errors() {
        let bundle = FakeBundle::new(&[("bad.md", "no frontmatter here")]);
        let mut idx = FakeIndex::default();
        let stats = index_bundle_full(&bundle, &FakeParser, &mut idx);
        assert_eq!(stats.total, 1);
        assert!(!stats.errors.is_empty());
        assert_eq!(stats.errors[0].0, "bad.md");
    }

    #[test]
    fn index_bundle_full_indexes_all_types() {
        let bundle = FakeBundle::new(&[
            ("notes/note.md",   "---\ntype: Note\ntitle: A\n---\n"),
            ("decisions/d.md",  "---\ntype: Decision\ntitle: D\n---\n"),
            ("logs/log.md",     "---\ntype: Log\ntitle: L\n---\n"),
        ]);
        let mut idx = FakeIndex::default();
        let stats = index_bundle_full(&bundle, &FakeParser, &mut idx);
        assert_eq!(stats.total, 3);
        assert!(stats.errors.is_empty());
        assert_eq!(idx.records.len(), 3);
    }

    #[test]
    fn index_bundle_full_extracts_correct_type() {
        let bundle = FakeBundle::new(&[
            ("metrics/rev.md", "---\ntype: Metric\ntitle: Revenue\n---\n"),
        ]);
        let mut idx = FakeIndex::default();
        index_bundle_full(&bundle, &FakeParser, &mut idx);
        assert_eq!(idx.records[0], ("metrics/rev.md".to_string(), "Metric".to_string()));
    }

    // ── SearchBundle ──────────────────────────────────────────────────────────

    #[test]
    fn search_bundle_full_returns_hits() {
        let search = FakeSearch {
            hits: vec![SearchHit { path: "notes/a.md".into(), concept_type: "Note".into(), title: "Alpha".into(), description: String::new(), trust: Trust::default(), via: SearchVia::Direct }],
        };
        let result = search_bundle_full(&search, &SearchQuery { text: "alpha".into(), limit: 10, ..Default::default() });
        assert_eq!(result.hits.len(), 1);
        assert_eq!(result.hits[0].path, "notes/a.md");
        assert_eq!(result.hits[0].via, SearchVia::Direct);
    }

    #[test]
    fn search_bundle_full_applies_type_filter() {
        let search = FakeSearch {
            hits: vec![
                SearchHit { path: "n/a.md".into(), concept_type: "Note".into(), title: "".into(), description: String::new(), trust: Trust::default(), via: SearchVia::Direct },
                SearchHit { path: "d/b.md".into(), concept_type: "Decision".into(), title: "".into(), description: String::new(), trust: Trust::default(), via: SearchVia::Direct },
            ],
        };
        let result = search_bundle_full(&search, &SearchQuery {
            text: "q".into(), concept_type: Some("Note".into()), limit: 10, ..Default::default()
        });
        assert_eq!(result.hits.len(), 1);
        assert_eq!(result.hits[0].concept_type, "Note");
    }

    #[test]
    fn search_bundle_full_empty_query_returns_nothing() {
        let search = FakeSearch { hits: vec![] };
        let result = search_bundle_full(&search, &SearchQuery::default());
        assert!(result.hits.is_empty());
    }

    #[test]
    fn search_bundle_full_respects_limit() {
        let hits: Vec<SearchHit> = (0..20)
            .map(|i| SearchHit {
                path: format!("n/{i}.md"),
                concept_type: "Note".into(),
                title: format!("{i}"),
                description: String::new(),
                trust: Trust::default(),
                via: SearchVia::Direct,
            })
            .collect();
        let search = FakeSearch { hits };
        let result = search_bundle_full(&search, &SearchQuery { text: "x".into(), limit: 5, ..Default::default() });
        assert_eq!(result.hits.len(), 5);
    }

    #[test]
    fn search_bundle_full_link_via_preserved() {
        let search = FakeSearch {
            hits: vec![SearchHit { path: "n/linked.md".into(), concept_type: "Note".into(), title: "".into(), description: String::new(), trust: Trust::default(), via: SearchVia::Link }],
        };
        let result = search_bundle_full(&search, &SearchQuery { text: "x".into(), limit: 10, ..Default::default() });
        assert_eq!(result.hits[0].via, SearchVia::Link);
    }

    // ── ExportBundle ──────────────────────────────────────────────────────────

    #[test]
    fn export_bundle_writes_all_raw_bytes() {
        let a = "---\ntype: Note\ntitle: A\n---\nBody A.\n";
        let b = "---\ntype: Note\ntitle: B\n---\nBody B.\n";
        let bundle = FakeBundle::new(&[("notes/a.md", a), ("notes/b.md", b)]);
        let sink = FakeSink::default();
        let stats = export_bundle(&bundle, &sink).unwrap();
        assert_eq!(stats.count, 2);
        let files = sink.files.borrow();
        assert_eq!(files.get("notes/a.md").unwrap(), a.as_bytes());
        assert_eq!(files.get("notes/b.md").unwrap(), b.as_bytes());
    }

    #[test]
    fn export_bundle_is_byte_exact_crlf() {
        // T2a: byte-exact round-trip through CRLF.
        let crlf = "---\r\ntype: Note\r\ntitle: CRLF\r\n---\r\nBody.\r\n";
        let bundle = FakeBundle::new(&[("crlf/concept.md", crlf)]);
        let sink = FakeSink::default();
        export_bundle(&bundle, &sink).unwrap();
        let files = sink.files.borrow();
        assert_eq!(files.get("crlf/concept.md").unwrap(), crlf.as_bytes(), "must be byte-exact");
    }

    #[test]
    fn export_bundle_empty_bundle() {
        let bundle = FakeBundle::new(&[]);
        let sink = FakeSink::default();
        let stats = export_bundle(&bundle, &sink).unwrap();
        assert_eq!(stats.count, 0);
        assert!(sink.files.borrow().is_empty());
    }

    #[test]
    fn export_bundle_unicode_path() {
        let content = "---\ntype: Note\ntitle: Unicode\n---\nBody.\n";
        let bundle = FakeBundle::new(&[("notes/café.md", content)]);
        let sink = FakeSink::default();
        let stats = export_bundle(&bundle, &sink).unwrap();
        assert_eq!(stats.count, 1);
        let files = sink.files.borrow();
        assert!(files.contains_key("notes/café.md"));
    }

    // ── LintBundle ────────────────────────────────────────────────────────────

    #[test]
    fn lint_clean_concept_no_problems() {
        let bundle = FakeBundle::new(&[
            ("notes/a.md", "---\ntype: Note\ntitle: A\nstatus: stable\n---\nBody.\n"),
        ]);
        let report = lint_bundle(&bundle, &FakeParser);
        assert!(report.is_clean(), "clean concept must produce no problems");
    }

    #[test]
    fn lint_missing_type_reported() {
        let bundle = FakeBundle::new(&[("bad.md", "---\ntitle: No type\n---\n")]);
        let report = lint_bundle(&bundle, &FakeParser);
        assert!(!report.is_clean());
        assert!(report.problems[0].message.contains("type"));
    }

    #[test]
    fn lint_invalid_status_reported() {
        let bundle = FakeBundle::new(&[
            ("notes/a.md", "---\ntype: Note\ntitle: A\nstatus: active\n---\n"),
        ]);
        let report = lint_bundle(&bundle, &FakeParser);
        assert!(report.problems.iter().any(|p| p.message.contains("status")));
    }

    #[test]
    fn lint_wikilinks_reported() {
        let bundle = FakeBundle::new(&[
            ("notes/a.md", "---\ntype: Note\ntitle: A\n---\n[[Some link]]"),
        ]);
        let report = lint_bundle(&bundle, &FakeParser);
        assert!(report.problems.iter().any(|p| p.message.contains("wikilink")));
    }

    #[test]
    fn lint_duplicate_tags_reported() {
        let parsed = ParsedConcept {
            path: "a.md".into(),
            concept_type: "Note".into(),
            status: "stable".into(),
            tags: vec!["rust".into(), "rust".into()],
            ..Default::default()
        };
        let mut report = LintReport::default();
        lint_one(&parsed, &mut report);
        assert!(report.problems.iter().any(|p| p.message.contains("duplicate tags")));
    }

    #[test]
    fn lint_missing_verified_by_reported() {
        let parsed = ParsedConcept {
            path: "a.md".into(),
            concept_type: "Note".into(),
            status: "stable".into(),
            verified_entries: vec![VerifiedEntry { by: None }],
            ..Default::default()
        };
        let mut report = LintReport::default();
        lint_one(&parsed, &mut report);
        assert!(report.problems.iter().any(|p| p.message.contains("missing required `by`")));
    }

    #[test]
    fn lint_duplicate_principal_reported() {
        let parsed = ParsedConcept {
            path: "a.md".into(),
            concept_type: "Note".into(),
            status: "stable".into(),
            verified_entries: vec![
                VerifiedEntry { by: Some("human:gary".into()) },
                VerifiedEntry { by: Some("human:gary".into()) },
            ],
            ..Default::default()
        };
        let mut report = LintReport::default();
        lint_one(&parsed, &mut report);
        assert!(report.problems.iter().any(|p| p.message.contains("duplicate verified principals")));
    }

    #[test]
    fn lint_source_missing_resource_reported() {
        let parsed = ParsedConcept {
            path: "a.md".into(),
            concept_type: "Note".into(),
            status: "stable".into(),
            has_source_missing_resource: true,
            ..Default::default()
        };
        let mut report = LintReport::default();
        lint_one(&parsed, &mut report);
        assert!(report.problems.iter().any(|p| p.message.contains("resource")));
    }

    #[test]
    fn lint_aggregates_across_multiple_files() {
        let bundle = FakeBundle::new(&[
            ("a.md", "---\ntype: Note\ntitle: A\nstatus: active\n---\n"),
            ("b.md", "---\ntype: Note\ntitle: B\nstatus: stable\n---\n"),
        ]);
        let report = lint_bundle(&bundle, &FakeParser);
        assert_eq!(report.problems.iter().filter(|p| p.path == "a.md").count(), 1);
        assert_eq!(report.problems.iter().filter(|p| p.path == "b.md").count(), 0);
    }

    // ── FormatBundle ──────────────────────────────────────────────────────────

    #[test]
    fn format_check_only_does_not_write() {
        // Parser that always returns different canonical bytes to trigger a "change."
        struct AlwaysChanges;
        impl ConceptParser for AlwaysChanges {
            fn parse(&self, path: &str, raw: &[u8]) -> ParsedConcept { FakeParser.parse(path, raw) }
            fn canonicalize(&self, _: &ParsedConcept) -> Option<Vec<u8>> {
                Some(b"---\ntype: Note\ntitle: A\nstatus: stable\n---\n\n".to_vec())
            }
            fn emit_new(&self, req: &NewConceptRequest) -> Vec<u8> { FakeParser.emit_new(req) }
        }
        let bundle = FakeBundle::new(&[("notes/a.md", "---\ntype: Note\ntitle: A\nstatus: stable\n---\n")]);
        let sink = FakeSink::default();
        let report = format_bundle(&bundle, &AlwaysChanges, &sink, true);
        assert_eq!(report.changed.len(), 1);
        assert!(sink.files.borrow().is_empty(), "check mode must not write");
    }

    #[test]
    fn format_skips_risky_constructs() {
        let content = "---\ntype: Note\ntitle: A\nverified:\n  - <<: *defaults\n---\n";
        let bundle = FakeBundle::new(&[("notes/a.md", content)]);
        let sink = FakeSink::default();
        let report = format_bundle(&bundle, &FakeParser, &sink, false);
        assert!(report.skipped.iter().any(|(p, _)| p == "notes/a.md"));
        assert!(sink.files.borrow().is_empty());
    }

    #[test]
    fn format_unchanged_content_not_written() {
        // FakeParser.canonicalize returns identity bytes → no change.
        let content = "---\ntype: Note\ntitle: A\nstatus: stable\n---\nBody.\n";
        let bundle = FakeBundle::new(&[("notes/a.md", content)]);
        let sink = FakeSink::default();
        let report = format_bundle(&bundle, &FakeParser, &sink, false);
        assert!(report.changed.is_empty());
        assert!(sink.files.borrow().is_empty());
    }

    #[test]
    fn format_writes_changed_content() {
        const CANONICAL: &[u8] = b"---\nstatus: stable\ntype: Note\ntitle: A\n---\n";

        struct ReorderParser;
        impl ConceptParser for ReorderParser {
            fn parse(&self, path: &str, raw: &[u8]) -> ParsedConcept { FakeParser.parse(path, raw) }
            fn canonicalize(&self, _: &ParsedConcept) -> Option<Vec<u8>> { Some(CANONICAL.to_vec()) }
            fn emit_new(&self, req: &NewConceptRequest) -> Vec<u8> { FakeParser.emit_new(req) }
        }

        let bundle = FakeBundle::new(&[("notes/a.md", "---\ntype: Note\ntitle: A\n---\n")]);
        let sink = FakeSink::default();
        let report = format_bundle(&bundle, &ReorderParser, &sink, false);
        assert_eq!(report.changed, vec!["notes/a.md"]);
        let files = sink.files.borrow();
        assert_eq!(files.get("notes/a.md").unwrap(), CANONICAL);
    }

    #[test]
    fn format_skips_when_reserialize_changes_values() {
        // Parser that returns canonical bytes but those bytes parse differently.
        struct BadCanonicalizer;
        impl ConceptParser for BadCanonicalizer {
            fn parse(&self, path: &str, raw: &[u8]) -> ParsedConcept { FakeParser.parse(path, raw) }
            fn canonicalize(&self, _: &ParsedConcept) -> Option<Vec<u8>> {
                // Returns bytes where type is different — T2e should reject this.
                Some(b"---\ntype: Different\ntitle: A\n---\n".to_vec())
            }
            fn emit_new(&self, req: &NewConceptRequest) -> Vec<u8> { FakeParser.emit_new(req) }
        }

        let bundle = FakeBundle::new(&[("notes/a.md", "---\ntype: Note\ntitle: A\n---\n")]);
        let sink = FakeSink::default();
        let report = format_bundle(&bundle, &BadCanonicalizer, &sink, false);
        assert!(report.skipped.iter().any(|(_, reason)| reason.contains("reserialization")));
        assert!(sink.files.borrow().is_empty());
    }

    // ── CaptureConcept ────────────────────────────────────────────────────────

    #[test]
    fn capture_creates_correct_path() {
        let req = CaptureRequest { concept_type: "Note".into(), title: "My New Note".into(), ..Default::default() };
        let sink = FakeSink::default();
        let path = capture_concept(&req, &FakeParser, &sink, &FakeClock("2026-08-01T00:00:00Z".into())).unwrap();
        assert_eq!(path, "notes/my-new-note.md");
    }

    #[test]
    fn capture_uses_subdir_override() {
        let req = CaptureRequest {
            concept_type: "Decision".into(),
            title: "Use Rust".into(),
            subdir: Some("arch/decisions".into()),
            ..Default::default()
        };
        let sink = FakeSink::default();
        let path = capture_concept(&req, &FakeParser, &sink, &FakeClock("2026-08-01T00:00:00Z".into())).unwrap();
        assert_eq!(path, "arch/decisions/use-rust.md");
    }

    #[test]
    fn capture_fails_if_path_exists() {
        let req = CaptureRequest { concept_type: "Note".into(), title: "Existing".into(), ..Default::default() };
        let sink = FakeSink::default();
        sink.write("notes/existing.md", b"content").unwrap();
        let result = capture_concept(&req, &FakeParser, &sink, &FakeClock("2026-08-01T00:00:00Z".into()));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("exists:"));
    }

    #[test]
    fn capture_content_is_parseable() {
        let req = CaptureRequest {
            concept_type: "Runbook".into(),
            title: "Deploy Service".into(),
            description: "How to deploy".into(),
            tags: vec!["ops".into()],
            ..Default::default()
        };
        let sink = FakeSink::default();
        let path = capture_concept(&req, &FakeParser, &sink, &FakeClock("2026-08-01T00:00:00Z".into())).unwrap();
        let files = sink.files.borrow();
        let raw = files.get(&path).unwrap();
        let parsed = FakeParser.parse(&path, raw);
        assert_eq!(parsed.concept_type, "Runbook");
        assert_eq!(parsed.title, "Deploy Service");
        assert_eq!(parsed.description, "How to deploy");
    }

    #[test]
    fn capture_with_generated_by() {
        let req = CaptureRequest {
            concept_type: "Note".into(),
            title: "Generated Note".into(),
            generated_by: Some("capture/claude-opus-5".into()),
            ..Default::default()
        };
        let sink = FakeSink::default();
        let path = capture_concept(&req, &FakeParser, &sink, &FakeClock("2026-08-01T00:00:00Z".into())).unwrap();
        let files = sink.files.borrow();
        let text = std::str::from_utf8(files.get(&path).unwrap()).unwrap();
        assert!(text.contains("generated:"));
        assert!(text.contains("capture/claude-opus-5"));
    }

    #[test]
    fn capture_status_is_draft() {
        let req = CaptureRequest { concept_type: "Note".into(), title: "Draft".into(), ..Default::default() };
        let sink = FakeSink::default();
        let path = capture_concept(&req, &FakeParser, &sink, &FakeClock("2026-08-01T00:00:00Z".into())).unwrap();
        let files = sink.files.borrow();
        let parsed = FakeParser.parse(&path, files.get(&path).unwrap());
        assert_eq!(parsed.status, "draft");
    }

    #[test]
    fn capture_id_is_12_hex_chars() {
        let req = CaptureRequest { concept_type: "Note".into(), title: "ID Test".into(), ..Default::default() };
        let sink = FakeSink::default();
        let path = capture_concept(&req, &FakeParser, &sink, &FakeClock("2026-08-01T00:00:00Z".into())).unwrap();
        let files = sink.files.borrow();
        let text = std::str::from_utf8(files.get(&path).unwrap()).unwrap();
        let id_val = text.lines()
            .find(|l| l.starts_with("id:")).expect("id field must be present")
            .strip_prefix("id:").unwrap().trim();
        assert_eq!(id_val.len(), 12, "id must be 12 chars");
        assert!(id_val.chars().all(|c| c.is_ascii_hexdigit()), "id must be hex: {id_val}");
    }

    #[test]
    fn capture_timestamp_comes_from_clock() {
        let ts = "2026-08-01T12:34:56Z";
        let req = CaptureRequest {
            concept_type: "Note".into(),
            title: "Timestamp Test".into(),
            generated_by: Some("agent:test".into()),
            ..Default::default()
        };
        let sink = FakeSink::default();
        let path = capture_concept(&req, &FakeParser, &sink, &FakeClock(ts.into())).unwrap();
        let files = sink.files.borrow();
        let text = std::str::from_utf8(files.get(&path).unwrap()).unwrap();
        assert!(text.contains(ts), "timestamp must come from the Clock port");
    }

    // ── legacy stubs still compile ────────────────────────────────────────────

    #[test]
    fn legacy_stubs_compile() {
        struct MinRepo;
        impl ConceptRepository for MinRepo { fn paths(&self) -> Vec<String> { vec![] } }
        struct MinIdx;
        impl touchstone_ports::SearchIndex for MinIdx { fn search(&self, _: &str) -> Vec<Concept> { vec![] } }
        assert_eq!(index_bundle(&MinRepo), 0);
        assert!(search_bundle(&MinIdx, "q").is_empty());
    }
}

// ── ReindexBundle ─────────────────────────────────────────────────────────────

/// What a full reindex did.
#[derive(Debug, Default)]
pub struct ReindexReport {
    pub total: usize,
    pub new: usize,
    pub changed: usize,
    pub removed: usize,
    pub indexes_written: usize,
    pub broken_links: usize,
    /// `(path, error)` for every concept that failed the conformance floor. Reported, never
    /// fatal: the spec requires consumers not to reject what they do not recognise.
    pub non_conformant: Vec<(String, String)>,
}

/// The whole index pipeline: walk, parse, upsert what changed, drop what left, resolve edges,
/// and regenerate every `index.md`.
///
/// **This function is why CLI and MCP cannot drift on the one command that writes the most.**
/// It used to live inside the CLI's `index` command, which meant the MCP `touchstone_index`
/// tool did everything except regenerate `index.md` — same name, same description, different
/// effect on disk. That is precisely the divergence parity is supposed to prevent, and it
/// survived because the two adapters shared a name rather than an implementation.
///
/// Incremental on content digest: a concept whose bytes are unchanged is not reparsed into the
/// index. The `index.md` files are regenerated unconditionally, because they depend on the
/// whole directory rather than on any one file.
pub fn reindex_bundle<F, P, I>(files: &F, parser: &P, index: &mut I) -> ReindexReport
where
    F: ConceptRepository + RawStore + ConceptSink,
    P: ConceptParser,
    // ?Sized so a caller holding `&mut dyn BundleIndex` -- which the CLI does -- can pass it
    // straight through without an extra generic layer.
    I: ports::BundleIndex + ?Sized,
{
    let paths = files.paths();
    let known: std::collections::HashSet<String> = paths.iter().cloned().collect();

    let parsed: Vec<ParsedConcept> = paths
        .iter()
        .map(|p| {
            let raw = files.raw_bytes(p).unwrap_or_default();
            parser.parse(p, &raw)
        })
        .collect();

    let mut report = ReindexReport { total: parsed.len(), ..Default::default() };
    let prev = index.prev_digests();

    for c in &parsed {
        if !c.conformant() {
            report
                .non_conformant
                .push((c.path.clone(), c.error.clone().unwrap_or_else(|| "not conformant".into())));
        }
        let digest = ports::fnv64(&c.raw);
        if prev.get(&c.path).map(String::as_str) == Some(digest.as_str()) {
            continue; // bytes unchanged -- nothing to re-derive
        }
        let rec = ports::IndexRecord {
            path: c.path.clone(),
            concept_type: c.concept_type.clone(),
            title: c.title.clone(),
            description: c.description.clone(),
            body: c.body.clone(),
            tags: c.tags.clone(),
            trust: c.trust,
            status: c.status.clone(),
            stale_after: None,
            fm_json: c.frontmatter_json.clone(),
            conformant: c.conformant(),
            error: c.error.clone(),
            digest,
            links: resolved_links(&c.path, &c.body),
        };
        let _ = index.upsert(&rec, &known);
        if prev.contains_key(&c.path) {
            report.changed += 1;
        } else {
            report.new += 1;
        }
    }

    for gone in prev.keys().filter(|p| !known.contains(p.as_str())) {
        let _ = index.remove(gone);
        report.removed += 1;
    }

    // Once, over the whole set: a concept indexed late can resolve a link written earlier.
    let _ = index.reresolve();
    let _ = index.commit();

    report.indexes_written = write_indexes(files, &parsed);
    report.broken_links = index.broken_link_count();
    report
}

fn resolved_links(path: &str, body: &str) -> Vec<String> {
    ports::extract_links(body)
        .into_iter()
        .filter_map(|(_, target)| ports::resolve_link(path, &target))
        .collect()
}

/// Regenerate `index.md` for every directory implied by the concept set, including ancestors
/// that hold no concepts of their own.
fn write_indexes<W: ConceptSink>(sink: &W, concepts: &[ParsedConcept]) -> usize {
    use std::collections::{BTreeMap, HashSet};

    let mut by_dir: BTreeMap<String, Vec<&ParsedConcept>> = BTreeMap::new();
    for c in concepts {
        let dir = c.path.rfind('/').map(|i| c.path[..i].to_string()).unwrap_or_default();
        by_dir.entry(dir).or_default().push(c);
    }

    // Every ancestor directory gets an index too, so the tree is navigable from the root.
    let mut dirs: HashSet<String> = HashSet::new();
    dirs.insert(String::new());
    for d in by_dir.keys() {
        let mut parts: Vec<&str> = if d.is_empty() { vec![] } else { d.split('/').collect() };
        while !parts.is_empty() {
            dirs.insert(parts.join("/"));
            parts.pop();
        }
    }

    let count_under = |d: &str| -> usize {
        by_dir
            .iter()
            .filter(|(k, _)| if d.is_empty() { true } else { *k == d || k.starts_with(&format!("{d}/")) })
            .map(|(_, v)| v.len())
            .sum()
    };

    let mut sorted: Vec<String> = dirs.into_iter().collect();
    sorted.sort();

    let mut written = 0;
    for d in &sorted {
        let depth = if d.is_empty() { 0 } else { d.split('/').count() };
        let subdirs: Vec<(String, usize)> = sorted
            .iter()
            .filter(|other| *other != d)
            .filter(|other| {
                let od = if other.is_empty() { 0 } else { other.split('/').count() };
                od == depth + 1
                    && if d.is_empty() {
                        !other.contains('/')
                    } else {
                        other.starts_with(&format!("{d}/")) && !other[d.len() + 1..].contains('/')
                    }
            })
            .map(|k| {
                (k.rsplit('/').next().unwrap_or(k).to_string(), count_under(k))
            })
            .collect();

        let empty: Vec<&ParsedConcept> = Vec::new();
        let here = by_dir.get(d).unwrap_or(&empty);
        let text = ports::render_index(d, here, &subdirs, d.is_empty());
        let rel = if d.is_empty() { "index.md".to_string() } else { format!("{d}/index.md") };
        if sink.write(&rel, text.as_bytes()).is_ok() {
            written += 1;
        }
    }
    written
}
