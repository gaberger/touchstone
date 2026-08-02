# touchstone — Architecture (current state)

> **This is the map, not the ledger.** It describes the system *as it is today* and is
> rewritten freely whenever the design shifts. For *why* a decision was made, read the
> Architecture Decision Records in [`docs/adrs/`](docs/adrs/) — append-only, never edited
> to match the present.
>
> **Status:** Built. The Rust workspace described below exists and passes its own conformance
> suite; the Python walking skeleton it was ported from has been deleted (FINDINGS E6). Research
> that produced these decisions:
> [DECISIONS.md](DECISIONS.md), [FINDINGS.md](FINDINGS.md), [PROTOTYPE.md](PROTOTYPE.md),
> [VERSION-CONTROL.md](VERSION-CONTROL.md), [RUST-PATH.md](RUST-PATH.md).

## What touchstone is

A knowledge base — corporate brain or personal brain, one architecture — built on
**Open Knowledge Format v0.2**, Google Cloud's open specification for representing
knowledge as a directory of markdown concepts with YAML frontmatter.

The substrate is deliberately boring: a bundle is a directory tree, each `.md` file is a
concept, the file path is its identity, and the only required frontmatter field is `type`.
Everything else in this system — index, search, graph, sync — is **derived from those files
and disposable**. Delete every derived artifact and one command rebuilds it byte-identically.
That property is verified, not asserted ([ADR-2608010910](docs/adrs/ADR-2608010910-files-are-the-source-of-truth.md)).

## The execution model

```
capture  →  concept file (raw markdown, authoritative)
              ↓  (CRDT checkpoint — write path)
            bundle on disk
              ↓  (touchstone index — idempotent, incremental on content hash)
            derived plane:  index.md · FTS · edges · vectors
              ↓
search   →  structured prefilter → BM25 (+vector) → 1-hop graph expansion → trust rank
              ↓
            concept PATHS, never chunks — the agent reads whole files
```

Two rules govern the whole pipeline:

1. **Raw text is authoritative.** Frontmatter is parsed for querying, never treated as the
   truth. Export writes raw bytes, so no serializer in the write path can drop an unknown
   key. The failure mode is structurally impossible rather than merely tested for.
2. **Authorization is a prefilter, and retrieval depth is ≥500.** Post-filtering an
   approximate index destroys recall at shallow depth; depth restores it fully
   ([ADR-2608010920](docs/adrs/ADR-2608010920-postfilter-authorization-at-depth.md)).

## Hexagonal layout

Per hex [ADR-001](https://github.com/gaberger/hex/blob/main/docs/adrs/ADR-001-hexagonal-architecture.md),
one agent = one adapter = one worktree = one merge unit.

```
touchstone-domain/          pure. Concept model, conformance floor, trust-tier derivation,
                     link resolution, ranking policy, index.md rendering, lint rules.
                     Zero external crates. Survives a stack rewrite untouched.

touchstone-ports/           traits only. FrontmatterParser · ConceptRepository · SearchIndex ·
                     VersionControl · SyncEngine · Embedder · Clock

touchstone-usecases/        compose ports. IndexBundle · SearchBundle · CaptureConcept ·
                     LintBundle · ExportBundle · FormatBundle

touchstone-adapters/
  primary/
    cli/             `touchstone` command surface
    mcp/             MCP tool surface, revision 2026-07-28. Graded against
                     api-ai-readiness by test; parity with the CLI enforced by test.
  secondary/
    yaml-serde/      FrontmatterParser  ← serde_yaml_ng
    fs-bundle/       ConceptRepository  ← filesystem walk + raw byte IO
    sqlite-index/    SearchIndex        ← rusqlite, FTS5 bundled
    git-attest/      VersionControl     ← gix (signed commits)
    crdt-sync/       SyncEngine         ← automerge or loro
    embed-local/     Embedder           ← fastembed (OPTIONAL — gated on A4)

touchstone-cli/             composition root. The ONLY crate that imports adapters.

touchstone-conformance/     the drills, as a black-box gate over the `touchstone` BINARY.
                     Names no touchstone-* crate at all (rule 7), so it can gate an
                     implementation it did not compile: TOUCHSTONE_BIN=… cargo test.
```

### Why the frontmatter parser is a port

This is the non-obvious one, and it is driven by measurement. Two conformant YAML libraries
disagree on the same bytes: `serde_yaml_ng` does not implement merge keys, so a concept
carrying `verified: [{<<: *defaults}]` reads as **human-verified** under PyYAML and
**unattributed** under Rust. The trust tier flips with the reader.

Making the parser a port means (a) the divergence is a **port-contract test** rather than a
latent production bug, and (b) swapping to a document-preserving parser later is a
single-adapter change. See [ADR-2608010940](docs/adrs/ADR-2608010940-rust-implementation-parser-as-port.md).

### Why `git` is an adapter, not the substrate

Git does two unrelated jobs, and only one of them it does well. Sync across
laptop/phone/server, offline, without conflicts — it is bad at. Durable signed history and
review — it is excellent at. The CRDT owns the write path; git becomes an append-only
signed attestation sink no human hand-edits.

The asymmetry that makes this safe: **the files are the source of truth and version history
is not.** Losing the entire history costs history, not knowledge.
[ADR-2608010930](docs/adrs/ADR-2608010930-git-as-attestation-sink.md).

## The trust invariant

Derived from the spec's own actor convention — no invention:

| `verified[].by` starts with `human:` | **trusted** |
| `generated` present, no human `verified` | **machine** |
| neither | **unattributed** |

**An agent may never write `verified: {by: human:...}`.** In a database row that is a claim;
in git it is meant to be a signed commit. This field is the only thing separating a curated
brain from a pile of plausible text, and every ranking decision in retrieval depends on it.

**Enforced by signature** (E12). `touchstone attest` signs a claim with an SSH key; the
signature covers the concept's **content digest**, not its path, so editing a concept after
signing invalidates the attestation rather than carrying it along. `touchstone verify` checks
every `human:` claim and exits non-zero if any is unbacked, stale, or signed by a key the
bundle does not list.

The manifest and `allowed_signers` live at `attest/` — not dot-prefixed, so `export` carries
them and a stranger who receives the bundle can check its claims. A signature that does not
travel with the bytes it signs protects nobody.

`attest` is **CLI-only, and it is the single deliberate hole in CLI–MCP parity.** Signing needs
a private key a human holds; exposing it over MCP would hand a model the one capability the
trust invariant forbids it. `touchstone_verify` exists for agents; `touchstone_attest` does not
and will not.

What this does *not* establish is who to trust — `allowed_signers` is the bundle's own
assertion about its signers, not a PKI. What it does establish is that a claim cannot be added,
or its content changed, without detection.

## The one seam: personal vs corporate

Everything is identical across both except **enforcement of who may hold which bytes**:

| | Personal / small team | Corporate |
|---|---|---|
| Enforcement | partition — mount table + repo-level permissions | row-level, applied before ANN |
| Identity | local | SSO group membership |
| Service | none | yes |

A personal concept graduates to corporate by gaining an ACL record, not by migrating.

**What justifies the corporate boundary is prefilter enforcement, not revocation.**
Authorization applied before ANN is measured (E1, recall ≥0.95); post-filtering an
approximate index destroys recall, and a file-first design has nowhere to put a prefilter.
That is the real seam.

**Revocation remains unsolved, but it is not a git problem** — and an earlier version of
this document implied it was. No substrate that hands someone the bytes can take them back:
not files, not SQLite, and least of all a CRDT, which is *designed* to replicate every
change to every replica. The only mechanism that changes the answer is crypto-shredding —
encrypt at rest, hold the keys centrally, destroy the key — which is orthogonal to storage
and works on git as well as anything else. It is weaker still here than elsewhere, because
retrieval hands whole files to a model: by the time you revoke, the content has been read.

## Evidence status

Every load-bearing claim, and whether it is measured:

| Claim | Status | Evidence |
|---|---|---|
| Everything touchstone GENERATES is byte-identically rebuildable | **measured** | T1 across 5 bundles; E4b resolved by narrowing A1 (E8) |
| Round-trip is byte-exact (CRLF, unicode, anchors) | **measured** | T2a, all 5 bundles |
| Unknown types/keys and broken links preserved | **measured** | T2c |
| Trust tier is stable across a canonical rewrite | **measured** | T2b |
| Post-filter authz sufficient at K≥500 | **measured** | E1, recall ≥0.95 all configs |
| Git merges OKF frontmatter without corruption | **measured** | E2, 0 silent failures / 400 trials |
| PyYAML breaks ISO 8601; serde_yaml_ng does not | **measured** | E3a |
| Parsers diverge on merge keys | **measured** | RUST-PATH §1 |
| Hybrid retrieval beats BM25 alone | **UNTESTED** (A4) | — |
| Trust tiers improve ranking | **UNTESTED** (A8) | — |
| Holds at 50k concepts | **measured** | E10: 11.85s cold index, 0.11s query, curve flat |
| **Beats `rg` + Obsidian** | **UNTESTED** (A10) | project-fatal |
| **Anyone writes into it unprompted** | **UNTESTED** (A3) | project-fatal |

The last two outrank everything buildable. A brain nobody writes to is an empty database
with good latency.

The first row was false for most of this project's life. E4b showed a real bundle whose
`index.md` could not be regenerated — and the resolution was not to make the implementation
cleverer but to correct the claim: touchstone cannot generate an index for a directory with
no concepts, so that file was never derived. **A1 now says what it means** — everything
touchstone *generates* is derived and disposable — and the drills enforce exactly that (E8).

## Hexagonal rules (enforced by `hex analyze .`)

1. `touchstone-domain/` imports only `touchstone-domain/`.
2. `touchstone-ports/` imports `touchstone-domain/` only, for value types.
3. `touchstone-usecases/` imports `touchstone-domain/` + `touchstone-ports/` only.
4a. `adapters/secondary/` import `touchstone-ports/` only — they **implement** ports.
4b. `adapters/primary/` import `touchstone-usecases/` + `touchstone-ports/` — they **drive**
   use cases, and must name `touchstone-usecases`.
5. Adapters NEVER import other adapters.
6. `touchstone-cli/` composition root is the ONLY file that imports adapters.
7. `touchstone-conformance/` imports **no** internal crate — it drives the built binary.

Rule 5 is what makes the parser swap, the git swap, and the CRDT addition independent
merge units — three agents, three worktrees, no coordination.

Rule 4 was a single rule until the MCP surface was built, and the split is the most
consequential correction in this document. Forbidding `touchstone-usecases` to *every*
adapter is right for a driven one and wrong for a driving one — and it is why the use-case
layer sat unreachable with 1,074 tested lines while the CLI reimplemented it. The gate did
not merely fail to catch that duplication; it required it
([ADR-2608021132](docs/adrs/ADR-2608021132-mcp-surface-graded-for-agents.md)).

Rule 7 is what stops the conformance suite from grading its own homework. A suite that
imported `touchstone-domain` could assert against the very code that produced the answer and would
pass by construction; one that imported an adapter could only ever gate *this* build. Naming
nothing internal is what lets the same drills be pointed at a future rewrite.

## Build & test

```bash
cargo check --workspace && cargo test --workspace
```

Conformance — the portable asset, and the gate any implementation must pass
([ADR-2608010950](docs/adrs/ADR-2608010950-conformance-suite.md)). It drives the built binary,
so build first; a missing binary is a hard error, never a skip, because a gate that quietly
skips reports green while asserting nothing:

```bash
cargo build --release --bin touchstone
cargo test -p touchstone-conformance

# ...or hold a different implementation to the same drills:
TOUCHSTONE_BIN=/path/to/other/impl cargo test -p touchstone-conformance
```

The whole gate — build, tests, layering, conformance, byte-exact export over every bundle:

```bash
bash tests/verify.sh
```

## Decision governance

ADRs live in [`docs/adrs/`](docs/adrs/) and follow hex's lifecycle:
`Proposed → Accepted → Completed`, with `Completed` meaning implementation is confirmed by
evidence, not merely merged. Status changes only via `adr_status_set`.
