# A10 — the null hypothesis

**The question:** does this architecture justify existing, or would `rg` + Obsidian over your
existing notes already do the job?

A10 is listed **project-fatal** and **first** in [PROTOTYPE.md](../PROTOTYPE.md). It is the
experiment nobody runs, because it is the one that can tell you to stop. Everything green in
`tests/verify.sh` proves the implementation is *correct*; none of it proves it is *useful*.
Those are different claims and only this one is load-bearing.

## The pre-registered kill criterion

> If **≥15 of 20** questions are answered acceptably by `rg` + Obsidian, **stop**. Build a
> frontmatter linter and a `touchstone new` template and go home. The rest of this
> architecture is unjustified.

That threshold was written before the code existed. It is not adjustable after seeing
results — that is what "pre-registered" means, and moving it is the single most likely way
this experiment gets quietly wasted.

## Two phases, in this order

**Phase 1 — the baseline.** Answer the 20 questions with `rg` + Obsidian over your existing
notes. This is the null hypothesis, and it runs *first* precisely because a pass means the
rest was unnecessary. Record for each question: answerable, how long, and — if it failed —
which of three modes:

| Mode | Meaning | Implication |
|---|---|---|
| **recall** | the note existed, grep missed it (wrong words) | vectors would help |
| **structure** | you needed "all decisions about X, still current" and grep cannot express `type` or `stale_after` | **frontmatter is the whole value — this is OKF's case** |
| **absence** | the note was never written | **no architecture fixes this; that is A3's problem** |

The *distribution* of failures matters more than the count. Failures clustering on
**structure** are the strongest possible evidence for this design. Failures clustering on
**absence** mean the corpus is too thin and A10 is premature — fix that before continuing.

**Phase 2 — the contender.** Only if Phase 1 fails to reach 15/20. Answer the same questions
with `touchstone search` against the same corpus converted to a bundle, and compare against
the baseline you already recorded. Phase 1's numbers are the benchmark; they are not
re-derived here.

## Corpus rules

**Use your real notes.** Not `_fixture`, not `_scale`. PROTOTYPE.md §2 is explicit:

> Do not measure search quality on synthetic text. Generated corpora have unrealistic term
> distributions and uniform link topology; retrieval that looks excellent on them regularly
> collapses on real notes.

`_fixture` is worse than synthetic for this purpose — it is 25 concepts written to satisfy
the conformance drills, so measuring retrieval on it grades the system against a corpus built
to make it look good. `run.sh` refuses both.

## Integrity

Three rules, enforced by the scripts rather than by good intentions:

1. **Questions are written before any search.** `run.sh` records the SHA-256 of
   `questions.md` into every result sheet, and `score.sh` refuses to score a sheet whose hash
   no longer matches. Editing a question after seeing results invalidates the run, loudly.
2. **Scoring is human.** Whether an answer was *usable* is a judgment. The harness collects
   evidence and structures the record; it never fills in a verdict. Unscored rows are counted
   and reported, never assumed.
3. **The threshold is not a variable.** It is a constant in `score.sh` with this file cited
   next to it.

## Running it

```bash
cp a10/questions.template.md a10/questions.md   # write 20 questions, then commit them
git add a10/questions.md && git commit -m "a10: pre-register 20 questions"

a10/run.sh baseline ~/notes                     # phase 1 -- collects rg evidence
$EDITOR a10/results-baseline.tsv                # score by hand
a10/score.sh a10/results-baseline.tsv

# only if the baseline scored below 15/20:
a10/run.sh touchstone ~/brain
$EDITOR a10/results-touchstone.tsv
a10/score.sh a10/results-touchstone.tsv
```

Commit the questions *before* running anything. That single act is what makes the result
evidence rather than an anecdote.
