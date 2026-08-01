"""brain -- Stage 1 walking skeleton (PROTOTYPE.md §3).

No CRDT, no ACL, no vectors, no agents, no server. Just enough to run the three
tests the whole architecture rests on: T1 rebuild, T2 round-trip, T6 service-death.
"""
from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import shutil
import sys
import uuid
from pathlib import Path

from . import okf, render, store

DB_REL = Path(".touchstone") / "index.db"


# ------------------------------------------------------------------ bundle io

def find_bundle(start: Path) -> Path:
    p = start.resolve()
    for cand in [p, *p.parents]:
        if (cand / "index.md").exists() or (cand / ".touchstone").exists():
            return cand
    return p


def iter_concept_files(root: Path):
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = sorted(d for d in dirnames
                             if not d.startswith((".", "_")) and d != "node_modules")
        for fn in sorted(filenames):
            if not fn.endswith(".md") or fn in okf.RESERVED:
                continue
            full = Path(dirpath) / fn
            yield full, full.relative_to(root).as_posix()


def load_all(root: Path) -> list[okf.Concept]:
    out = []
    for full, rel in iter_concept_files(root):
        raw = full.read_text(encoding="utf-8")
        out.append(okf.parse(rel, raw))
    return out


# --------------------------------------------------------------------- index

def cmd_index(args) -> int:
    root = find_bundle(Path(args.bundle))
    con = store.connect(root / DB_REL)
    concepts = load_all(root)
    known = {c.path for c in concepts}

    prev = {r["path"]: r["digest"] for r in con.execute("SELECT path,digest FROM concepts")}
    changed = new = 0
    for c in concepts:
        if prev.get(c.path) == c.digest:
            continue
        store.upsert(con, c, known)
        if c.path in prev:
            changed += 1
        else:
            new += 1
    removed = [p for p in prev if p not in known]
    for p in removed:
        for t in ("concepts", "tags", "fts"):
            con.execute(f"DELETE FROM {t} WHERE path=?", (p,))
        con.execute("DELETE FROM edges WHERE src=?", (p,))
    store.reresolve(con)
    con.commit()

    written = write_indexes(root, concepts)

    bad = [c for c in concepts if not c.conformant]
    broken = con.execute("SELECT COUNT(*) FROM edges WHERE resolved=0").fetchone()[0]
    if not args.quiet:
        print(f"indexed {len(concepts)} concepts "
              f"({new} new, {changed} changed, {len(removed)} removed)")
        print(f"index.md files written: {written}")
        print(f"broken links: {broken} (legal per spec -- not-yet-written knowledge)")
        if bad:
            print(f"NON-CONFORMANT: {len(bad)}")
            for c in bad[:10]:
                print(f"  {c.path}: {c.error}")
    return 0


def write_indexes(root: Path, concepts: list[okf.Concept]) -> int:
    by_dir: dict[str, list[okf.Concept]] = {}
    for c in concepts:
        d = c.path.rsplit("/", 1)[0] if "/" in c.path else ""
        by_dir.setdefault(d, []).append(c)

    dirs: set[str] = {""}
    for d in list(by_dir):
        parts = d.split("/") if d else []
        for i in range(len(parts) + 1):
            dirs.add("/".join(parts[:i]))

    def count_under(d: str) -> int:
        return sum(len(v) for k, v in by_dir.items()
                   if k == d or (d == "" and True) or k.startswith(d + "/"))

    n = 0
    for d in sorted(dirs):
        kids = sorted({
            k.split("/")[len(d.split("/")) if d else 0]
            for k in dirs
            if k != d and (k.startswith(d + "/") if d else k)
        })
        subdirs = []
        for kid in kids:
            full = f"{d}/{kid}" if d else kid
            if full in dirs:
                subdirs.append((kid, count_under(full)))
        text = render.render_index(d, by_dir.get(d, []), subdirs, is_root=(d == ""))
        target = (root / d / "index.md") if d else (root / "index.md")
        target.parent.mkdir(parents=True, exist_ok=True)
        old = target.read_text(encoding="utf-8") if target.exists() else None
        if old != text:
            target.write_text(text, encoding="utf-8")
        n += 1
    return n


# -------------------------------------------------------------------- search

def cmd_search(args) -> int:
    root = find_bundle(Path(args.bundle))
    con = store.connect(root / DB_REL)
    today = dt.date.today().isoformat()
    try:
        hits = store.search(con, args.query, type_=args.type, tag=args.tag,
                            status=args.status, trust=args.trust, today=today,
                            expand=not args.no_expand, limit=args.limit)
    except ValueError as e:
        print(e, file=sys.stderr)
        return 2
    if not hits:
        print("no results")
        return 1
    for h in hits:
        mark = {"human": "*", "machine": "~", "unattributed": " "}[h["trust"]]
        via = "" if h["via"] == "direct" else "  (via link)"
        print(f"{mark} {h['path']}{via}")
        print(f"    {h['title']}  [{h['type']}]")
        if h["description"]:
            print(f"    {h['description']}")
    print("\n* human-verified   ~ machine-generated")
    return 0


# ----------------------------------------------------------------------- new

TEMPLATE_TYPES = ["Note", "Source", "Person", "Project", "Decision", "Meeting",
                  "Term", "Runbook", "System", "Metric"]


def slugify(s: str) -> str:
    keep = [ch.lower() if ch.isalnum() else "-" for ch in s]
    out = "".join(keep)
    while "--" in out:
        out = out.replace("--", "-")
    return out.strip("-") or "untitled"


def cmd_new(args) -> int:
    root = find_bundle(Path(args.bundle))
    typ = args.type
    sub = args.dir or (typ.lower() + "s")
    rel = f"{sub}/{slugify(args.title)}.md"
    target = root / rel
    if target.exists():
        print(f"exists: {rel}", file=sys.stderr)
        return 1
    now = dt.datetime.now(dt.timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")
    fm = {
        "id": uuid.uuid4().hex[:12],
        "type": typ,
        "title": args.title,
        "description": args.description or "",
        "tags": args.tag or [],
        "status": "draft",
    }
    if args.generated:
        fm["generated"] = {"by": args.generated, "at": now}
    body = f"\n# {args.title}\n\n"
    text = f"---\n{okf.canonical_frontmatter(fm)}---\n{body}"
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(text, encoding="utf-8")
    print(rel)
    return 0


# ------------------------------------------------------------- fmt / lint

def cmd_fmt(args) -> int:
    root = find_bundle(Path(args.bundle))
    changed, skipped = [], []
    for full, rel in iter_concept_files(root):
        c = okf.parse(rel, full.read_text(encoding="utf-8"))
        reason = okf.formattable(c)
        if reason:
            if c.fm:                       # only report files we deliberately spared
                skipped.append((rel, reason))
            continue
        new = okf.format_concept(c)
        # Never write a rewrite that does not survive a re-parse unchanged.
        if okf.parse(rel, new).fm != c.fm:
            skipped.append((rel, "reserialization would change values"))
            continue
        if new != c.raw:
            changed.append(rel)
            if not args.check:
                full.write_text(new, encoding="utf-8")
    for r, why in skipped:
        print(f"skipped: {r} -- {why}")
    for r in changed:
        print(f"{'would reformat' if args.check else 'formatted'}: {r}")
    print(f"{len(changed)} file(s) {'need formatting' if args.check else 'formatted'}, "
          f"{len(skipped)} skipped")
    return 1 if (args.check and changed) else 0


def cmd_lint(args) -> int:
    root = find_bundle(Path(args.bundle))
    total = 0
    for full, rel in iter_concept_files(root):
        c = okf.parse(rel, full.read_text(encoding="utf-8"))
        problems = okf.lint(c)
        if problems:
            print(rel)
            for p in problems:
                print(f"  - {p}")
            total += len(problems)
    print(f"\n{total} problem(s)")
    return 1 if total else 0


# -------------------------------------------------------------------- export

def cmd_export(args) -> int:
    """Writes RAW bytes from the index. If anything was lost on ingest, T2 shows
    it as a diff -- which is the entire point of storing raw."""
    root = find_bundle(Path(args.bundle))
    out = Path(args.out)
    if out.exists() and args.force:
        shutil.rmtree(out)
    out.mkdir(parents=True, exist_ok=True)
    con = store.connect(root / DB_REL)
    n = 0
    for r in con.execute("SELECT path FROM concepts ORDER BY path"):
        src = root / r["path"]
        dst = out / r["path"]
        dst.parent.mkdir(parents=True, exist_ok=True)
        dst.write_bytes(src.read_bytes())
        n += 1
    for dirpath, _, filenames in os.walk(root):
        for fn in filenames:
            if fn in okf.RESERVED:
                rel = (Path(dirpath) / fn).relative_to(root)
                if ".touchstone" in rel.parts:
                    continue
                d = out / rel
                d.parent.mkdir(parents=True, exist_ok=True)
                d.write_bytes((Path(dirpath) / fn).read_bytes())
    print(f"exported {n} concepts to {out}")
    return 0


# --------------------------------------------------------------------- stats

def cmd_stats(args) -> int:
    root = find_bundle(Path(args.bundle))
    con = store.connect(root / DB_REL)
    q = lambda s: con.execute(s).fetchall()
    total = q("SELECT COUNT(*) n FROM concepts")[0]["n"]
    print(f"bundle: {root}")
    print(f"concepts: {total}")
    print("\nby type:")
    for r in q("SELECT type,COUNT(*) n FROM concepts GROUP BY type ORDER BY n DESC"):
        print(f"  {r['n']:>5}  {r['type'] or '(none)'}")
    print("\nby trust tier:")
    for r in q("SELECT trust,COUNT(*) n FROM concepts GROUP BY trust ORDER BY n DESC"):
        print(f"  {r['n']:>5}  {r['trust']}")
    print("\nby status:")
    for r in q("SELECT status,COUNT(*) n FROM concepts GROUP BY status ORDER BY n DESC"):
        print(f"  {r['n']:>5}  {r['status']}")
    e = q("SELECT COUNT(*) n, SUM(resolved) r FROM edges")[0]
    print(f"\nlinks: {e['n']} ({e['n'] - (e['r'] or 0)} broken -- legal per spec)")
    return 0


# ---------------------------------------------------------------------- main

def main(argv=None) -> int:
    p = argparse.ArgumentParser(prog="touchstone", description="Touchstone -- provenance and portability for machine-written knowledge")
    p.add_argument("--bundle", default=".", help="bundle root (default: discover)")
    sub = p.add_subparsers(dest="cmd", required=True)

    s = sub.add_parser("index", help="rebuild the derived index and index.md files")
    s.add_argument("-q", "--quiet", action="store_true")
    s.set_defaults(fn=cmd_index)

    s = sub.add_parser("search", help="search the bundle")
    s.add_argument("query")
    s.add_argument("--type"); s.add_argument("--tag")
    s.add_argument("--status"); s.add_argument("--trust")
    s.add_argument("--limit", type=int, default=10)
    s.add_argument("--no-expand", action="store_true")
    s.set_defaults(fn=cmd_search)

    s = sub.add_parser("new", help="scaffold a conformant concept")
    s.add_argument("type", choices=TEMPLATE_TYPES)
    s.add_argument("title")
    s.add_argument("--dir"); s.add_argument("--description")
    s.add_argument("--tag", action="append")
    s.add_argument("--generated", help="agent id, e.g. capture/claude-opus-5")
    s.set_defaults(fn=cmd_new)

    s = sub.add_parser("fmt", help="canonicalize frontmatter (the cheap CRDT alternative)")
    s.add_argument("--check", action="store_true")
    s.set_defaults(fn=cmd_fmt)

    s = sub.add_parser("lint", help="conformance + duplicate checks")
    s.set_defaults(fn=cmd_lint)

    s = sub.add_parser("export", help="write raw bytes back out")
    s.add_argument("out"); s.add_argument("--force", action="store_true")
    s.set_defaults(fn=cmd_export)

    s = sub.add_parser("stats", help="bundle summary")
    s.set_defaults(fn=cmd_stats)

    args = p.parse_args(argv)
    return args.fn(args)
