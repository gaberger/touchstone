# ADR-2608010960: Adoption gates precede the Rust port

**Status:** Proposed
**Date:** 2026-08-01
**Epoch:** stage-1-skeleton
**Drivers:** Five ADRs now authorize a Rust workspace. Without an explicit gate, automation will build it before anyone has established the system is worth using.

## Context

The decisions recorded in this ledger are all about *correctness*: derivability,
round-trip fidelity, authorization recall, merge integrity, parser divergence. Every one of
them has been measured. **None of them establishes that the system is worth using.**

Two assumptions remain untested, and both are project-fatal
([PROTOTYPE.md](../../PROTOTYPE.md) §0 ranks them above everything else by
*(probability wrong × cost of learning late) ÷ cost to test*):

- **A10 — does this beat `rg` + Obsidian?** Never tested, because it is uncomfortable to
  test. Twenty real questions answered against existing notes with grep, classifying each
  failure as *recall* (right note, wrong words → vectors justified), *structure* ("all
  decisions about X still current" → frontmatter justified, which is the case for OKF), or
  *absence* (the note was never written → **no architecture fixes this**).
- **A3 — will anyone write into it unprompted?** Measured as concepts created on days not
  spent working on this project. A brain nobody writes to is an empty database with good
  latency.

Two further assumptions are cheap and would materially simplify the build if they fail:

- **A4** — if hybrid retrieval does not beat BM25 alone on a real corpus, **drop vectors**,
  and the `Embedder` port, embedding cost, debounce logic and model-drift rebuilds all
  disappear.
- **A8** — if the trust-tier boost does not measurably improve ranking, trust tiers stay for
  audit and stop affecting retrieval, which is a simpler system.

The Rust port is estimated at 1,500–2,500 lines re-deriving behaviour that already works.
Spending that before A10 and A3 is the most expensive available way to discover nobody
writes into the brain.

## Decision

**No implementation phase in [ADR-2608010940](ADR-2608010940-rust-implementation-parser-as-port.md)
beyond P1 may begin until A10 and A3 have reported.**

- **A10 and A3 are answered against the Python reference implementation.** The language is
  irrelevant to both — they are questions about human behaviour, answered by throwing code
  away quickly, which is the phase Rust is worst suited to.
- **Kill criterion, pre-registered:** if ≥15 of 20 questions are answered acceptably by
  `rg` + Obsidian, **stop**. Ship a frontmatter linter and a `brain new` template. The rest
  of this ledger is unjustified.
- **A3 kill criterion:** if concept-creation on non-project days trends to zero by week
  three, editing is not easy enough. Stop building retrieval; the problem is capture.
- **A4 and A8 run in the same window** on the real corpus, because both can *delete* work
  from the Rust build rather than add it.
- **Thresholds are pre-registered and not revised after seeing results.** A threshold chosen
  after the number is not a test.
- The null hypothesis stays alive: the grep baseline is re-run at every stage. The system
  must keep beating `rg`, not merely beat its own previous version.

Exception: **ADR-2608010950 (conformance suite) P1 may proceed in parallel.** Extracting
fixtures into language-neutral data strengthens the Python oracle and is not wasted under
any outcome.

## Consequences

**Positive:**
- The most likely failure mode is discovered for the cost of an afternoon and three weeks of
  ordinary use, not a rewrite.
- A4 and A8 may remove the `Embedder` port and the ranking boost before either is built.
- Forces the project to state, in advance, what would make it stop.

**Negative:**
- Blocks visibly productive work behind a three-week behavioural signal.
- A3 cannot be shortened; it is elapsed-time-bound.
- Automation prefers building to waiting, and this ADR exists to override that preference.

**Mitigations:**
- A10 costs hours, not weeks, and can report immediately once a real corpus is available.
- Conformance-suite extraction proceeds in parallel and is valuable under every outcome.
- The 50k scale corpus is generated and its timing run is unblocked — a measurement, not a
  build.

## Implementation

| Phase | Description | Status | Verification |
|-------|------------|--------|--------------|
| P1 | Convert a real corpus into an OKF bundle | Pending | test:python -m brain --bundle <real> index |
| P2 | A10 — 20 questions against `rg` + Obsidian, failures classified recall/structure/absence | Pending | code:docs/experiments/a10-grep-baseline.md |
| P3 | A3 — instrument `brain new`, three weeks, split by project/non-project days | Pending | code:docs/experiments/a3-adoption.md |
| P4 | A4 — lexical vs hybrid on a frozen golden set built **before** tuning | Pending | code:docs/experiments/a4-hybrid.md |
| P5 | A8 — ranking with and without the trust-tier boost | Pending | code:docs/experiments/a8-trust-rank.md |
| P6 | 50k cold-index timing (currently unmeasured) | Pending | test:python -m brain --bundle _scale index |
| P7 | **Gate review** — proceed to ADR-2608010940 P2 only if A10 and A3 pass | Pending | code:docs/experiments/gate-review.md |

## References

- [PROTOTYPE.md](../../PROTOTYPE.md) — assumption ledger and pre-registered kill criteria
- [FINDINGS.md](../../FINDINGS.md) — what has and has not been measured
- [ADR-2608010940](ADR-2608010940-rust-implementation-parser-as-port.md) — the port this gates
- [ADR-2608010950](ADR-2608010950-conformance-suite.md) — P1 exempted
