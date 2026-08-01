"""Build a test bundle: a small realistic brain + the adversarial concepts that
T2 (round-trip fidelity) exists to break.

The adversarial set is deliberately nastier than anything `brain new` produces --
YAML anchors, comments between keys, multiline scalars, flow style, CRLF, unicode
paths, unknown types, unknown keys, and broken links. The spec REQUIRES all of
these survive. A store that models a fixed schema loses them silently; that is
the failure mode this fixture is built to detect.
"""
from __future__ import annotations

import json
import shutil
import sys
from pathlib import Path

ROOT = Path(sys.argv[1] if len(sys.argv) > 1 else "_fixture")

REAL = [
    ("notes/why-files-win.md", "Note", "Why files win", "Portability outlives infrastructure.",
     ["architecture", "okf"], "stable", "human",
     "Storage should be the most boring thing available. See [OKF spec](/terms/okf.md)\n"
     "and the [indexing decision](/decisions/derive-everything.md)."),
    ("notes/second-brain-capture.md", "Note", "Capture beats retrieval",
     "A brain nobody writes to is an empty database.", ["capture"], "stable", "human",
     "If capture takes more than twenty seconds it does not happen.\n"
     "Related: [adoption test](/decisions/measure-adoption.md)."),
    ("notes/draft-idea.md", "Note", "Half-formed idea about graphs",
     "Unfinished.", ["graph"], "draft", "machine",
     "Something about [edges](/terms/edge.md) being the real index."),
    ("decisions/derive-everything.md", "Decision", "Everything above the bundle is derived",
     "The index is disposable and rebuildable from files alone.",
     ["architecture"], "stable", "human",
     "We rejected a database as source of truth. See [why files win](/notes/why-files-win.md).\n"
     "Rejected alternative: Postgres with an OKF export."),
    ("decisions/measure-adoption.md", "Decision", "Measure adoption before retrieval quality",
     "Concept-creation rate on non-project days is the signal.",
     ["process"], "stable", "human",
     "Retrieval work is wasted if nobody writes. Links to [capture](/notes/second-brain-capture.md)."),
    ("decisions/postfilter-is-enough.md", "Decision", "Post-filter authorization at depth",
     "Retrieve at K>=500 and post-filter rather than building a query planner.",
     ["security", "search"], "stable", "human",
     "Simulation showed recall recovers fully at K=500.\n"
     "Does not address revocation -- see [revocation](/terms/revocation.md)."),
    ("terms/okf.md", "Term", "OKF", "Open Knowledge Format, a Google Cloud open specification.",
     ["okf"], "stable", "human",
     "A bundle is a directory of markdown concepts. Only `type` is required."),
    ("terms/edge.md", "Term", "Edge", "A resolved markdown link between two concepts.",
     ["graph"], "stable", "unattributed",
     "Edges turn a directory into a queryable graph."),
    ("terms/revocation.md", "Term", "Revocation",
     "Withdrawing access to knowledge already distributed.",
     ["security"], "stable", "human",
     "Git cannot revoke: once cloned, revoked is fiction. This is unsolved here."),
    ("terms/trust-tier.md", "Term", "Trust tier",
     "Derived from the `human:` prefix in verified[].by.",
     ["okf", "trust"], "stable", "human",
     "human > machine > unattributed. Used by [search ranking](/systems/retrieval.md)."),
    ("projects/okf-brain.md", "Project", "OKF second brain",
     "Building a knowledge base on the Open Knowledge Format.",
     ["okf", "architecture"], "stable", "human",
     "Stage 1 is the walking skeleton. Depends on [retrieval](/systems/retrieval.md)\n"
     "and [not-yet-written capture agent](/systems/capture-agent.md)."),
    ("systems/retrieval.md", "System", "Retrieval pipeline",
     "Structured prefilter, BM25, graph expansion, trust rank.",
     ["search"], "stable", "machine",
     "Over-retrieves to depth 500 because [post-filtering](/decisions/postfilter-is-enough.md)\n"
     "needs depth to preserve recall."),
    ("people/gary.md", "Person", "Gary", "Owner of this brain.", ["people"], "stable", "human",
     "Working on [okf-brain](/projects/okf-brain.md)."),
    ("runbooks/rebuild-index.md", "Runbook", "Rebuild the index from scratch",
     "The T1 drill: delete everything derived, regenerate, diff.",
     ["ops"], "stable", "human",
     "1. `rm -rf .brain/`\n2. `find . -name index.md -delete`\n3. `brain index`\n"
     "4. `git diff --exit-code`\n\nAny diff falsifies the derivability claim."),
    ("metrics/concepts-per-day.md", "Metric", "Concepts created per day",
     "Split by whether the day involved active project work.",
     ["adoption"], "draft", "machine",
     "The A3 adoption signal. See [measure adoption](/decisions/measure-adoption.md)."),
]


def build_real(root: Path) -> None:
    for path, typ, title, desc, tags, status, trust, body in REAL:
        # json.dumps gives a valid YAML double-quoted scalar. Naive concatenation
        # here produced invalid YAML the first time (a `: ` inside a description) --
        # exactly why `brain new` emits through a YAML dumper instead.
        fm = [f"type: {typ}", f"title: {json.dumps(title)}",
              f"description: {json.dumps(desc)}"]
        fm.append("tags: [" + ", ".join(tags) + "]")
        fm.append(f"status: {status}")
        if trust == "human":
            fm.append("verified:")
            fm.append("  - by: human:gary")
            fm.append("    at: 2026-07-15T09:00:00Z")
        elif trust == "machine":
            fm.append("generated: { by: capture/claude-opus-5, at: 2026-07-20T12:00:00Z }")
        text = "---\n" + "\n".join(fm) + "\n---\n\n" + body + "\n"
        p = root / path
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(text, encoding="utf-8")


# ------------------------------------------------------------- adversarial

ADVERSARIAL: list[tuple[str, str]] = [
    # unknown type + unknown top-level keys + comment inside frontmatter
    ("adversarial/unknown-type.md", """---
type: ThreatModel          # not in our vocabulary -- spec says tolerate it
title: Unknown type survives
retention: 7y              # unknown key -- MUST round-trip
classification: internal
---

Body with a [broken link](/does/not/exist.md).
"""),
    # YAML anchors and aliases
    ("adversarial/anchors.md", """---
type: Note
title: Anchors and aliases
defaults: &defaults
  by: human:gary
  at: 2026-01-01T00:00:00Z
verified:
  - <<: *defaults
  - by: human:bob
    at: 2026-02-01T00:00:00Z
---

Anchors must survive.
"""),
    # multiline scalars, both styles
    ("adversarial/multiline.md", """---
type: Runbook
title: Multiline scalars
description: >
  A folded scalar that spans
  several source lines.
script: |
  #!/bin/sh
  echo "literal block"
  exit 0
---

Body.
"""),
    # flow style, quoting variants, empty values
    ("adversarial/flow-style.md", """---
type: Note
title: "Quoted: with a colon"
tags: [a, "b c", 'd']
description: ''
resource: https://example.com/x?y=1&z=2
empty_key:
nested: {deep: {deeper: true}}
---

Body.
"""),
    # three-entry verified chain + sources with footnote refs
    ("adversarial/verified-chain.md", """---
type: Metric
title: Three-entry verification chain
verified:
  - by: human:alice
    at: 2026-01-01T00:00:00Z
  - by: process:nightly
    at: 2026-02-01T00:00:00Z
  - by: human:carol
    at: 2026-03-01T00:00:00Z
sources:
  - id: src-a
    resource: https://example.com/a
    title: Source A
    usage_count: 5000
  - id: src-b
    resource: https://example.com/b
usage_window: { from: 2026-06-01, to: 2026-06-30 }
---

A claim with attribution.[^src-a]
"""),
    # deliberately non-conformant: empty type
    ("adversarial/empty-type.md", """---
type: ""
title: Empty type is non-conformant
---

Should be reported, not crash the indexer.
"""),
    # no frontmatter at all
    ("adversarial/no-frontmatter.md", """# Just a markdown file

No frontmatter. Should be reported, not crash.
"""),
    # unicode in path and content
    ("adversarial/ünïcode-pàth.md", """---
type: Note
title: Unicode path and content — em dash, café, 日本語
tags: [ünïcode]
---

Content with emoji and CJK: 知識 — fine.
"""),
    # attested computation, spec-defined type
    ("adversarial/attested.md", """---
type: Attested Computation
title: Revenue by year
runtime: bigquery
parameters:
  - { name: year, type: integer, required: true }
computation: references/computations/revenue.sql
executor:
  resource: references/skills/run-on-bq.md
  receipt: [job_id, executed_sql, result]
---

Sanctioned execution logic.
"""),
]

CRLF_FILE = ("adversarial/crlf.md",
             "---\r\ntype: Note\r\ntitle: CRLF line endings\r\n---\r\n\r\nWindows body.\r\n")


def build_adversarial(root: Path) -> None:
    for path, text in ADVERSARIAL:
        p = root / path
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(text, encoding="utf-8")
    p = root / CRLF_FILE[0]
    p.write_bytes(CRLF_FILE[1].encode("utf-8"))


if __name__ == "__main__":
    if ROOT.exists():
        shutil.rmtree(ROOT)
    ROOT.mkdir(parents=True)
    build_real(ROOT)
    build_adversarial(ROOT)
    n = len(list(ROOT.rglob("*.md")))
    print(f"fixture built at {ROOT}: {n} concepts "
          f"({len(REAL)} realistic, {len(ADVERSARIAL) + 1} adversarial)")
