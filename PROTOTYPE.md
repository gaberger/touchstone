# Prototype Plan — Testing the Assumptions

A prototype here is not a demo. A demo is built to work; this is built to **fail fast on
the assumptions that would waste the most effort if discovered late.** Every stage below
has a pre-registered kill criterion, written before the code exists, because a threshold
chosen after seeing the number is not a test.

## 0. The assumption ledger

`DECISIONS.md` lists six tests, and five of them are about scale. Scale is not the most
likely killer. Ranked by *(probability wrong × cost of learning late) ÷ cost to test*:

| # | Assumption | If wrong | Cost to test | When |
|---|---|---|---|---|
| **A10** | This beats `rg` + Obsidian by enough to justify existing | **Project-fatal** | Hours | **First** |
| **A3** | R2 — a human will actually write into it unprompted | **Project-fatal** | Cheap, but needs 2–3 weeks elapsed | Starts week 1, reads out week 3 |
| **A1** | Everything above L1 is reconstructible from L1 | Design-fatal — the core claim | ~1 day | Stage 1 |
| **A2** | Round-trip is byte-lossless incl. unknown keys | Design-fatal — "portable" becomes a slogan | ~1 day | Stage 1 |
| **A6** | Authz must prefilter; post-filter destroys recall | Decides whether the corp seam exists | ~1 day, **no prototype needed** | Now |
| **A4** | Hybrid retrieval beats lexical alone on a real corpus | Vectors are unjustified complexity | ~2 days | Stage 3 |
| **A8** | Trust tiers (`verified` > `generated`) improve results | A whole frontmatter family is decoration | ~1 day | Stage 3 |
| **A5** | Latency holds at 100k–1M concepts | Adds ANN; changes nothing structural | Expensive — needs corpus | Stage 4 |
| **A7** | CRDT write path loses no updates, corrupts no YAML | Rebuild the write path | Expensive | Stage 5 |
| **A9** | Capture agents don't pollute the brain faster than curation cleans | Agents become net-negative | Weeks | Stage 5 |

**A10 and A3 are the two nobody ever tests, and the two that kill knowledge-base projects.**
A5 and A7 — the ones the adversaries spent their energy on — are the *least* urgent, because
being wrong about them changes an implementation, not the design.

## 1. A10 — the null hypothesis, first

Before building anything, establish the control. The honest baseline is:

```bash
mkdir brain && cd brain   # markdown files, no frontmatter discipline, no tooling
rg -i "search term"       # plus Obsidian for links and backlinks
```

Take 20 real questions you'd want your brain to answer. Answer them with `rg` + Obsidian
over your existing notes. Record: how many were answerable, how long each took, and where
it failed — *and why*. Failure modes worth separating:

- **Recall failure** — the note existed, grep didn't surface it (wrong words). → vectors help.
- **Structure failure** — you needed "all decisions about X, still current" and grep can't
  express `type` or `stale_after`. → **frontmatter is the whole value; that's OKF's case.**
- **Absence** — the note was never written. → **no architecture fixes this. A3 is your problem.**

**Kill criterion (pre-registered):** if ≥15 of 20 questions are answered acceptably by
`rg` + Obsidian, stop. Build a frontmatter linter and a `brain new` template and go home.
The rest of this architecture is unjustified.

If the failures cluster on *structure*, that is the strongest possible evidence for the OKF
profile, and you now have a benchmark the real system must beat.

## 2. The corpus problem — blocking, solve before Stage 1

Nothing below runs without concepts. Three sources, each testing something different, and
they are **not** interchangeable:

| Corpus | Source | Tests | Do not use for |
|---|---|---|---|
| **Foreign** | Upstream sample bundles: `okf/bundles/{ga4,stackoverflow,crypto_bitcoin}/` | A2 round-trip — authored by someone else against the real spec, so it will contain conventions we didn't anticipate. This is the *point*. | Search quality — it's dataset metadata, not a brain |
| **Real** | Your actual notes/docs, converted | A10, A3, A4, A8 | Scale |
| **Synthetic** | Generator: count, size, link density, frontmatter adversarialism | A1, A5, A6, A7 | **Search quality — see below** |

**Do not measure search quality on synthetic text.** Generated corpora have unrealistic term
distributions and uniform link topology; retrieval that looks excellent on them regularly
collapses on real notes, where vocabulary is idiosyncratic and link density is
power-law. Synthetic is for load and correctness, never for relevance.

The foreign bundles are the highest-value cheap asset here: they are real OKF written
without knowledge of our assumptions.

## 3. Stage 1 — walking skeleton (~1–2 days est.)

Smallest thing that tests A1 and A2. Roughly 300 lines. **No** CRDT, **no** ACL, **no**
vectors, **no** agents, **no** server.

```
brain new <type> <title>    # scaffold conformant frontmatter
brain index                 # parse → SQLite (concepts, FTS5, edges) → regenerate index.md
brain search <query>        # structured filter + BM25, returns paths
brain export <dir>          # write every concept back out
```

**T1 — rebuild drill.** The core claim of the whole architecture.

```bash
rm -rf .brain/ && find . -name index.md -delete && brain index
git diff --exit-code
```

*Kill criterion:* any generated `index.md` differing by one byte, or any query in the
golden set returning a different ranked top-10. If L3 is not perfectly reconstructible,
"derived and disposable" is rhetoric and files-as-truth was the wrong call.

**T2 — round-trip fidelity.** Ingest → export → `git diff --exit-code`, against:
1. The three foreign bundles, unmodified.
2. ~200 hand-built adversarial concepts: unknown `type` values, three custom keys,
   multi-entry `verified[]` chains, YAML anchors, multiline scalars, CRLF, unicode paths,
   comments between frontmatter keys, and 40 deliberately broken links.

*Kill criterion:* a single non-identical byte, or any dropped unknown key. The spec
**requires** unknown keys and broken links survive; failing this means we are not an OKF
consumer, we are a lookalike.

This is also the exact test that would have caught the Operator's silent-truncation
failure mode — worth running against any future service-backed store before trusting it.

**T6 — service-death drill.** Delete every derived artifact and the tooling. Point the
upstream `reference_agent` CLI (`enrich`, `visualize`) at the bare bundle. *Kill criterion:*
concept count differs by >0, or the upstream tool can't read it. This proves portability is
real and not a claim — and it costs an afternoon.

## 4. Stage 2 — A3, the adoption test (3 weeks elapsed, ~0 build)

Runs concurrently with everything else. Convert your real notes, then **use it as your
only brain for three weeks.**

The measurement is not a survey. It is: **did you write into it on days nobody asked you
to?** Instrument `brain new` and count concepts created per day, split by whether you were
actively working on this project that day.

*Kill criterion:* if concept-creation rate on non-project days trends to zero by week three,
editing is not easy enough, and no amount of retrieval quality will save it. A brain nobody
writes to is an empty database with good latency.

Secondary signals worth logging, because they're cheap and diagnostic:
- Time from "I want to record this" to "it's saved." Over ~20 seconds and capture dies.
- How often you hand-edit `index.md` or fight the frontmatter. Either means R2 is failing.
- Whether you use standard markdown links or reflexively type `[[wikilinks]]`. Wikilinks
  are **not** OKF; if the habit is unbreakable, that needs a lint-and-rewrite step, and it's
  better to learn that now than after 5,000 notes.

## 5. Tests that need no prototype at all — run this week

Two of the most decision-relevant experiments require none of the above:

**A6 — the ACL prefilter question (decides §7 of the architecture).** Pure simulation. Take
any embedding index over any 100k+ document corpus. Assign 400 synthetic principals,
each entitled to 2–8% of documents at random. For 500 queries, compare:
- **prefilter:** restrict the candidate set, *then* search
- **post-filter:** retrieve top-100, *then* drop unentitled

Measure recall@10 against exhaustive exact search. *Falsifies the Operator's OPEN-5 win —
and removes the corporate service entirely — if post-filter reaches ≥0.95× prefilter recall
at equal p95.* An afternoon's work that could delete a whole subsystem from the design.

**The YAML merge-corruption claim.** No CRDT needed. Create a branch pair where each adds a
tag to the same concept's `tags:` list, and another where each appends to `verified:`.
Merge with git's default driver. *If git produces a clean merge with a duplicated or
malformed list and no conflict marker, the CRDT write path is justified.* If git conflicts
loudly every time, the strongest argument for the CRDT weakens considerably, and the write
path could stay plain git for far longer.

Both are cheap enough that not running them before writing code is indefensible.

## 6. Stage 3 — retrieval quality (A4, A8) — after Stage 2 has real content

**Build the golden set before tuning anything.** 100+ real questions with human-judged
relevant concepts, frozen. A golden set built after you've seen the results measures how
well you tuned to it, not how well it works.

- **A4:** lexical-only vs lexical+vector+RRF, on the *real* corpus. *If hybrid doesn't beat
  BM25 alone by a meaningful margin on nDCG@10, vectors are unjustified complexity* — drop
  them, and the embedding cost question, the debounce logic, and the model-drift rebuild
  problem all disappear with them.
- **A8:** rank with and without the trust-tier boost. *If it doesn't measurably improve
  results, the `verified`/`generated` distinction is governance-only* — still worth keeping
  for audit, but it should stop influencing ranking, which is a simpler system.

## 7. Stage 4 — scale (A5), only once 1–3 pass

Synthetic ramp: 1k → 10k → 100k → 250k → 1M concepts. **Find the cliff; don't assert it.**
The architecture currently *estimates* brute-force vector search dies around 1–2M — that
number is unverified and should be treated as a hypothesis to break, not a fact.

Record p95 for structured, lexical, vector, and full hybrid separately at each size, plus
cold rebuild time. Being wrong here adds an ANN index and changes nothing structural, which
is exactly why it waits.

## 8. Stage 5 — CRDT and agents (A7, A9), last

Deliberately last: the CRDT is **the first component that can compromise the files**, and it
should not exist until reconstruction (T1/T2/T6) is proven and the merge-corruption test of
§5 has justified it.

*A7:* 40 simulated editors, 8 hours, Zipf-distributed over the corpus, ~15% collision on hot
concepts. *Kill criterion:* any lost update, or any YAML-invalid-after-clean-merge event.

*A9:* run capture agents for two weeks against real input. Measure the ratio of concepts
created to concepts a human later verified or deleted. *If curation can't keep up with
capture, agents are net-negative* and should propose into a queue rather than write.

## 9. How not to fool yourself

- **Pre-register every kill criterion.** A threshold set after seeing the number is not a test.
- **Keep the null hypothesis alive.** Re-run the §1 grep baseline at every stage. The system
  must keep beating `rg`, not just beat its own previous version.
- **Never tune on the golden set**, and never measure relevance on synthetic text.
- **Report what failed.** A prototype that only produces confirmations wasn't testing anything.

## 10. Decision tree

| Result | Consequence |
|---|---|
| A10 fails (grep is fine) | **Stop.** Ship a linter and a template. |
| A3 fails (nobody writes) | Stop building retrieval. The problem is capture, not search. |
| A1 or A2 fails | Files-as-truth is not achievable with this tooling — revisit the Operator's position seriously. |
| A6 falsified (post-filter fine) | **Delete the corporate service seam.** One design serves both, fully. |
| A4 fails (hybrid ≈ lexical) | Drop vectors. Embedding cost, debounce, and model drift all vanish. |
| A8 fails | Trust tiers stay for audit, stop affecting ranking. |
| A5 fails early | Add ANN. Structural design unaffected. |
| A7 fails | Write path reverts to git + PR; accept the latency, revisit CRDT later. |

The first four rows are the ones worth knowing within a month. The last four are engineering.
