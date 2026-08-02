//! Secondary adapter -- one adapter, one crate, one merge unit (hex ADR-001).
//! Imports `touchstone-ports` ONLY: it cannot reach another adapter.
//!
//! SearchIndex backed by SQLite FTS5. The index is DERIVED and disposable (A1):
//! building it twice from the same input must give the same result; deleting it
//! loses nothing. rusqlite "bundled" feature means no system SQLite is required.
//!
//! Retrieval pipeline (ARCHITECTURE.md):
//!   structured prefilter → BM25 → [graph expansion] → [trust rank]
//! This adapter implements the first two stages. Expansion and trust-rank belong
//! to the usecase layer and are not wired here.

use touchstone_ports::{Concept, SearchIndex, Trust};
use rusqlite::types::Value;
use rusqlite::{params, Connection};
use std::path::Path;

// ── FTS5 schema ─────────────────────────────────────────────────────────────

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS concepts (
    path         TEXT PRIMARY KEY,
    concept_type TEXT NOT NULL DEFAULT '',
    title        TEXT NOT NULL DEFAULT '',
    status       TEXT NOT NULL DEFAULT 'stable',
    trust        TEXT NOT NULL DEFAULT 'unattributed'
);
CREATE TABLE IF NOT EXISTS tags (
    path TEXT NOT NULL,
    tag  TEXT NOT NULL,
    PRIMARY KEY (path, tag)
);
CREATE INDEX IF NOT EXISTS tags_tag ON tags(tag);
CREATE VIRTUAL TABLE IF NOT EXISTS fts USING fts5(
    path UNINDEXED, title, description, tags, body,
    tokenize='porter unicode61'
);
";

// ── Trust enum → stored string ───────────────────────────────────────────────

fn trust_str(t: Trust) -> &'static str {
    match t {
        Trust::Verified  => "human",
        Trust::Attested  => "attested",
        Trust::Generated => "machine",
        Trust::Unknown   => "unattributed",
    }
}

// ── Rich record handed to the index for building ─────────────────────────────

/// The adapter-local view of a concept. Contains everything the FTS and prefilter
/// stages need. The domain `Concept` is the minimal view returned after search.
#[derive(Debug, Clone)]
pub struct ConceptRecord {
    pub path: String,
    pub concept_type: String,
    pub title: String,
    pub description: String,
    pub body: String,
    pub tags: Vec<String>,
    pub trust: Trust,
    pub status: String,
}

// ── Search filter ─────────────────────────────────────────────────────────────

/// Optional structured prefilter for `search_filtered`. All fields are AND-ed.
#[derive(Debug, Clone)]
pub struct SearchFilter {
    pub concept_type: Option<String>,
    pub tag: Option<String>,
    pub status: Option<String>,
    pub trust: Option<Trust>,
    /// How many results to return. 0 → use default (10).
    pub limit: usize,
}

impl Default for SearchFilter {
    fn default() -> Self {
        Self {
            concept_type: None,
            tag: None,
            status: None,
            trust: None,
            limit: 10,
        }
    }
}

// ── SqliteIndex ───────────────────────────────────────────────────────────────

pub struct SqliteIndex {
    conn: Connection,
}

impl SqliteIndex {
    /// Create an in-memory index (useful for tests and ephemeral sessions).
    pub fn open_in_memory() -> Result<Self, rusqlite::Error> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    /// Open or create a file-backed index at `path`.
    pub fn open(path: &Path) -> Result<Self, rusqlite::Error> {
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    /// Insert or replace a concept in the index.
    ///
    /// Idempotent: calling this twice with the same record yields the same state.
    /// FTS5 has no ON CONFLICT clause, so the existing row must be deleted first.
    pub fn insert(&mut self, record: &ConceptRecord) -> Result<(), rusqlite::Error> {
        let trust = trust_str(record.trust);
        // Clear stale FTS and tag rows before upserting.
        self.conn
            .execute("DELETE FROM fts WHERE path=?1", params![record.path])?;
        self.conn
            .execute("DELETE FROM tags WHERE path=?1", params![record.path])?;

        self.conn.execute(
            "INSERT INTO concepts(path, concept_type, title, status, trust)
             VALUES(?1,?2,?3,?4,?5)
             ON CONFLICT(path) DO UPDATE SET
               concept_type=excluded.concept_type,
               title=excluded.title,
               status=excluded.status,
               trust=excluded.trust",
            params![record.path, record.concept_type, record.title, record.status, trust],
        )?;

        let tags_text = record.tags.join(" ");
        for tag in &record.tags {
            self.conn.execute(
                "INSERT OR IGNORE INTO tags(path, tag) VALUES(?1,?2)",
                params![record.path, tag],
            )?;
        }

        self.conn.execute(
            "INSERT INTO fts(path, title, description, tags, body)
             VALUES(?1,?2,?3,?4,?5)",
            params![
                record.path,
                record.title,
                record.description,
                tags_text,
                record.body
            ],
        )?;

        Ok(())
    }

    /// Structured prefilter + BM25 search.
    ///
    /// Prefilter runs as SQL WHERE on the concepts table; BM25 ordering is
    /// FTS5-native. One-hop graph expansion and trust-rank are left to the
    /// usecase layer (ARCHITECTURE.md retrieval pipeline).
    ///
    /// FINDINGS.md E1: over-retrieve at depth = max(limit×50, 500) so that a
    /// usecase-layer post-filter has room to maintain recall.
    pub fn search_filtered(
        &self,
        q: &str,
        filter: &SearchFilter,
    ) -> Result<Vec<Concept>, rusqlite::Error> {
        if q.trim().is_empty() {
            return Ok(vec![]);
        }

        let limit = if filter.limit == 0 { 10 } else { filter.limit };
        let depth = std::cmp::max(limit * 50, 500) as i64;

        // Build the WHERE extension and collect positional params.
        let mut where_extra = String::new();
        let mut param_values: Vec<Value> = vec![Value::Text(q.to_string())];

        if let Some(ref ct) = filter.concept_type {
            where_extra.push_str(" AND c.concept_type = ?");
            param_values.push(Value::Text(ct.clone()));
        }
        if let Some(ref s) = filter.status {
            where_extra.push_str(" AND c.status = ?");
            param_values.push(Value::Text(s.clone()));
        }
        if let Some(t) = filter.trust {
            where_extra.push_str(" AND c.trust = ?");
            param_values.push(Value::Text(trust_str(t).to_string()));
        }
        if let Some(ref tag) = filter.tag {
            where_extra
                .push_str(" AND EXISTS(SELECT 1 FROM tags t WHERE t.path=c.path AND t.tag=?)");
            param_values.push(Value::Text(tag.clone()));
        }

        param_values.push(Value::Integer(depth));

        let sql = format!(
            "SELECT c.path, c.concept_type, c.title \
             FROM fts JOIN concepts c ON c.path = fts.path \
             WHERE fts MATCH ?{where_extra} \
             ORDER BY bm25(fts) LIMIT ?"
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let results: rusqlite::Result<Vec<Concept>> = stmt
            .query_map(rusqlite::params_from_iter(param_values), |row| {
                Ok(Concept {
                    path: row.get(0)?,
                    concept_type: row.get(1)?,
                    title: row.get(2)?,
                })
            })?
            .collect();

        Ok(results?)
    }
}

impl SearchIndex for SqliteIndex {
    fn search(&self, q: &str) -> Vec<Concept> {
        self.search_filtered(q, &SearchFilter::default())
            .unwrap_or_default()
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_record(
        path: &str,
        concept_type: &str,
        title: &str,
        description: &str,
        body: &str,
    ) -> ConceptRecord {
        ConceptRecord {
            path: path.to_string(),
            concept_type: concept_type.to_string(),
            title: title.to_string(),
            description: description.to_string(),
            body: body.to_string(),
            tags: vec![],
            trust: Trust::Unknown,
            status: "stable".to_string(),
        }
    }

    #[test]
    fn test_search_by_title() {
        let mut idx = SqliteIndex::open_in_memory().unwrap();
        idx.insert(&make_record(
            "notes/hexagonal.md",
            "Note",
            "Hexagonal Architecture",
            "Ports and adapters pattern",
            "The hexagonal architecture separates domain from infrastructure.",
        ))
        .unwrap();

        let results = idx.search("hexagonal");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "notes/hexagonal.md");
        assert_eq!(results[0].title, "Hexagonal Architecture");
        assert_eq!(results[0].concept_type, "Note");
    }

    #[test]
    fn test_search_by_description() {
        let mut idx = SqliteIndex::open_in_memory().unwrap();
        idx.insert(&make_record(
            "notes/ports.md",
            "Note",
            "Ports Pattern",
            "inversion of control via dependency injection",
            "Body text here.",
        ))
        .unwrap();

        let results = idx.search("inversion");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "notes/ports.md");
    }

    #[test]
    fn test_search_by_body() {
        let mut idx = SqliteIndex::open_in_memory().unwrap();
        idx.insert(&make_record(
            "notes/bm25.md",
            "Note",
            "BM25 Ranking",
            "",
            "Okapi BM25 is a bag-of-words retrieval function used in information retrieval.",
        ))
        .unwrap();

        let results = idx.search("retrieval");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "notes/bm25.md");
    }

    #[test]
    fn test_search_no_results() {
        let mut idx = SqliteIndex::open_in_memory().unwrap();
        idx.insert(&make_record(
            "notes/cat.md",
            "Note",
            "Cats",
            "felines",
            "Cats are animals.",
        ))
        .unwrap();

        let results = idx.search("quantum");
        assert!(results.is_empty());
    }

    #[test]
    fn test_prefilter_by_type() {
        let mut idx = SqliteIndex::open_in_memory().unwrap();
        idx.insert(&make_record(
            "notes/a.md",
            "Note",
            "Alpha",
            "common word",
            "shared body text about architecture",
        ))
        .unwrap();
        idx.insert(&make_record(
            "decisions/b.md",
            "Decision",
            "Beta",
            "common word",
            "shared body text about architecture",
        ))
        .unwrap();

        let filter = SearchFilter {
            concept_type: Some("Decision".to_string()),
            limit: 10,
            ..Default::default()
        };
        let results = idx.search_filtered("architecture", &filter).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "decisions/b.md");
        assert_eq!(results[0].concept_type, "Decision");
    }

    #[test]
    fn test_prefilter_by_tag() {
        let mut idx = SqliteIndex::open_in_memory().unwrap();
        let mut tagged = make_record(
            "notes/tagged.md",
            "Note",
            "Tagged Concept",
            "",
            "content about indexing",
        );
        tagged.tags = vec!["search".to_string(), "index".to_string()];
        idx.insert(&tagged).unwrap();

        let untagged = make_record(
            "notes/plain.md",
            "Note",
            "Plain Concept",
            "",
            "content about indexing",
        );
        idx.insert(&untagged).unwrap();

        let filter = SearchFilter {
            tag: Some("search".to_string()),
            limit: 10,
            ..Default::default()
        };
        let results = idx.search_filtered("indexing", &filter).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "notes/tagged.md");
    }

    #[test]
    fn test_prefilter_by_status() {
        let mut idx = SqliteIndex::open_in_memory().unwrap();
        let stable = ConceptRecord {
            path: "notes/stable.md".into(),
            concept_type: "Note".into(),
            title: "Stable".into(),
            description: "".into(),
            body: "information retrieval system".into(),
            tags: vec![],
            trust: Trust::Unknown,
            status: "stable".into(),
        };
        let draft = ConceptRecord {
            path: "notes/draft.md".into(),
            concept_type: "Note".into(),
            title: "Draft".into(),
            description: "".into(),
            body: "information retrieval system".into(),
            tags: vec![],
            trust: Trust::Unknown,
            status: "draft".into(),
        };
        idx.insert(&stable).unwrap();
        idx.insert(&draft).unwrap();

        let filter = SearchFilter {
            status: Some("draft".to_string()),
            limit: 10,
            ..Default::default()
        };
        let results = idx.search_filtered("information", &filter).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "notes/draft.md");
    }

    #[test]
    fn test_prefilter_by_trust() {
        let mut idx = SqliteIndex::open_in_memory().unwrap();
        let human = ConceptRecord {
            path: "notes/verified.md".into(),
            concept_type: "Note".into(),
            title: "Verified".into(),
            description: "".into(),
            body: "knowledge management system".into(),
            tags: vec![],
            trust: Trust::Verified,
            status: "stable".into(),
        };
        let machine = ConceptRecord {
            path: "notes/generated.md".into(),
            concept_type: "Note".into(),
            title: "Generated".into(),
            description: "".into(),
            body: "knowledge management system".into(),
            tags: vec![],
            trust: Trust::Generated,
            status: "stable".into(),
        };
        idx.insert(&human).unwrap();
        idx.insert(&machine).unwrap();

        let filter = SearchFilter {
            trust: Some(Trust::Verified),
            limit: 10,
            ..Default::default()
        };
        let results = idx.search_filtered("knowledge", &filter).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "notes/verified.md");
    }

    #[test]
    fn test_idempotent_build() {
        // Inserting the same record twice must give the same result.
        // This verifies the A1 "derived and disposable" property at the adapter level.
        let mut idx = SqliteIndex::open_in_memory().unwrap();
        let record = make_record(
            "notes/idempotent.md",
            "Note",
            "Idempotent Index",
            "rebuilding from the same input",
            "The index can be rebuilt from the bundle alone.",
        );
        idx.insert(&record).unwrap();
        idx.insert(&record).unwrap(); // second insert must not duplicate

        let results = idx.search("rebuilding");
        assert_eq!(
            results.len(),
            1,
            "duplicate insert must not produce duplicate results"
        );
        assert_eq!(results[0].path, "notes/idempotent.md");
    }

    #[test]
    fn test_empty_query_returns_empty() {
        let mut idx = SqliteIndex::open_in_memory().unwrap();
        idx.insert(&make_record("notes/x.md", "Note", "Title", "", "body"))
            .unwrap();

        let results = idx.search("");
        assert!(results.is_empty(), "empty query must return no results");
    }

    #[test]
    fn test_bm25_ordering_better_match_first() {
        // The document with stronger term signal should rank first.
        let mut idx = SqliteIndex::open_in_memory().unwrap();
        idx.insert(&make_record(
            "notes/weak.md",
            "Note",
            "Something",
            "",
            "This mentions retrieval once.",
        ))
        .unwrap();
        idx.insert(&make_record(
            "notes/strong.md",
            "Note",
            "Retrieval Focus",
            "retrieval is the topic",
            "This is about retrieval: BM25 retrieval, FTS5 retrieval. Retrieval matters.",
        ))
        .unwrap();

        let results = idx.search("retrieval");
        assert!(!results.is_empty());
        assert_eq!(
            results[0].path, "notes/strong.md",
            "document with higher term signal should rank first"
        );
    }

    #[test]
    fn test_port_trait_search() {
        // Verify the SearchIndex trait method is callable via a trait object.
        let mut idx = SqliteIndex::open_in_memory().unwrap();
        idx.insert(&make_record(
            "notes/trait.md",
            "Note",
            "Trait Object",
            "dynamic dispatch test",
            "",
        ))
        .unwrap();

        let index: &dyn SearchIndex = &idx;
        let results = index.search("dispatch");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "notes/trait.md");
    }

    #[test]
    fn test_search_across_multiple_concepts() {
        let mut idx = SqliteIndex::open_in_memory().unwrap();
        for i in 0..5 {
            idx.insert(&make_record(
                &format!("notes/{i}.md"),
                "Note",
                &format!("Concept {i}"),
                "shared description about okf knowledge",
                "body content",
            ))
            .unwrap();
        }

        let results = idx.search("knowledge");
        assert_eq!(results.len(), 5);
    }
}
