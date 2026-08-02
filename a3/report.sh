#!/usr/bin/env bash
# A3 readout — did a human write into the brain on days nobody asked them to?
#
#   a3/report.sh <bundle> [a3/project-days.txt]
#
# PROTOTYPE.md §4, pre-registered:
#
#   "if concept-creation rate on non-project days trends to zero by week three, editing is not
#    easy enough, and no amount of retrieval quality will save it."
#
# The measurement is not a survey and this script does not ask you how it went. It reads the
# append-only capture log the binary writes and applies that criterion.
set -uo pipefail
cd "$(dirname "$0")/.." || exit 2

BUNDLE=${1:-}
DAYS=${2:-a3/project-days.txt}
[ -n "$BUNDLE" ] || { echo "usage: a3/report.sh <bundle> [project-days.txt]" >&2; exit 2; }
LOG="$BUNDLE/.a3/capture.jsonl"
[ -f "$LOG" ] || { echo "no capture log at $LOG -- nothing has been written into this bundle yet" >&2; exit 2; }

field() { sed -n "s/.*\"$1\":\"\([^\"]*\)\".*/\1/p"; }

# ── Human captures only ─────────────────────────────────────────────────────
# An agent writing daily via MCP is not adoption. A3 asks whether a PERSON writes, so the
# primary series is `surface=cli` and everything else is reported separately.
human=$(grep '"surface":"cli"' "$LOG" 2>/dev/null || true)
agent=$(grep '"surface":"mcp"' "$LOG" 2>/dev/null || true)

hn=$(printf '%s' "$human" | grep -c . || true)
an=$(printf '%s' "$agent" | grep -c . || true)

printf '\n\033[1mA3 — adoption\033[0m  %s\n\n' "$BUNDLE"
printf '  human captures (cli)   %s\n' "$hn"
printf '  agent captures (mcp)   %s   \033[2m(not adoption; reported for contrast)\033[0m\n\n' "$an"

[ "$hn" -gt 0 ] || { printf '  \033[31mNo human captures at all.\033[0m That is the answer.\n\n'; exit 1; }

# ── Per-day series ──────────────────────────────────────────────────────────
series=$(printf '%s\n' "$human" | field at | cut -c1-10 | sort | uniq -c | awk '{print $2"\t"$1}')
first=$(printf '%s\n' "$series" | head -1 | cut -f1)
last=$(printf '%s\n' "$series" | tail -1 | cut -f1)
active=$(printf '%s\n' "$series" | grep -c . || true)

printf '  first capture  %s\n  last capture   %s\n  active days    %s\n\n' "$first" "$last" "$active"

if [ ! -f "$DAYS" ]; then
  printf '  \033[33mNo project-days file at %s.\033[0m\n' "$DAYS"
  printf '  The tool cannot know which days you were working ON this project -- that is the\n'
  printf '  split the whole experiment turns on, and it is yours to supply. One YYYY-MM-DD per\n'
  printf '  line. Without it, only the raw series below is available.\n\n'
  printf '%s\n' "$series" | awk -F'\t' '{printf "    %s  %s\n", $1, $2}'
  printf '\n  \033[33mINCOMPLETE\033[0m — no verdict without the day split.\n\n'
  exit 2
fi

# ── The split that matters ──────────────────────────────────────────────────
onp=0; offp=0; ondays=0; offdays=0
while IFS=$'\t' read -r day n; do
  if grep -qx "$day" "$DAYS" 2>/dev/null; then
    onp=$((onp + n)); ondays=$((ondays + 1)); tag="project"
  else
    offp=$((offp + n)); offdays=$((offdays + 1)); tag="NON-project"
  fi
  printf '    %s  %-3s %s\n' "$day" "$n" "$tag"
done <<< "$series"

printf '\n  on project days      %s captures across %s days\n' "$onp" "$ondays"
printf '  on NON-project days  %s captures across %s days\n\n' "$offp" "$offdays"

# ── Week-three trend, which is the actual criterion ─────────────────────────
# "Trends to zero BY WEEK THREE" -- so the last seven days of non-project capture decide it,
# not the total. A strong first week followed by silence is the exact failure this catches.
cutoff=$(date -u -v-7d +%Y-%m-%d 2>/dev/null || date -u -d '7 days ago' +%Y-%m-%d)
recent=0
while IFS=$'\t' read -r day n; do
  if ! grep -qx "$day" "$DAYS" 2>/dev/null && [ "$day" \> "$cutoff" ]; then
    recent=$((recent + n))
  fi
done <<< "$series"

elapsed_days=$(( ( $(date -u -j -f %Y-%m-%d "$last" +%s 2>/dev/null || date -u -d "$last" +%s) - $(date -u -j -f %Y-%m-%d "$first" +%s 2>/dev/null || date -u -d "$first" +%s) ) / 86400 + 1 ))

printf '\033[1m  Verdict\033[0m\n\n'
if [ "$elapsed_days" -lt 21 ]; then
  printf '  \033[33mTOO EARLY\033[0m — %s days elapsed, the criterion reads out at 21.\n' "$elapsed_days"
  printf '  Non-project captures in the last 7 days: %s. Keep going.\n\n' "$recent"
  exit 2
fi

if [ "$recent" -eq 0 ]; then
  printf '  \033[31mKILL CRITERION MET.\033[0m No non-project captures in the final week.\n\n'
  printf '  Per PROTOTYPE.md: editing is not easy enough, and no amount of retrieval quality\n'
  printf '  will save it. Stop building retrieval -- the problem is capture, not search.\n\n'
  exit 1
fi

printf '  \033[32m%s non-project captures in the final week.\033[0m Adoption did not trend to zero.\n' "$recent"
printf '  A3 survives. Record the series in FINDINGS.md with the day split.\n\n'
