# Automated verification

`touchstone-verify.mjs` is a weave **code skill** — deterministic, no LLM, no tokens. It runs
[`tests/verify.sh`](../tests/verify.sh) and reports pass/fail with the failing lines.

## Install and run on an interval

```bash
cp weave/touchstone-verify.mjs ~/.weave/skills/
weave up --fake                                    # a peer to claim the work
weave loop --skill touchstone-verify --interval 1h "verify touchstone" --notify --to slack
```

`--fake` is deliberate: the gate is a shell script, so the peer needs no LLM backend at all.

## What it guards

Build, `cargo test --workspace`, `hex analyze` (asserting it *recognised* the layout, not merely that
it exited 0), the `touchstone-conformance` drills across `_fixture` and every bundle in `_upstream/`,
byte-exact export, and a check that no tracked Python has reappeared.

Adjudicated defects live in `KNOWN_DEFECTS` inside `touchstone-conformance/tests/drills.rs`, and the gate
prints them as `XFAIL` on every run. An unlisted failure fails the gate. So does a listed one that
*disappears* — that means FINDINGS.md is now asserting something false and needs closing out.

## On the Python prototype

It is gone (FINDINGS E6), and this gate is where the cost landed. While it existed, the gate ran a
true differential: two implementations over the same bytes, which is the only way to catch a
disagreement nobody thought to look for — that is how E3a and E4a were found.

What replaced it is not equivalent, and the README does not pretend otherwise. The drills now assert
each property directly, so regressions are still caught; novel disagreements are not. The partial
compensation is that `touchstone-conformance` names no `touchstone-*` crate, so it drives the *binary*:

```bash
TOUCHSTONE_BIN=/path/to/other/impl cargo test -p touchstone-conformance
```

One differential compared two implementations. This gates any number of them, including ones that do
not exist yet.
