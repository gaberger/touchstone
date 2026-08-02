#!/usr/bin/env bash
# Touchstone acceptance gate: build, tests, architecture, conformance, and byte-exact export.
#
# This is the standing check that the implementation stays correct. Run it by hand, from CI, or on
# an interval (`weave loop`). Exits non-zero on any regression.
#
# HISTORY. This gate used to run a Rust/Python differential: two implementations indexing the same
# bundle, with a KNOWN_DIVERGENCE table recording the disagreements FINDINGS.md had adjudicated.
# That was the right check while a second implementation existed, because a divergence is only
# information if you already know which side should win. The Python oracle is gone (FINDINGS E6),
# so there is nothing left to differ from, and the differential is replaced by `okf-conformance` --
# which asserts the same properties directly instead of by comparison, and keeps working when
# there is only one implementation. What the oracle taught us did not go with it: E4a is now an
# assertion in okf-conformance/tests/fixture.rs, and E4b is a recorded known-defect in
# okf-conformance/tests/drills.rs rather than a table entry here.
set -uo pipefail

cd "$(dirname "$0")/.." || exit 2
ROOT=$(pwd)
HEX=${HEX_BIN:-/Volumes/SSD/Development/hex/target/release/hex}
RS=target/release/touchstone

pass=0; fail=0
ok()   { printf '  \033[32mPASS\033[0m  %s\n' "$1"; pass=$((pass+1)); }
bad()  { printf '  \033[31mFAIL\033[0m  %s -- %s\n' "$1" "${2:-}"; fail=$((fail+1)); }
note() { printf '        %s\n' "$1"; }
head_() { printf '\n\033[1m%s\033[0m\n' "$1"; }

# Bundles the export check runs over. _fixture is self-authored; _upstream is third-party OKF,
# which is the stronger test (FINDINGS E4) and MUST NOT be mutated -- every run works on a copy.
BUNDLES=(_fixture)
for b in _upstream/*/; do [ -d "$b" ] && BUNDLES+=("${b%/}"); done

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

head_ "Conformance"
# The drills, as a black-box gate over the binary: T1, T1b, T2a-T2e, T6, the conformance floor,
# the trust invariant, and search. Every drill runs against every bundle, on a copy.
# --nocapture so recorded-defect (XFAIL) lines reach this script; cargo swallows them otherwise,
# and a known defect nobody sees is indistinguishable from one nobody has.
conf=$(cargo test -p okf-conformance -- --nocapture 2>&1)
ct=$(echo "$conf" | grep -E "^test result:" | awk '{p+=$4; f+=$6} END {print p" "f}')
cp_=${ct% *}; cf=${ct#* }
if [ "${cf:-1}" = "0" ] && [ "${cp_:-0}" -gt 0 ]; then
  ok "cargo test -p okf-conformance ($cp_ passed)"
  # Surface recorded-but-unfixed defects. They are not failures, but a gate that prints only
  # green hides the fact that a load-bearing claim is still falsified.
  echo "$conf" | grep -E "^\s+XFAIL" | while IFS= read -r l; do note "${l#  }"; done
else
  bad "cargo test -p okf-conformance" "$cp_ passed, $cf failed"
  echo "$conf" | grep -E "^\s+(FAIL|FIXED)" | head -8 | while IFS= read -r l; do note "${l#  }"; done
fi

head_ "Round-trip: raw bytes out"
# T2a again, but end-to-end through the shipped binary rather than the test harness. Raw bytes are
# authoritative; this is the claim that makes "portable" mean something rather than being a slogan.
[ -x "$RS" ] || bad "round-trip" "no rust binary"
for b in "${BUNDLES[@]}"; do
  [ -x "$RS" ] || break
  tmp=$(mktemp -d); cp -R "$b" "$tmp/r"; rm -rf "$tmp/r/.touchstone"
  "$RS" --bundle "$tmp/r" index -q >/dev/null 2>&1
  n=$("$RS" --bundle "$tmp/r" stats 2>/dev/null | awk '/^concepts:/{print $2}')
  "$RS" --bundle "$tmp/r" export "$tmp/exp" --force >/dev/null 2>&1
  diffs=0
  while IFS= read -r f; do
    rel=${f#"$tmp/r/"}
    case "$rel" in .touchstone/*|index.md|*/index.md) continue;; esac
    cmp -s "$f" "$tmp/exp/$rel" || diffs=$((diffs+1))
  done < <(find "$tmp/r" -name '*.md')
  if [ "$diffs" -eq 0 ] && [ -n "${n:-}" ]; then ok "$b -- $n concepts, round-trip byte-exact"
  else bad "$b" "${diffs} file(s) differ through export"; fi
  rm -rf "$tmp"
done

head_ "No Python"
# The oracle is deleted, not deprecated (FINDINGS E6). This guards the deletion: a stray module or
# drill script reintroducing a second implementation is exactly the state this gate exists to end.
stray=$(git ls-files '*.py' | grep -v '^_upstream/' || true)
if [ -z "$stray" ]; then ok "no tracked Python outside vendored bundles"
else bad "Python reintroduced" "$(echo "$stray" | tr '\n' ' ')"; fi

head_ "Result"
printf '  %s passed, %s failed\n\n' "$pass" "$fail"
[ "$fail" -eq 0 ] || exit 1
