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
| E4b (A1 fails on concept-free dirs) | a recorded known-defect in `touchstone-conformance/tests/drills.rs` |

Because the suite names no `touchstone-*` crate — a rule the architecture test enforces — it can gate an
implementation it did not compile:

```bash
TOUCHSTONE_BIN=/path/to/other/impl cargo test -p touchstone-conformance
```

That is the property the differential used to provide, generalised: any implementation can be held
to the same drills, not just the two that happened to exist.

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

Filters: `--type`, `--tag`, `--status`, `--trust`, `--limit`, `--no-expand`.

## Design rules

**Raw text is authoritative.** Frontmatter is parsed for querying, never treated as the
truth. `export` writes raw bytes, so there is no serializer in the write path that could
drop an unknown key — the failure mode is structurally impossible rather than merely
tested for.

**Everything above the bundle is derived and disposable.** Delete `.touchstone/` and every
`index.md`, run `touchstone index`, and the result is byte-identical. T1 enforces this — and
currently falsifies it in one recorded case, E4b below.

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

Failing, recorded, undecided:

- **E4b** — `acme_retail/attesters/` holds no concepts, so the `index.md` upstream ships for
  it cannot be regenerated. A1 — *everything above the bundle is derived* — is therefore
  false as stated. Either `index` reconstructs concept-free directories, or A1 narrows to
  directories that contain concepts. The gate reports this as `XFAIL` on every run rather
  than hiding it, and will fail loudly if it silently starts passing.

Not tested, and load-bearing:

- **A10** — whether this beats `rg` + Obsidian on 20 real questions. If it doesn't, stop.
- **A3** — whether anyone writes into it unprompted. A brain nobody writes to is an empty
  database with good latency.
- **Scale** — the fixture is 25 concepts. Nothing here says anything about 50,000.
- **Round-trip against the upstream sample bundles** — the fixture is adversarial but
  self-authored, which is its weakness.
- **Revocation** — git cannot revoke. Unsolved, and the one surviving argument for a
  corporate service boundary.
