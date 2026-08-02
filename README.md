# Touchstone

**Provenance and portability for machine-written knowledge.**

Built on [Open Knowledge Format](https://github.com/GoogleCloudPlatform/knowledge-catalog/blob/main/okf/SPEC.md)
v0.2 — Google Cloud's open spec for representing knowledge as a directory of markdown
concepts with YAML frontmatter.

## Why this exists

In April 2026 Karpathy published a pattern that spread fast: point an LLM at your raw
sources and have it *compile* a markdown wiki, rather than retrieving over your notes with
RAG. Three layers — `raw/` (immutable), `wiki/` (generated), and a `CLAUDE.md` schema. One
topic reached ~100 articles and 400,000 words nobody typed.

The architecture is right, and this project is the same shape. What the pattern leaves open
is what happens once most of your knowledge base is machine-written:

| the pattern | what is missing | what Touchstone adds |
|---|---|---|
| `raw/` is authoritative | — | same rule: raw bytes are truth, frontmatter is a query view |
| `wiki/` is regenerable | — | A1, enforced by the T1 drill |
| `CLAUDE.md` is a private schema | no interchange, no portability | **an open spec** — bundles move between tools, byte-losslessly |
| generated pages read like facts | no way to tell what was verified | **trust tiers** — `verified` > `attested` > `generated`, derived and never authored |

So the bet is not "an LLM can maintain a wiki" — that is settled and freely available. The
bet is that once it does, you will need to know **which parts you can rely on** and **how to
leave with your corpus**. Those are format and provenance problems, and they are what this
is for.

That also names the real mess: knowledge management was already fragmented across tools that
do not interoperate, and machine-written knowledge is about to make the fragmentation worse
and the reliability question urgent at the same time.

**Status: Stage 1 walking skeleton.** The design is adjudicated and the core claims are
tested; the two highest-risk assumptions are not yet answered.

## Documents

| | |
|---|---|
| [ARCHITECTURE.md](ARCHITECTURE.md) | The design. Layers, bundle layout, type vocabulary, retrieval, the one ACL seam |
| [DECISIONS.md](DECISIONS.md) | Adversarial review record — two opposing reviewers, five open questions adjudicated, dissents preserved |
| [PROTOTYPE.md](PROTOTYPE.md) | How to falsify all of it. Assumption ledger ranked by risk, pre-registered kill criteria |
| [FINDINGS.md](FINDINGS.md) | Experimental results. Three experiments so far; two overturned load-bearing claims |

## Status: Rust, port complete

**Touchstone is a Rust project, and now only a Rust project.** The implementation lives in a
13-crate hexagonal workspace whose layering is enforced by Cargo itself — a boundary violation is a
compile error, not a lint (`touchstone-cli/tests/architecture.rs` guards the dependency graph that makes
that true).

| layer | crate | state |
|---|---|---|
| domain | `touchstone-domain` | types only, zero dependencies |
| ports | `touchstone-ports` | 7 traits |
| use cases | `touchstone-usecases` | **done** |
| adapters | `touchstone-yaml-serde` FrontmatterParser | **done** — temporal values stay ISO 8601 strings (E3a) |
| | `touchstone-fs-bundle` ConceptRepository | **done** — `log.md` is a concept, not a reserved name (E4a) |
| | `touchstone-sqlite-index` SearchIndex | **done** — FTS5 + structured prefilter |
| | `touchstone-git-attest` VersionControl | **done** |
| | `crdt-sync`, `embed-local` | deferred — gated on A7 and A4, untested assumptions |
| composition | `touchstone-cli` | **done** — full command surface |
| conformance | `touchstone-conformance` | **done** — the drills, as a black-box gate |
| primary | `touchstone-mcp-adapter` | **done** — MCP 2026-07-28, graded for agents |

```bash
cargo build --release --bin touchstone   # the conformance suite drives this binary
cargo test --workspace                   # 171 tests
bash tests/verify.sh                     # the standing acceptance gate
```

### The Python prototype is gone

`touchstone/` was the original Stage 1 walking skeleton, kept as a **differential oracle** while the
port was in flight: the Rust output was checked against it byte-for-byte on every bundle. It was
deleted once the port reached parity and the differential ran clean, which is exactly the condition
this README committed to. [FINDINGS.md E6](FINDINGS.md) records the deletion and what replaced it.

What the oracle was for did not go away, so it was moved rather than dropped:

| the oracle provided | now provided by |
|---|---|
| the ten drills | `touchstone-conformance`, driving the binary as a black box |
| a second opinion on ambiguous bytes | the drills assert the property directly instead of by comparison |
| E4a (`log.md` is a concept) | an assertion in `touchstone-conformance/tests/fixture.rs` |
| E4b (A1 fails on concept-free dirs) | resolved — A1 narrowed to what touchstone generates (E8) |

Because the suite names no `touchstone-*` crate — a rule the architecture test enforces — it can gate an
implementation it did not compile:

```bash
TOUCHSTONE_BIN=/path/to/other/impl cargo test -p touchstone-conformance
```

That is the property the differential used to provide, generalised: any implementation can be held
to the same drills, not just the two that happened to exist.

## A five-minute tour

Every block below is real output, captured from a live run. Start with an empty directory.

**Capture something.** Frontmatter is emitted through a YAML dumper, never string
concatenation — hand-written frontmatter with a `:` inside a value is the most common
authoring error there is.

```console
$ touchstone --bundle ~/brain new Note "Why files win" \
    --description "Raw bytes outlive every tool that reads them." \
    --tag design --generated capture/claude-opus-5
notes/why-files-win.md
```

```yaml
---
id: fd43e3b93413
type: Note
title: Why files win
description: Raw bytes outlive every tool that reads them.
tags:
- design
status: draft
generated:
  by: capture/claude-opus-5
  at: 2026-08-02T15:53:17Z
---

# Why files win
```

Note what is **not** there: no `verified`. An agent may not assert human verification, so
`--generated` produces a `machine` concept and there is no flag that produces a `human` one.

**Index it.** Idempotent, incremental on content hash.

```console
$ touchstone --bundle ~/brain index
indexed 2 concepts (2 new, 0 changed, 0 removed)
index.md files written: 3
broken links: 0 (legal per spec -- not-yet-written knowledge)
```

Broken links are reported, never rejected — a link to a concept you have not written yet is
knowledge about your own gaps.

**Search returns paths, not chunks.** The agent reads whole files.

```console
$ touchstone --bundle ~/brain search "raw bytes" --limit 3
~ notes/why-files-win.md
    Why files win  [Note]
    Raw bytes outlive every tool that reads them.

* human-verified   ~ machine-generated
```

The `~` is the trust tier, and it is **derived, never authored**: `verified[].by` starting
with `human:` outranks `generated` outranks neither. Ranking depends on it.

**Everything above the bundle is disposable.** Delete the derived plane and rebuild:

```console
$ rm -rf ~/brain/.touchstone && find ~/brain -name index.md -delete
$ touchstone --bundle ~/brain index -q && touchstone --bundle ~/brain stats
concepts: 2
```

Byte-identical, and drill T1 asserts it on every run — against this fixture and four
third-party bundles.

Note the precision: *generated* `index.md`. An `index.md` in a directory holding no concepts
is authored knowledge — often the only description of a PDF or a script sitting beside it —
and touchstone neither writes nor destroys it (E8).

**Lint catches what actually goes wrong**, which is duplicates rather than schema violations:

```console
$ touchstone --bundle ~/brain lint
notes/bad.md
  - duplicate tags: a
  - verified entry missing required `by`
  - body contains [[wikilinks]] -- not OKF, will not resolve

3 problem(s)
$ echo $?
1
```

**Leaving is a supported operation.** `export` copies raw bytes, so nothing can be dropped
on the way out — there is no serializer in the write path to drop it:

```console
$ touchstone --bundle ~/brain export /tmp/leaving
exported 2 concepts to /tmp/leaving
```

### Point an agent at it

```console
$ touchstone --bundle ~/brain mcp        # stdio; add --http 127.0.0.1:8765 for remote
```

```
touchstone_discover    read-only
touchstone_export      writes
touchstone_fmt         writes
touchstone_index       writes
touchstone_lint        read-only
touchstone_new         writes
touchstone_search      read-only
touchstone_show        read-only
touchstone_stats       read-only
```

Every tool is annotated, so a client knows which calls need a human in the loop. An agent
that knows nothing starts with `touchstone_discover` — one argument-free call returning the
bundle's shape, the available filters, and the whole tool list.

For Claude Code, add to `.mcp.json`:

```json
{
  "mcpServers": {
    "touchstone": {
      "command": "touchstone",
      "args": ["--bundle", "/absolute/path/to/brain", "mcp"]
    }
  }
}
```

## Commands

| Command | What it does |
|---|---|
| `touchstone new <Type> <Title>` | Scaffold a conformant concept. Emits frontmatter through a YAML dumper, never string concatenation |
| `touchstone index` | Rebuild the derived index and every generated `index.md`. Idempotent, incremental on content hash |
| `touchstone search <query>` | Structured prefilter → BM25 → one-hop graph expansion → trust rank |
| `touchstone lint` | Conformance floor plus the duplicate checks E2 showed are actually needed |
| `touchstone fmt [--check]` | Canonicalize frontmatter. **Refuses** files it cannot safely reproduce |
| `touchstone export <dir>` | Write raw bytes back out. Byte-exact by construction |
| `touchstone stats` | Concepts by type, trust tier, status; link and broken-link counts |
| `touchstone show <path>` | One concept's derived view. `--json` emits parsed frontmatter verbatim |
| `touchstone mcp [--http ADDR]` | Serve the MCP tool surface. stdio by default; Streamable HTTP on request |

Filters: `--type`, `--tag`, `--status`, `--trust`, `--limit`, `--no-expand`.

## For agents: the MCP surface

```bash
touchstone --bundle ~/brain mcp                      # stdio, for a local agent
touchstone --bundle ~/brain mcp --http 127.0.0.1:8765 # Streamable HTTP
```

Nine tools at MCP revision **2026-07-28**, each with input *and* output schemas, structured
content, and annotations so a client knows which calls need human confirmation:
`discover`, `search`, `show`, `stats`, `index`, `lint`, `fmt`, `new`, `export`.

The surface is graded against [api-ai-readiness](https://github.com/gaberger/api-ai-readiness),
and the gradeable part is a **test**, not a report — see
`touchstone-adapters/primary/mcp/tests/ai_readiness.rs`:

| Dimension | What the surface does |
|---|---|
| Response discipline | `limit` with a declared default of 10, ceiling 200; results carry `total`/`returned`, never a bare array |
| Field selection | `fields` on every result-bearing tool — ask for `["path"]` and get paths |
| Retrieval shape | `type`/`tag`/`status`/`trust` filter inside the query, not after |
| Self-description | recoverable failures say *what to do next*; only an unknown tool is a protocol error |
| Workflow atomicity | a concept path is the only identifier, and one call gets you one |
| Discovery | `touchstone_discover`, callable with no arguments |

**Parity is enforced.** `tests/parity.rs` fails the build if a CLI command has no MCP tool or
vice versa — the check [hex ADR-019](https://github.com/gaberger/hex/blob/main/docs/adrs/ADR-019-cli-mcp-parity.md)
specifies and leaves unimplemented. A human can do everything an agent can, and the reverse.

**No tool can write `verified`.** A concept an agent creates is `machine`, never `human`.

> **The HTTP transport has no authentication** and can read *and write* the bundle. stdio is the
> default for that reason. Bind HTTP to loopback, or put auth in front of it.

## Design rules

**Raw text is authoritative.** Frontmatter is parsed for querying, never treated as the
truth. `export` writes raw bytes, so there is no serializer in the write path that could
drop an unknown key — the failure mode is structurally impossible rather than merely
tested for.

**Everything touchstone generates is derived and disposable.** Delete `.touchstone/` and
every *generated* `index.md`, run `touchstone index`, and the result is byte-identical. T1
enforces this. The wording is precise on purpose: an `index.md` in a directory holding no
concepts is authored knowledge — often the only description of an artifact — and touchstone
neither generates nor destroys it (E8).

**The spec's tolerance is honored, not narrowed.** Unknown `type` values, unknown
frontmatter keys, and broken links are all preserved and indexed — the spec requires
consumers not reject them, and broken links legitimately represent not-yet-written
knowledge.

**Trust tiers are spec-derived.** `verified[].by` starting with `human:` → trusted;
`generated` present without human verification → machine; neither → unattributed. Search
ranks on this. No agent may ever write `verified`.

## What is tested, and what is not

Passing — 23 conformance tests, every drill against every bundle (the adversarial `_fixture`
plus four vendored third-party ones): index rebuild determinism, idempotence, byte-exact
round-trip through CRLF / unicode paths / YAML anchors / multiline scalars, unknown key and
type preservation, broken-link tolerance, ISO 8601 preservation, formatter safety, the trust
invariant, and recovery of the whole index from files alone.

Not tested, and load-bearing:

- **A10** — whether this beats `rg` + Obsidian on 20 real questions. If it doesn't, stop.
- **A3** — whether anyone writes into it unprompted. A brain nobody writes to is an empty
  database with good latency.
- **Scale** — the fixture is 25 concepts. Nothing here says anything about 50,000.
- **Round-trip against the upstream sample bundles** — the fixture is adversarial but
  self-authored, which is its weakness.
- **The trust invariant is enforced by nothing.** `verified: {by: human:...}` is meant to be
  backed by a signed commit, with CI rejecting unsigned deltas. Neither exists: there is no
  CI, and nothing calls the attestation adapter. `touchstone new` cannot write `verified`, so
  the tier survives accident but not an adversary — and `export` carries raw bytes with no
  signature, so a consumer of a shared bundle cannot check a claim at all. **This is the
  highest-value open item**, because provenance you cannot verify is the one thing this
  project says it is for.
- **Revocation** — unsolved, and *not* a git problem: no substrate that hands over the bytes
  can take them back, a CRDT least of all. Crypto-shredding is the only mechanism that
  changes the answer, and it is orthogonal to storage. The corporate boundary is justified by
  prefilter enforcement (measured, E1), not by revocation.
