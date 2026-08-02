#!/usr/bin/env bash
# Convert this repository's own documentation into an OKF bundle, for use as an A10 corpus.
#
#   a10/convert-repo.sh /tmp/a10-repo-corpus
#
# WHY THIS IS A REASONABLE CORPUS: it is real prose, written as a working record rather than
# to make retrieval look good, and it is genuinely structured -- ADRs carry Status and Date,
# FINDINGS carries measured-vs-untested claims, PROTOTYPE carries pre-registered criteria.
# Questions like "which decisions are still Proposed?" or "which load-bearing claims are
# unmeasured?" are exactly the structured queries grep cannot express, which is the dimension
# A10 is trying to measure.
#
# WHY IT IS NOT A SUBSTITUTE FOR YOUR NOTES, and this matters:
#
#   1. It is SMALL -- around two dozen documents. PROTOTYPE.md warns that failures clustering
#      on *absence* mean the corpus is too thin to conclude anything. Expect that here.
#   2. It is HOMOGENEOUS. Every document is about one project, so terms like "trust tier" and
#      "conformance" appear everywhere. That flattens BM25 discrimination in a way a real,
#      idiosyncratic corpus does not -- it can make retrieval look worse than it is.
#   3. Its author bias runs the wrong way. Much of it was written recently, in this session,
#      by the same process now being measured.
#
# Treat a run over this as a PILOT: it exercises the harness and gives an early read on
# whether structure failures dominate. It does not settle A10.
set -uo pipefail
cd "$(dirname "$0")/.." || exit 2

OUT=${1:-}
[ -n "$OUT" ] || { echo "usage: a10/convert-repo.sh <out-dir>" >&2; exit 2; }
[ -e "$OUT" ] && { echo "refusing to overwrite $OUT" >&2; exit 2; }

mkdir -p "$OUT/decisions" "$OUT/records" "$OUT/notes"

# Frontmatter is emitted here rather than by `touchstone new` because these are EXISTING
# documents being described, not new concepts being scaffolded. Nothing below writes a
# `verified` key: an agent may not assert human verification, and a conversion script is an
# agent (ARCHITECTURE.md, "The trust invariant").
emit() {
  local src=$1 dst=$2 type=$3 title=$4 status=$5 tags=$6
  {
    printf -- '---\n'
    printf 'type: %s\n' "$type"
    printf 'title: "%s"\n' "$(printf '%s' "$title" | sed 's/"/\\"/g')"
    printf 'tags: [%s]\n' "$tags"
    printf 'status: %s\n' "$status"
    printf 'generated:\n  by: a10/convert-repo.sh\n  at: %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    printf -- '---\n\n'
    cat "$src"
  } > "$dst"
}

# ── ADRs → Decision concepts, carrying their real Status ────────────────────
# The Status field is the whole point: "which decisions are still Proposed?" is answerable
# from frontmatter and not from grep, which cannot tell a Status line from a mention.
n=0
for adr in docs/adrs/ADR-*.md; do
  [ -e "$adr" ] || continue
  title=$(head -1 "$adr" | sed 's/^# *//')
  status=$(grep -m1 '^\*\*Status:\*\*' "$adr" | sed 's/.*\*\*Status:\*\* *//' | tr 'A-Z' 'a-z' | tr -d '\r')
  case "$status" in
    accepted)  st=stable ;;
    proposed)  st=draft ;;
    *)         st=deprecated ;;
  esac
  emit "$adr" "$OUT/decisions/$(basename "${adr%.md}").md" Decision "$title" "$st" "adr, architecture"
  n=$((n + 1))
done

# ── The research record → Note concepts ─────────────────────────────────────
for doc in FINDINGS PROTOTYPE DECISIONS ARCHITECTURE RUST-PATH VERSION-CONTROL README; do
  [ -f "$doc.md" ] || continue
  title=$(head -1 "$doc.md" | sed 's/^# *//')
  emit "$doc.md" "$OUT/records/$(printf '%s' "$doc" | tr 'A-Z' 'a-z').md" Note "$title" stable "record"
  n=$((n + 1))
done

# ── Crate doc-comments → Note concepts ──────────────────────────────────────
# The //! headers are where the design reasoning actually lives in this codebase.
for lib in touchstone-*/src/lib.rs touchstone-adapters/*/*/src/lib.rs; do
  [ -f "$lib" ] || continue
  crate=$(printf '%s' "$lib" | sed 's|/src/lib.rs||; s|.*/||')
  doc=$(sed -n '/^\/\/!/p' "$lib" | sed 's|^//! \{0,1\}||')
  [ -n "$doc" ] || continue
  printf '%s\n' "$doc" > /tmp/a10-doc.$$
  emit /tmp/a10-doc.$$ "$OUT/notes/$crate.md" Note "$crate" stable "crate, design"
  rm -f /tmp/a10-doc.$$
  n=$((n + 1))
done

printf 'converted %s documents -> %s\n' "$n" "$OUT"
if [ -x ./target/release/touchstone ]; then
  ./target/release/touchstone --bundle "$OUT" index -q && ./target/release/touchstone --bundle "$OUT" stats | sed -n '1,3p'
fi
printf '\nPILOT ONLY -- see the header of this script for why this corpus cannot settle A10.\n'
