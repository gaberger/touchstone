# The Rust path — measured, not asserted

E3a and E3b were both **library** failures, not language failures. So the first question is
whether Rust's libraries actually fix them or just relocate them. That is testable, and was
tested (`serde_yaml_ng` 0.10.0, Rust 1.95.0, probe in the session scratchpad).

## 1. YAML — the measured part

| Behaviour | PyYAML | serde_yaml_ng | Verdict |
|---|---|---|---|
| `at: 2026-01-01T00:00:00Z` after parse | **coerced to `datetime`** | stays `String` | **Rust wins** |
| ISO 8601 survives re-emit | **no** — `2026-01-01 00:00:00+00:00` | yes, byte-identical | **Rust wins** |
| `stale_after: 2026-09-23` | coerced to `date` | stays `String` | **Rust wins** |
| Literal block scalar `script: \|` | flattened to a quoted string | **block style preserved** | **Rust wins** |
| Anchors on re-emit | expanded, **and a new `&id001` invented elsewhere** | expanded cleanly, nothing invented | Rust better |
| Merge key `<<: *defaults` | resolved into the mapping (YAML 1.1) | **not implemented — `<<` kept as a literal key** | **Rust worse** |
| Comments on re-emit | dropped | dropped | tie |
| Unknown types / unknown keys | preserved | preserved | tie |

**E3a — the dangerous one — is fixed for free.** In Python it required surgery on the
loader's implicit resolvers. In Rust the default behaviour is already correct: temporal
values stay strings, so `generated.at`, `verified[].at`, `stale_after` and `usage_window`
round-trip exactly as the spec requires. That bug silently rewrote spec-mandated fields, and
Rust simply does not have it.

**A new hazard appears, though, and it is an interop one.** `serde_yaml_ng` does not
implement merge keys. Given the same bytes:

```yaml
verified:
  - <<: *defaults      # {by: human:gary, at: ...}
```

Python resolves the merge and reports `verified[0].by == "human:gary"`. Rust reports a key
literally named `<<` and **no `by` at all** — so a concept that is human-verified under one
implementation is unattributed under the other. **The trust tier flips depending on which
reader you use.** That is exactly the class of silent divergence OKF exists to prevent, and
it argues for a conformance suite shared across implementations (see §6) rather than for
either language.

**The formatter problem (E3b) is NOT solved by switching.** Neither `serde_yaml_ng` nor
PyYAML preserves comments, and both expand anchors. Document-preserving formatting requires
a document-model library in either language — `ruamel.yaml` in Python, `saphyr` or a custom
emitter in Rust. **This is not a reason to switch, because Python already has the better
answer available today.** If a comment-preserving formatter is what you want, `ruamel.yaml`
is a one-line dependency, not a rewrite.

## 2. Where Rust genuinely wins

**CRDT — the decisive one.** Automerge, Loro and y-crdt are all written in Rust. Python
would bind to them through FFI; Rust uses them natively. Given the version-control
conclusion (CRDT becomes the write path and sync engine), this stops being a preference.

**Git as a library, not a CLI.** `gitoxide` (`gix`) is pure Rust with no C dependency;
`git2`/libgit2 is the mature alternative. This is what makes option B in
[VERSION-CONTROL.md](VERSION-CONTROL.md) — git's object model with none of git's UX —
actually pleasant to build. *Unverified:* commit-signing support in `gix` should be
confirmed before committing to it, because signing is the enforcement mechanism for the
`verified` invariant and is not optional.

**Distribution.** A single static binary versus `python3 -m venv .venv && pip install`.
This is a real R2 win, not an aesthetic one — a venv is friction the user feels every single
day, and R2 is one of the two project-fatal requirements.

**The filesystem watcher.** Long-running, low-overhead, cross-platform (`notify`). Python is
poorly suited to this and it is on the critical path for automatic indexing.

**Mobile.** If capture must work from a phone to clear the 20-second bar, Rust compiles to
iOS and Android targets. Python effectively does not.

## 3. Where Rust costs you

**Embeddings and ML.** Python's ecosystem is overwhelming; Rust has `fastembed` and
`candle`. Mitigating factor: **A4 is untested, and may delete embeddings entirely** — if
hybrid retrieval does not beat BM25 on a real corpus, this cost evaporates.

**Iteration speed during the phase we are actually in.** Stage 1 exists to answer A10 and
A3. Those are questions about *human behaviour*, answered by throwing code away quickly.
Rust is the wrong tool for the phase where the code is disposable and the answer is
sociological.

**Rewrite cost.** ~700 lines of Python → an estimated 1,500–2,500 lines of Rust. Days, not
weeks — but days spent re-deriving working behaviour rather than answering an open question.

## 4. What performance actually says

**Unmeasured.** The 50k-concept corpus is generated and sitting on disk; the timing run was
stopped before it completed. I will not assert a number I do not have.

What can be said structurally: SQLite does the heavy lifting in both languages, and
embedding is network-bound. The Python-specific cost is parse-and-hash across 50k files, and
it lands on the *watcher* and *pre-commit* paths — where latency is felt — not on query.

**Performance is not the deciding input, and should not be treated as one.** If Python's
cold index on 50k lands in single-digit seconds, the performance argument is dead. Worth
measuring, but it does not change the recommendation either way.

## 5. Recommendation

**Do not rewrite now. Rewrite when you commit to the CRDT write path.**

The trigger is architectural, not performance and not YAML:

| Decision | Language |
|---|---|
| Stay on option A (git demoted, CRDT layer thin) | **Python is adequate.** Add `ruamel.yaml` for the formatter and stop |
| Take option B or C (git-as-library, or custom VC on Automerge/Loro) | **Rust**, and not reluctantly — you would otherwise be writing FFI bindings to Rust libraries |

Concretely, in order:

1. **Answer A10 and A3 in Python.** They are questions about whether anyone uses this. The
   language is irrelevant to both, and rewriting first is the most expensive possible way to
   discover nobody writes into the brain.
2. **Measure the 50k index.** One number, currently missing.
3. **If you commit to CRDT sync, port then** — with the CRDT layer written in Rust first and
   the rest following, rather than a big-bang rewrite.

## 6. The migration asset — and it is not the code

If a port happens, **the drills are the thing worth keeping, not the implementation.**

`tests/drills.py` encodes T1 (byte-identical rebuild), T2a–T2e (raw round-trip, semantic
round-trip, unknown-key preservation, timestamp non-coercion, formatter safety) and T6
(service-death recovery). Those are statements about *OKF conformance*, not about Python.

So: promote the drills to a **language-independent conformance suite** — a fixture bundle
plus expected outputs — and require any implementation to pass it. That converts a rewrite
from "hope it behaves the same" into a pass/fail gate, and it is the only thing that would
have caught the merge-key divergence in §1 before it silently flipped a trust tier in
production.

Build that before porting anything.
