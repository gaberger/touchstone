"""T1 / T2 / T6 -- the three tests the architecture rests on (PROTOTYPE.md §3).

Each has a pre-registered kill criterion. Exit code is non-zero if any fails.

Implementation selector
-----------------------
Pass --impl rust  (or set TOUCHSTONE_IMPL=rust) to route every brain() call
through target/release/touchstone instead of `python -m touchstone`.  Drills
that call Python library functions directly (T2b, T2d, T2e) are recorded as
N/A in Rust mode because they test the Python parser, not the Rust binary.
"""
from __future__ import annotations

import argparse
import os
import subprocess
import shutil
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from touchstone import okf                                     # noqa: E402
from touchstone.cli import iter_concept_files, load_all        # noqa: E402

PY = sys.executable
ROOT = Path(__file__).resolve().parents[1]

# The Rust binary lives in the main git working tree, not the worktree.
# git rev-parse --git-common-dir returns the shared .git dir; its parent
# is the main workspace root that owns target/.
_git_common = subprocess.run(
    ["git", "rev-parse", "--git-common-dir"],
    capture_output=True, text=True, cwd=str(ROOT),
).stdout.strip()
CARGO_ROOT = Path(_git_common).parent if _git_common else ROOT
RUST_BIN = CARGO_ROOT / "target" / "release" / "touchstone"

# Parse --impl early (parse_known_args so the positional bundle arg is not consumed here)
_pre = argparse.ArgumentParser(add_help=False)
_pre.add_argument("--impl", choices=["python", "rust"],
                  default=os.environ.get("TOUCHSTONE_IMPL", "python"))
IMPL = _pre.parse_known_args()[0].impl

RESULTS: list[tuple[str, bool | None, str]] = []


def record(name: str, ok: bool | None, detail: str = "") -> None:
    RESULTS.append((name, ok, detail))
    label = "PASS" if ok is True else ("N/A " if ok is None else "FAIL")
    print(f"{label}  {name}" + (f"  -- {detail}" if detail else ""))


def sh(*a, cwd=None):
    return subprocess.run(a, cwd=cwd, capture_output=True, text=True)


def brain(bundle: Path, *a):
    """Shell out to whichever implementation is selected."""
    if IMPL == "rust":
        return sh(str(RUST_BIN), "--bundle", str(bundle), *a, cwd=str(ROOT))
    return sh(PY, "-m", "touchstone", "--bundle", str(bundle), *a, cwd=str(ROOT))


# --------------------------------------------------------------------- T1

def t1_rebuild(bundle: Path) -> None:
    """KILL: any generated index.md differing by one byte after a full rebuild."""
    brain(bundle, "index", "-q")
    before = {p.relative_to(bundle).as_posix(): p.read_bytes()
              for p in bundle.rglob("index.md")}

    # nuke everything derived
    sh("rm", "-rf", str(bundle / ".touchstone"))
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
    This is the diagnostic for the Operator's silent-truncation failure mode.

    Python library only -- skipped in Rust mode.  This drill calls okf.parse /
    okf.format_concept directly and never invokes brain(), so its result is
    identical regardless of which binary ran.  Running it in Rust mode would
    silently test Python, not Rust.
    """
    if IMPL == "rust":
        record("T2b semantic round-trip", None, "N/A -- Python library test, not Rust binary")
        return
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
    db = bundle / ".touchstone" / "index.db"
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
    Nothing in a parsed concept may be a date/datetime object.

    Python library only -- skipped in Rust mode.  Rust's parser is not PyYAML;
    the coercion failure mode being tested is specific to the Python loader.
    """
    if IMPL == "rust":
        record("T2d no timestamp coercion", None, "N/A -- Python library test (PyYAML regression guard)")
        return
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
    whose authored structure it cannot reproduce (anchors, block scalars).

    Python library only -- skipped in Rust mode.  okf.formattable and
    okf.format_concept are Python functions; there is no Rust equivalent yet.
    """
    if IMPL == "rust":
        record("T2e fmt safety", None, "N/A -- Python library test (okf.format_concept)")
        return
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
    db = bundle / ".touchstone" / "index.db"
    con = sqlite3.connect(db)
    before = con.execute("SELECT COUNT(*) FROM concepts").fetchone()[0]
    con.close()

    sh("rm", "-rf", str(bundle / ".touchstone"))
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
    ap = argparse.ArgumentParser(
        description="Touchstone stage-1 drills",
        epilog="default bundle: _fixture;  default impl: python",
    )
    ap.add_argument("bundle", nargs="?", help="bundle directory (default: _fixture)")
    ap.add_argument("--impl", choices=["python", "rust"],
                    default=os.environ.get("TOUCHSTONE_IMPL", "python"),
                    help="which implementation to exercise (default: python)")
    args = ap.parse_args()

    # IMPL was set at module load via parse_known_args; sync it in case the
    # user is running __main__ with an explicit --impl flag.
    globals()["IMPL"] = args.impl

    source = Path(args.bundle).resolve() if args.bundle else ROOT / "_fixture"
    tmp = ROOT / ("_export" if not args.bundle else f"_export-{source.name}")

    # T1 is DESTRUCTIVE by design -- it deletes every index.md and the derived dir to prove the
    # rebuild is byte-identical. Run in place and it consumes the corpus it is testing: the first
    # run of E4 deleted acme_retail/attesters/index.md (brain will not regenerate an index for a
    # concept-free directory), which both destroyed vendored third-party data AND erased the
    # evidence for the very defect it had just found. The failure is self-erasing -- a second run
    # passes, because the missing file is no longer there to be missing.
    #
    # So: always work on a copy. The source bundle is read-only as far as the drills are concerned.
    work = ROOT / f"_work-{source.name}"
    if work.exists():
        shutil.rmtree(work)
    shutil.copytree(source, work, ignore=shutil.ignore_patterns(".touchstone"))
    bundle = work
    if not bundle.exists():
        print(f"no such bundle: {bundle}")
        print("usage: python tests/drills.py [BUNDLE_DIR]   (default: _fixture)")
        print("build the fixture first: python tests/make_fixture.py _fixture")
        sys.exit(2)

    print("=" * 66)
    print(f"STAGE 1 DRILLS  [{IMPL}]  {source.name}")
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

    failed = [n for n, ok, _ in RESULTS if ok is False]
    na = [n for n, ok, _ in RESULTS if ok is None]
    passed_n = len(RESULTS) - len(failed) - len(na)
    print("=" * 66)
    print(f"{passed_n}/{len(RESULTS) - len(na)} passed"
          + (f"  ({len(na)} N/A)" if na else ""))
    if failed:
        print("FAILED: " + ", ".join(failed))
    shutil.rmtree(work, ignore_errors=True)
    sys.exit(1 if failed else 0)
