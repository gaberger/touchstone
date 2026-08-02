#!/usr/bin/env bash
# A10 evidence collector.
#
#   a10/run.sh baseline    <corpus-dir>   # phase 1: rg over your existing notes
#   a10/run.sh touchstone  <bundle-dir>   # phase 2: touchstone search over the same corpus
#
# Runs each pre-registered question against the arm and writes a scoring sheet. It does NOT
# score anything: whether an answer was usable is a judgment, and a harness that guessed it
# would be measuring itself. Rows come out with empty verdict columns for you to fill in.
set -uo pipefail

cd "$(dirname "$0")/.." || exit 2
ARM=${1:-}
CORPUS=${2:-}
Q=a10/questions.md

die() { printf '\033[31m%s\033[0m\n' "$1" >&2; exit 1; }

[ -n "$ARM" ] && [ -n "$CORPUS" ] || die "usage: a10/run.sh <baseline|touchstone> <corpus-dir>"
[ -d "$CORPUS" ] || die "no such corpus: $CORPUS"
[ -f "$Q" ] || die "no $Q -- copy a10/questions.template.md, write 20 questions, and COMMIT them first"

# ── Corpus guard ────────────────────────────────────────────────────────────
# PROTOTYPE.md §2: "Do not measure search quality on synthetic text." _fixture is worse than
# synthetic here -- it is 25 concepts written to satisfy the conformance drills, so scoring
# retrieval on it grades the system against a corpus built to make it look good.
case "$(cd "$CORPUS" && pwd)" in
  */_scale*)   die "_scale is generated filler -- word-salad bodies. Retrieval numbers over it are meaningless (PROTOTYPE.md §2)." ;;
  */_fixture*) die "_fixture was authored to satisfy the drills. Measuring retrieval on it is grading the system on its own homework." ;;
esac

# ── Question integrity ──────────────────────────────────────────────────────
# The hash is what makes this an experiment rather than an anecdote: it pins the questions to
# the moment before any result was seen. score.sh refuses a sheet whose hash has drifted.
QHASH=$(shasum -a 256 "$Q" | cut -c1-16)
if ! git diff --quiet -- "$Q" 2>/dev/null || ! git ls-files --error-unmatch "$Q" >/dev/null 2>&1; then
  printf '\033[33mwarning: %s is uncommitted or modified.\033[0m\n' "$Q" >&2
  printf '  Pre-registration is the point. Commit the questions before collecting evidence.\n\n' >&2
fi

OUT="a10/results-${ARM}.tsv"
[ -e "$OUT" ] && die "$OUT exists -- move it aside rather than overwriting recorded evidence"

# Questions are numbered lines starting `1. `.
mapfile_compat() { grep -nE '^[0-9]+\. ' "$Q" | sed 's/^[0-9]*://; s/^[0-9]*\. //'; }
N=$(mapfile_compat | wc -l | tr -d ' ')
[ "$N" -ge 1 ] || die "no questions found in $Q (expected lines like '1. How do I ...')"
[ "$N" -eq 20 ] || printf '\033[33mnote: %s questions, not 20. The kill criterion is stated as 15/20.\033[0m\n\n' "$N"

{
  printf '# a10 arm=%s corpus=%s questions=%s qhash=%s\n' "$ARM" "$CORPUS" "$N" "$QHASH"
  printf '# Fill in: answered (y/n)  seconds  failure (recall|structure|absence|-)  note\n'
  printf 'n\tquestion\tanswered\tseconds\tfailure\tnote\n'
} > "$OUT"

i=0
while IFS= read -r q; do
  i=$((i + 1))
  printf '\033[1m[%s/%s]\033[0m %s\n' "$i" "$N" "$q"

  if [ "$ARM" = "baseline" ]; then
    # Naive keyword search, which is exactly the null hypothesis: words the asker chose,
    # matched literally. Stopwords dropped so the query is the content terms.
    terms=$(printf '%s' "$q" | tr 'A-Z' 'a-z' | tr -cs 'a-z0-9' '\n' |
            grep -vwE 'the|a|an|of|to|in|is|are|do|does|did|what|which|how|why|when|where|i|we|our|my|and|or|for|on|with|that|this|it|be|was|were|can|should|all|any' |
            grep -vE '^.{1,2}$' | head -6 | paste -sd'|' -)
    printf '  rg: %s\n' "$terms"
    rg -il --max-count=1 "$terms" "$CORPUS" 2>/dev/null | head -8 | sed 's/^/      /'
  else
    ./target/release/touchstone --bundle "$CORPUS" search "$q" --limit 8 2>/dev/null |
      sed 's/^/      /'
  fi
  echo

  printf '%s\t%s\t\t\t\t\n' "$i" "$q" >> "$OUT"
done < <(mapfile_compat)

printf '\033[1mEvidence written to %s\033[0m\n' "$OUT"
printf 'Score it by hand, then: a10/score.sh %s\n' "$OUT"
