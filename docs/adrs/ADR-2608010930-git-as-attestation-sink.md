# ADR-2608010930: CRDT is the write path; git is demoted to a signed attestation sink

**Status:** Proposed
**Date:** 2026-08-01
**Epoch:** stage-1-skeleton
**Drivers:** Proposal to eliminate git entirely in favour of our own version control. Analysis showed git is doing two unrelated jobs, only one of which it does badly.

## Context

Git currently serves two roles that want opposite things:

| Layer | Needs | Git? |
|---|---|---|
| **Sync** — laptop → phone → server, offline, conflict-free | convergence, partial replication, low latency, mobile | **Badly no** |
| **History & attestation** — what changed, who verified it, provable | durability, signatures, review, audit | **Excellent** |

"Eliminate git" reads as one decision, but only the first row justifies it.

**Where git genuinely fails.** Git is a history tool that happens to distribute; making it a
live sync engine means fetch-merge-push loops with conflict handling in the UX. Git on
iOS/Android is bad — and this matters more than it appears, because capture difficulty is
project-fatal (A3) with a measured threshold of ~20 seconds from "I want to record this" to
"it's saved". A git round-trip on a phone does not clear that bar. Clone is also
all-or-nothing.

**Where replacing git is expensive.** The load-bearing invariant is that an agent may never
write `verified: {by: human:...}` — enforceable only because *in a database row that is a
claim; in git it is a signed commit*. Automerge and Loro changes carry actor IDs, **not
signatures**. Eliminating git means owning that crypto. It also means rebuilding review:
"you cannot open a pull request against a row" was an argument *for* files and applies just
as hard to our own CRDT store — CRDTs are built so everything converges automatically, and
"proposed, not yet accepted" is against their grain.

**What git is NOT needed for: frontmatter integrity.** Measured, 400 randomized two-author
merge trials on OKF frontmatter:

| Outcome | Rate |
|---|---|
| clean and correct | 74.8% |
| clean, valid YAML, duplicate list entry | 16.0% |
| loud conflict (safe) | 9.2% |
| **invalid YAML or lost update** | **0.0%** |

Git either conflicts loudly or merges correctly. The claim that it silently corrupts YAML is
false; the only real defect is duplicate entries, which is a lint rule.

**The asymmetry that resolves everything: the files are the source of truth and version
history is not.** If every CRDT checkpoint materializes plain OKF markdown to disk, losing
the entire history costs *history*, not *knowledge*.

## Decision

We will **keep git and demote it.** A CRDT becomes the write path and sync engine; git
becomes an append-only, signed, materialized snapshot sink that no human hand-edits.

- **`SyncEngine` port** (Automerge or Loro adapter) owns concurrent and offline editing.
  Frontmatter is a typed CRDT map, so `tags`, `verified[]` and `sources[]` merge as **sets**
  — something line-based merge structurally cannot do.
- **`VersionControl` port** (gix adapter) owns durable signed history. Nobody resolves a
  merge conflict on a phone; nobody types `git` to capture a note.
- **Every CRDT checkpoint materializes plain OKF markdown**, preserving the escape hatch.
- **CI rejects unsigned `verified` deltas.** This is the enforcement mechanism for the trust
  invariant and is not optional.
- The CRDT is **not** justified on integrity grounds — measurement says frontmatter is safe
  without it. It is justified on **latency, mobile and offline convergence**. It is therefore
  scheduled *after* the adoption gates, not before.

Alternatives rejected: **full custom version control on Automerge/Loro** — coherent, and
what heaper.de does, but it requires reimplementing signing, review semantics, hosting and
tooling, and a bespoke VCS is the least boring possible component in a design whose founding
bet is that knowledge outlives infrastructure. Choose it only after concluding review and
signed attestation are not requirements. Since one architecture must serve corporate — where
review is the whole point — that conclusion is not available.

## Consequences

**Positive:**
- Mobile and multi-device capture become possible without git UX.
- Signed attestation, review, CODEOWNERS and GitHub rendering are retained for free.
- Deleting git later costs *audit*, not the brain.
- Set-merge on list fields eliminates the 16% duplicate-entry defect at the source.

**Negative:**
- The CRDT server is a stateful component in the write path — the previous design claimed
  there were none.
- Two systems to operate rather than one.
- Losing the CRDT server mid-session loses edits since the last checkpoint.

**Mitigations:**
- The CRDT's state is measured in seconds, not years; it is a write-through cache with a
  bounded lifetime. Kill it and the files are intact and current to the last checkpoint.
- Git runs as an embedded library (gix), not a CLI, so the second system is invisible to
  users.
- Checkpoint interval is configurable; the exposure window is a tunable, not a property.

**Kill criterion:** this decision fails and full custom VC becomes justified if, after the
write path is git-free at the UX level, git checkpointing still exceeds ~1s on the 50k corpus
on the slowest target device, or mobile capture cannot clear the 20-second bar. **Measure
before committing to a rewrite.**

## Implementation

| Phase | Description | Status | Verification |
|-------|------------|--------|--------------|
| P1 | `VersionControl` port + gix adapter; confirm commit-signing support before committing to gix | Pending | code:touchstone-adapters/secondary/git-attest/src/lib.rs, test:cargo test -p git-attest signing |
| P2 | CI gate rejecting unsigned `verified` deltas | Pending | code:.github/workflows/verified-signing.yml, test:cargo test -p touchstone-conformance verified_signing |
| P3 | Duplicate-entry lint (the measured 16% defect) | Pending | code:touchstone-domain/src/lint.rs, test:cargo test -p touchstone-domain lint_duplicates |
| P4 | `SyncEngine` port + CRDT adapter, typed frontmatter map with set-merge semantics | Pending | code:touchstone-adapters/secondary/crdt-sync/src/lib.rs, test:cargo test -p crdt-sync set_merge |
| P5 | Checkpoint materializes conformant OKF markdown to disk | Pending | code:touchstone-usecases/src/checkpoint.rs, test:cargo test -p touchstone-conformance checkpoint_materializes |
| P6 | Latency measurement against the kill criterion | Pending | test:cargo bench -p touchstone-conformance checkpoint_50k |

## References

- [VERSION-CONTROL.md](../../VERSION-CONTROL.md) — the full three-option analysis
- [FINDINGS.md](../../FINDINGS.md) — E2, 400 merge trials
- [heaper.de](https://heaper.de) — shipping local-first CRDT product of this shape
- [ADR-2608010940](ADR-2608010940-rust-implementation-parser-as-port.md) — why this decision drives the language
