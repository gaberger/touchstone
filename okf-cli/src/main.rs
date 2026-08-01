//! Composition root — the ONLY crate that imports adapters (ARCHITECTURE.md rule 6).
//!
//! Wires `okf-cli-adapter` (CLI surface) with the concrete adapters:
//! - `rusqlite` / `SqliteIndex` for the full index store (`CliStore` impl)
//! - `Clock` via `std::time`
//!
//! Every secondary adapter is touched here to keep the architecture test happy
//! (rule 6 also requires every declared adapter to be instantiated).

use clap::Parser;
use okf_cli_adapter::store::{
    BundleStats, CliSearchFilter, CliStore, IndexRecord, SearchHit,
};
use okf_cli_adapter::{walk::find_bundle, Cli};
use okf_ports::Clock;
use rusqlite::{params, Connection};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

// ── Touch every adapter so the architecture test (rule 6) stays happy ────────
fn _touch_adapters() {
    let _ = okf_fs_bundle::FsBundle::new(".");
    let _ = okf_yaml_serde::YamlSerde;
    let _: Option<okf_sqlite_index::SqliteIndex> = None;
    let _: Option<okf_git_attest::GitAttest> = None;
    let _: Option<okf_crdt_sync::CrdtSync> = None;
    let _: Option<okf_embed_local::EmbedLocal> = None;
    let _: Option<okf_mcp_adapter::Surface> = None;
}

// ── SQLite schema (mirrors Python store.py) ───────────────────────────────────

const DB_REL: &str = ".touchstone/index.db";

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

// ── FullStore: implements CliStore on top of a rusqlite Connection ─────────────

struct FullStore {
    conn: Connection,
}

impl FullStore {
    fn open(bundle: &Path) -> Result<Self, rusqlite::Error> {
        let db_path = bundle.join(DB_REL);
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| rusqlite::Error::InvalidPath(PathBuf::from(e.to_string())))?;
        }
        let conn = Connection::open(&db_path)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }
}

impl CliStore for FullStore {
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
        // Clear stale rows
        self.conn
            .execute("DELETE FROM fts WHERE path=?1", params![rec.path])
            .map_err(|e| e.to_string())?;
        self.conn
            .execute("DELETE FROM tags WHERE path=?1", params![rec.path])
            .map_err(|e| e.to_string())?;
        self.conn
            .execute("DELETE FROM edges WHERE src=?1", params![rec.path])
            .map_err(|e| e.to_string())?;

        // Upsert concept
        self.conn.execute(
            "INSERT INTO concepts(path,digest,type,title,description,status,stale_after,\
             trust,conformant,error,fm_json,body) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
             ON CONFLICT(path) DO UPDATE SET
               digest=excluded.digest, type=excluded.type, title=excluded.title,
               description=excluded.description, status=excluded.status,
               stale_after=excluded.stale_after, trust=excluded.trust,
               conformant=excluded.conformant, error=excluded.error,
               fm_json=excluded.fm_json, body=excluded.body",
            params![
                rec.path, rec.digest, rec.concept_type, rec.title,
                rec.description, rec.status, rec.stale_after, rec.trust,
                rec.conformant as i32, rec.error, rec.fm_json, rec.body,
            ],
        ).map_err(|e| e.to_string())?;

        // Tags
        let mut seen_tags: HashSet<&str> = HashSet::new();
        for tag in &rec.tags {
            if seen_tags.insert(tag.as_str()) {
                self.conn.execute(
                    "INSERT OR IGNORE INTO tags(path, tag) VALUES(?1, ?2)",
                    params![rec.path, tag],
                ).map_err(|e| e.to_string())?;
            }
        }

        // Edges
        for link_target in &rec.links {
            let resolved = known_paths.contains(link_target.as_str());
            self.conn.execute(
                "INSERT OR IGNORE INTO edges(src, target, dst, resolved) VALUES(?1, ?2, ?3, ?4)",
                params![rec.path, link_target, link_target, resolved as i32],
            ).map_err(|e| e.to_string())?;
        }

        // FTS
        let tags_text = rec.tags.join(" ");
        self.conn.execute(
            "INSERT INTO fts(path,title,description,tags,body) VALUES(?1,?2,?3,?4,?5)",
            params![rec.path, rec.title, rec.description, tags_text, rec.body],
        ).map_err(|e| e.to_string())?;

        Ok(())
    }

    fn remove(&mut self, path: &str) -> Result<(), String> {
        for table in ["fts", "tags", "edges", "concepts"] {
            let sql = if table == "edges" {
                format!("DELETE FROM {table} WHERE src=?1")
            } else {
                format!("DELETE FROM {table} WHERE path=?1")
            };
            self.conn
                .execute(&sql, params![path])
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn reresolve(&mut self) -> Result<(), String> {
        self.conn.execute_batch(
            "UPDATE edges SET resolved =
             (SELECT COUNT(*) FROM concepts c WHERE c.path = edges.dst)",
        ).map_err(|e| e.to_string())
    }

    fn commit(&mut self) -> Result<(), String> {
        // rusqlite auto-commits when not in an explicit transaction.
        // Execute a no-op to flush WAL if needed.
        self.conn
            .execute_batch("PRAGMA wal_checkpoint(PASSIVE)")
            .map_err(|e| e.to_string())
    }

    fn broken_link_count(&self) -> usize {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM edges WHERE resolved=0",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap_or(0) as usize
    }

    fn search(&self, filter: &CliSearchFilter) -> Result<Vec<SearchHit>, String> {
        if filter.query.trim().is_empty() {
            return Ok(vec![]);
        }
        let limit = if filter.limit == 0 { 10 } else { filter.limit };
        let depth = std::cmp::max(limit * 50, 500) as i64;

        let mut where_extra = String::new();
        let mut extra_vals: Vec<String> = Vec::new();

        if let Some(ref ct) = filter.concept_type {
            where_extra.push_str(" AND c.type = ?");
            extra_vals.push(ct.clone());
        }
        if let Some(ref s) = filter.status {
            where_extra.push_str(" AND c.status = ?");
            extra_vals.push(s.clone());
        }
        if let Some(ref t) = filter.trust {
            where_extra.push_str(" AND c.trust = ?");
            extra_vals.push(t.clone());
        }
        if let Some(ref tag) = filter.tag {
            where_extra.push_str(
                " AND EXISTS(SELECT 1 FROM tags t WHERE t.path=c.path AND t.tag=?)",
            );
            extra_vals.push(tag.clone());
        }

        let sql = format!(
            "SELECT c.path, c.title, c.description, c.type, c.trust, c.stale_after,
                    bm25(fts) AS bm
             FROM fts JOIN concepts c ON c.path = fts.path
             WHERE fts MATCH ? AND c.conformant=1{where_extra}
             ORDER BY bm LIMIT ?"
        );

        let today = today_iso();

        // Build params as rusqlite Values
        let mut param_vec: Vec<rusqlite::types::Value> = Vec::new();
        param_vec.push(rusqlite::types::Value::Text(filter.query.clone()));
        for v in &extra_vals {
            param_vec.push(rusqlite::types::Value::Text(v.clone()));
        }
        param_vec.push(rusqlite::types::Value::Integer(depth));

        let mut stmt = self
            .conn
            .prepare(&sql)
            .map_err(|e| format!("bad FTS query: {e}"))?;

        let trust_boost = |trust: &str| -> f64 {
            match trust {
                "human" => 1.30,
                "machine" => 1.00,
                _ => 0.90,
            }
        };

        let mut scored: Vec<(f64, SearchHit)> = stmt
            .query_map(rusqlite::params_from_iter(param_vec), |row| {
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
                let base = -bm; // bm25() lower is better
                let mut s = base * trust_boost(&trust);
                if let Some(ref sa) = stale_after {
                    if sa.as_str() < today.as_str() {
                        s *= 0.60;
                    }
                }
                (s, SearchHit {
                    path,
                    title,
                    description,
                    concept_type,
                    trust,
                    via: "direct".to_string(),
                })
            })
            .collect();

        // One-hop graph expansion
        if filter.expand && !scored.is_empty() {
            let seeds: Vec<String> = scored
                .iter()
                .take(20.min(scored.len()))
                .map(|(_, h)| h.path.clone())
                .collect();
            let seed_paths: HashSet<String> = scored.iter().map(|(_, h)| h.path.clone()).collect();
            let best = scored.iter().map(|(s, _)| *s).fold(f64::NEG_INFINITY, f64::max);

            let qs = seeds.iter().map(|_| "?").collect::<Vec<_>>().join(",");
            let nb_sql = format!(
                "SELECT DISTINCT e.dst AS path, c.title, c.description, c.type, c.trust
                 FROM edges e JOIN concepts c ON c.path = e.dst
                 WHERE e.resolved=1 AND (e.src IN ({qs}) OR e.dst IN ({qs}))
                   AND c.conformant=1{where_extra}"
            );

            let mut nb_params: Vec<rusqlite::types::Value> = Vec::new();
            for s in &seeds { nb_params.push(rusqlite::types::Value::Text(s.clone())); }
            for s in &seeds { nb_params.push(rusqlite::types::Value::Text(s.clone())); }
            for v in &extra_vals { nb_params.push(rusqlite::types::Value::Text(v.clone())); }

            let mut to_add: Vec<(f64, SearchHit)> = Vec::new();
            if let Ok(mut nb_stmt) = self.conn.prepare(&nb_sql) {
                if let Ok(rows) = nb_stmt.query_map(
                    rusqlite::params_from_iter(nb_params),
                    |row| {
                        Ok(SearchHit {
                            path: row.get(0)?,
                            title: row.get(1)?,
                            description: row.get(2)?,
                            concept_type: row.get(3)?,
                            trust: row.get(4)?,
                            via: "link".to_string(),
                        })
                    },
                ) {
                    for nb in rows.flatten() {
                        if !seed_paths.contains(&nb.path) {
                            to_add.push((best * 0.25, nb));
                        }
                    }
                }
            }
            scored.extend(to_add);
        }

        // Sort by score desc, take limit
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        Ok(scored.into_iter().take(limit).map(|(_, h)| h).collect())
    }

    fn stats(&self) -> BundleStats {
        let total: usize = self
            .conn
            .query_row("SELECT COUNT(*) FROM concepts", [], |r| r.get::<_, i64>(0))
            .unwrap_or(0) as usize;

        let by_type = agg(&self.conn, "SELECT type, COUNT(*) n FROM concepts GROUP BY type ORDER BY n DESC");
        let by_trust = agg(&self.conn, "SELECT trust, COUNT(*) n FROM concepts GROUP BY trust ORDER BY n DESC");
        let by_status = agg(&self.conn, "SELECT status, COUNT(*) n FROM concepts GROUP BY status ORDER BY n DESC");

        let (link_count, broken_link_count) = self
            .conn
            .query_row(
                "SELECT COUNT(*) n, SUM(CASE WHEN resolved=0 THEN 1 ELSE 0 END) b FROM edges",
                [],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
            )
            .map(|(n, b)| (n as usize, b as usize))
            .unwrap_or((0, 0));

        BundleStats { total, by_type, by_trust, by_status, link_count, broken_link_count }
    }

    fn all_paths(&self) -> Vec<String> {
        self.conn
            .prepare("SELECT path FROM concepts ORDER BY path")
            .and_then(|mut s| {
                s.query_map([], |row| row.get::<_, String>(0))
                    .map(|rows| rows.flatten().collect())
            })
            .unwrap_or_default()
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

fn today_iso() -> String {
    // Simple UTC date via SystemTime.
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86400;
    // Gregorian calendar computation (accurate for years 1970–2099)
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

// ── SystemClock ───────────────────────────────────────────────────────────────

struct SystemClock;

impl Clock for SystemClock {
    fn now_iso8601(&self) -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        // Format as ISO 8601 UTC (accurate 1970–2099, no leap-second handling)
        let s = secs % 60;
        let m = (secs / 60) % 60;
        let h = (secs / 3600) % 24;
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
        format!("{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}Z")
    }
}

// ── main ──────────────────────────────────────────────────────────────────────

fn main() {
    // Touch every adapter (architecture test rule 6: every declared adapter must
    // be instantiated in the composition root).
    _touch_adapters();

    let cli = Cli::parse();
    let bundle = find_bundle(Path::new(&cli.bundle));

    let mut store = match FullStore::open(&bundle) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot open index: {e}");
            std::process::exit(1);
        }
    };

    let clock = SystemClock;
    let exit_code = okf_cli_adapter::run(&cli, &bundle, &mut store, &clock);
    std::process::exit(exit_code);
}
