# ADR-2608010940: Rust workspace under hex; the frontmatter parser is a port

**Status:** Proposed
**Date:** 2026-08-01
**Epoch:** stage-1-skeleton
**Drivers:** hex is the development automation and is a Rust hexagonal-architecture AIOS. Separately, two YAML libraries were measured and found to disagree on the same bytes in a way that flips a trust tier.

## Context

**Language.** The Stage 1 reference implementation is ~700 lines of Python passing 10/10
conformance drills. The question was whether to port. Measurement, not preference:

| Behaviour | PyYAML | serde_yaml_ng 0.10 |
|---|---|---|
| `at: 2026-01-01T00:00:00Z` after parse | **coerced to `datetime`** | stays `String` |
| ISO 8601 survives re-emit | **no** — `2026-01-01 00:00:00+00:00` | yes, byte-identical |
| `stale_after: 2026-09-23` | coerced to `date` | stays `String` |
| Literal block scalar `script: \|` | flattened to a quoted string | **block style preserved** |
| Anchors on re-emit | expanded, **and a new `&id001` invented elsewhere** | expanded cleanly |
| Merge key `<<: *defaults` | resolved (YAML 1.1) | **not implemented — `<<` kept as a literal key** |
| Comments on re-emit | dropped | dropped |

The Python implementation required surgery on PyYAML's implicit resolvers to stop it
silently rewriting `generated.at`, `verified[].at`, `stale_after` and `usage_window` out of
ISO 8601. Rust's default behaviour is already correct.

**But a new hazard appeared.** Given identical bytes:

```yaml
verified:
  - <<: *defaults      # {by: human:gary, at: ...}
```

Python resolves the merge and reports `verified[0].by == "human:gary"` → **human-verified**.
Rust reports a literal key `<<` and no `by` at all → **unattributed**. The trust tier — which
every ranking decision depends on — flips with the reader. This is precisely the silent
divergence OKF exists to prevent.

Note what this is *not* an argument for: the formatter problem is **not** solved by
switching. Neither library preserves comments and both expand anchors; document-preserving
formatting needs a document-model library in either language, and Python already has the
better one available (`ruamel.yaml`, a one-line dependency). Performance is likewise not the
deciding input and remains unmeasured.

**What actually decides it** is [ADR-2608010930](ADR-2608010930-git-as-attestation-sink.md):
Automerge, Loro, y-crdt and gitoxide are all Rust. Committing to a CRDT write path and
git-as-library means Python would be writing FFI bindings to Rust libraries. Add a single
static binary (a `.venv` is friction felt daily, and R2 is project-fatal) and mobile
compilation targets, and the direction is settled — by architecture, not by benchmark.

## Decision

We will build **Touchstone as a Rust workspace conforming to hex's hexagonal architecture**
([hex ADR-001](https://github.com/gaberger/hex/blob/main/docs/adrs/ADR-001-hexagonal-architecture.md)),
and we will make the **frontmatter parser a port**.

Crate layout, obeying hex's seven enforced rules:

```
touchstone-domain/     pure — Concept, conformance floor, trust tiers, link resolution,
                ranking policy, index.md rendering, lint. Zero external crates.
touchstone-ports/      FrontmatterParser · ConceptRepository · SearchIndex ·
                VersionControl · SyncEngine · Embedder · Clock
touchstone-usecases/   IndexBundle · SearchBundle · CaptureConcept · LintBundle · ExportBundle
touchstone-adapters/primary/{cli,mcp}
touchstone-adapters/secondary/{yaml-serde,fs-bundle,sqlite-index,git-attest,crdt-sync,embed-local}
touchstone-cli/        composition root — the ONLY crate importing adapters
```

**The parser is a port because parsers disagree.** Consequences:

- The merge-key divergence becomes a **port-contract test** rather than a latent production
  bug. Any parser adapter must state its behaviour and pass the same contract suite.
- Swapping to a document-preserving parser (`saphyr`, or a custom emitter) to fix the
  formatter is a **single-adapter change** under rule 5.
- OKF v0.2 is explicitly not a finished standard; spec drift becomes an adapter change
  rather than a refactor.

**Migration is incremental, not big-bang.** Python remains the reference oracle until the
Rust conformance suite reproduces all ten drills.

## Consequences

**Positive:**
- ISO 8601 preservation and block-scalar fidelity come free, without loader surgery.
- Native CRDT and gitoxide, no FFI.
- Single static binary; iOS/Android targets reachable.
- One agent = one adapter = one worktree = one merge unit — six secondary adapters are six
  independent, non-colliding units of agent work.

**Negative:**
- Rust's embedding/ML ecosystem is materially weaker than Python's.
- Iteration speed is worse during exactly the phase we are in — Stage 1 exists to answer A10
  and A3, which are questions about human behaviour answered by throwing code away quickly.
- Estimated 1,500–2,500 lines of Rust to re-derive working behaviour.
- `serde_yaml_ng` is a maintained fork of the archived `serde_yaml`; sustainability is a
  standing risk.

**Mitigations:**
- The `Embedder` port is **optional and feature-gated**. A4 is untested and may delete
  embeddings entirely, in which case this cost evaporates.
- **A10 and A3 are answered in Python first.** The language is irrelevant to both, and
  rewriting first is the most expensive possible way to discover nobody writes into the brain.
- Parser-as-port makes the `serde_yaml_ng` bet reversible.
- Confirm gix commit-signing before depending on it; signing enforces the trust invariant.

## Implementation

| Phase | Description | Status | Verification |
|-------|------------|--------|--------------|
| P1 | Workspace scaffold; hex boundary rules pass with zero violations | Completed | code:Cargo.toml, test:cargo test -p touchstone-cli --test architecture |
| P2 | `touchstone-domain` — pure, zero external crates | Completed | code:touchstone-domain/src/lib.rs, test:cargo test -p touchstone-domain |
| P3 | `touchstone-ports` — traits only, imports domain for value types only | Completed | code:touchstone-ports/src/lib.rs, test:cargo check -p touchstone-ports |
| P4 | `yaml-serde` adapter + **parser port-contract suite incl. merge-key behaviour** | Pending — adapter done, contract suite outstanding (ADR-2608010950 P4) | code:touchstone-adapters/secondary/yaml-serde/src/lib.rs, test:cargo test -p touchstone-conformance parser_contract |
| P5 | `fs-bundle` + `sqlite-index` adapters | Completed | code:touchstone-adapters/secondary/sqlite-index/src/lib.rs, test:cargo test -p touchstone-sqlite-index |
| P6 | `touchstone-cli` composition root; CLI-MCP parity per hex ADR-019 | Pending — CLI complete, MCP surface is still a stub | code:touchstone-cli/src/main.rs, test:cargo test -p touchstone-cli parity |
| P7 | Rust suite reproduces all 10 Python drills; retire the oracle | Completed — FINDINGS E6 | test:cargo test -p touchstone-conformance |

## References

- [RUST-PATH.md](../../RUST-PATH.md) — the measured probe, full results
- [FINDINGS.md](../../FINDINGS.md) — E3a, the PyYAML coercion bug
- [hex ADR-001](https://github.com/gaberger/hex/blob/main/docs/adrs/ADR-001-hexagonal-architecture.md)
- [hex ADR-019](https://github.com/gaberger/hex/blob/main/docs/adrs/ADR-019-cli-mcp-parity.md) — CLI/MCP parity
- [ADR-2608010950](ADR-2608010950-conformance-suite.md) — the gate that makes porting safe
