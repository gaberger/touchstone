"""T1 / T2 / T6 -- the three tests the architecture rests on (PROTOTYPE.md §3).

Each has a pre-registered kill criterion. Exit code is non-zero if any fails.
"""
from __future__ import annotations

import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from brain import okf                                     # noqa: E402
from brain.cli import iter_concept_files, load_all        # noqa: E402

PY = sys.executable
RESULTS: list[tuple[str, bool, str]] = []


def record(name: str, ok: bool, detail: str = "") -> None:
    RESULTS.append((name, ok, detail))
    print(f"{'PASS' if ok else 'FAIL'}  {name}" + (f"  -- {detail}" if detail else ""))


def sh(*a, cwd=None):
    return subprocess.run(a, cwd=cwd, capture_output=True, text=True)


def brain(bundle: Path, *a):
    return sh(PY, "-m", "brain", "--bundle", str(bundle), *a,
              cwd=str(Path(__file__).resolve().parents[1]))


# --------------------------------------------------------------------- T1

def t1_rebuild(bundle: Path) -> None:
    """KILL: any generated index.md differing by one byte after a full rebuild."""
    brain(bundle, "index", "-q")
    before = {p.relative_to(bundle).as_posix(): p.read_bytes()
              for p in bundle.rglob("index.md")}

    # nuke everything derived
    sh("rm", "-rf", str(bundle / ".brain"))
    for p in list(bundle.rglob("index.md")):
        p.unlink()

    r = brain(bundle, "index", "-q")
    if r.returncode != 0:
        record("T1 rebuild drill", False, f"index failed: {r.stderr.strip()[:200]}")
        return

    after = {p.relative_to(bundle).as_posix(): p.read_bytes()
             for p in bundle.rglob("index.md")}
    if set(before) != set(after):
        missing = set(before) ^ set(after)
        record("T1 rebuild drill", False, f"index.md set changed: {sorted(missing)[:5]}")
        return
    diff = [k for k in before if before[k] != after[k]]
    record("T1 rebuild drill", not diff,
           "byte-identical across full rebuild" if not diff
           else f"{len(diff)} file(s) differ: {diff[:3]}")


def t1_idempotent(bundle: Path) -> None:
    """A second index run must change nothing."""
    brain(bundle, "index", "-q")
    a = {p: p.read_bytes() for p in bundle.rglob("index.md")}
    brain(bundle, "index", "-q")
    b = {p: p.read_bytes() for p in bundle.rglob("index.md")}
    diff = [str(k) for k in a if a[k] != b[k]]
    record("T1b idempotence", not diff,
           "second run is a no-op" if not diff else f"{len(diff)} changed")


# --------------------------------------------------------------------- T2

def t2_raw_roundtrip(bundle: Path, tmp: Path) -> None:
    """KILL: a single non-identical byte through ingest -> export."""
    brain(bundle, "index", "-q")
    r = brain(bundle, "export", str(tmp), "--force")
    if r.returncode != 0:
        record("T2a raw round-trip", False, r.stderr.strip()[:200])
        return
    bad = []
    for full, rel in iter_concept_files(bundle):
        out = tmp / rel
        if not out.exists():
            bad.append(f"{rel} (missing)")
        elif out.read_bytes() != full.read_bytes():
            bad.append(rel)
    record("T2a raw round-trip", not bad,
           "byte-identical incl. CRLF, unicode paths, anchors"
           if not bad else f"{len(bad)} differ: {bad[:3]}")


def t2_semantic_roundtrip(bundle: Path) -> None:
    """Would a schema-backed store lose anything? Parse -> canonical reserialize
    -> reparse, and compare the PARSED VALUES. Formatting may change; data must not.
    This is the diagnostic for the Operator's silent-truncation failure mode."""
    lost = []
    for c in load_all(bundle):
        if not c.fm:
            continue
        try:
            re_text = okf.format_concept(c)
            c2 = okf.parse(c.path, re_text)
        except Exception as e:
            lost.append(f"{c.path}: reserialize raised {type(e).__name__}")
            continue
        missing = set(c.fm) - set(c2.fm)
        if missing:
            lost.append(f"{c.path}: lost keys {sorted(missing)}")
            continue
        for k, v in c.fm.items():
            if c2.fm.get(k) != v:
                lost.append(f"{c.path}: key `{k}` changed value")
                break
        if c2.body.strip() != c.body.strip():
            lost.append(f"{c.path}: body changed")
    record("T2b semantic round-trip", not lost,
           "no key or value lost through reserialization"
           if not lost else f"{len(lost)} loss(es): {lost[:3]}")


def t2_unknown_keys(bundle: Path) -> None:
    """Spec: consumers MUST NOT reject unknown types/keys, and MUST tolerate
    broken links. Verify we actually indexed them rather than dropping them."""
    import json as _json
    import sqlite3
    db = bundle / ".brain" / "index.db"
    con = sqlite3.connect(db); con.row_factory = sqlite3.Row
    row = con.execute("SELECT fm_json,type FROM concepts WHERE path=?",
                      ("adversarial/unknown-type.md",)).fetchone()
    if not row:
        record("T2c unknown keys/types", False, "concept not indexed at all")
        return
    fm = _json.loads(row["fm_json"])
    problems = []
    if row["type"] != "ThreatModel":
        problems.append("unknown type not preserved")
    for k in ("retention", "classification"):
        if k not in fm:
            problems.append(f"unknown key `{k}` dropped")
    broken = con.execute("SELECT COUNT(*) c FROM edges WHERE resolved=0").fetchone()["c"]
    if broken == 0:
        problems.append("broken links not recorded")
    record("T2c unknown keys/types", not problems,
           f"unknown type + 2 unknown keys preserved, {broken} broken links recorded"
           if not problems else "; ".join(problems))


def t2_no_type_coercion(bundle: Path) -> None:
    """REGRESSION: PyYAML's implicit timestamp resolver silently rewrites
    `2026-01-01T00:00:00Z` to `2026-01-01 00:00:00+00:00`, which is not ISO 8601.
    Nothing in a parsed concept may be a date/datetime object."""
    import datetime as _dt
    bad = []

    def walk(v, path):
        if isinstance(v, (_dt.date, _dt.datetime)):
            bad.append(path)
        elif isinstance(v, dict):
            for k, vv in v.items():
                walk(vv, f"{path}.{k}")
        elif isinstance(v, list):
            for i, vv in enumerate(v):
                walk(vv, f"{path}[{i}]")

    for c in load_all(bundle):
        walk(c.fm, c.path)
    record("T2d no timestamp coercion", not bad,
           "all temporal values remain ISO 8601 strings"
           if not bad else f"{len(bad)} coerced to datetime: {bad[:3]}")


def t2_fmt_is_safe(bundle: Path) -> None:
    """`brain fmt` is the only command that rewrites a file. Every rewrite it
    would perform must preserve parsed values exactly, and it must REFUSE files
    whose authored structure it cannot reproduce (anchors, block scalars)."""
    unsafe, refused = [], 0
    for c in load_all(bundle):
        reason = okf.formattable(c)
        if reason:
            refused += 1
            continue
        out = okf.format_concept(c)
        c2 = okf.parse(c.path, out)
        if c2.fm != c.fm or c2.body.strip() != c.body.strip():
            unsafe.append(c.path)
    record("T2e fmt safety", not unsafe,
           f"every rewrite value-preserving; {refused} file(s) correctly refused"
           if not unsafe else f"{len(unsafe)} unsafe rewrite(s): {unsafe[:3]}")


# --------------------------------------------------------------------- T6

def t6_service_death(bundle: Path, tmp: Path) -> None:
    """Destroy everything derived and the index db, then rebuild from the bare
    bundle. KILL: concept count differs by >0."""
    brain(bundle, "index", "-q")
    import sqlite3
    db = bundle / ".brain" / "index.db"
    con = sqlite3.connect(db)
    before = con.execute("SELECT COUNT(*) FROM concepts").fetchone()[0]
    con.close()

    sh("rm", "-rf", str(bundle / ".brain"))
    for p in list(bundle.rglob("index.md")):
        p.unlink()
    r = brain(bundle, "index", "-q")
    if r.returncode != 0:
        record("T6 service-death drill", False, r.stderr.strip()[:200])
        return
    con = sqlite3.connect(db)
    after = con.execute("SELECT COUNT(*) FROM concepts").fetchone()[0]
    con.close()
    record("T6 service-death drill", before == after,
           f"{after} concepts recovered from files alone (was {before})")


# -------------------------------------------------------------------- search

def t_search_smoke(bundle: Path) -> None:
    r = brain(bundle, "search", "revocation", "--limit", "3")
    ok = r.returncode == 0 and "revocation" in r.stdout.lower()
    record("search smoke", ok, (r.stdout.strip().splitlines() or ["no output"])[0])

    r = brain(bundle, "search", "index", "--type", "Decision", "--limit", "5")
    ok2 = r.returncode == 0 and "decisions/" in r.stdout
    record("search structured filter", ok2, "type=Decision filter applied")


if __name__ == "__main__":
    root = Path(__file__).resolve().parents[1]
    # Optional bundle argument. The self-authored fixture is the default, but FINDINGS notes it is
    # "adversarial but self-authored, which is its weakness" -- real OKF written by people who did not
    # know our assumptions is the stronger test. Without this the drills could only ever run against
    # our own fixture, which is why T2-against-upstream had never been run.
    bundle = Path(sys.argv[1]).resolve() if len(sys.argv) > 1 else root / "_fixture"
    tmp = root / ("_export" if len(sys.argv) < 2 else f"_export-{bundle.name}")
    if not bundle.exists():
        print(f"no such bundle: {bundle}")
        print("usage: python tests/drills.py [BUNDLE_DIR]   (default: _fixture)")
        print("build the fixture first: python tests/make_fixture.py _fixture")
        sys.exit(2)

    print("=" * 66)
    print("STAGE 1 DRILLS")
    print("=" * 66)
    t1_rebuild(bundle)
    t1_idempotent(bundle)
    t2_raw_roundtrip(bundle, tmp)
    t2_semantic_roundtrip(bundle)
    t2_unknown_keys(bundle)
    t2_no_type_coercion(bundle)
    t2_fmt_is_safe(bundle)
    t6_service_death(bundle, tmp)
    t_search_smoke(bundle)

    failed = [n for n, ok, _ in RESULTS if not ok]
    print("=" * 66)
    print(f"{len(RESULTS) - len(failed)}/{len(RESULTS)} passed")
    if failed:
        print("FAILED: " + ", ".join(failed))
    sys.exit(1 if failed else 0)
