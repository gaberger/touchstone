# Touchstone

**Provenance and portability for machine-written knowledge.**

An agent reads your source material and compiles a knowledge base where every claim cites what
it came from, and the claims a human has actually checked are cryptographically signed.

Built on [Open Knowledge Format](https://github.com/GoogleCloudPlatform/knowledge-catalog/blob/main/okf/SPEC.md)
v0.2 — Google Cloud's open spec for knowledge as a directory of markdown concepts with YAML
frontmatter.

## The problem

Once an LLM writes most of your knowledge base, two questions decide whether it is worth
anything, and most tools answer neither:

- **Which parts can you rely on?** Generated pages read exactly like verified ones. Without a
  trust signal that cannot be forged, a knowledge base is a pile of plausible text.
- **Can you leave with it?** Hosted tools export lossily by design. A corpus you cannot take
  with you is not yours.

Touchstone answers both by making them properties of the format rather than features of the
app: raw bytes on disk are the truth, provenance is signed and travels with the bundle, and
everything else — index, search, graph — is derived and disposable.

## How it works

```
raw/         source documents, immutable, never parsed on the way in
  ↓          an agent reads them and writes concepts that CITE them
concepts     markdown + YAML frontmatter, one file per idea, path is identity
  ↓          a human signs the ones they have actually checked
attest/      signatures over content digests, travelling with the bundle
```

Everything above the bundle is derived. Delete the index and every generated `index.md`,
re-run `touchstone index`, and the result is byte-identical.

## Install

```bash
cargo install --git https://github.com/gaberger/touchstone touchstone-cli
```

Or a prebuilt binary from [Releases](https://github.com/gaberger/touchstone/releases) — each
ships with a `.sha256` and is only cut after the conformance suite passes on that platform.

## Use it

```bash
touchstone --bundle ~/brain init
touchstone --bundle ~/brain ingest ~/Documents/notes    # recursive, deduplicated
touchstone --bundle ~/brain watch &                     # index stays current
```

Capture a thought — no editor, no title needed, 0.01s:

```console
$ touchstone --bundle ~/brain capture "Q4 was never restated. Only Q1 got the corrected basis."
notes/q4-was-never-restated.md
```

Ask a question months later, in the words you actually have:

```console
$ touchstone --bundle ~/brain search "why don't the quarterly figures agree"
* decisions/margin-restatement-is-incomplete.md
    Margin restatement is incomplete  [Decision]
    Only Q1 was corrected; Q4 still uses the old basis.

* human-verified   ~ machine-generated
```

You did not remember the title, the filename, or the word "margin". The `*` says a human
signed off on that one.

Ask something grep cannot express at all — a *field*, not a word:

```console
$ touchstone --bundle ~/brain search "margin" --type Decision --status stable --trust human
```

Then ask where the claim came from:

```console
$ touchstone --bundle ~/brain show decisions/margin-restatement-is-incomplete.md --json
{
  "path": "decisions/margin-restatement-is-incomplete.md",
  "trust": "human",
  "sources": [
    "raw/interview-priya.txt",
    "raw/email-thread.eml"
  ]
}
```

An interview and an email you ingested weeks ago. Both still in the bundle, byte-identical,
and both travel with it when you leave.

Finally: is that `human` tier worth anything?

```console
$ touchstone --bundle ~/brain verify
0 of 1 human claims backed                      # a claim is just text until it is signed

$ touchstone --bundle ~/brain attest decisions/margin-...md --as human:gary --key ~/.ssh/id_ed25519
$ touchstone --bundle ~/brain verify
1 of 1 human claims backed

$ echo "an edit nobody verified" >> decisions/margin-...md
$ touchstone --bundle ~/brain verify
STALE: decisions/margin-restatement-is-incomplete.md
  signed, but the concept changed since -- the verified bytes are not these bytes
```

The signature covers the content digest, not the path. Editing a signed concept invalidates its
attestation instead of carrying it along.

## For agents

```bash
touchstone --bundle ~/brain mcp                        # stdio
touchstone --bundle ~/brain mcp --http 127.0.0.1:8765  # Streamable HTTP
```

Thirteen tools at MCP revision **2026-07-28**, each with input *and* output schemas and
annotations so a client knows which calls need a human in the loop. `touchstone_unprocessed`
returns the uncompiled sources *with their content*, so an agent gets the work and the material
in one call.

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

**No tool can write `verified`.** Signing needs a private key a human holds, and `attest` is
deliberately absent from the MCP surface — it is the one capability an agent must not have.

## Commands

| | |
|---|---|
| `init` | Create the bundle layout |
| `capture <text>` | Record a thought in one command |
| `ingest <path>...` | Copy sources into `raw/`, byte-exact, recursive, deduplicated |
| `unprocessed` | Raw documents nothing cites yet — the work queue |
| `index` | Rebuild the derived plane. Idempotent, incremental on content hash |
| `watch` | Reindex continuously as files change |
| `search <query>` | Structured prefilter → BM25 → one graph hop → trust rank |
| `show <path>` | One concept's derived view; `--json` for parsed frontmatter |
| `stats` | Counts by type, trust tier, status; links and broken links |
| `lint` | Conformance floor, duplicate checks, uncited machine-written concepts |
| `fmt [--check]` | Canonicalise frontmatter; refuses what it cannot reproduce |
| `attest <path>` | Sign a concept's `verified` claim |
| `verify` | Check every claim against the signed manifest |
| `export <dir>` | Write everything back out — concepts, artifacts, sources, signatures |
| `new <Type> <Title>` | Scaffold a conformant concept |
| `mcp` | Serve the tool surface |

Filters: `--type`, `--tag`, `--status`, `--trust`, `--limit`, `--no-expand`.

## Design rules

**Raw text is authoritative.** Frontmatter is parsed for querying, never treated as the truth.
`export` writes raw bytes, so there is no serializer in the write path that could drop an
unknown key — the failure mode is structurally impossible rather than merely tested for.

**Everything touchstone generates is derived and disposable.** An `index.md` in a directory
with no concepts is authored knowledge, not generated, and is left alone.

**The spec's tolerance is honoured, not narrowed.** Unknown types, unknown keys and broken
links are all preserved and indexed. A broken link is knowledge about a gap.

**Trust tiers are derived, never authored.** `verified[].by` starting with `human:` → trusted;
`generated` present without it → machine; neither → unattributed. Search ranks on this.

## How we prove it works

Correctness is a **conformance suite that drives the shipped binary as a black box** — it names
no internal crate, so the same drills can gate a different implementation:

```bash
TOUCHSTONE_BIN=/path/to/other/impl cargo test -p touchstone-conformance
```

40 drills run against five bundles — an adversarial fixture plus four vendored third-party ones
written by people who never saw this code:

| | |
|---|---|
| Byte-exact round trip | CRLF, unicode paths, YAML anchors, block scalars, PNG/PDF/MP4 |
| Deterministic rebuild | delete the derived plane; output is byte-identical |
| Spec tolerance | unknown types and keys preserved, broken links recorded |
| Signed provenance | unbacked, stale and forged claims each detected distinctly |
| CLI/MCP parity | the two surfaces produce identical bytes for the same operation |
| Scale | 50,000 concepts index in 11.85s; query in 0.11s |

Plus 184 unit and integration tests, a layering check that makes an architecture violation a
compile error, and the whole gate on every push.

**What is not proven, stated plainly:**

- **Does it beat `rg` + Obsidian?** Weak evidence. On a real corpus with a fairly-ranked
  baseline, touchstone answered 18 of 20 questions against the baseline's 11 — but the
  questions were not written by a disinterested party. See `a10/`.
- **Will anyone write into it unprompted?** Untested. Instrumented and waiting for three weeks
  of real use. See `a3/`.
- **Who do you trust?** `attest/allowed_signers` is the bundle's own assertion about its
  signers, not a PKI. It makes tampering detectable, not identity certain.

## Documents

| | |
|---|---|
| [docs/USING.md](docs/USING.md) | How to run one as a business brain — what to ingest, what to write, what to sign |
| [ARCHITECTURE.md](ARCHITECTURE.md) | The design as it is today |
| [docs/adrs/](docs/adrs/) | Why each decision was made — append-only |
| [PROTOTYPE.md](PROTOTYPE.md) | How to falsify it. Pre-registered kill criteria |
| [FINDINGS.md](FINDINGS.md) | Experimental results, including the claims that turned out false |
| [_sample/](_sample/) | A mixed-media corpus to try it against |

MIT licensed.
