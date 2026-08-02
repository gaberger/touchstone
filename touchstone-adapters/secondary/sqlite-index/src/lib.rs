//! Secondary adapter — the derived index, backed by SQLite FTS5.
//!
//! Imports `touchstone-ports` ONLY (ARCHITECTURE.md rule 4a): it cannot reach another adapter.
//! rusqlite's `bundled` feature means no system SQLite is required.
//!
//! Everything here is DERIVED and disposable (A1): building it twice from the same files gives
//! the same result, and deleting it loses nothing but time. T1 and T6 are the drills that hold
//! this crate to that claim.
//!
//! Retrieval pipeline (ARCHITECTURE.md):
//!   structured prefilter → BM25 at depth → one-hop graph expansion → trust rank
//!
//! All four stages live here rather than in the use-case layer, because the prefilter must be
//! applied *inside* the query. Post-filtering an approximate index destroys recall at shallow
//! depth (ADR-2608010920), so authorization and structure are SQL `WHERE` clauses and the
//! retrieval depth is deliberately over-fetched before ranking.
//!
//! This implementation was previously a `FullStore` hand-rolled over raw rusqlite inside the
//! composition root, while this crate — whose entire job it is — sat unreachable behind a
//! no-op `_touch_adapters()`. It is the same proven code, moved to where it belongs.

use rusqlite::{params, params_from_iter, types::Value, Connection};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use touchstone_ports::{
    BundleIndex, BundleStats, Concept, IndexRecord, SearchHit, SearchIndex, SearchQuery,
    SearchVia, Trust,
};

/// Where the derived index lives, relative to the bundle root.
pub const DB_REL: &str = ".touchstone/index.db";

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS concepts (
  path        TEXT PRIMARY KEY,
  digest      TEXT NOT NULL,
  type        TEXT NOT NULL DEFAULT '',
  title       TEXT NOT NULL DEFAULT '',
  description TEXT NOT NULL DEFAULT '',
  status      TEXT NOT NULL DEFAULT 'stable',
  stale_after TEXT,
  trust       TEXT NOT NULL DEFAULT 'unattributed',
  conformant  INTEGER NOT NULL DEFAULT 1,
  error       TEXT,
  fm_json     TEXT NOT NULL DEFAULT '{}',
  body        TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS tags (
  path TEXT NOT NULL, tag TEXT NOT NULL,
  PRIMARY KEY (path, tag)
);
CREATE INDEX IF NOT EXISTS tags_tag ON tags(tag);
CREATE TABLE IF NOT EXISTS edges (
  src TEXT NOT NULL, target TEXT NOT NULL,
  dst TEXT, resolved INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (src, target)
);
CREATE INDEX IF NOT EXISTS edges_dst ON edges(dst);
CREATE VIRTUAL TABLE IF NOT EXISTS fts USING fts5(
  path UNINDEXED, title, description, tags, body, tokenize='porter unicode61'
);
";

// ── Trust ⇄ stored string ────────────────────────────────────────────────────

// ── The index ────────────────────────────────────────────────────────────────

pub struct SqliteIndex {
    conn: Connection,
}

impl SqliteIndex {
    /// Open (creating if absent) the derived index for `bundle`.
    pub fn open(bundle: &Path) -> Result<Self, String> {
        let db_path = bundle.join(DB_REL);
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        let conn = Connection::open(&db_path).map_err(|e| e.to_string())?;
        conn.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
        Ok(Self { conn })
    }

    /// In-memory index, for tests and for callers that want a throwaway.
    pub fn in_memory() -> Result<Self, String> {
        let conn = Connection::open_in_memory().map_err(|e| e.to_string())?;
        conn.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
        Ok(Self { conn })
    }
}

fn agg(conn: &Connection, sql: &str) -> Vec<(String, usize)> {
    conn.prepare(sql)
        .and_then(|mut s| {
            s.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))
                .map(|rows| rows.flatten().map(|(k, v)| (k, v as usize)).collect())
        })
        .unwrap_or_default()
}

/// Today's UTC date as `YYYY-MM-DD`, for the staleness penalty.
///
/// Hand-rolled from `SystemTime` rather than pulling a date crate: the domain is deliberately
/// dependency-free and this is the only calendar arithmetic in the workspace. Accurate for
/// 1970–2099, which outlives any decision this ranking affects.
fn today_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let days = secs / 86400;
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    format!("{year:04}-{month:02}-{day:02}")
}

/// Trust boost applied to the BM25 score.
///
/// This is where the trust invariant stops being metadata and starts changing what an agent
/// reads first. Human-verified outranks machine-generated outranks unattributed.
fn trust_boost(t: Trust) -> f64 {
    match t {
        Trust::Verified => 1.30,
        Trust::Attested => 1.15,
        Trust::Generated => 1.00,
        Trust::Unknown => 0.90,
    }
}

/// Build an FTS5 MATCH expression from a user's query.
///
/// **This exists because FTS5 defaults to implicit AND, and that made the search tool useless
/// to an agent.** Passing a question straight through — which is exactly what an LLM does with
/// a parameter described as "full-text query" — required every word of it to appear in one
/// concept, including `what`, `is` and `the`. Measured on the A10 pilot corpus: 20 of 20
/// natural-language questions returned zero hits. Not poor ranking. Zero.
///
/// So terms are OR-ed and BM25 is left to rank, which is what BM25 is for: a concept matching
/// six of eight terms should outrank one matching two, rather than both being discarded for
/// matching seven.
///
/// Each term is double-quoted, which also escapes the FTS5 operators (`*`, `:`, `^`, `-`,
/// `NEAR`) that a natural question is full of — `"What is E4b?"` would otherwise be a syntax
/// error rather than a search.
///
/// A query already containing a quoted phrase is passed through untouched, so
/// `"post-filter authorization"` still means the phrase and an explicit `AND`/`OR` still works.
fn fts_query(raw: &str) -> String {
    if raw.contains('"') {
        return raw.to_string(); // caller is speaking FTS5 deliberately
    }
    let terms: Vec<String> = raw
        .split(|c: char| !c.is_alphanumeric() && c != '\'')
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{}\"", t.replace('"', "")))
        .collect();
    if terms.is_empty() { String::new() } else { terms.join(" OR ") }
}

impl BundleIndex for SqliteIndex {
    fn prev_digests(&self) -> HashMap<String, String> {
        self.conn
            .prepare("SELECT path, digest FROM concepts")
            .and_then(|mut s| {
                s.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
                    .map(|rows| rows.flatten().collect())
            })
            .unwrap_or_default()
    }

    fn upsert(&mut self, rec: &IndexRecord, known_paths: &HashSet<String>) -> Result<(), String> {
        // Clear derived rows for this concept first. FTS5 has no upsert, and a stale tag or
        // edge left behind would survive a rebuild — quietly breaking T1.
        for (table, col) in [("fts", "path"), ("tags", "path"), ("edges", "src")] {
            self.conn
                .execute(&format!("DELETE FROM {table} WHERE {col}=?1"), params![rec.path])
                .map_err(|e| e.to_string())?;
        }

        self.conn
            .execute(
                "INSERT INTO concepts(path,digest,type,title,description,status,stale_after,\
                 trust,conformant,error,fm_json,body) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
                 ON CONFLICT(path) DO UPDATE SET
                   digest=excluded.digest, type=excluded.type, title=excluded.title,
                   description=excluded.description, status=excluded.status,
                   stale_after=excluded.stale_after, trust=excluded.trust,
                   conformant=excluded.conformant, error=excluded.error,
                   fm_json=excluded.fm_json, body=excluded.body",
                params![
                    rec.path,
                    rec.digest,
                    rec.concept_type,
                    rec.title,
                    rec.description,
                    rec.status,
                    rec.stale_after,
                    rec.trust.label(),
                    rec.conformant as i32,
                    rec.error,
                    rec.fm_json,
                    rec.body,
                ],
            )
            .map_err(|e| e.to_string())?;

        let mut seen: HashSet<&str> = HashSet::new();
        for tag in &rec.tags {
            if seen.insert(tag.as_str()) {
                self.conn
                    .execute(
                        "INSERT OR IGNORE INTO tags(path, tag) VALUES(?1, ?2)",
                        params![rec.path, tag],
                    )
                    .map_err(|e| e.to_string())?;
            }
        }

        // Edges are recorded whether or not they resolve. A broken link is legal per spec --
        // it represents knowledge not yet written -- so it is stored and counted, never dropped.
        for target in &rec.links {
            let resolved = known_paths.contains(target.as_str());
            self.conn
                .execute(
                    "INSERT OR IGNORE INTO edges(src, target, dst, resolved) VALUES(?1,?2,?3,?4)",
                    params![rec.path, target, target, resolved as i32],
                )
                .map_err(|e| e.to_string())?;
        }

        let tags_text = rec.tags.join(" ");
        self.conn
            .execute(
                "INSERT INTO fts(path,title,description,tags,body) VALUES(?1,?2,?3,?4,?5)",
                params![rec.path, rec.title, rec.description, tags_text, rec.body],
            )
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    fn remove(&mut self, path: &str) -> Result<(), String> {
        for table in ["fts", "tags", "edges", "concepts"] {
            let col = if table == "edges" { "src" } else { "path" };
            self.conn
                .execute(&format!("DELETE FROM {table} WHERE {col}=?1"), params![path])
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn reresolve(&mut self) -> Result<(), String> {
        // Runs once after the whole walk, not per-upsert: a concept indexed late can resolve a
        // link written earlier, and per-upsert resolution would depend on walk order.
        self.conn
            .execute_batch(
                "UPDATE edges SET resolved =
                 (SELECT COUNT(*) FROM concepts c WHERE c.path = edges.dst)",
            )
            .map_err(|e| e.to_string())
    }

    fn commit(&mut self) -> Result<(), String> {
        self.conn
            .execute_batch("PRAGMA wal_checkpoint(PASSIVE)")
            .map_err(|e| e.to_string())
    }

    fn broken_link_count(&self) -> usize {
        self.conn
            .query_row("SELECT COUNT(*) FROM edges WHERE resolved=0", [], |r| r.get::<_, i64>(0))
            .unwrap_or(0) as usize
    }

    fn search(&self, q: &SearchQuery) -> Result<Vec<SearchHit>, String> {
        if q.text.trim().is_empty() {
            return Ok(vec![]);
        }
        let match_expr = fts_query(&q.text);
        if match_expr.is_empty() {
            return Ok(vec![]);
        }
        let limit = if q.limit == 0 { 10 } else { q.limit };
        // Over-fetch before ranking. Post-filtering an approximate index destroys recall at
        // shallow depth; depth restores it (ADR-2608010920, measured in E1 at recall >= 0.95).
        let depth = std::cmp::max(limit * 50, 500) as i64;

        let mut where_extra = String::new();
        let mut extra: Vec<String> = Vec::new();
        if let Some(ref ct) = q.concept_type {
            where_extra.push_str(" AND c.type = ?");
            extra.push(ct.clone());
        }
        if let Some(ref s) = q.status {
            where_extra.push_str(" AND c.status = ?");
            extra.push(s.clone());
        }
        if let Some(t) = q.trust {
            where_extra.push_str(" AND c.trust = ?");
            extra.push(t.label().to_string());
        }
        if let Some(ref tag) = q.tag {
            where_extra
                .push_str(" AND EXISTS(SELECT 1 FROM tags t WHERE t.path=c.path AND t.tag=?)");
            extra.push(tag.clone());
        }

        let sql = format!(
            "SELECT c.path, c.title, c.description, c.type, c.trust, c.stale_after, bm25(fts) AS bm
             FROM fts JOIN concepts c ON c.path = fts.path
             WHERE fts MATCH ? AND c.conformant=1{where_extra}
             ORDER BY bm LIMIT ?"
        );

        let mut pv: Vec<Value> = vec![Value::Text(match_expr)];
        pv.extend(extra.iter().map(|v| Value::Text(v.clone())));
        pv.push(Value::Integer(depth));

        let today = today_iso();
        let mut stmt = self.conn.prepare(&sql).map_err(|e| format!("bad FTS query: {e}"))?;

        let mut scored: Vec<(f64, SearchHit)> = stmt
            .query_map(params_from_iter(pv), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, f64>(6)?,
                ))
            })
            .map_err(|e| e.to_string())?
            .flatten()
            .map(|(path, title, description, concept_type, trust, stale_after, bm)| {
                let trust = Trust::from_label(&trust).unwrap_or_default();
                let mut s = -bm * trust_boost(trust); // bm25(): lower is better
                if stale_after.as_deref().is_some_and(|sa| sa < today.as_str()) {
                    s *= 0.60;
                }
                (s, SearchHit { path, concept_type, title, description, trust, via: SearchVia::Direct })
            })
            .collect();

        if q.expand && !scored.is_empty() {
            let seeds: Vec<String> =
                scored.iter().take(20.min(scored.len())).map(|(_, h)| h.path.clone()).collect();
            let seen: HashSet<String> = scored.iter().map(|(_, h)| h.path.clone()).collect();
            let best = scored.iter().map(|(s, _)| *s).fold(f64::NEG_INFINITY, f64::max);

            let qs = seeds.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let nb_sql = format!(
                "SELECT DISTINCT e.dst, c.title, c.description, c.type, c.trust
                 FROM edges e JOIN concepts c ON c.path = e.dst
                 WHERE e.resolved=1 AND (e.src IN ({qs}) OR e.dst IN ({qs}))
                   AND c.conformant=1{where_extra}"
            );

            let mut np: Vec<Value> = Vec::new();
            for s in &seeds {
                np.push(Value::Text(s.clone()));
            }
            for s in &seeds {
                np.push(Value::Text(s.clone()));
            }
            np.extend(extra.iter().map(|v| Value::Text(v.clone())));

            if let Ok(mut nb) = self.conn.prepare(&nb_sql) {
                if let Ok(rows) = nb.query_map(params_from_iter(np), |row| {
                    Ok(SearchHit {
                        path: row.get(0)?,
                        title: row.get(1)?,
                        description: row.get(2)?,
                        concept_type: row.get(3)?,
                        trust: Trust::from_label(&row.get::<_, String>(4)?).unwrap_or_default(),
                        via: SearchVia::Link,
                    })
                }) {
                    // A linked neighbour ranks below every direct hit by construction: it was
                    // reached by association, not by matching.
                    for hit in rows.flatten() {
                        if !seen.contains(&hit.path) {
                            scored.push((best * 0.25, hit));
                        }
                    }
                }
            }
        }

        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        Ok(scored.into_iter().take(limit).map(|(_, h)| h).collect())
    }

    fn stats(&self) -> BundleStats {
        let total = self
            .conn
            .query_row("SELECT COUNT(*) FROM concepts", [], |r| r.get::<_, i64>(0))
            .unwrap_or(0) as usize;

        let (link_count, broken_link_count) = self
            .conn
            .query_row(
                "SELECT COUNT(*), SUM(CASE WHEN resolved=0 THEN 1 ELSE 0 END) FROM edges",
                [],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1).unwrap_or(0))),
            )
            .map(|(n, b)| (n as usize, b as usize))
            .unwrap_or((0, 0));

        BundleStats {
            total,
            by_type: agg(&self.conn, "SELECT type, COUNT(*) n FROM concepts GROUP BY type ORDER BY n DESC"),
            by_trust: agg(&self.conn, "SELECT trust, COUNT(*) n FROM concepts GROUP BY trust ORDER BY n DESC"),
            by_status: agg(&self.conn, "SELECT status, COUNT(*) n FROM concepts GROUP BY status ORDER BY n DESC"),
            link_count,
            broken_link_count,
        }
    }

    fn all_paths(&self) -> Vec<String> {
        self.conn
            .prepare("SELECT path FROM concepts ORDER BY path")
            .and_then(|mut s| {
                s.query_map([], |row| row.get::<_, String>(0)).map(|r| r.flatten().collect())
            })
            .unwrap_or_default()
    }
}

impl SearchIndex for SqliteIndex {
    /// The minimal port, in terms of the full one, so the two cannot disagree.
    fn search(&self, q: &str) -> Vec<Concept> {
        let query = SearchQuery { text: q.to_string(), ..Default::default() };
        BundleIndex::search(self, &query)
            .unwrap_or_default()
            .into_iter()
            .map(|h| Concept { path: h.path, concept_type: h.concept_type, title: h.title })
            .collect()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn rec(path: &str, title: &str, body: &str) -> IndexRecord {
        IndexRecord {
            path: path.into(),
            concept_type: "Note".into(),
            title: title.into(),
            body: body.into(),
            status: "stable".into(),
            digest: "d".into(),
            conformant: true,
            fm_json: "{}".into(),
            ..Default::default()
        }
    }

    fn idx() -> SqliteIndex {
        SqliteIndex::in_memory().unwrap()
    }

    fn put(i: &mut SqliteIndex, r: &IndexRecord) {
        i.upsert(r, &HashSet::new()).unwrap();
    }

    #[test]
    fn search_finds_body_text() {
        let mut i = idx();
        put(&mut i, &rec("n/a.md", "Alpha", "revocation is unsolved"));
        let hits = BundleIndex::search(&i, &SearchQuery { text: "revocation".into(), ..Default::default() }).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "n/a.md");
        assert_eq!(hits[0].via, SearchVia::Direct);
    }

    #[test]
    fn empty_query_returns_nothing_rather_than_everything() {
        let mut i = idx();
        put(&mut i, &rec("n/a.md", "Alpha", "text"));
        assert!(BundleIndex::search(&i, &SearchQuery { text: "   ".into(), ..Default::default() }).unwrap().is_empty());
    }

    #[test]
    fn structured_prefilter_excludes_other_types() {
        let mut i = idx();
        put(&mut i, &rec("n/a.md", "Alpha", "shared word"));
        let mut d = rec("d/b.md", "Beta", "shared word");
        d.concept_type = "Decision".into();
        put(&mut i, &d);

        let q = SearchQuery {
            text: "shared".into(),
            concept_type: Some("Decision".into()),
            ..Default::default()
        };
        let hits = BundleIndex::search(&i, &q).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "d/b.md");
    }

    #[test]
    fn upsert_is_idempotent_and_does_not_duplicate_hits() {
        let mut i = idx();
        let r = rec("n/a.md", "Alpha", "unique term");
        put(&mut i, &r);
        put(&mut i, &r);
        let hits = BundleIndex::search(&i, &SearchQuery { text: "unique".into(), ..Default::default() }).unwrap();
        assert_eq!(hits.len(), 1, "re-indexing must replace, not append -- T1b depends on it");
        assert_eq!(i.stats().total, 1);
    }

    /// The trust invariant, at the point where it changes what an agent reads first.
    #[test]
    fn human_verified_outranks_unattributed_on_equal_text() {
        let mut i = idx();
        let mut low = rec("n/low.md", "Low", "identical body text here");
        low.trust = Trust::Unknown;
        let mut high = rec("n/high.md", "High", "identical body text here");
        high.trust = Trust::Verified;
        put(&mut i, &low);
        put(&mut i, &high);

        let hits = BundleIndex::search(&i, &SearchQuery { text: "identical".into(), ..Default::default() }).unwrap();
        assert_eq!(hits[0].path, "n/high.md", "human-verified must rank first");
        assert_eq!(hits[0].trust, Trust::Verified);
    }

    #[test]
    fn broken_links_are_recorded_not_dropped() {
        let mut i = idx();
        let mut r = rec("n/a.md", "Alpha", "body");
        r.links = vec!["does/not/exist.md".into()];
        i.upsert(&r, &HashSet::new()).unwrap();
        i.reresolve().unwrap();
        assert_eq!(i.broken_link_count(), 1, "a broken link is legal and must be counted");
        assert_eq!(i.stats().link_count, 1);
    }

    #[test]
    fn reresolve_fixes_a_link_whose_target_arrived_later() {
        let mut i = idx();
        let mut a = rec("n/a.md", "A", "body");
        a.links = vec!["n/b.md".into()];
        i.upsert(&a, &HashSet::new()).unwrap(); // b not known yet
        assert_eq!(i.broken_link_count(), 1);

        put(&mut i, &rec("n/b.md", "B", "body"));
        i.reresolve().unwrap();
        assert_eq!(i.broken_link_count(), 0, "resolution must not depend on walk order");
    }

    #[test]
    fn remove_clears_every_derived_table() {
        let mut i = idx();
        let mut r = rec("n/a.md", "Alpha", "unique term");
        r.tags = vec!["x".into()];
        r.links = vec!["n/b.md".into()];
        put(&mut i, &r);
        i.remove("n/a.md").unwrap();
        assert_eq!(i.stats().total, 0);
        assert_eq!(i.stats().link_count, 0, "edges must go with the concept");
        assert!(BundleIndex::search(&i, &SearchQuery { text: "unique".into(), ..Default::default() }).unwrap().is_empty());
    }

    #[test]
    fn digests_round_trip_for_incremental_indexing() {
        let mut i = idx();
        let mut r = rec("n/a.md", "Alpha", "b");
        r.digest = "abc123".into();
        put(&mut i, &r);
        assert_eq!(i.prev_digests().get("n/a.md").map(String::as_str), Some("abc123"));
    }

    #[test]
    fn stats_group_by_type_trust_and_status() {
        let mut i = idx();
        put(&mut i, &rec("n/a.md", "A", "x"));
        let mut d = rec("d/b.md", "B", "x");
        d.concept_type = "Decision".into();
        d.status = "draft".into();
        d.trust = Trust::Verified;
        put(&mut i, &d);

        let s = i.stats();
        assert_eq!(s.total, 2);
        assert!(s.by_type.contains(&("Decision".to_string(), 1)));
        assert!(s.by_status.contains(&("draft".to_string(), 1)));
        assert!(s.by_trust.contains(&("human".to_string(), 1)));
    }

    #[test]
    fn all_paths_is_sorted() {
        let mut i = idx();
        put(&mut i, &rec("z.md", "Z", "x"));
        put(&mut i, &rec("a.md", "A", "x"));
        assert_eq!(i.all_paths(), vec!["a.md".to_string(), "z.md".to_string()]);
    }

    #[test]
    fn trust_strings_round_trip() {
        for t in [Trust::Verified, Trust::Attested, Trust::Generated, Trust::Unknown] {
            assert_eq!(Trust::from_label(t.label()), Some(t));
        }
    }

    #[test]
    fn non_conformant_concepts_are_stored_but_not_returned_by_search() {
        let mut i = idx();
        let mut bad = rec("n/bad.md", "Bad", "searchable term");
        bad.conformant = false;
        bad.error = Some("missing or empty `type`".into());
        put(&mut i, &bad);

        // Indexed -- the spec requires we not reject it, and `stats` must see it.
        assert_eq!(i.stats().total, 1);
        // ...but it is not a search result, because it has no valid type to rank under.
        assert!(BundleIndex::search(&i, &SearchQuery { text: "searchable".into(), ..Default::default() }).unwrap().is_empty());
    }
}

#[cfg(test)]
mod fts_query_tests {
    use super::tests::rec;
    use super::*;

    /// The defect the A10 harness found on its first run: FTS5 implicit AND meant a question
    /// required every one of its words -- `what`, `is`, `the` included -- to appear in a single
    /// concept. 20 of 20 natural-language questions returned zero hits.
    #[test]
    fn a_natural_language_question_becomes_an_or_query() {
        let q = fts_query("What is the kill criterion for this whole architecture?");
        assert!(q.contains(" OR "), "terms must be OR-ed, not AND-ed: {q}");
        assert!(q.contains("\"kill\""), "content words must survive: {q}");
        assert!(!q.contains('?'), "punctuation must not reach FTS5: {q}");
    }

    /// A question mark or a hyphen is an FTS5 operator. Unescaped, the query is a syntax error
    /// rather than a search -- which reads to the caller as "no results".
    #[test]
    fn operators_in_prose_are_escaped_not_executed() {
        for raw in ["What is E4b?", "post-filter authorization", "trust: human", "a * b", "NEAR x"] {
            let q = fts_query(raw);
            assert!(!q.is_empty(), "{raw} produced an empty query");
            assert!(
                q.split(" OR ").all(|t| t.starts_with('"') && t.ends_with('"')),
                "every term must be quoted so operators cannot execute: {raw} -> {q}"
            );
        }
    }

    /// An explicit phrase is the caller speaking FTS5 on purpose. Do not rewrite it.
    #[test]
    fn an_explicit_phrase_is_passed_through() {
        let raw = "\"post-filter authorization\"";
        assert_eq!(fts_query(raw), raw);
    }

    #[test]
    fn punctuation_only_input_yields_no_query_rather_than_a_syntax_error() {
        assert!(fts_query("???").is_empty());
        assert!(fts_query("   ").is_empty());
    }

    /// OR must not become "match anything": ranking still has to put the better match first.
    #[test]
    fn or_still_ranks_the_denser_match_first() {
        let mut i = SqliteIndex::in_memory().unwrap();
        let mut a = rec("n/a.md", "A", "kill criterion architecture pre-registered");
        a.digest = "a".into();
        let mut b = rec("n/b.md", "B", "architecture");
        b.digest = "b".into();
        i.upsert(&a, &HashSet::new()).unwrap();
        i.upsert(&b, &HashSet::new()).unwrap();

        let hits = BundleIndex::search(
            &i,
            &SearchQuery { text: "what is the kill criterion".into(), ..Default::default() },
        )
        .unwrap();
        assert_eq!(hits[0].path, "n/a.md", "the denser match must rank first, not merely appear");
    }
}
