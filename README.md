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
| generated pages read like facts | no way to tell what was verified | **trust tiers** — `human` > `machine` > `unattributed`, derived and never authored, and now **signed** |

So the bet is not "an LLM can maintain a wiki" — that is settled and freely available. The
bet is that once it does, you will need to know **which parts you can rely on** and **how to
leave with your corpus**. Those are format and provenance problems, and they are what this
is for.

That also names the real mess: knowledge management was already fragmented across tools that
do not interoperate, and machine-written knowledge is about to make the fragmentation worse
and the reliability question urgent at the same time.

**Status: Stage 1, built and instrumented.** The correctness claims are measured, the
provenance claims are now signed and verifiable, and it holds at 50k concepts. The question it
cannot answer about itself — whether anyone writes into it unprompted — is instrumented and
running (A3). Retrieval beat a fair `rg` baseline in a caveated pilot (A10, E11).

## Documents

| | |
|---|---|
| [ARCHITECTURE.md](ARCHITECTURE.md) | The design. Layers, bundle layout, type vocabulary, retrieval, the one ACL seam |
| [DECISIONS.md](DECISIONS.md) | Adversarial review record — two opposing reviewers, five open questions adjudicated, dissents preserved |
| [PROTOTYPE.md](PROTOTYPE.md) | How to falsify all of it. Assumption ledger ranked by risk, pre-registered kill criteria |
| [FINDINGS.md](FINDINGS.md) | Experimental results. Fourteen experiments; several overturned load-bearing claims, including two found by the harnesses built to test them |

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
cargo test --workspace                   # 173 tests (build the binary first -- conformance drives it)
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

## Install

```bash
cargo install --git https://github.com/gaberger/touchstone touchstone-cli
```

Or take a prebuilt binary from [Releases](https://github.com/gaberger/touchstone/releases) —
each is published with a `.sha256` and is only cut after the conformance suite passes on that
platform.

Then:

```bash
touchstone --bundle ~/brain init
```

`init` creates the layout and prints the two things it will not do for you: declaring who may
sign, and wiring your MCP client. Writing into someone's `.mcp.json` from a CLI is
presumptuous, and the one time it guesses wrong it has edited a config it does not own.

## Evidence

Every block below is real output from a live run against [`_sample/`](_sample/) — a corpus of
actual PDFs, PNGs, JPEGs, an MP4, CSV and prose. Nothing here is illustrative.

### One command from a pile of files to a knowledge base

```console
$ touchstone --bundle ~/brain init
$ touchstone --bundle ~/brain ingest _sample
ingested raw/standup-2026-07-14.md
ingested raw/demo-walkthrough.mp4
...
12 ingested. Nothing cites them yet -- `touchstone unprocessed` is the work queue.
```

Binaries come back byte-identical — the property that makes "portable" mean something:

```console
$ cmp _sample/images/whiteboard-schema.png ~/brain/raw/whiteboard-schema.png && echo identical
identical      # and the same for the MP4 and the PDF
```

### The agent gets the work and the material in one call

```console
$ touchstone --bundle ~/brain unprocessed --content --limit 1
--- raw/interview-priya.txt
Interview with Priya, 16 July 2026

Q: which created_at does the finance report use?
A: the ingest timestamp, not the order time. It has been wrong since the March migration.
```

### Capture is 0.01s warm, 0.38s cold

```console
$ time touchstone --bundle ~/brain capture "Finance report uses ingested_at, not order time."
decisions/finance-report-uses-ingested-at-not-order-time.md
real 0.38          # first run; 0.01 once the binary is in page cache
```

Both numbers are quoted because the first measurement was the cold one, and quietly swapping in
the warmer figure later would be the kind of thing this file exists not to do. Either clears the
~20 second bar PROTOTYPE.md sets, by a factor of fifty or more — and it observes that capture
dies above that bar, which is why the number matters at all.

### Citing a source drains the queue

The concept cites two raw documents in `sources:`. Both leave the queue — no state file, no
bookkeeping; a document is processed exactly when something cites it.

```console
$ touchstone --bundle ~/brain unprocessed
9 of 11 raw documents uncited
```

### Search finds it by what was asked, not what was typed

```console
$ touchstone --bundle ~/brain search "which timestamp does the finance report use" --limit 2
* decisions/finance-report-uses-ingested-at-not-order-time.md
    Finance report uses ingested_at, not order time  [Decision]
```

The question shares almost no vocabulary with the title. Until recently this returned
**nothing at all** — see the honest note below.

### The provenance chain, end to end

```console
$ touchstone --bundle ~/brain verify
  claims human verification with no attestation
0 of 1 human claims backed                                    # a claim is just text

$ touchstone --bundle ~/brain attest decisions/finance-...md --as human:gary --key ~/.ssh/id_ed25519
$ touchstone --bundle ~/brain verify
1 of 1 human claims backed                                    # now it is checkable

$ echo "an edit nobody verified" >> decisions/finance-...md
$ touchstone --bundle ~/brain verify
STALE: decisions/finance-report-uses-ingested-at-not-order-time.md
  signed, but the concept changed since -- the verified bytes are not these bytes
```

The signature covers the **content digest**, not the path. Editing a signed concept invalidates
its attestation rather than carrying it along — which is the difference between provenance and
a badge.

### It holds at scale

| | before | after |
|---|---|---|
| index 50,000 concepts | 1,028 s | **11.85 s** |
| query | 0.26 s | **0.11 s** |

The 17 minutes was one word: the FTS table declared `path UNINDEXED`, so deleting by path was a
full scan once per concept — quadratic. `FINDINGS.md` E10.

### Evidence that the tooling is the point

While writing this section, hand-editing frontmatter to add a source, I wrote:

```yaml
title: Re: margin restatement
```

which is invalid YAML — a `: ` inside an unquoted value. `lint` caught it immediately:

```console
error: invalid YAML: mapping values are not allowed in this context at line 17 column 14
```

FINDINGS E3d names that exact mistake as "the single most likely authoring error", which is why
`new` and `capture` emit frontmatter through a YAML dumper and never by string concatenation.
The failure above happened because I bypassed them.

### What is measured, and what is not

Honest, because a README that only lists wins is not evidence:

| | |
|---|---|
| Byte-exact round trip, incl. binaries | **measured** — T2a, M1 |
| Deterministic rebuild | **measured** — T1 across 5 bundles |
| 50k concepts | **measured** — E10 |
| Signed provenance, tamper-evident | **measured** — 5 attestation drills |
| Beats `rg` on real questions | **weak** — 18/20 vs a fair baseline's 11/20, caveated (E11) |
| Anyone writes into it unprompted | **untested** — A3, instrumented and waiting |

Two of this project's own claims were found false by the harnesses built to test them:
search returned **zero results for 20 of 20** natural-language questions, and `export` silently
dropped **every** artifact. Both are fixed; both passed every correctness test while broken,
because correctness testing cannot see usefulness.

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
| `touchstone init` | Create the bundle layout; print the setup that is yours to do |
| `touchstone ingest <file>...` | Copy source documents into the immutable `raw/` layer, byte-exact |
| `touchstone unprocessed` | Raw documents no concept cites yet — the work queue |
| `touchstone attest <path>` | Sign a concept's existing `verified` claim. **CLI only** — agents may not attest |
| `touchstone verify` | Check every `human:` claim against the signed manifest. Non-zero if any fails |

Filters: `--type`, `--tag`, `--status`, `--trust`, `--limit`, `--no-expand`.

## The pipeline

```
raw/          source documents, immutable, never parsed on the way in
  ↓           an agent reads them and writes concepts that CITE them
concepts      derived knowledge, each `sources:` entry naming what it was compiled from
  ↓           a human signs the ones they have actually checked
attest/       signatures over content digests, travelling with the bundle
```

```console
$ touchstone ingest ~/Downloads/interview.txt
ingested raw/interview.txt

$ touchstone unprocessed
raw/interview.txt

1 of 1 raw documents uncited
Compile them into concepts that cite them in `sources`, then they leave this list.
```

The work queue is **derived, not tracked**: a raw document is processed exactly when some
concept cites it. No state file, nothing to fall out of sync, and deleting the derived plane
changes nothing — the same rule the index follows.

Raw sources travel with `export`, because a concept citing `raw/interview.txt` in a bundle that
does not carry it has a provenance chain with a hole in it. And `lint` flags a machine-written
concept that cites nothing: the tier already says `machine`, and without a source there is no
way to check what the machine read.

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

**Trust tiers are spec-derived, and now signed.** `verified[].by` starting with `human:` →
trusted; `generated` present without human verification → machine; neither → unattributed.
Search ranks on this. No agent may ever write `verified` — and `touchstone verify` checks that
every such claim carries a valid signature over the concept's *current* bytes, so editing after
signing invalidates the attestation rather than inheriting it.

```console
$ touchstone --bundle ~/brain verify
STALE: notes/checked.md
  signed, but the concept changed since -- the verified bytes are not these bytes

0 of 1 human claims backed
```

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
- **Who to trust is still open.** Claims are now signed and verifiable (E12), but
  `attest/allowed_signers` is the bundle's own assertion about its signers, not a PKI. A
  bundle shipping a forged signers file is internally consistent. What is established is that
  a claim cannot be added, or its content changed, without detection.
- **No CI gate.** There is still no `.github/workflows`, so nothing runs `verify` or the
  conformance suite on push.
- **Revocation** — unsolved, and *not* a git problem: no substrate that hands over the bytes
  can take them back, a CRDT least of all. Crypto-shredding is the only mechanism that
  changes the answer, and it is orthogonal to storage. The corporate boundary is justified by
  prefilter enforcement (measured, E1), not by revocation.
