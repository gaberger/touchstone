# ADR-2608010900: Open Knowledge Format v0.2 as the knowledge substrate

**Status:** Proposed
**Date:** 2026-08-01
**Epoch:** stage-1-skeleton
**Drivers:** Need a knowledge base serving either a corporate or a personal brain, easily editable, searchable, and automatically indexed — without inventing a proprietary store.

## Context

A second brain must outlive the tooling that reads it. Every knowledge product that stores
its content in a proprietary database eventually becomes unreadable when that product is
deprecated, repriced, or outgrown. The storage format therefore has to be the most boring,
most portable thing available.

Open Knowledge Format (OKF) is a Google Cloud open specification — v0.1 June 2026,
currently **v0.2** — that formalizes exactly this. Verified against the spec rather than
secondary coverage:

- A **bundle** is a directory tree of markdown files. Each file is a **concept**. The file
  path is the concept's identity.
- **Exactly one required frontmatter field: `type`** (non-empty string, no central
  registry). Recommended: `title`, `description`, `resource`, `tags`.
- Trust/provenance families: `sources[]` (each entry requires `resource`),
  `generated: {by, at}`, `verified: [{by, at}]`, `status: draft|stable|deprecated`,
  `stale_after: YYYY-MM-DD`.
- Actor convention: `producer/version`, `human:id`, `process:id` — trust tier is derived by
  detecting the `human:` prefix.
- Reserved filenames: `index.md`, `log.md`. `okf_version` is declared in the **root
  `index.md` frontmatter only**.
- Links are standard markdown links, bundle-absolute preferred. **Broken links are legal**
  — they represent not-yet-written knowledge.
- Conformance floor: valid YAML frontmatter + non-empty `type`. Consumers **MUST NOT**
  reject a bundle for unknown types, unknown keys, broken links, or a missing `index.md`.

The critical consequence: **OKF is a format, not a platform.** It specifies interchange and
says nothing about storage, search, indexing, access control, or editing.

Alternatives considered: Obsidian vault (no interchange contract, wikilinks are not
markdown links); Notion/Confluence (proprietary, no local truth); a bespoke schema (fails
the portability requirement outright); plain markdown with no frontmatter convention
(loses every structured query, which is the actual value — see Consequences).

## Decision

We will adopt **OKF v0.2 as the canonical storage format**, and honor its tolerance rules
rather than narrowing them.

- Every concept is one `.md` file with YAML frontmatter. The path is the identity.
- We enforce the spec's conformance floor and **nothing stricter at read time**: unknown
  `type` values, unknown frontmatter keys, and broken links are preserved and indexed, never
  rejected.
- We define a closed **type vocabulary** for authoring convenience only — `Note`, `Source`,
  `Person`, `Project`, `Decision`, `Meeting`, `Term`, `Runbook`, `System`, `Metric`,
  `Attested Computation` — which constrains what `brain new` scaffolds, not what the reader
  accepts.
- **Trust tiers are spec-derived, not invented**: `verified[].by` beginning `human:` →
  trusted; `generated` present without human verification → machine; neither →
  unattributed.
- The bundle boundary equals the ACL boundary. Git's read granularity *is* the repository;
  a clone hands over all history, and there is no subtree read ACL. Multiple bundles are
  unioned at read time by a mount table.
- **`id` is identity, path is an address.** Required by the CRDT write path
  ([ADR-2608010930](ADR-2608010930-git-as-attestation-sink.md)) — a live editing session
  cannot be keyed on a mutable string. `id` and `aliases` are unknown keys, which the spec
  requires consumers preserve, so they cost zero conformance.

## Consequences

**Positive:**
- The bundle renders on GitHub and opens in Obsidian, VS Code, or vim with zero adapters.
- Any other OKF consumer can read our output; the upstream `reference_agent` CLI is a
  ready-made independent verifier.
- Structured queries (`type`, `tags`, `status`, trust tier) are available without a database
  — this is the capability plain markdown lacks and the reason frontmatter earns its cost.
- Portability is testable, not aspirational (see [ADR-2608010950](ADR-2608010950-conformance-suite.md)).

**Negative:**
- OKF v0.2 is explicitly "a starting point, not a finished standard" — it will change.
- Path-as-identity means renames break inbound links, and the spec's tolerance of broken
  links makes this silent.
- No access control whatsoever. The format has nothing to say about it.

**Mitigations:**
- `okf_version` is recorded in root `index.md`, so a future migration has a version to key
  on; the parser is a port ([ADR-2608010940](ADR-2608010940-rust-implementation-parser-as-port.md))
  so spec drift is an adapter change.
- Renames rewrite every inbound link atomically and leave `aliases: [old/path.md]` behind.
- ACL is handled at the bundle boundary, not in frontmatter — a `visibility:` key is a note
  on the outside of an unlocked door and must never gate anything.

## Implementation

| Phase | Description | Status | Verification |
|-------|------------|--------|--------------|
| P1 | Concept model, conformance floor, trust-tier derivation — pure domain | Pending | code:touchstone-domain/src/concept.rs, test:cargo test -p touchstone-domain |
| P2 | Type vocabulary + `brain new` scaffolding via a YAML emitter, never string concatenation | Pending | code:touchstone-usecases/src/capture.rs, test:cargo test -p touchstone-usecases capture |
| P3 | Tolerance rules: unknown types/keys preserved, broken links recorded | Pending | code:touchstone-domain/src/conformance.rs, test:cargo test -p touchstone-conformance tolerance |
| P4 | Mount table for multi-bundle union | Pending | code:touchstone-adapters/secondary/fs-bundle/src/mounts.rs, test:cargo test -p fs-bundle mounts |

## References

- [OKF v0.2 SPEC.md](https://github.com/GoogleCloudPlatform/knowledge-catalog/blob/main/okf/SPEC.md)
- [How the Open Knowledge Format can improve data sharing — Google Cloud](https://cloud.google.com/blog/products/data-analytics/how-the-open-knowledge-format-can-improve-data-sharing)
- [ADR-2608010910](ADR-2608010910-files-are-the-source-of-truth.md) — derived plane
- [DECISIONS.md](../../DECISIONS.md) — adversarial review that produced this
