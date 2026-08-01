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

## Still open

- **Revocation** — unaddressed by any experiment, and the surviving argument for a
  corporate boundary.
- **A10 / A3** (does it beat grep; will anyone write into it) — need the real corpus and
  three weeks. Unchanged, and now the **only** things standing between this and a verdict.
- **A6 on real embeddings with ANN** — before deleting the corporate seam for good.
- ~~**T2 against the upstream sample bundles**~~ — run; see E4. A2 survives; A1 does not.
- **Scale** — the fixture is 25 concepts. Nothing here says anything about 50k.
