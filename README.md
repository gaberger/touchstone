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

| the pattern | what is missing | what okf adds |
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

## Quick start

```bash
python3 -m venv .venv && .venv/bin/pip install pyyaml
```

```bash
.venv/bin/python tests/make_fixture.py _fixture && .venv/bin/python -m touchstone --bundle _fixture index
```

Run the drills — T1 rebuild, T2 round-trip, T6 service-death:

```bash
.venv/bin/python tests/drills.py
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

Filters: `--type`, `--tag`, `--status`, `--trust`, `--limit`, `--no-expand`.

## Design rules

**Raw text is authoritative.** Frontmatter is parsed for querying, never treated as the
truth. `export` writes raw bytes, so there is no serializer in the write path that could
drop an unknown key — the failure mode is structurally impossible rather than merely
tested for.

**Everything above the bundle is derived and disposable.** Delete `.touchstone/` and every
`index.md`, run `touchstone index`, and the result is byte-identical. T1 enforces this.

**The spec's tolerance is honored, not narrowed.** Unknown `type` values, unknown
frontmatter keys, and broken links are all preserved and indexed — the spec requires
consumers not reject them, and broken links legitimately represent not-yet-written
knowledge.

**Trust tiers are spec-derived.** `verified[].by` starting with `human:` → trusted;
`generated` present without human verification → machine; neither → unattributed. Search
ranks on this. No agent may ever write `verified`.

## What is tested, and what is not

Passing (10/10 drills): index rebuild determinism, idempotence, byte-exact round-trip
through CRLF / unicode paths / YAML anchors / multiline scalars, unknown key and type
preservation, broken-link tolerance, ISO 8601 preservation, formatter safety, and recovery
of the whole index from files alone.

Not tested, and load-bearing:

- **A10** — whether this beats `rg` + Obsidian on 20 real questions. If it doesn't, stop.
- **A3** — whether anyone writes into it unprompted. A brain nobody writes to is an empty
  database with good latency.
- **Scale** — the fixture is 25 concepts. Nothing here says anything about 50,000.
- **Round-trip against the upstream sample bundles** — the fixture is adversarial but
  self-authored, which is its weakness.
- **Revocation** — git cannot revoke. Unsolved, and the one surviving argument for a
  corporate service boundary.
