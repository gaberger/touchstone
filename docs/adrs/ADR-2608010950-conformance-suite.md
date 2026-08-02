# ADR-2608010950: A language-independent OKF conformance suite is the portable asset

**Status:** Accepted
**Date:** 2026-08-01
**Epoch:** stage-1-skeleton
**Drivers:** A Rust port is authorized. The measured merge-key divergence between two conformant parsers proves that "reimplement and hope it behaves the same" silently flips a trust tier.

## Context

The Python Stage 1 implementation passes ten drills. Those drills are not statements about
Python — they are statements about **OKF conformance**:

| Drill | Asserts |
|---|---|
| T1 | `index.md` regenerates byte-identically after every derived artifact is deleted |
| T1b | A second index run is a no-op |
| T2a | Ingest → export is byte-exact (CRLF, unicode paths, anchors, multiline scalars) |
| T2b | No key or value lost through reserialization |
| T2c | Unknown types and unknown keys preserved; broken links recorded, not rejected |
| T2d | No temporal value coerced off ISO 8601 |
| T2e | Every formatter rewrite is value-preserving; unsafe files are refused |
| T6 | The entire index recovers from files alone |

Two findings make a shared suite non-optional rather than good practice:

1. **Parsers disagree.** `serde_yaml_ng` does not implement merge keys, so
   `verified: [{<<: *defaults}]` is human-verified under PyYAML and unattributed under Rust.
   Nothing but a contract test catches this before it reaches ranking.
2. **The formatter was unsafe until it was tested.** The naive canonicalizer resolved merge
   keys, invented a new `&id001` anchor on an unrelated timestamp, and flattened a shell
   script under `script: |` into a quoted string with interleaved blank lines. T2e exists
   because building it revealed the danger; without the drill the damage ships silently.

A third motivation is external: the upstream `reference_agent` CLI is an **independent
implementation** we do not control, and it is the strongest available check that our output
is really OKF and not a lookalike.

## Decision

We will promote the drills to a **language-independent conformance suite** — a fixture
bundle plus expected outputs — and require every implementation to pass it.

- **`touchstone-conformance` is its own crate**, depending on ports only, so it can be run against
  any adapter set.
- **The fixture bundle is data, not code**: a checked-in OKF bundle plus expected
  `index.md` outputs and a golden query set. Any language can consume it.
- **The adversarial half is mandatory.** Unknown types, unknown keys, YAML anchors, merge
  keys, block scalars, flow style, CRLF, unicode paths, empty `type`, absent frontmatter,
  and deliberately broken links.
- **Parser adapters must declare and test their behaviour** on constructs where conformant
  implementations legitimately differ — merge keys first among them. A divergence must be a
  *declared, tested* property, never a discovered one.
- **T2 additionally runs against the upstream sample bundles** (`ga4`, `stackoverflow`,
  `crypto_bitcoin`). Our own fixture is adversarial but self-authored, which is its weakness;
  real OKF written by people who did not know our assumptions is the stronger test.
- **T6 runs the upstream `reference_agent`** against our bare bundle. Independent
  verification that portability is real.
- The suite is a **CI gate on every push**, and the Python implementation remains the oracle
  until the Rust suite reproduces all ten drills.

## Consequences

**Positive:**
- Porting becomes a pass/fail gate instead of a hope.
- Cross-implementation divergence is caught at the contract, not in production ranking.
- The suite is the durable asset: implementations are disposable, conformance is not — which
  is the same claim the architecture makes about the index, applied to itself.
- Spec drift (OKF v0.2 is explicitly unfinished) shows up as a failing gate.

**Negative:**
- A second artifact to maintain alongside the implementation.
- Fixtures encode *our* reading of the spec, which may itself be wrong.
- Byte-exactness assertions are brittle against benign formatting changes.

**Mitigations:**
- Running against upstream bundles and the upstream CLI checks our reading against an
  implementation we do not control.
- Byte-exactness is asserted only where it is genuinely load-bearing — generated `index.md`
  and raw round-trip — never on hand-authored content.
- Fixtures live as data files, so correcting a misreading is an edit, not a refactor.

## Implementation

| Phase | Description | Status | Verification |
|-------|------------|--------|--------------|
| P1 | The fixture bundle is language-neutral checked-in data, not a generator | Completed | code:_fixture/, test:cargo test -p touchstone-conformance --test fixture the_adversarial_corpus_is_actually_present |
| P2 | `touchstone-conformance` crate; T1, T1b, T6 | Completed | code:touchstone-conformance/tests/drills.rs, test:cargo test -p touchstone-conformance --test drills |
| P3 | T2a–T2e | Completed | code:touchstone-conformance/tests/drills.rs, test:cargo test -p touchstone-conformance --test drills |
| P4 | Declared-divergence table per parser adapter (merge keys, comments, anchors) | Pending | code:touchstone-conformance/tests/parser_contract.rs, test:cargo test -p touchstone-conformance parser_contract |
| P5 | Run every drill against the upstream sample bundles, not just T2 | Completed | code:touchstone-conformance/src/lib.rs all_bundles(), test:cargo test -p touchstone-conformance |
| P6 | T6 via the upstream `reference_agent` CLI | Pending — unblocked | test:TOUCHSTONE_BIN=<reference_agent> cargo test -p touchstone-conformance |
| P7 | CI gate on every push | Completed | code:.github/workflows/gate.yml, test:bash tests/verify.sh |

**Deviations from the decision as written, and why.**

*"`touchstone-conformance` is its own crate, depending on ports only."* It depends on **nothing**
internal — not even ports. Depending on ports would mean the suite links the same value types the
implementation uses, so a shared misreading could not be detected, and it could only ever gate a
Rust build. Driving the binary instead makes `TOUCHSTONE_BIN=… cargo test -p touchstone-conformance` hold
*any* implementation to the drills, which is what P6 needs and what the decision was reaching for.
Enforced as architecture rule 7.

*P1 said "extract the fixture into a data directory."* `_fixture/` already was checked-in data, so
extraction was a no-op; what was removed is the **generator** (`tests/make_fixture.py`). Deleting it
found that generator and committed fixture had already drifted, which is the argument for data over
code stated better than the ADR stated it.

*"T2 additionally runs against the upstream sample bundles."* Every drill now runs against every
bundle, not just T2. That is what surfaced E4b — a bundle-specific falsification of A1 that a
fixture-only T1 could never have found.

*"The Python implementation remains the oracle until the Rust suite reproduces all ten drills."*
Satisfied, then acted on: all ten are reproduced and the oracle is deleted (FINDINGS E6).

## References

- [FINDINGS.md](../../FINDINGS.md) — E3, the drills and what building them revealed
- [RUST-PATH.md](../../RUST-PATH.md) — §1 the merge-key divergence, §6 why this is the migration asset
- [PROTOTYPE.md](../../PROTOTYPE.md) — pre-registered kill criteria
- [ADR-2608010940](ADR-2608010940-rust-implementation-parser-as-port.md)
- [knowledge-catalog upstream bundles](https://github.com/GoogleCloudPlatform/knowledge-catalog/tree/main/okf/bundles)
