# ADR-2608010910: Files are the source of truth; the index plane is derived and disposable

**Status:** Proposed
**Date:** 2026-08-01
**Epoch:** stage-1-skeleton
**Drivers:** Adversarial review deadlocked on whether a corporate-scale brain forces a service-backed store, making OKF an export format rather than the source of truth. Resolved by measurement.

## Context

Two reviewers argued opposite theses (full record in [DECISIONS.md](../../DECISIONS.md)):

- **Files-as-truth**: a git-tracked directory of markdown, with every index, embedding and
  graph a pure function of the tree — disposable and rebuildable. Any design where a service
  becomes the source of truth has abandoned the point of adopting an open format.
- **Service-as-truth**: an organizational KB is an operational system; Postgres is the
  system of record and the OKF bundle a materialized replica. A wire format is not a runtime.

The decisive argument for files: **a lossy export is undetectable.** `type` has no registry,
`verified[]` is an ordered multi-entry chain, and unknown keys MUST round-trip. A store with
a fixed schema models the fields it knew about at design time. An acquired team's bundle
carrying `type: ThreatModel` and a custom `retention:` key gets imported, stored, exported —
and the key is gone, with no error, no diff, no alarm.

The service side's strongest counters were **post-filter recall** (answered by
[ADR-2608010920](ADR-2608010920-postfilter-authorization-at-depth.md)) and **revocation**
(unresolved, see Consequences).

The claim "everything above the bundle is reconstructible from the bundle" is either true or
it is rhetoric, and it is cheap to test. So it was tested.

## Decision

We will treat the **OKF bundle as the sole source of truth**, and every artifact above it as
derived, disposable, and reconstructible by one idempotent command.

- **Raw text is authoritative.** Frontmatter is parsed into a structure for querying, but
  that structure is never the truth. `export` writes raw bytes. There is therefore **no
  serializer in the write path that could drop an unknown key** — the failure mode is
  structurally impossible rather than merely tested for.
- **Two indexes, both derived, neither hand-edited:**
  - *Human* — `index.md` per directory, generated from child `title`/`description` in the
    spec's prescribed shape, and **committed** so the bundle browses correctly on GitHub and
    in Obsidian with zero tooling. Because it is generated *and* committed it would conflict
    on every merge; a `.gitattributes` merge driver **discards both sides and regenerates
    from the merged tree**. A derived file has no merge semantics, only a recompute.
  - *Machine* — concepts, FTS, edges, optional vectors. Not committed. Incremental on
    content hash.
- **Indexing splits by cost, not by timing.** Structural + lexical + edges are eager (pure
  CPU, no API spend). Embeddings are debounced and **skipped entirely while
  `status: draft`** — you never pay to embed a draft deleted an hour later, and never eat
  first-query latency for lexical search.
- **The rebuild drill is a CI gate, not a nice-to-have.** Delete every derived artifact,
  regenerate, and require byte-identical output.

## Consequences

**Positive:**
- Verified on the Python reference implementation, 10/10 drills: `index.md` regenerates
  byte-identically after full deletion (T1); export is byte-exact through CRLF, unicode
  paths, YAML anchors and multiline scalars (T2a); unknown types and keys survive (T2c); the
  entire index rebuilds from files alone (T6).
- Any component above the bundle can be replaced without migration — there is nothing to
  migrate.
- Reviewed change is available: you cannot open a pull request against a database row.

**Negative:**
- **Revocation is unsolved.** Git cannot revoke; once cloned, revoked is fiction. This
  breaks at N=1 — the first compensation band or terminated-employee record — not at scale.
- Scale is unvalidated. The reference implementation was exercised on 25 concepts; a
  50,000-concept corpus exists but is unmeasured.
- Rebuilding embeddings on a model change is a full re-embed.

**Mitigations:**
- Revocation is explicitly out of scope here and is the surviving argument for a corporate
  boundary; it is *not* claimed to be solved.
- The rebuild and service-death drills run in CI on every push, so derivability degrading is
  a red build rather than a slow discovery.
- Re-embed cost is estimated at single-digit dollars for 50k concepts — to be confirmed, not
  assumed.

## Implementation

| Phase | Description | Status | Verification |
|-------|------------|--------|--------------|
| P1 | `ConceptRepository` port + fs adapter; raw bytes preserved end to end | Pending | code:touchstone-adapters/secondary/fs-bundle/src/lib.rs, test:cargo test -p fs-bundle roundtrip |
| P2 | `SearchIndex` port + sqlite adapter (FTS5, edges), incremental on content hash | Pending | code:touchstone-adapters/secondary/sqlite-index/src/lib.rs, test:cargo test -p sqlite-index incremental |
| P3 | Deterministic `index.md` rendering — pure domain function | Pending | code:touchstone-domain/src/render.rs, test:cargo test -p touchstone-domain render_deterministic |
| P4 | T1 rebuild drill + T6 service-death drill as CI gates | Pending | code:touchstone-conformance/tests/rebuild.rs, test:cargo test -p touchstone-conformance rebuild |
| P5 | `.gitattributes` merge driver regenerating `index.md` from the merged tree | Pending | code:.gitattributes, test:cargo test -p touchstone-conformance merge_driver |

## References

- [DECISIONS.md](../../DECISIONS.md) — full adversarial record, both theses preserved
- [FINDINGS.md](../../FINDINGS.md) — E3, Stage 1 drill results
- [ADR-2608010900](ADR-2608010900-okf-as-knowledge-substrate.md) — the substrate
- [ADR-2608010950](ADR-2608010950-conformance-suite.md) — how derivability stays true
