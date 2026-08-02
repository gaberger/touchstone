#!/usr/bin/env bash
# A10 verdict.
#
#   a10/score.sh a10/results-baseline.tsv
#
# Applies the pre-registered kill criterion and reports the failure distribution. Refuses to
# score a sheet whose questions have changed since the evidence was collected.
set -uo pipefail
cd "$(dirname "$0")/.." || exit 2

SHEET=${1:-}
[ -f "$SHEET" ] || { echo "usage: a10/score.sh <results.tsv>" >&2; exit 2; }

# ── Pre-registered, and not a variable ──────────────────────────────────────
# PROTOTYPE.md §1: "if >=15 of 20 questions are answered acceptably by rg + Obsidian, stop."
# Written before the code existed. Moving it after seeing results is how this experiment gets
# quietly wasted -- so it is a constant, cited, and any change shows up in the diff.
THRESHOLD=15
DENOM=20

header=$(head -1 "$SHEET")
ARM=$(sed -n 's/.*arm=\([^ ]*\).*/\1/p' <<<"$header")
QHASH=$(sed -n 's/.*qhash=\([^ ]*\).*/\1/p' <<<"$header")

now=$(shasum -a 256 a10/questions.md 2>/dev/null | cut -c1-16)
if [ -n "$QHASH" ] && [ -n "$now" ] && [ "$QHASH" != "$now" ]; then
  printf '\033[31mREFUSING TO SCORE.\033[0m questions.md has changed since this evidence was collected.\n' >&2
  printf '  recorded %s, now %s\n' "$QHASH" "$now" >&2
  printf '  Editing a question after seeing results turns the experiment into an anecdote.\n' >&2
  printf '  Restore the questions, or re-run the arm against the new ones.\n' >&2
  exit 1
fi

rows=$(awk -F'\t' 'NR>3 && NF>=5' "$SHEET" | wc -l | tr -d ' ')
yes=$(awk -F'\t'  'NR>3 && tolower($3)=="y"' "$SHEET" | wc -l | tr -d ' ')
no=$(awk -F'\t'   'NR>3 && tolower($3)=="n"' "$SHEET" | wc -l | tr -d ' ')
unscored=$((rows - yes - no))

printf '\n\033[1mA10 — %s\033[0m  (%s questions)\n\n' "${ARM:-unknown arm}" "$rows"
printf '  answered      %s\n' "$yes"
printf '  not answered  %s\n' "$no"
[ "$unscored" -gt 0 ] && printf '  \033[33munscored      %s\033[0m  <- score these; they are not counted either way\n' "$unscored"

med=$(awk -F'\t' 'NR>3 && $4 ~ /^[0-9]+$/ {print $4}' "$SHEET" | sort -n | awk '{a[NR]=$1} END {if(NR) print (NR%2)?a[(NR+1)/2]:int((a[NR/2]+a[NR/2+1])/2)}')
[ -n "$med" ] && printf '  median time   %ss\n' "$med"

printf '\n\033[1mFailure distribution\033[0m  (the informative part)\n\n'
for mode in recall structure absence; do
  c=$(awk -F'\t' -v m="$mode" 'NR>3 && tolower($5)==m' "$SHEET" | wc -l | tr -d ' ')
  printf '  %-10s %s\n' "$mode" "$c"
done

recall=$(awk -F'\t'    'NR>3 && tolower($5)=="recall"'    "$SHEET" | wc -l | tr -d ' ')
structure=$(awk -F'\t' 'NR>3 && tolower($5)=="structure"' "$SHEET" | wc -l | tr -d ' ')
absence=$(awk -F'\t'   'NR>3 && tolower($5)=="absence"'   "$SHEET" | wc -l | tr -d ' ')

echo
if [ "$absence" -gt 0 ] && [ "$absence" -ge "$structure" ] && [ "$absence" -ge "$recall" ]; then
  printf '  \033[33mFailures cluster on ABSENCE.\033[0m The notes were never written. No architecture\n'
  printf '  fixes that -- it is A3. A10 over this corpus is premature; the corpus is too thin.\n'
elif [ "$structure" -gt 0 ] && [ "$structure" -ge "$recall" ]; then
  printf '  \033[32mFailures cluster on STRUCTURE.\033[0m Queries needing `type` or `stale_after` that\n'
  printf '  grep cannot express. This is the strongest available evidence for the OKF profile.\n'
elif [ "$recall" -gt 0 ]; then
  printf '  Failures cluster on RECALL -- the note existed, the words differed. Vectors (A4)\n'
  printf '  are the indicated fix, not frontmatter.\n'
fi

printf '\n\033[1mVerdict\033[0m\n\n'
if [ "$unscored" -gt 0 ]; then
  printf '  \033[33mINCOMPLETE\033[0m — %s row(s) unscored. No verdict until every question is judged.\n\n' "$unscored"
  exit 2
fi

if [ "${ARM:-}" = "baseline" ]; then
  if [ "$yes" -ge "$THRESHOLD" ]; then
    printf '  \033[31mKILL CRITERION MET: %s/%s >= %s.\033[0m\n\n' "$yes" "$DENOM" "$THRESHOLD"
    printf '  rg + Obsidian already answer these. Per PROTOTYPE.md §1: stop. Ship a frontmatter\n'
    printf '  linter and a `touchstone new` template. The rest of the architecture is unjustified.\n\n'
    exit 1
  fi
  printf '  \033[32mBaseline scored %s/%s, below the %s threshold.\033[0m\n\n' "$yes" "$DENOM" "$THRESHOLD"
  printf '  The null hypothesis does not hold. You now have a benchmark the real system must\n'
  printf '  beat -- run phase 2:  a10/run.sh touchstone <bundle>\n\n'
else
  base="a10/results-baseline.tsv"
  if [ -f "$base" ]; then
    b=$(awk -F'\t' 'NR>3 && tolower($3)=="y"' "$base" | wc -l | tr -d ' ')
    printf '  touchstone %s/%s vs baseline %s/%s\n\n' "$yes" "$DENOM" "$b" "$DENOM"
    if [ "$yes" -gt "$b" ]; then
      printf '  \033[32mBeats the baseline by %s.\033[0m Worth recording in FINDINGS.md with the\n' "$((yes - b))"
      printf '  failure distribution, which is what says *why*.\n\n'
    else
      printf '  \033[31mDoes not beat the baseline.\033[0m That is the answer A10 exists to give.\n\n'
      exit 1
    fi
  else
    printf '  %s/%s answered. No baseline sheet found -- phase 1 is the comparison, run it.\n\n' "$yes" "$DENOM"
  fi
fi
