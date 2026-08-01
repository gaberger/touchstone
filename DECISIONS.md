# Adjudication Record — OKF Second Brain

Two adversarial reviewers were run against `ARCHITECTURE.md` (draft v0):

- **THE ARCHIVIST** — thesis: git-tracked markdown files are the source of truth; every
  index, embedding, and graph above them is a pure function of the tree, disposable and
  rebuildable. OKF-as-export is a lie you cannot detect.
- **THE OPERATOR** — thesis: an organizational KB is an operational system; Postgres is
  the system of record, the OKF bundle is a continuously materialized byte-exact replica.
  Format is a wire contract, not a runtime.

## The result neither was arguing for

Both reviewers, independently, reached for **CRDTs at their single weakest point.**

The Archivist's admitted failure was the hot concept during a live incident — 20 people
editing one runbook at 02:00, git offering them merge conflicts and PR latency exactly
when latency is the whole game. His answer was a Yjs/Automerge session whose only durable
output is a git commit on quiesce, and he conceded it does not fully work: kill the CRDT
server mid-incident and you lose everything since the last quiesce.

The Operator built the same component in from the start — Yjs keyed on concept `id`,
checkpointing to storage every ~2s — and used it to attack git's line-based merge.

That convergence is the finding. **The CRDT is not a patch on git; it is the write path,
and git is the durable history beneath it.** [heaper.de](https://heaper.de) demonstrates
this shape as a shipping product: local-first, all changes merging via CRDT including
from offline devices, content-hash dedup, full-text search that works offline, self-host
or hosted, open export and direct data access.

Promoting CRDT from patch to primary dissolves four disputes at once:

| Dispute | Why it dissolves |
|---|---|
| Git merge conflicts on concept bodies | There is no line-based merge in the write path |
| Operator's attack (b): clean merges silently corrupting YAML | Frontmatter is a typed CRDT map; `tags`/`verified[]`/`sources[]` merge as **sets**, which line-based merge structurally cannot do |
| Draft weakness #5: generated-and-committed `index.md` conflicting constantly | Derived files have no merge semantics, only a recompute — `.gitattributes` merge driver regenerates from the merged tree |
| Archivist's weakest point (live editing) | Solved as designed, not as a concession |
| Operator's "day-scale write latency, a wiki that dies of staleness" | Write-to-visible is seconds |

What it does **not** dissolve is access control. That is the one real divergence, below.

## Verdicts

### [OPEN-1] One bundle with subtrees, or several overlaid? → **Several. Bundle boundary = ACL boundary.**

Both reviewers converged from opposite directions — the Archivist because git's read
granularity *is* the repository (a clone hands over all history; there is no subtree read
ACL, and pretending otherwise is the most dangerous thing this design could do), the
Operator because a bundle is an export-time slice over ACL.

`okf_version` being root-`index.md`-only is a rule about **serialization**, not about
storage layout. N conformant bundles beat one bundle with an invented overlay.

Personal brain: one bundle. Corporate: one per blast-radius class, unioned at read time
by a mount table. Mount point prefixes the path (`corp:/decisions/x.md`), so collisions
are impossible by construction. Cross-slice links degrade to broken links — which the
spec **requires** consumers to tolerate.

*Breaks if:* dangling cross-bundle links prove intolerable in practice, and `shared/terms/`
dedup becomes an ongoing tax.

### [OPEN-2] Path as identity? → **No. Stable `id` is identity; path is an address.**

The draft had this wrong. A CRDT document needs a stable identity independent of its
path — you cannot key a live editing session on a mutable string. This is forced by the
write path, not a matter of taste.

`id:` is an unknown key, which the spec explicitly requires consumers to preserve and
tolerate, so it costs zero conformance. Rename = update the address, rewrite every inbound
markdown link in one atomic commit, leave `aliases: [old/path.md]` behind. The Archivist's
objection that link rewriting "mutates files we did not author" is answered: the bundle is
one versioned artifact and git records the rewrite atomically.

*Breaks if:* another OKF consumer treats path as identity — our renames look to them like
delete-plus-create and destroy their history. External deep links (Slack, Jira) still rot;
`aliases` only helps consumers that read it.

### [OPEN-3] `visibility` in frontmatter or path? → **Neither is authoritative. It is an ACL record.**

The Operator's framing is correct and decisive: frontmatter saying `visibility: corp` is a
note on the outside of an unlocked door. Once a file is in a repo you can clone, its
frontmatter has **zero** enforcement power.

Path is the default projection (it falls out of OPEN-1 for free). Export writes visibility
into frontmatter as a *courtesy annotation* for downstream consumers. Neither gates
anything. The Archivist's real concern — never keep two copies of an access decision — is
honored by making the frontmatter copy explicitly non-authoritative rather than by deleting it.

### [OPEN-4] Eager or lazy indexing? → **Dissolved. Split by cost, not by timing.** *(unanimous)*

Both reviewers independently produced the same answer, which is the strongest signal in
the whole exercise:

- **Structural + lexical + edges: eager.** Pure CPU, no API spend, sub-second. Maintained
  in the same transaction (Operator) or by watcher (Archivist).
- **Embeddings: debounced, and skipped entirely for `status: draft`.**

You never pay to embed a draft deleted an hour later, and you never eat first-query latency
for lexical search. The draft's dilemma was false — it treated one knob as governing two
costs that differ by three orders of magnitude.

The Archivist's cost arithmetic, which the Operator did not contest: 50k concepts × ~800
tokens ≈ 40M tokens; at commodity small-embedding pricing a **full re-embed of an entire
corporate brain is single-digit dollars** *(his estimate — verify at build time)*. Embedding
cost is not an argument for or against anything.

### [OPEN-5] Access control → **The Operator wins for corporate. Unresolved-by-design for personal.**

This is the item that actually decides the debate, and the Archivist conceded the ground
rather than fake it: per-concept ACLs inside one bundle are unimplementable on git and
should be **refused, not simulated**. His answer is partition — one repo per sensitivity
class — and he explicitly accepted that an org needing thousands of ad-hoc per-concept
grants gets repo sprawl and that his design fails for them.

Two Operator arguments survive unrebutted:

1. **Git cannot revoke.** This breaks at N=1, not at scale — the first compensation band,
   security incident, or terminated-employee record. Once cloned, revoked is fiction.
2. **Post-filter destroys recall.** 200k concepts, a user entitled to 5%: retrieve top-100
   by vector similarity, then filter by entitlement, and ~5 survive — while the relevant
   ones were likely outside the 100. Authorization must be a **prefilter inside the query
   planner**, not a post-filter. File-first has nowhere to put it.

**Therefore ACL granularity is the single pluggable seam** in the architecture, and the
only axis on which personal and corporate genuinely diverge:

- **Personal / small team** → partition. Mount table + repo-level permissions. No service.
- **Corporate** → row-level enforcement with authorization applied before ANN.

Everything else — the bundle format, the type vocabulary, the CRDT write path, the index
plane, the retrieval pipeline — is identical across both. That is how R1 is satisfied with
one architecture: not by pretending the two cases are the same, but by isolating the one
place they differ to a single swappable component.

## What each side kept

**The Archivist keeps the source of truth**, and one argument the Operator explicitly
conceded he could not take: a structural guarantee beats a governance guarantee. The
Operator's losslessness rests on a CI round-trip assertion plus a constitutional rule that
all concept-scoped state must serialize to frontmatter — and he admitted governance decays
under product pressure, that someone eventually marks the check flaky and deletes it, and
that his design makes drift *detectable* rather than *impossible*.

His other two attacks stand and are adopted as invariants:

- **You cannot open a pull request against a row.** Reviewed change is what makes a
  corporate brain trustworthy. A runbook silently edited to drop a rollback step, with an
  `updated_by` column and no reviewer, no diff, no revert-to-commit, is the failure.
- **`verified: {by: human:gary}` in a database row is a claim; in git it is a signed
  commit.** The invariant that agents may never write `verified` is enforceable only if
  authorship is cryptographic. Otherwise an agent with a write token sets `verified` on
  4,000 concepts and every trust tier in retrieval is poisoned with no tamper-evident
  record. **CI rejects unsigned `verified` deltas.**

**The Operator keeps the query plane**: authorization as prefilter, transactional index
maintenance, and identity-independent-of-path.

## Open, and honestly so

1. **The CRDT server is a stateful component in the write path.** The Archivist's bound —
   its state is measured in seconds, not years; losing it costs one editing session, not
   the brain — is the right frame, but it is a real dependency and the draft claimed there
   were none.
2. **Scale remains unvalidated.** Nobody produced a measurement. The Archivist's estimate
   is that brute-force vector search over int8 768-dim stops being viable around 1–2M
   concepts (~1.5 GB, >500ms) — an estimate, not a finding.
3. **Corporate concurrency at 500 editors is untested** in either design.

## Tests that would settle it

Adopted from both reviewers, deduplicated, kept only where a result would actually change
the design:

| # | Test | Falsifies |
|---|---|---|
| T1 | **Rebuild drill.** `rm -rf .touchstone/ && find . -name index.md -delete && touchstone index`. Every generated `index.md` byte-identical; a 500-query golden set returns identical ranked top-10. | The whole "derived and disposable" claim, if it fails |
| T2 | **Round-trip fidelity.** 1,000 adversarial concepts — unknown `type` values, custom keys, 3-entry `verified[]` chains, YAML anchors, multiline scalars, CRLF, unicode paths, 200 deliberately broken links. Ingest → export → `git diff --exit-code`. | Either design, at a single non-identical byte. The Archivist predicts a service-backed store loses unknown keys and broken links — both of which the spec requires be preserved |
| T3 | **ACL-aware recall.** 200k concepts, 400 principals each entitled to 2–8%. Prefiltered ANN vs. ANN-then-filter, recall@10 against exact search. | The Operator's OPEN-5 win, if post-filter reaches ≥0.95× prefilter recall at equal p95 |
| T4 | **Latency at corporate scale.** 200k concepts, 768-dim, hybrid + one-hop expansion, 50 QPS. Target p95 < 400ms, recall@10 ≥ 0.95. | The need for a service, if SQLite FTS5 + sqlite-vec on the bare bundle lands within 2× |
| T5 | **Concurrency.** 40 simulated editors, 8 hours, Zipf-distributed over 200k concepts, ~1,200 edits/hr, ~15% collision on hot concepts. Measure lost updates, YAML-invalid-after-clean-merge events, p50 write-to-visible. | The CRDT write path, if it produces any lost update or malformed frontmatter |
| T6 | **Service-death drill.** Destroy every non-file component and all backups. Rebuild from the bundle alone using the upstream `reference_agent` CLI (`enrich`, `visualize`). | Portability, if concept count differs by >0 or reconstruction exceeds 4 hours |

T1, T2 and T6 are the ones to run first: they are cheap, they need no corpus at scale, and
they test the property everything else is built on.

## Sources

- [OKF v0.2 SPEC.md](https://github.com/GoogleCloudPlatform/knowledge-catalog/blob/main/okf/SPEC.md)
- [How the Open Knowledge Format can improve data sharing — Google Cloud](https://cloud.google.com/blog/products/data-analytics/how-the-open-knowledge-format-can-improve-data-sharing)
- [Heaper — local-first, CRDT sync](https://heaper.de/)
- [Heaper: local-first storage with CRDT sync — Geeky Gadgets](https://www.geeky-gadgets.com/heaper-folderless-file-management/)
