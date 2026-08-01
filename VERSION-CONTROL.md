# Should we replace git with our own version control?

## The question is really two questions

Git is doing two unrelated jobs in this architecture, and they want opposite things:

| Layer | What it needs | Is git good at it? |
|---|---|---|
| **Sync** — getting an edit from a laptop to a phone to a server, offline, without conflicts | convergence, partial replication, low latency, mobile | **No. Badly no.** |
| **History & attestation** — what changed, who verified it, can it be proven | durability, signatures, review, audit | **Yes. Excellently.** |

"Eliminate git" reads as a single decision but only the first row justifies it. Replacing
git in the second row is a large liability for a small gain, and it costs the one property
that makes a curated brain different from a pile of plausible text.

## Where git genuinely fails

These are real and not fixable with better tooling:

1. **Multi-device sync is not git's model.** Git is a history tool that happens to
   distribute. Making it a live sync engine across laptop/phone/web means fetch-merge-push
   loops with conflict handling in the UX — the thing [heaper.de](https://heaper.de) exists
   to avoid.
2. **Mobile.** Git on iOS/Android is bad. This matters more than it looks: **A3 says the
   project dies if capture is hard, and the FINDINGS threshold is ~20 seconds from "I want
   to record this" to "it's saved."** A git round-trip on a phone does not clear that bar.
3. **Clone is all-or-nothing.** 50k concepts plus five years of history onto a phone is
   the wrong shape. Git has partial clone, but it is an operational sharp edge, not a
   default.
4. **Offline concurrent edits** converge via merge, not automatically. E2 showed git's
   merge is *safe* for OKF frontmatter (0/400 silent failures), but "safe" means it
   conflicts rather than corrupts — and a conflict on a phone is a dead end.

## Where replacing git is expensive

What you would have to build, that you currently get free:

1. **Signing — the big one.** The architecture's load-bearing invariant is that an agent
   may never write `verified: {by: human:...}`, enforced because *in a database row that is
   a claim; in git it is a signed commit*. Automerge and Loro changes carry actor IDs, not
   signatures. Eliminating git means writing and owning the crypto that makes trust tiers
   trustworthy. Git gives this away for free now that SSH-key commit signing exists.
2. **Review.** "You cannot open a pull request against a row" was an argument *for* files.
   It applies to a custom CRDT store just as hard. CRDTs are built so everything converges
   automatically — "proposed, not yet accepted" is against their grain, and you would be
   building branch semantics that Automerge does not natively shape for review.
3. **The ecosystem.** GitHub renders an OKF bundle for free today — which is a stated
   design goal, not a nicety. Diff, blame, bisect, hosting, backup, every editor
   integration, and every auditor who already knows how to read a git log.
4. **The founding bet.** The Archivist's thesis — the one that selected OKF in the first
   place — is *knowledge outlives infrastructure, so storage must be the most boring,
   most portable thing available.* A bespoke version control system is the least boring
   component you could introduce. R5 is directly threatened.

## The asymmetry that resolves it

**The files are the source of truth. Version history is not.**

If every CRDT checkpoint materializes plain OKF markdown to disk, then losing the entire
version history costs you *history* — not *knowledge*. That is a completely different risk
profile from losing the brain, and it is what makes replacing the sync layer survivable
while replacing the attestation layer is not.

## Three options

### A. Keep git, demote it to an attestation sink *(recommended)*

CRDT becomes the write path and the sync engine — which [ARCHITECTURE.md](ARCHITECTURE.md)
§3 already says. Git stops being the *interface* and becomes an append-only, signed,
materialized snapshot stream that no human hand-edits. Nobody resolves a merge conflict on
a phone; nobody types `git` to capture a note.

You keep: signed attestation, review where it is wanted, GitHub rendering, and a bundle any
OKF consumer can read. You drop: git as user experience.

If you later decide git earns nothing, you delete it and lose *audit*, not the brain.

### B. Embed git as a library, not a CLI

Most complaints about git are about the porcelain, not the object model. `gitoxide` (Rust)
or `libgit2` let you drive the object database directly — commits, signatures, history —
without ever showing a user a conflict marker. This is option A with the seams hidden, and
it is the option that most rewards a Rust rewrite.

### C. Full custom version control on Automerge/Loro

Coherent, and it is what heaper does. Automerge's change history is already a
content-addressed hash-linked DAG — structurally the same idea as git. You would be
building: change signing, review semantics, hosting, backup, and every tool.

Choose this only if you conclude that **review and signed attestation are not
requirements** — because those are precisely what you would be reimplementing worst.

## What decides it

One question, and it is not a technical one:

> **Does this brain need "proposed, reviewed, then accepted" — or only "written, synced,
> and attributable"?**

- **Corporate brain** → review is the whole point. A runbook silently edited to drop a
  rollback step, with no reviewer and no revert, is the failure the design exists to
  prevent. **Keep git.**
- **Purely personal brain** → review is ceremony you will never perform. Signing still
  matters if agents write into it, but a signed CRDT change would do. **C becomes
  defensible.**

Since R1 requires **one** architecture for both, and the corporate case needs review,
option A is the only one that serves both without maintaining two systems.

## Recommended sequencing

This is a Stage 5 decision and we have not passed the Stage 1 gates. Concretely:

1. **A10 and A3 first.** If nobody writes into the brain, the version control layer is
   irrelevant. Investing in custom VC before adoption is proven is the single most
   expensive way to be wrong.
2. **Make the write path git-free at the UX level** (option A). Capture goes through
   `brain new` and a CRDT checkpoint; git is a background sink. This gets essentially all
   of the mobile/latency benefit at a fraction of the cost.
3. **Measure what git actually costs at scale** before assuming it is the bottleneck —
   the 50k corpus is on disk and unmeasured.
4. **Revisit C only if** step 2 leaves real pain *and* you have concluded review is not a
   requirement.

## Kill criterion

Option A fails, and C becomes justified, if: after the write path is git-free at the UX
level, **git checkpointing still exceeds ~1s on the 50k corpus on the slowest target
device, or mobile capture still cannot clear the 20-second bar.** Measure before
committing to a rewrite.

## Note on the language question

The two questions are connected, and this is the important part:

**Performance is not a good reason to leave Python. Building your own version control is.**

Automerge, Loro, and y-crdt are all Rust; `gitoxide` is Rust. If you take option B or C,
Rust stops being a preference and becomes the path of least resistance — you would be
binding to Rust libraries from Python otherwise. If you take option A and keep the CRDT
layer thin, Python remains adequate and the rewrite is unjustified until a measurement says
otherwise.

Also worth weighing: a single static binary is a genuine advantage for R2 (a `.venv` is
friction a user feels every day), and a long-running filesystem watcher is work Python is
poorly suited to. Neither is urgent; both point the same direction if a rewrite happens
anyway.
