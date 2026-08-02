# Using touchstone as a business brain

Practical guidance for cataloguing what an organisation actually produces — email, meetings,
documents, decisions, people, systems. Opinionated on purpose: most of these rules exist
because the alternative degrades into a folder of files nobody searches.

## The one rule everything else follows

> **Ingest everything. Compile selectively. Sign rarely.**

Three different economies, and confusing them is the usual failure:

| | cost | so |
|---|---|---|
| `raw/` | nearly free — bytes on disk | ingest indiscriminately, decide later |
| concepts | your attention | write one only when someone would search for it |
| signatures | your credibility | sign only what you have actually checked |

A bundle with 4,000 raw documents and 200 concepts is healthy. One with 4,000 concepts is a
second inbox.

## What is a concept?

**One idea somebody would go looking for.** Not one document, not one meeting, not one file.

The test: could you imagine typing a question that this concept answers? If not, it is raw
material, not a concept.

```
✗  "Meeting notes 14 July"          — a document; ingest it
✓  "We ship the margin fix behind a flag"  — a decision someone will search for
```

A single meeting might produce three concepts, or none. A forty-page policy PDF might produce
one. The ratio is not fixed and should not be.

## Mapping business material

| You have | Goes to | Becomes a concept when |
|---|---|---|
| Email thread | `raw/` | it settles something — then a `Decision` citing it |
| Meeting recording / notes | `raw/` | a commitment or conclusion came out of it |
| Contract, policy, SOW | `raw/` | its *operative terms* matter — a `Note` or `Term` citing it |
| Slack export | `raw/` | rarely. Most of it is process, not knowledge |
| Dashboard, report | `raw/` snapshot | the *definition* matters — a `Metric` |
| Runbook, procedure | write directly | it is already the thing people search for |
| Someone's role and context | write directly | a `Person` concept, cited by decisions they own |

**Emails and meetings are sources, not knowledge.** What you *learned* from them is the
knowledge. This is the distinction that keeps a business brain from becoming an archive.

## Types

The scaffolder offers ten: `Note` `Source` `Person` `Project` `Decision` `Meeting` `Term`
`Runbook` `System` `Metric`.

They are a starting vocabulary, not a schema. OKF requires consumers not to reject unknown
types, and touchstone honours that — real bundles in the wild use `Policy`, `Reference`,
`BigQuery Table`, `Attested Computation`. Invent one when you need it, then use it
consistently, because `--type` is a filter and inconsistency is what makes filters useless.

Worth being strict about:

- **`Decision`** — something was settled and could later be questioned. These are what people
  search for most and remember least.
- **`Term`** — your organisation's private vocabulary. "Margin", "active customer" and
  "shipped" mean something specific to you, and disagreement about them is expensive.
- **`Person`** — who owns what, who to ask. Cite them from decisions.
- **`Metric`** — the *definition*, not the value. Values live in dashboards; definitions rot
  in people's heads.

## Directory layout

Path is identity, so directories are the one structural decision that is expensive to change.
Keep it shallow and about *kind*, not chronology:

```
raw/                every ingested source, flat
decisions/          settled questions
terms/              vocabulary
people/
projects/
systems/
runbooks/
notes/              everything else
```

**Do not organise by date.** `2026/q3/july/` buries knowledge exactly as it becomes useful, and
touchstone already tracks time in frontmatter. Search and filters replace hierarchy — the
directory is for humans browsing, not for retrieval.

## Trust: what to sign

A signature says *"I read this and it is correct."* It ranks the concept above machine-written
text in every search, so it is a real assertion.

- **Sign**: decisions with consequences, term definitions, anything a person will act on
  without re-checking.
- **Do not sign**: notes, meeting captures, anything an agent wrote that you skimmed.
- **Never sign for someone else.** `attest --as` will let you, and `verify` will reject it,
  because your key does not vouch for them.

Unbacked claims are **not** a defect to clear. `verify` reporting `0 of 40 backed` on a young
bundle is honest — it means nobody has checked anything yet, which is usually true.

Re-sign after editing. A signature covers the content digest, so an edit makes it `STALE` by
design. That is the feature.

## The daily loop

**Capture is the habit that matters.** Everything else is recoverable; a thought you did not
record is gone.

```bash
touchstone --bundle ~/brain capture "the thing you just realised"
```

Under a second, no editor, no title. If capture takes longer than about twenty seconds you
stop doing it, and then nothing else matters.

Run the watcher so the index is never stale:

```bash
touchstone --bundle ~/brain watch &
```

Weekly, drain the queue with an agent:

```bash
touchstone --bundle ~/brain ingest ~/Downloads/*.pdf ~/exports/email
touchstone --bundle ~/brain unprocessed
```

Then let the agent read the uncompiled sources and write concepts citing them. It gets the
work and the material in one call, so this is a conversation, not a pipeline you maintain.

## Working with the agent

Point your MCP client at the bundle and tell it what you want. Prompts that work:

- *"What's uncompiled? Read the three oldest and write concepts for anything that settles a
  question. Cite the source in each."*
- *"Find every Decision about pricing still marked draft."*
- *"What do we not have written down about the March migration?"* — broken links and thin areas
  are visible to it.

**The agent cannot forge trust.** It cannot write `verified`, and it cannot sign. Everything it
writes is `machine` until a human reads it and attests. That asymmetry is the point: an agent
that could vouch for itself would make the trust tier meaningless.

## Anti-patterns

**Hand-editing frontmatter.** A `: ` inside an unquoted value is invalid YAML, and it is the
most common authoring mistake there is. Use `capture`, `new` and `fmt`; run `lint` before you
trust a batch.

**Treating broken links as errors.** They are legal, and they are your backlog — a link to a
concept you have not written is a note about a gap. `stats` counts them; do not chase them to
zero.

**Signing everything.** If every concept is `human`, the tier carries no information and
ranking learns nothing from it.

**One concept per document.** You end up with a filing cabinet that happens to have full-text
search, which you already had.

**Deferring ingestion.** Raw material is cheapest to capture at the moment it exists and most
expensive to reconstruct later. Ingest first, decide what matters afterwards.

## Health check

```bash
touchstone --bundle ~/brain stats        # shape: types, trust tiers, link density
touchstone --bundle ~/brain lint         # conformance, duplicates, uncited machine writing
touchstone --bundle ~/brain verify       # which claims are actually backed
touchstone --bundle ~/brain unprocessed  # how far behind the compiling is
```

Signals worth watching:

- `unprocessed` growing steadily → you are ingesting faster than anyone compiles. Either enlist
  the agent or accept the backlog explicitly.
- Zero broken links → suspicious. Either the corpus is trivial or nobody is writing about
  things that do not exist yet.
- Everything `unattributed` → nobody is verifying anything, and the trust tier is decorative.
- `lint` flagging uncited machine writing → an agent asserted something with no source, which
  cannot be audited.

## Scripting

Exit codes follow grep, so these compose the way you would expect:

| | `search` | `lint` / `verify` / `fmt --check` |
|---|---|---|
| `0` | results found | clean |
| `1` | no results | problems found |
| `2` | bad query | — |

```bash
# nightly: fail the job if an unbacked claim has crept in
touchstone --bundle ~/brain verify || notify "unsigned claims in the brain"

# find nothing? that is an answer, not an error
touchstone --bundle ~/brain search "$q" || echo "nothing on that yet"
```

`no results` exiting non-zero is deliberate: it is what `grep` does, and it means a shell
conditional reads correctly without parsing output.

## Leaving

```bash
touchstone --bundle ~/brain export /somewhere/else
```

Concepts, artifacts, raw sources and signatures — byte-identical. The exported directory is a
complete bundle: `verify` works on it, and any OKF consumer can read it. There is no lock-in to
argue about, which is the point of building on an open format rather than a schema.
