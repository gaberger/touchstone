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
it exited 0), both drill implementations, and the Rust/Python differential across `_fixture` and every
bundle in `_upstream/`.

Adjudicated divergences live in `KNOWN_DIVERGENCE` inside `verify.sh`. An unlisted divergence fails.
So does an expected one that disappears — that would mean the Rust side stopped fixing a defect the
Python side still has.

## On deleting the Python prototype

This gate is the main reason to keep it. Once `touchstone` is the only implementation there is no
differential, and this becomes build + tests + architecture + drills. That is a real loss of signal,
so retiring the prototype should be a decision, not a cleanup.
