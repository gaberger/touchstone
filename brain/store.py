"""Derived index. Disposable by construction -- `brain index` rebuilds it from
the bundle alone. Nothing here is ever the source of truth (ARCHITECTURE.md §2).
"""
from __future__ import annotations

import json
import posixpath
import sqlite3
from pathlib import Path

from .okf import Concept

SCHEMA = """
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
"""


def connect(db_path: Path) -> sqlite3.Connection:
    db_path.parent.mkdir(parents=True, exist_ok=True)
    con = sqlite3.connect(db_path)
    con.row_factory = sqlite3.Row
    con.executescript(SCHEMA)
    return con


def resolve_link(src_path: str, target: str) -> str | None:
    """Bundle-absolute (leading /) or relative, per spec. Returns a bundle-relative
    path, or None for external/anchor targets."""
    t = target.split("#", 1)[0]
    if not t or t.startswith(("http://", "https://", "mailto:", "tel:")):
        return None
    if t.startswith("/"):
        return posixpath.normpath(t.lstrip("/"))
    base = posixpath.dirname(src_path)
    return posixpath.normpath(posixpath.join(base, t))


def upsert(con: sqlite3.Connection, c: Concept, known: set[str]) -> None:
    con.execute("DELETE FROM tags WHERE path=?", (c.path,))
    con.execute("DELETE FROM edges WHERE src=?", (c.path,))
    con.execute("DELETE FROM fts WHERE path=?", (c.path,))
    con.execute(
        """INSERT INTO concepts(path,digest,type,title,description,status,stale_after,
                                trust,conformant,error,fm_json,body)
           VALUES(?,?,?,?,?,?,?,?,?,?,?,?)
           ON CONFLICT(path) DO UPDATE SET
             digest=excluded.digest, type=excluded.type, title=excluded.title,
             description=excluded.description, status=excluded.status,
             stale_after=excluded.stale_after, trust=excluded.trust,
             conformant=excluded.conformant, error=excluded.error,
             fm_json=excluded.fm_json, body=excluded.body""",
        (c.path, c.digest, c.type, c.title, c.description, c.status,
         c.fm.get("stale_after") and str(c.fm["stale_after"]), c.trust,
         int(c.conformant), c.error, json.dumps(c.fm, default=str), c.body),
    )
    tags = c.tags
    for t in set(tags):
        con.execute("INSERT OR IGNORE INTO tags(path,tag) VALUES(?,?)", (c.path, t))
    for _, target in c.links():
        dst = resolve_link(c.path, target)
        if dst is None:
            continue
        con.execute(
            "INSERT OR IGNORE INTO edges(src,target,dst,resolved) VALUES(?,?,?,?)",
            (c.path, target, dst, int(dst in known)),
        )
    con.execute("INSERT INTO fts(path,title,description,tags,body) VALUES(?,?,?,?,?)",
                (c.path, c.title, c.description, " ".join(tags), c.body))


def reresolve(con: sqlite3.Connection) -> None:
    """Broken links are legal (spec), so we record them rather than reject them."""
    con.execute("""UPDATE edges SET resolved =
                   (SELECT COUNT(*) FROM concepts c WHERE c.path = edges.dst)""")


TRUST_BOOST = {"human": 1.30, "machine": 1.00, "unattributed": 0.90}


def search(con: sqlite3.Connection, query: str, *, type_=None, tag=None,
           status=None, trust=None, today: str | None = None,
           expand: bool = True, limit: int = 10) -> list[dict]:
    """structured prefilter -> BM25 -> one-hop graph expansion -> trust rank."""
    where, params = ["c.conformant=1"], []
    if type_:
        where.append("c.type = ?"); params.append(type_)
    if status:
        where.append("c.status = ?"); params.append(status)
    if trust:
        where.append("c.trust = ?"); params.append(trust)
    if tag:
        where.append("EXISTS(SELECT 1 FROM tags t WHERE t.path=c.path AND t.tag=?)")
        params.append(tag)

    sql = f"""
      SELECT c.path, c.title, c.description, c.type, c.status, c.trust,
             c.stale_after, bm25(fts) AS bm
      FROM fts JOIN concepts c ON c.path = fts.path
      WHERE fts MATCH ? AND {' AND '.join(where)}
      ORDER BY bm LIMIT ?
    """
    # over-retrieve: FINDINGS.md E1 showed depth is what protects recall under
    # any post-filter, at 5x the cost of the cheapest stage in the pipeline.
    depth = max(limit * 50, 500)
    try:
        rows = con.execute(sql, [query, *params, depth]).fetchall()
    except sqlite3.OperationalError as e:
        raise ValueError(f"bad FTS query: {e}") from e

    scored: dict[str, dict] = {}
    for r in rows:
        base = -float(r["bm"])                      # bm25(): lower is better
        s = base * TRUST_BOOST.get(r["trust"], 1.0)
        if today and r["stale_after"] and str(r["stale_after"]) < today:
            s *= 0.60                               # penalized, not hidden
        scored[r["path"]] = {**dict(r), "score": s, "via": "direct"}

    if expand and scored:
        seeds = list(scored)[: min(20, len(scored))]
        qs = ",".join("?" * len(seeds))
        # The structured filter binds here too. An expanded neighbour that fails
        # the caller's filter is not a weaker match -- it is an excluded one.
        nb = con.execute(
            f"""SELECT DISTINCT e.dst AS path, c.title, c.description, c.type,
                       c.status, c.trust, c.stale_after
                FROM edges e JOIN concepts c ON c.path = e.dst
                WHERE e.resolved=1 AND (e.src IN ({qs}) OR e.dst IN ({qs}))
                  AND c.conformant=1 AND {' AND '.join(where)}""",
            seeds + seeds + params).fetchall()
        best = max((v["score"] for v in scored.values()), default=1.0)
        for r in nb:
            if r["path"] in scored:
                continue
            scored[r["path"]] = {**dict(r), "score": best * 0.25, "via": "link"}

    out = sorted(scored.values(), key=lambda d: -d["score"])[:limit]
    return out
