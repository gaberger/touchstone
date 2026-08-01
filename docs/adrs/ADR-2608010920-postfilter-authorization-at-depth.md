# ADR-2608010920: Post-filter authorization at retrieval depth K≥500

**Status:** Proposed
**Date:** 2026-08-01
**Epoch:** stage-1-skeleton
**Drivers:** The claim that per-concept authorization must be a prefilter inside a query planner was the single argument forcing a service-backed corporate deployment. It was simulated and did not survive.

## Context

The service-first reviewer's decisive argument was:

> 200k concepts, a user entitled to 5%: retrieve top-100 by vector similarity, then filter by
> entitlement → ~5 survive, and the relevant ones were likely outside the 100. Authorization
> must be a prefilter inside the query planner.

If true, a corporate brain needs row-level security in a database and the file-first design
cannot serve it — the personal and corporate cases would need different search architectures,
breaking the requirement that one architecture serve both.

A pre-registered kill criterion was set before running anything: **post-filter reaching
≥0.95× prefilter recall falsifies the requirement.**

The original test design missed the variable that governs the result: **whether entitlement
correlates with query topic.** Real organizations are not random — the legal team's documents
are about legal, and the legal team asks legal questions.

**Method.** 100k documents, 64-dim, clustered into 50 topics. 300 queries near topic
centroids, 40 principals. Ground truth = exact top-10 within the entitled set (what a
prefilter achieves by definition). Post-filter = global top-K, drop unentitled, take top-10.
Three entitlement models: `random`, `topical`, and `mixed` (80% topical + 20% scattered).

**Results — recall@10:**

| Entitlement | Model | K=100 | K=500 | K=2000 |
|---|---|---|---|---|
| 5% | random | 0.501 | **1.000** | 1.000 |
| 5% | topical | 0.971 | 1.000 | 1.000 |
| 5% | **mixed** | 0.410 | **0.953** | 1.000 |
| 20% | mixed | 0.960 | 1.000 | 1.000 |
| 50% | all | 1.000 | 1.000 | 1.000 |

The arithmetic in the original argument was correct — at K=100 and 5% entitlement, recall
really is 0.41–0.50 with ~5 survivors. **The conclusion did not follow.** The remedy is not a
query planner; it is to retrieve deeper. Note also that the realistic `mixed` model is the
*hardest* case, worse than uniform-random at K=100, because a principal's few scattered
entitlements are crowded out by their own topic cluster.

## Decision

Retrieval will **over-retrieve to depth ≥500 and post-filter**, rather than requiring
authorization to be pushed into the query planner.

- Default retrieval depth is `max(limit × 50, 500)`.
- The structured filter binds to **every** stage, including graph expansion. An expanded
  neighbour that fails the caller's filter is not a weaker match — it is an excluded one.
- The corporate seam shrinks from "a service with row-level security inside the query
  planner" to **"which bundles you may hold, plus a larger K"**.
- Authorization by **partition** (bundle boundary = ACL boundary) is the default for personal
  and small-team deployments and requires no service at all.

## Consequences

**Positive:**
- One search architecture serves personal and corporate. The only divergence left is *which
  bytes you may hold*, not *how you query them*.
- Depth is a multiplier on the cheapest stage in the pipeline; K=100→500 costs 5× on a stage
  measured in milliseconds.
- Deletes an entire subsystem from the proposed design.

**Negative:**
- Simulation used 64-dim synthetic clustered vectors with exact search — no ANN, no real
  embeddings. Real corpora have messier topology.
- Post-filtering presumes you already **hold** the bytes and are declining to rank them. It
  says nothing about whether you should hold them.
- Deeper retrieval costs memory and latency proportional to K at very large corpora.

**Mitigations:**
- Re-run on a real corpus with an ANN index before the corporate seam is deleted from the
  design for good. This ADR reorders the build plan; it does not yet authorize deleting code.
- **Revocation is explicitly not addressed** and remains the standing argument for a
  corporate boundary ([ADR-2608010910](ADR-2608010910-files-are-the-source-of-truth.md)).
- Depth is configurable, so a measured latency cliff is a config change, not a redesign.

## Implementation

| Phase | Description | Status | Verification |
|-------|------------|--------|--------------|
| P1 | Ranking policy as a pure domain function — depth, trust boost, staleness penalty | Pending | code:okf-domain/src/ranking.rs, test:cargo test -p okf-domain ranking |
| P2 | `SearchIndex` port over-retrieves to `max(limit*50, 500)` | Pending | code:okf-ports/src/search.rs, test:cargo test -p sqlite-index depth |
| P3 | Structured filter binds to graph-expansion stage, not only BM25 | Pending | code:okf-usecases/src/search.rs, test:cargo test -p okf-usecases expansion_respects_filter |
| P4 | ACL-aware recall harness reproducing the simulation on a real corpus + ANN | Pending | code:okf-conformance/tests/acl_recall.rs, test:cargo test -p okf-conformance acl_recall |

## References

- [FINDINGS.md](../../FINDINGS.md) — E1, full result table and limitations
- [DECISIONS.md](../../DECISIONS.md) — OPEN-5, the argument this overturns
- [ADR-2608010910](ADR-2608010910-files-are-the-source-of-truth.md)
