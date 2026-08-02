# Experimental Findings

Results from the two no-code experiments in [PROTOTYPE.md](PROTOTYPE.md) §5. Both were
run to falsify pre-registered claims. Both falsified them. Scripts are in the session
scratchpad (`acl_sim.py`, `merge_trial.py`).

---

## E1 — A6: post-filter vs. prefilter authorization

**Claim under test** (DECISIONS.md OPEN-5, the Operator's argument that decided the
corporate seam): *"200k concepts, a user entitled to 5%: retrieve top-100 by vector
similarity, then filter by entitlement → ~5 survive, and the relevant ones were likely
outside the 100. Authorization must be a prefilter inside the query planner."*

**Pre-registered kill criterion:** post-filter reaching ≥0.95× prefilter recall falsifies
the requirement and deletes the corporate service seam.

**Method.** 100k documents, 64-dim, clustered into 50 topics. 300 queries drawn near topic
centroids. 40 principals. Ground truth = exact top-10 within the entitled set (what
prefilter achieves by definition). Post-filter = global top-K, drop unentitled, take top-10.
Metric = recall@10.

The original test design missed the variable that turns out to govern the whole result:
**whether entitlement correlates with query topic.** Real orgs are not random — the legal
team's documents are about legal, and the legal team asks legal questions. Three models
were tested: `random` (uniform), `topical` (principal owns whole topics), and `mixed`
(80% topical + 20% scattered, modelling shared/general docs).

**Results** — recall@10 vs. exact-within-entitled:

| Entitlement | Model | K=100 | K=500 | K=2000 | survivors@100 |
|---|---|---|---|---|---|
| **5%** | random | 0.501 | **1.000** | 1.000 | 5.0 |
| **5%** | topical | 0.971 | 1.000 | 1.000 | 27.9 |
| **5%** | mixed | 0.410 | **0.953** | 1.000 | 5.2 |
| 20% | random | 1.000 | 1.000 | 1.000 | 20.0 |
| 20% | mixed | 0.960 | 1.000 | 1.000 | 19.8 |
| 50% | all | 1.000 | 1.000 | 1.000 | ~50 |

**Verdict: A6 FALSIFIED.** Post-filter clears the 0.95 bar in **every** configuration at
K=500, and is perfect at K=2000.

The Operator's arithmetic was right and his conclusion was wrong. At K=100 and 5%
entitlement the effect is real and severe — recall 0.41–0.50, ~5 survivors, exactly as
claimed. But the remedy is not a query planner with row-level security. **The remedy is to
retrieve deeper.** K=100 → K=500 is a 5× multiplier on the cheapest stage in the pipeline,
and it fully recovers recall.

Note also that the worst case is `mixed`, not `random` — the realistic model is *harder*
than the uniform one, because a principal's few scattered entitlements sit outside their
topic cluster and get crowded out. Topical entitlement alone is nearly fine even at K=100
(0.971), which means an org whose ACLs align cleanly with subject matter barely has this
problem at all.

**What this does and does not kill.** It kills the *recall* argument for a corporate
search service. It leaves the Operator's *other* OPEN-5 argument completely intact:

> **Git cannot revoke.** Once cloned, revoked is fiction.

That argument was never about ranking. Post-filtering presumes you already hold the bytes
and are declining to *rank* them; it says nothing about whether you should hold them. So
the corporate seam survives — but it shrinks from "a service with RLS inside the query
planner" to **"which repos you may clone, plus a larger K."** That is much closer to the
Archivist's partition model than to the Operator's Postgres.

**Limitations.** 64-dim, synthetic clustered vectors, exact search (no ANN). Real
embeddings have messier topology, and adding an approximate index would degrade both arms
— though not obviously asymmetrically. This should be re-run on a real corpus before the
seam is deleted from the design. It is strong enough to reorder the build plan, not yet
strong enough to delete code.

---

## E2 — Does git silently corrupt OKF frontmatter?

**Claim under test** (the Operator's attack (b), and the strongest single argument for
putting a CRDT in the write path): *"Two PRs each appending to `tags:` or `verified:`
merge cleanly into a duplicated or malformed list — no conflict marker, semantic
corruption."*

**Method.** 400 randomized two-author merge trials. Realistic OKF concepts (4–9 tags, 1–4
`verified[]` entries, 6 body paragraphs). Each author applies one random structured edit —
insert tag at a random position, remove tag, insert `verified` entry at a random position,
change `status`, change `stale_after`, or edit a body paragraph. Merge with git's default
driver, then classify.

**Results:**

| Outcome | Count | Rate |
|---|---|---|
| `CLEAN_CORRECT` — merged, valid, both intents preserved | 299 | **74.8%** |
| `CLEAN_DEFECT` — merged, valid YAML, duplicate list entry | 64 | **16.0%** |
| `CONFLICT` — git refused, loudly | 37 | **9.2%** |
| `CLEAN_INVALID` — merged into unparseable YAML | 0 | **0.0%** |
| `CLEAN_LOST` — merged, but an author's edit vanished | 0 | **0.0%** |

**Verdict: the corruption claim is FALSIFIED. Zero silent integrity failures in 400 trials.**

Targeted cases confirm the mechanism:
- **Adjacent insertions** (both appending at the same point) → git **conflicts loudly**.
  Safe. Costs PR friction, not integrity.
- **Separated insertions** → git merges cleanly and **correctly**, producing exactly the
  set union both authors intended. Git gets the right answer by accident.
- **Multi-line `verified[]` block entries** inserted at different positions → all entries
  preserved, structure intact.

The claim survives only in its weakest form: **16% duplicate entries**. A real example
from the trial —

```yaml
verified:
  - by: human:frank
    at: '2026-01-01T00:00:00Z'
  - by: human:bob
    at: '2026-07-01T00:00:00Z'
  - by: human:bob        # <-- duplicate principal, two timestamps
    at: '2026-03-01T00:00:00Z'
```

That is a genuine defect — a duplicate verification record matters for a trust tier. But it
is **valid YAML, detectable by a five-line lint rule**, and it is not corruption. "Run a
duplicate-key linter in CI" and "build a CRDT write path" are not the same order of
investment.

**The important limitation, which cuts in a useful direction.** Both branches re-serialized
frontmatter through a canonical YAML dumper, so formatting was normalized on both sides.
Hand-edited YAML — irregular indentation, comments, flow style, anchors — would merge
worse. The obvious fix is a pre-commit canonicalizing formatter, making the trial's
conditions the real conditions.

> **Superseded in part by E3.** Building that formatter showed it is materially more
> dangerous than this paragraph assumed. See below — the claim that "a formatter plus a
> duplicate linter captures most of what the CRDT was bought for" survives only for the
> subset of files a formatter can safely touch, which is smaller than expected.

A second limitation biases the *other* way: one edit per author is optimistic for a real
PR, which bundles many. More edits means more overlap — but overlap produces **conflicts**,
which are safe, not corruption. The likely error in this trial is that it understates PR
friction, not that it overstates integrity.

---

---

## E3 — Stage 1 skeleton: what building it actually taught

The walking skeleton (`brain/`, ~700 lines: `new`, `index`, `search`, `export`, `fmt`,
`lint`, `stats`) exists and passes all ten drills, including T1, T2 and T6.

```
PASS  T1  rebuild drill          -- byte-identical across full rebuild
PASS  T1b idempotence            -- second run is a no-op
PASS  T2a raw round-trip         -- byte-identical incl. CRLF, unicode paths, anchors
PASS  T2b semantic round-trip    -- no key or value lost through reserialization
PASS  T2c unknown keys/types     -- unknown type + 2 unknown keys preserved, 2 broken links recorded
PASS  T2d no timestamp coercion  -- all temporal values remain ISO 8601 strings
PASS  T2e fmt safety             -- every rewrite value-preserving; 4 file(s) correctly refused
PASS  T6  service-death drill    -- 25 concepts recovered from files alone
PASS  search smoke / structured filter
```

**T1, T2 and T6 pass. The derivability claim survives first contact.** `index.md`
regenerates byte-identically after deleting every derived artifact; export is byte-exact
through CRLF, unicode paths, YAML anchors and multiline scalars; unknown types and unknown
keys are preserved; broken links are recorded rather than rejected.

But the drills that *earned their keep* were the two that did not exist until the code did.

### E3a — PyYAML silently breaks ISO 8601, everywhere

`yaml.safe_load` applies an implicit timestamp resolver. `at: 2026-01-01T00:00:00Z` becomes
a Python `datetime`, and re-emitting it produces:

```yaml
at: 2026-01-01 00:00:00+00:00     # not ISO 8601 -- silently non-conformant
```

This is exactly the class of failure the Archivist accused a service-backed store of — a
schema layer quietly rewriting values — and it appeared in the *file-first* implementation,
in the default configuration of the most obvious YAML library. It affects `generated.at`,
`verified[].at`, `stale_after`, and `usage_window`: every temporal field the spec defines.

Fix: a loader with the timestamp resolver stripped, so temporal values stay strings.
Guarded by **T2d**, which fails if any parsed value is a `date` or `datetime`.

The general lesson is worth more than the fix: **"we just store markdown" does not mean
"nothing can rewrite our data."** A parser is a schema layer whether or not you call it one.

### E3b — the canonicalizing formatter is not the cheap CRDT substitute E2 claimed

Run against the adversarial fixture, `touchstone fmt` did three unacceptable things:

| Input | Naive formatter output | Damage |
|---|---|---|
| `at: 2026-01-01T00:00:00Z` | `at: 2026-01-01 00:00:00+00:00` | breaks spec conformance |
| `verified: [{<<: *defaults}]` | merge key resolved, **new** `&id001` anchor invented on an unrelated timestamp | authored structure destroyed |
| `script: \|` holding a shell script | single-quoted string with blank lines interleaved | a runbook becomes unreadable |

So the formatter now **refuses** any file containing anchors, aliases, merge keys, or block
scalars, and refuses any rewrite that does not survive a re-parse unchanged (**T2e**). On
the 25-concept fixture it declines 4 files and rewrites 20.

**This narrows E2's conclusion rather than overturning it.** A formatter that only touches
frontmatter it can safely reproduce still delivers the merge benefit for ordinary concepts —
which is most of them. But "50 lines and you don't need a CRDT" was wrong: a *safe*
formatter needs a refusal predicate, a re-parse verification, and a loader fix, and it still
leaves the most structurally interesting files untouched and merging as raw text.

### E3c — a filter that the expansion stage ignores is not a filter

Graph expansion returned a `Note` for a query filtered to `--type Decision`. Structured
filters now bind to the expansion query as well as the BM25 query. Small bug, but it is the
kind that makes retrieval quietly untrustworthy rather than visibly broken.

### E3d — hand-authored frontmatter breaks on contact

The fixture generator produced invalid YAML on its first run: an unquoted `description`
containing `: `. This is the single most likely authoring error in real use, it is invisible
until parse time, and it is the reason `touchstone new` emits through a YAML dumper instead of
string concatenation. **`lint` catching it is not optional — it is the main event for R2.**

---

## Consequences for the architecture

Both results push toward the Archivist, and both **simplify** the design:

| Was | Now |
|---|---|
| Corporate needs RLS inside the query planner (§7) | Retrieve at K≥500 and post-filter. Seam shrinks to repo-level clone permissions |
| CRDT write path justified by frontmatter integrity (§3) | **Not justified on integrity grounds — 0/400 failures.** A *refusing* formatter + duplicate linter instead (E3b: the naive formatter is itself unsafe) |
| CRDT is core infrastructure | CRDT is a **UX** feature — live co-editing latency and presence — worth building when someone feels the pain, not before |
| One design, one pluggable ACL component | One design, and the difference is *distribution* (which repos you can clone), not *query* |

The CRDT is not dead — it is demoted. It remains the right answer for 20 people editing a
runbook at 02:00, which is a latency and presence problem. It is simply not needed to keep
the files valid, which is what it was primarily justified by.

**Revised build order:** the canonicalizing formatter and duplicate linter move into
Stage 1 alongside `touchstone index`. The CRDT moves behind the adoption test — if A3 shows one
person barely writing into the brain, concurrent editing was never the bottleneck.

---

## E4 — the drills against upstream OKF, not our own fixture

FINDINGS' own "Still open" list named this: *"T2 against the upstream sample bundles (`ga4`,
`stackoverflow`, `crypto_bitcoin`) — the fixture is adversarial but self-authored, which is its
weakness."* It had never been run because `tests/drills.py` hard-coded `bundle = root / "_fixture"`.
The four bundles from `GoogleCloudPlatform/knowledge-catalog` are now vendored at `_upstream/` and the
drills take a bundle argument.

**The load-bearing claims survive.** T2a raw round-trip is **byte-identical on all four bundles** —
A2 holds against OKF authored by people who did not know our assumptions. T2b semantic round-trip,
T2d no-timestamp-coercion (E3a's fix holds on real data), T2e fmt safety and T6 service-death pass
everywhere; T1 passes on three of four.

### E4a — a reserved FILENAME silently drops a legitimate concept

`brain/okf.py:21` — `RESERVED = {"index.md", "log.md"}`. `acme_retail/log.md` is a real concept with
`type: Log` and a title. Brain indexes 9 of that bundle's 10 concepts and reports nothing.

This contradicts our own stated rule: *"The spec's tolerance is honored, not narrowed. Unknown `type`
values, unknown frontmatter keys, and broken links are all preserved and indexed."* A reserved
*filename* narrows the spec in exactly the way the design forbids — and the spec authors' own bundle
uses that name for a concept.

**Verdict: a real defect. The reservation must move from the filename to something the spec actually
reserves, or be dropped.**

### E4b — A1 fails on real data

`acme_retail/attesters/` contains only `sql_equality.py` — no markdown. Upstream ships an
`attesters/index.md` for it; T1 deletes generated index files and brain does not regenerate that one,
because the directory holds no concepts. So a real bundle contains a derived artifact brain cannot
reconstruct, which is **A1 — "everything above L1 is reconstructible from L1", listed in the ledger as
design-fatal.**

The honest nuance: this could be called an upstream bug rather than ours. But it is a genuine
disagreement about what OKF permits, surfaced exactly where this document predicted the fixture was
weak. Deciding it is deciding whether we honor the spec's tolerance or our reading of it.

**Verdict: A1 is falsified as stated. Either brain reconstructs index.md for concept-free directories,
or A1 must be narrowed to directories that contain concepts.**

### E4c — the harness was destroying its own evidence

T1 is destructive by design: it deletes every `index.md` and the derived dir to prove the rebuild is
byte-identical. Run in place, it consumes the corpus it is testing. The first E4 run deleted
`acme_retail/attesters/index.md` — brain will not regenerate an index for a concept-free directory, which
is E4b — and that had two consequences:

1. Vendored third-party data was silently modified: six `index.md` files rewritten, one deleted.
2. **The E4b failure is self-erasing.** Once the file is gone it is no longer missing, so a second run
   reports T1 as passing. acme_retail went 6/10 → 7/10 between runs, and the evidence for the defect
   disappeared with it. The bundles vendored in the first commit were already mutated, so E4b was not
   reproducible from this repo until the fix.

The drills now copy the bundle before touching it; the source is read-only as far as they are concerned.
Verified: two consecutive runs of `acme_retail` both report 6/10 with T1 failing, and the bundle is
byte-identical afterwards.

**Worth generalizing: a destructive test that runs in place cannot be trusted the second time.** If the
first run changes the input, a green second run means nothing.

### Not defects — the drills are not yet bundle-portable

Three failures were test artifacts, verified before being reported. `T2c` hard-codes a lookup for
`adversarial/unknown-type.md`, a fixture-only file. Both search drills assume the fixture's vocabulary —
they search for "index" and filter `type: Decision`, while upstream types are BigQuery, Reference,
Metric, Policy, Attested, Skill and Log. Making the drills bundle-portable is follow-up work; until
then a non-fixture run reports three false failures.

---

## E5 — the Rust port against the Python oracle

The Rust workspace reached a runnable `touchstone` binary: domain, ports, six use cases, four secondary
adapters, the CLI adapter and the composition root. First differential run against the Python prototype.

**Parity on `_fixture`** — both index 25 concepts; both export byte-exact, **0 round-trip differences**
either side. A2 holds in both implementations.

**Divergence on `_upstream/acme_retail`, exactly where predicted:**

| | concepts indexed | `log.md` |
|---|---|---|
| Rust | **10 / 10** | indexed |
| Python | 9 / 10 | **dropped** |

E4a said `RESERVED = {"index.md", "log.md"}` narrows the spec in the way the design forbids. The Rust
adapter does not reserve the filename, so a legitimate `type: Log` concept survives. **The port is
correct where the oracle is wrong**, which is the outcome the differential exists to detect — a
disagreement is only useful if you already know which side should win, and FINDINGS recorded that
before the Rust existed.

### E5a — the drills, run both ways

`tests/drills.py` now takes `--impl python|rust`. Against `_fixture`:

| | python | rust |
|---|---|---|
| T1 rebuild / T1b idempotence | PASS | PASS |
| T2a raw round-trip | PASS | PASS |
| T2c unknown keys + types | PASS | PASS |
| T6 service-death | PASS | PASS (25 recovered) |
| search smoke / structured filter | PASS | PASS |
| **T2b, T2d, T2e** | PASS | **N/A** |

The three N/A drills import the Python `okf` module directly — they test a *library API*, not the CLI,
so there is nothing to route to a binary. That is a property of the drill, not a gap in the port. Each
is covered by a Rust unit test instead:

| drill | Rust equivalent |
|---|---|
| T2b semantic round-trip | `parse::tests::format_roundtrip`, plus E5's byte-exact export |
| T2d no timestamp coercion | `parse_temporal_stays_string`, `temporal_values_stay_iso8601_strings`, `combined_unknown_key_unknown_type_and_iso8601_timestamp` |
| T2e fmt safety | `is_risky()` (anchors / merge keys / block scalars) + `format_skips_risky_constructs`, `format_skips_when_reserialize_changes_values` |

Recording the mapping because the bare "N/A" reads like missing coverage — it isn't, and a first pass at
checking it produced a false alarm from a badly-chosen grep.

**Remaining before the Python prototype can be deleted:** nothing functional. It stays only as long as
it is useful as a second opinion; T2b/T2d/T2e would need expressing through the CLI to be checked
identically against both, which is the only reason to keep it after that.

## E6 — the oracle is deleted, and the drills outlive it

The condition E5a named was met: T2b, T2d and T2e are now expressed through the CLI, so nothing about
conformance depends on being able to call a Python function. The prototype was deleted — 1,417 lines
across `touchstone/`, `tests/drills.py`, and `tests/make_fixture.py`.

Deleting an implementation is easy. Deleting one **without losing what it taught you** is the part
worth recording, because the oracle was carrying four distinct things and only one of them was code.

| what the oracle carried | where it went |
|---|---|
| the ten drills | `touchstone-conformance`, driving the binary as a black box |
| a second opinion on ambiguous bytes | gone, deliberately — see below |
| E4a (`log.md` is a concept, not a reserved name) | an assertion, `touchstone-conformance/tests/fixture.rs` |
| E4b (A1 fails on concept-free directories) | a recorded known-defect, `touchstone-conformance/tests/drills.rs` |

**The second opinion is the real loss, and it is worth being honest about it.** A differential test
answers "do two independent readings of these bytes agree?", which is a question no single
implementation can ask itself. What replaces it is weaker in one way and stronger in another: the
drills now assert each property *directly* rather than by comparison, so they still catch a
regression, but they can no longer surface a disagreement nobody thought to look for. E3a and E4a
were both found that way. Nothing in the current suite would have found them.

What partially compensates: `touchstone-conformance` names no `touchstone-*` crate — enforced as architecture rule
7 — so it drives the binary rather than the library. `TOUCHSTONE_BIN=… cargo test -p touchstone-conformance`
holds *any* implementation to the same drills. The differential could compare two implementations;
this can gate arbitrarily many, including ones that do not exist yet. That is a fair trade for a
project whose stated asset is the format rather than the code, but it is a trade, not a free win.

**On E4b.** The suite records it as an expected failure rather than fixing it, which needs
justifying, because an expected-failure list is one careless commit away from being an ignore-list.
Two things keep it honest: the failure must still reproduce *in the recorded shape* (a different T1
failure on `acme_retail` fails the gate), and if it ever **stops** reproducing the gate fails too,
on the grounds that FINDINGS would then be asserting something false. It is not fixed because
fixing it means deciding what `index.md` should contain for a directory holding no concepts — a
reading of the spec, not a bug fix, and one nobody has made yet.

**One thing the port did not carry over, noticed while writing the trust drill:** `Trust::Attested`
exists in `touchstone-domain` and is never derived by anything. Both implementations only ever produced
`human` / `machine` / `unattributed`, so this is not a port defect — it is a documented tier
(`verified` > `attested` > `generated`) with no derivation rule behind it. Left alone; recorded here
because a trust tier that cannot be reached is either dead code or a missing rule, and which one it
is has not been decided.

## E7 — E4b is the media case, and export dropped every artifact

Building the media drills (`touchstone-conformance/tests/media.rs`) to answer "can this
attach knowledge to photos, PDFs and files?" surfaced two defects, one of them worse than
the question that prompted it.

**M1 — `export` carried no artifacts at all.** `export_bundle` iterated
`ConceptRepository::paths()`, which by design returns only `.md`. So the command the README
calls *"leaving is a supported operation"* and *"byte-exact by construction"* silently
dropped every PNG, PDF, recording and script in the bundle. The markdown was byte-exact; the
bundle was not. Portability is this project's central claim and it held only for prose.

Fixed: `ConceptRepository` gained `artifact_paths()` — deliberately **not** defaulted, since a
default returning empty would let an implementer forget it and still compile, which is how
the defect existed in the first place. Export now carries concepts *and* artifacts.

**M3 — E4b is not an A1 technicality.** `_media/orphan/attester.sql` is described only by a
hand-written `index.md` in a directory holding no concepts. `index` deletes every `index.md`
as generated and cannot regenerate that one, so the artifact survives and the knowledge about
it does not. That is the same shape as `_upstream/acme_retail/attesters/sql_equality.py`.

E4b has been carried all session as an abstract falsification of A1. It is not abstract: it
is the **first instance of attaching knowledge to a file, and it loses the knowledge**. The
bytes stay on disk; what is lost is what the file is, who verified it, and when it goes
stale. An artifact nobody can account for is not an asset.

That reframing does not decide it — either `index` reconstructs concept-free directories, or
A1 narrows to directories that contain concepts — but it changes the stakes from a spec
nicety to a data-loss path in the use case that motivated the question.

Recorded as an XFAIL asserting the *exact* known set, so a different orphan fails and so does
an empty one: a defect that quietly stops reproducing means this file is asserting something
false.

**What passed matters too.** M4: an artifact is findable by what it is *about* — "on-call
escalation procedure" surfaces `handbook.pdf` — not by filename. M5: a signed PDF reads as
`human` while an unreviewed screen recording reads as `unattributed`. Those two are the
difference between this and a filesystem with good names.

## E8 — E4b resolved: A1 narrows to what touchstone generates

E4b said A1 — "everything above the bundle is derived and disposable" — was falsified by
`acme_retail/attesters/`, which holds only `sql_equality.py` and a hand-written `index.md`
describing it. Rebuild deleted the index and could not regenerate it.

The adjudication offered two horns: either `index` reconstructs concept-free directories, or
A1 narrows to directories that contain concepts. **The second is correct, and the reason is
not a preference.** Touchstone cannot generate an index for a directory holding no concepts —
there is nothing to list. So that file was never derived. The drill was deleting *authored
content* and then reporting the loss as a defect in the implementation.

A1, restated: **everything touchstone generates is derived and disposable.** An `index.md` in
a concept-free directory is authored knowledge — frequently the only description of an
artifact — and is out of scope for the claim.

Implemented as `Bundle::generated_index_files()`: the indexes touchstone actually produces
are those in a directory containing a concept, or an ancestor of one. T1 and T1b compare that
set, and `destroy_derived` removes only that set.

A rejected alternative worth recording, because it looked right: keying off the generation
banner (`<!-- generated by touchstone index -->`). Upstream bundles carry `index.md` files
generated by *their* tooling, which does not write our banner — so a banner test would have
made touchstone skip every upstream index and turned T1 into a vacuous pass. The narrowing
was checked against that failure mode rather than merely adopted.

**The gate would not let this close quietly.** Fixing the behaviour made the recorded defect
stop reproducing, and the KNOWN_DEFECTS mechanism failed the build with "recorded defect no
longer reproduces -- FINDINGS.md must be updated to close it." The rule that a disappearing
defect is also a regression did its job on its author.

Drill M3 needed correcting too: it counted only concepts as describing documents, so it
reported the artifact orphaned while the file describing it sat on disk. It now counts any
surviving markdown that touchstone did not generate.

## E9 — A5 measured at last: 50k concepts index in 17 minutes

Unmeasured since the beginning, and listed "corpus generated, unmeasured" in every status
table. Cold index of `_scale`:

| | |
|---|---|
| concepts | 50,000 |
| cold index | **1,028 s (17 min)** — see E10, now 11.85 s |
| index size | 248 MB |
| query latency | 0.26 s |

Query is fine. Indexing is not, and the split says why: **user 321 s, sys 606 s.** Twice as
much time in the kernel as in our code means this is syscall-bound, not compute-bound — the
indexer issues roughly five separate `execute()` calls per concept, each its own implicit
transaction and therefore its own commit. Batching the run into one transaction is the
indicated fix, and it is a small change to one adapter.

Worth setting against RUST-PATH §4, which declined to assert a number and said performance
"is not the deciding input". It still is not — but the number is now known, and it lands on
exactly the paths that document predicted would hurt: the filesystem watcher and pre-commit.

## E10 — the 17 minutes was one word in a schema

A5 came back at 1,028 s for 50k and the obvious reading was write amplification: five
statements per concept, each its own implicit transaction. Batching them cut 5k from 25.4 s
to 7.5 s — a 3.4x win that made the diagnosis look confirmed.

It was not. At 50k the same change gave **11%**. Worse, wrapping the whole run in a single
transaction was actively harmful: nothing checkpoints until the end, so `index.db` stayed at
0 bytes while `index.db-wal` passed 216 MB and every upsert's conflict lookup searched an
ever-growing WAL. The fix had to become *periodic* commits, which bounded the WAL and
recovered the 11%.

The real cause only showed up in the scaling curve:

| concepts | before | after |
|---|---|---|
| 2,000 | 0.6 ms/concept | 0.34 |
| 5,000 | 1.5 | 0.25 |
| 10,000 | 3.3 | 0.28 |
| 20,000 | 6.3 | 0.39 |

Per-concept cost doubling as the corpus doubles is **quadratic** — ten times the concepts
costing a hundred times the time. Quadratic is an algorithm property, and no storage engine
fixes one; it only moves the constant.

The cause was one word. The FTS5 schema declares `path UNINDEXED`, which means exactly that:
no index. So `DELETE FROM fts WHERE path=?`, issued once per concept on re-index, was a **full
scan of the entire FTS table**. Deleting by `rowid` — which is indexed — makes it logarithmic.

| | before | after |
|---|---|---|
| 50k cold index | 1,028 s | **11.85 s** |
| 50k query | 0.26 s | 0.11 s |

**87x, and the curve is flat.** A5 is answered: this holds at 50k.

Three things worth keeping from how this went wrong:

1. **5k was too small to contain the bug.** The 3.4x from batching was real and led nowhere.
   Extrapolating a fix from a corpus an order of magnitude below the target is how you end up
   optimising the wrong thing with data to back you.
2. **The obvious diagnosis fitted the evidence and was still wrong.** `sys` at twice `user`
   genuinely did indicate syscall pressure — it just was not the dominant term. A measurement
   consistent with a hypothesis is not the same as a measurement that discriminates between
   hypotheses. The scaling *curve* discriminated; the single data point did not.
3. **The engine question was a red herring, and nearly a costly one.** Columnar and OLAP
   engines were raised twice while this looked like a storage problem. Either would have been
   a large migration that left the quadratic in place.

Regression tests assert the *shape* rather than a timing, since a wall-clock assertion would
be flaky on shared CI: re-indexing must leave exactly one FTS row per concept, FTS rowids must
track their concept, and `remove` must take the FTS row with it. A stale row is the symptom
that forced the scan.

## E11 — A10 run, with heavy caveats: baseline 11/20, touchstone 18/20

The pre-registered kill criterion — *if ≥15 of 20 questions are answered acceptably by
`rg` + Obsidian, stop; the architecture is unjustified* — **is not met**.

| arm | score |
|---|---|
| `rg` alone, no ranking | 4 / 20 |
| `rg` + term-overlap ranking (a stand-in for Obsidian) | **11 / 20** |
| touchstone | **18 / 20** |

The first baseline was too weak to be honest: `rg -il` returns files in directory order, so
taking the top 5 of an unranked list punishes it for something a human with Obsidian does not
suffer. Ranking each file by how many distinct query terms it contains lifts it from 4 to 11 —
and 11 is the number the verdict should rest on. Reported both, because quoting only the 4
would have inflated the result by a factor of two.

**What this is not.** Every caveat below is a reason to treat this as a pilot:

- **The questions and the answer key were written by Claude**, at the user's instruction, over
  a corpus Claude partly wrote. The bias runs toward touchstone.
- **The scorer knows the corpus**, which inflates any judgement of "answered acceptably".
- **"Answered" is a proxy** — an answer document in the arm's top 5 — not the human judgement
  the protocol asks for.
- **27 concepts.** PROTOTYPE.md warns that a corpus this thin produces absence-dominated
  failures and cannot settle A10.
- The ranked baseline is an *approximation* of Obsidian, not Obsidian.

Mitigations that make the comparison worth something: the bulk of the corpus
(DECISIONS, PROTOTYPE, VERSION-CONTROL, RUST-PATH, ADR-2608010900..0960) predates the session;
the answer key was committed **before either arm ran**; and both arms faced identical
questions, so the *comparison* holds even where the absolute number does not.

**Where the gap comes from.** The baseline's failures are precision, not recall: with six
OR-ed terms it matches most of a 27-document corpus and cannot order them, so the answer is
present but buried. That is the *structure* failure the rubric predicts — questions like
"which decisions are still proposed?" need a field, and grep cannot tell a `Status:` line from
a mention of the word. Touchstone's two misses (Q3 parser-as-port, Q20 broken links) were both
cases where the answer document ranked sixth or lower.

**This does not substitute for the real A10.** Twenty questions the user actually wanted
answered, over their own notes, remains the experiment that decides. What this run establishes
is narrower and still useful: on a real corpus, with a fair baseline, the null hypothesis did
not hold.

## Still open

- **The trust invariant has no enforcement mechanism.** Signed commits and a CI gate are
  specified (ADR-2608010930) and neither is built: no `.github/workflows`, and
  `VersionControl::attest` has no caller. `export` carries no signature either, so a shared
  bundle's `verified` claims are uncheckable by design-so-far. Highest-value open item.
- **Revocation** — unaddressed by any experiment, and *not* a git problem: no substrate that
  distributes the bytes can retract them, a CRDT least of all. Crypto-shredding is the only
  mechanism that changes it. The corporate boundary is justified by prefilter enforcement
  (E1), not by revocation — an earlier reading of this file said otherwise and was wrong.
- **A10** — a caveated pilot run (E11) put the baseline at 11/20 against the 15/20 kill
  threshold, and touchstone at 18/20. Questions written by Claude over a corpus it partly
  wrote; the real experiment needs the user's questions over the user's notes.
- **A3** (will anyone write into it) — untouched. Needs three weeks, and is now the single
  largest unanswered risk.
- **A6 on real embeddings with ANN** — before deleting the corporate seam for good.
- ~~**T2 against the upstream sample bundles**~~ — run; see E4. A2 survives; A1 does not.
- ~~**E4b**~~ — resolved; see E8. A1 narrows to what touchstone generates.
- ~~**Index throughput**~~ — fixed; see E10. 50k in 11.85 s, curve flat.
- **`Trust::Attested` is unreachable** — a documented tier with no derivation rule (E6).
- **Scale** — the fixture is 25 concepts. Nothing here says anything about 50k.
