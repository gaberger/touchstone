"""OKF v0.2 concept parsing.

Design rule, carried over from FINDINGS.md E2: the RAW TEXT IS AUTHORITATIVE.
We parse frontmatter into a dict for querying, but we never treat that dict as
the truth. `export` writes raw bytes back. This makes the Operator's silent-
truncation failure mode structurally impossible rather than merely tested for:
there is no serializer in the write path that could drop an unknown key.

`brain fmt` is the ONE place we rewrite a file, and it is explicit and opt-in.
"""
from __future__ import annotations

import hashlib
import re
from dataclasses import dataclass, field
from typing import Any

import yaml

DELIM = "---"
RESERVED = {"index.md", "log.md"}
OKF_VERSION = "0.2"


class Loader(yaml.SafeLoader):
    """SafeLoader with implicit timestamp resolution REMOVED.

    PyYAML's default resolver turns `at: 2026-01-01T00:00:00Z` into a Python
    datetime, and re-emitting it produces `2026-01-01 00:00:00+00:00` -- which is
    NOT ISO 8601 and breaks the spec's `at` / `stale_after` contract for every
    downstream consumer. The spec's timestamps are strings; we keep them strings.
    Caught by inspecting `brain fmt` output on the adversarial fixture.
    """


for _ch, _res in list(Loader.yaml_implicit_resolvers.items()):
    Loader.yaml_implicit_resolvers[_ch] = [
        (tag, rx) for tag, rx in _res if tag != "tag:yaml.org,2002:timestamp"
    ]

# YAML constructs `brain fmt` cannot rewrite without destroying authored
# structure: anchors, aliases, merge keys, and block scalars.
_RISKY = re.compile(r"(^|\s)[&*]\w|<<\s*:|:\s*[|>][-+0-9]*\s*$", re.MULTILINE)

# [text](target) -- but not ![img](...)
_LINK = re.compile(r"(?<!\!)\[([^\]]*)\]\(([^)\s]+)(?:\s+\"[^\"]*\")?\)")


@dataclass
class Concept:
    path: str                      # bundle-relative, posix, e.g. "notes/foo.md"
    raw: str                       # exact file text -- the authoritative value
    fm: dict[str, Any] = field(default_factory=dict)
    fm_text: str = ""              # frontmatter source, between the delimiters
    body: str = ""
    error: str | None = None       # non-conformance reason, if any

    @property
    def digest(self) -> str:
        return hashlib.blake2b(self.raw.encode("utf-8"), digest_size=16).hexdigest()

    @property
    def type(self) -> str:
        v = self.fm.get("type")
        return v if isinstance(v, str) else ""

    @property
    def title(self) -> str:
        v = self.fm.get("title")
        if isinstance(v, str) and v.strip():
            return v
        # spec: consumers MAY derive from filename when title is absent
        stem = self.path.rsplit("/", 1)[-1].removesuffix(".md")
        return stem.replace("-", " ").replace("_", " ").strip()

    @property
    def description(self) -> str:
        v = self.fm.get("description")
        return v if isinstance(v, str) else ""

    @property
    def tags(self) -> list[str]:
        v = self.fm.get("tags")
        if isinstance(v, list):
            return [str(t) for t in v]
        if isinstance(v, str):
            return [v]
        return []

    @property
    def status(self) -> str:
        v = self.fm.get("status")
        return v if isinstance(v, str) else "stable"   # spec default

    @property
    def trust(self) -> str:
        """Spec-derived trust tier. Detect the `human:` prefix, per the actor convention."""
        ver = self.fm.get("verified")
        entries = ver if isinstance(ver, list) else ([ver] if isinstance(ver, dict) else [])
        for e in entries:
            by = e.get("by") if isinstance(e, dict) else None
            if isinstance(by, str) and by.startswith("human:"):
                return "human"
        if self.fm.get("generated"):
            return "machine"
        return "unattributed"

    @property
    def conformant(self) -> bool:
        # SPEC conformance floor: valid YAML frontmatter + non-empty `type`.
        return self.error is None and bool(self.type.strip())

    def links(self) -> list[tuple[str, str]]:
        """(label, target) for every markdown link in the body."""
        return [(m.group(1), m.group(2)) for m in _LINK.finditer(self.body)]


def split_frontmatter(raw: str) -> tuple[str | None, str]:
    """Return (frontmatter_text_or_None, body). Byte-preserving on the body."""
    text = raw.lstrip("﻿")                 # tolerate BOM
    norm = text.replace("\r\n", "\n")
    if not norm.startswith(DELIM + "\n") and norm.strip() != DELIM:
        return None, raw
    # find the closing delimiter on its own line
    lines = norm.split("\n")
    if lines[0].strip() != DELIM:
        return None, raw
    for i in range(1, len(lines)):
        if lines[i].strip() == DELIM:
            fm_text = "\n".join(lines[1:i])
            body = "\n".join(lines[i + 1:])
            return fm_text, body
    return None, raw                            # unterminated -- treat as body


def parse(path: str, raw: str) -> Concept:
    fm_text, body = split_frontmatter(raw)
    c = Concept(path=path, raw=raw, body=body, fm_text=fm_text or "")
    if fm_text is None:
        c.error = "no frontmatter"
        return c
    try:
        data = yaml.load(fm_text, Loader=Loader)
    except yaml.YAMLError as e:
        c.error = f"invalid YAML: {str(e).splitlines()[0]}"
        return c
    if data is None:
        data = {}
    if not isinstance(data, dict):
        c.error = "frontmatter is not a mapping"
        return c
    c.fm = data
    if not isinstance(data.get("type"), str) or not data["type"].strip():
        c.error = "missing or empty `type`"
    return c


def is_reserved(path: str) -> bool:
    return path.rsplit("/", 1)[-1] in RESERVED


# ---------------------------------------------------------------- formatting

# Order that reads well and puts identity first. Unknown keys keep their
# original relative order and are appended -- never dropped.
KEY_ORDER = ["id", "type", "title", "description", "resource", "tags", "aliases",
             "status", "stale_after", "generated", "verified", "sources"]


def canonical_frontmatter(fm: dict) -> str:
    """Deterministic YAML. Exists because E2 showed git merges canonical
    frontmatter cleanly (0/400 silent failures) but makes no such promise about
    hand-irregular YAML. This is the cheap alternative to a CRDT."""
    ordered: dict[str, Any] = {}
    for k in KEY_ORDER:
        if k in fm:
            ordered[k] = fm[k]
    for k, v in fm.items():                     # unknown keys preserved, in order
        if k not in ordered:
            ordered[k] = v
    return yaml.dump(ordered, sort_keys=False, default_flow_style=False,
                     allow_unicode=True, width=100)


def formattable(c: Concept) -> str | None:
    """None if safe to reformat, else the reason it is not.

    Re-emitting YAML is lossy for authored structure. Measured on the adversarial
    fixture: merge keys get resolved and PyYAML invents anchors elsewhere; literal
    block scalars (a shell script under `script: |`) flatten into quoted strings.
    We refuse rather than mangle. This is why the formatter is NOT the blanket
    CRDT substitute FINDINGS.md E2 first claimed it was.
    """
    if not c.fm:
        return "no frontmatter"
    if c.error:
        return c.error
    if _RISKY.search(c.fm_text):
        return "contains anchors, aliases, merge keys or block scalars"
    return None


def format_concept(c: Concept) -> str:
    body = c.body
    if not body.startswith("\n"):
        body = "\n" + body
    body = body.rstrip("\n") + "\n"
    return f"{DELIM}\n{canonical_frontmatter(c.fm)}{DELIM}\n{body}"


# -------------------------------------------------------------------- lint

def lint(c: Concept) -> list[str]:
    """Rules that E2 showed are actually needed. The duplicate checks are the
    16% CLEAN_DEFECT class -- git's only real frontmatter failure mode."""
    out: list[str] = []
    if c.error:
        out.append(f"error: {c.error}")
        if not c.fm:
            return out

    tags = c.tags
    dupes = {t for t in tags if tags.count(t) > 1}
    if dupes:
        out.append(f"duplicate tags: {', '.join(sorted(dupes))}")

    ver = c.fm.get("verified")
    entries = ver if isinstance(ver, list) else ([ver] if isinstance(ver, dict) else [])
    bys = [e.get("by") for e in entries if isinstance(e, dict) and e.get("by")]
    vd = {b for b in bys if bys.count(b) > 1}
    if vd:
        out.append(f"duplicate verified principals: {', '.join(sorted(map(str, vd)))}")

    for e in entries:
        if isinstance(e, dict) and not e.get("by"):
            out.append("verified entry missing required `by`")

    srcs = c.fm.get("sources")
    if isinstance(srcs, list):
        for s in srcs:
            if isinstance(s, dict) and not s.get("resource"):
                out.append(f"source `{s.get('id', '?')}` missing required `resource`")

    st = c.fm.get("status")
    if st is not None and st not in ("draft", "stable", "deprecated"):
        out.append(f"status `{st}` is not draft|stable|deprecated")

    for _, tgt in c.links():
        if tgt.startswith(("http://", "https://", "#", "mailto:")):
            continue
        if "[[" in tgt:
            out.append("wikilink syntax is not OKF -- use a markdown link")
    if "[[" in c.body:
        out.append("body contains [[wikilinks]] -- not OKF, will not resolve")
    return out
