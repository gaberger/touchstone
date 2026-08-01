#!/usr/bin/env bash
# Touchstone acceptance gate: build, tests, architecture, drills, and the Rust/Python differential.
#
# This is the standing check that the Rust port stays correct. Run it by hand, from CI, or on an
# interval (`weave loop`). Exits non-zero on any regression.
#
# The differential is the interesting part. Two implementations disagreeing is only information if you
# already know which side should win — so KNOWN_DIVERGENCE below records the disagreements FINDINGS.md
# has already adjudicated. An expected divergence that disappears is a regression too: it means the
# Rust side stopped fixing a defect the Python side still has.
set -uo pipefail

cd "$(dirname "$0")/.." || exit 2
ROOT=$(pwd)
HEX=${HEX_BIN:-/Volumes/SSD/Development/hex/target/release/hex}
PY=${PY_BIN:-.venv/bin/python}
RS=target/release/touchstone

pass=0; fail=0
ok()   { printf '  \033[32mPASS\033[0m  %s\n' "$1"; pass=$((pass+1)); }
bad()  { printf '  \033[31mFAIL\033[0m  %s -- %s\n' "$1" "${2:-}"; fail=$((fail+1)); }
note() { printf '        %s\n' "$1"; }
head_() { printf '\n\033[1m%s\033[0m\n' "$1"; }

# Bundles the differential runs over. _fixture is self-authored; _upstream is third-party OKF, which
# is the stronger test (FINDINGS E4) and MUST NOT be mutated -- every run works on a copy.
BUNDLES=(_fixture)
for b in _upstream/*/; do [ -d "$b" ] && BUNDLES+=("${b%/}"); done

# Divergences FINDINGS.md has adjudicated: "<bundle>:<rust_count>:<py_count>:<why>".
# Anything not listed here must match exactly.
KNOWN_DIVERGENCE=(
  "_upstream/acme_retail:10:9:E4a -- python reserves the filename log.md and drops a legitimate type:Log concept"
)

head_ "Build"
if cargo build -q --release --bin touchstone 2>/dev/null; then ok "cargo build --release"; else bad "cargo build --release" "see cargo output"; fi
if cargo check -q --workspace 2>/dev/null; then ok "cargo check --workspace"; else bad "cargo check --workspace"; fi

head_ "Tests"
t=$(cargo test --workspace 2>&1 | grep -E "^test result:" | awk '{p+=$4; f+=$6} END {print p" "f}')
tp=${t% *}; tf=${t#* }
if [ "${tf:-1}" = "0" ] && [ "${tp:-0}" -gt 0 ]; then ok "cargo test --workspace ($tp passed)"; else bad "cargo test --workspace" "$tp passed, $tf failed"; fi

head_ "Architecture"
if [ -x "$HEX" ]; then
  # A green hex result is only meaningful if it recognised the layout -- a score of 100 with zero
  # layers detected is a vacuous pass, which is exactly how this gate used to lie.
  layers=$("$HEX" analyze . --json --quiet 2>/dev/null | grep -o '"layer"' | wc -l | tr -d ' ')
  if "$HEX" analyze . --quiet --exit-code >/dev/null 2>&1; then
    if [ "${layers:-0}" -ge 4 ]; then ok "hex analyze ($layers layers recognised)"
    else bad "hex analyze" "passed but only $layers layers recognised -- vacuous"; fi
  else bad "hex analyze" "boundary violations"; fi
else note "hex not found at $HEX -- skipped (set HEX_BIN)"; fi

head_ "Drills"
for impl in python rust; do
  out=$($PY tests/drills.py --impl "$impl" 2>&1 | grep -E "^[0-9]+/[0-9]+ passed")
  case "$out" in
    *"passed"*) [ -n "$(echo "$out" | grep -o 'FAILED')" ] && bad "drills --impl $impl" "$out" || ok "drills --impl $impl -- $out" ;;
    *) bad "drills --impl $impl" "no summary line" ;;
  esac
done

head_ "Differential: rust vs python"
[ -x "$RS" ] || { bad "differential" "no rust binary"; }
for b in "${BUNDLES[@]}"; do
  [ -x "$RS" ] || break
  tmp=$(mktemp -d); cp -R "$b" "$tmp/r"; cp -R "$b" "$tmp/p"
  rm -rf "$tmp/r/.touchstone" "$tmp/p/.touchstone"
  "$RS" --bundle "$tmp/r" index -q >/dev/null 2>&1
  $PY -m touchstone --bundle "$tmp/p" index -q >/dev/null 2>&1
  rc=$("$RS" --bundle "$tmp/r" stats 2>/dev/null | awk '/^concepts:/{print $2}')
  pc=$($PY -m touchstone --bundle "$tmp/p" stats 2>/dev/null | awk '/^concepts:/{print $2}')

  expected=""
  for k in "${KNOWN_DIVERGENCE[@]}"; do
    [ "${k%%:*}" = "$b" ] && expected="$k"
  done

  if [ -n "$expected" ]; then
    er=$(echo "$expected" | cut -d: -f2); ep=$(echo "$expected" | cut -d: -f3); why=$(echo "$expected" | cut -d: -f4-)
    if [ "${rc:-x}" = "$er" ] && [ "${pc:-x}" = "$ep" ]; then
      ok "$b -- expected divergence holds (rust $rc / python $pc)"; note "$why"
    else
      bad "$b" "adjudicated divergence changed: expected rust $er/python $ep, got rust ${rc:-?}/python ${pc:-?}"
    fi
  elif [ "${rc:-x}" = "${pc:-y}" ] && [ -n "${rc:-}" ]; then
    ok "$b -- parity ($rc concepts both)"
  else
    bad "$b" "unadjudicated divergence: rust ${rc:-?} vs python ${pc:-?} -- decide which is correct, then record it in FINDINGS.md"
  fi

  # T2a byte-exact export, Rust side. Raw bytes are authoritative; this is the claim that makes
  # "portable" mean something rather than being a slogan.
  "$RS" --bundle "$tmp/r" export "$tmp/exp" --force >/dev/null 2>&1
  diffs=0
  while IFS= read -r f; do
    rel=${f#"$tmp/r/"}
    case "$rel" in .touchstone/*|index.md|*/index.md) continue;; esac
    cmp -s "$f" "$tmp/exp/$rel" || diffs=$((diffs+1))
  done < <(find "$tmp/r" -name '*.md')
  [ "$diffs" -eq 0 ] && ok "$b -- rust round-trip byte-exact" || bad "$b" "$diffs file(s) differ through export"
  rm -rf "$tmp"
done

head_ "Result"
printf '  %s passed, %s failed\n\n' "$pass" "$fail"
[ "$fail" -eq 0 ] || exit 1
