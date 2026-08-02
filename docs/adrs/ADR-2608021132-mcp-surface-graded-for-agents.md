# ADR-2608021132: The MCP surface is a driving adapter, and it is graded for agents

**Status:** Accepted
**Date:** 2026-08-02
**Epoch:** stage-1-skeleton
**Drivers:** Touchstone's premise is that human and agent access to a knowledge base are the same capability wearing two skins. That premise was untested: the MCP adapter was a two-line stub, and building it revealed that the CLI could not have shared code with it even if someone had tried.

## Context

Three things forced this decision at once.

**1. The parity claim was unbacked.** hex [ADR-019](https://github.com/gaberger/hex/blob/main/docs/adrs/ADR-019-cli-mcp-parity.md)
states that every capability in one primary adapter must exist in the other, and specifies a CI
check comparing the CLI subcommand set against the tool registry — recording that check as
unimplemented. Touchstone inherited both the rule and the gap.

**2. The layering forbade the sharing it demanded.** Architecture rule 4 said *every* adapter may
import only `touchstone-ports`. For a driven adapter that is right — it implements a port. For a
driving adapter it is wrong: it is the reason `touchstone-usecases` sat unreachable with 1,074
tested lines while the CLI reimplemented the entire layer. Building the MCP surface under the old
rule would have produced a *third* implementation, and parity would have been a statement about
names rather than behaviour.

**3. "Exposes an MCP server" is not a quality bar.** `gaberger/api-ai-readiness` grades an API on
six dimensions for how well it serves an autonomous agent — response discipline, field selection,
retrieval shape, self-description, workflow atomicity, discovery — and its SPEC observes that
roughly 70% is statically gradable. A surface can be perfectly valid MCP and still blow an agent's
context window on its first call.

## Decision

**The MCP surface is a driving adapter that calls the use cases, at MCP revision 2026-07-28, and
its tool schemas are held to the AI-readiness rubric by test.**

- **Rule 4 splits.** `4a`: secondary adapters import `touchstone-ports` only. `4b`: primary
  adapters import `touchstone-usecases` + `touchstone-ports`, and **must** name
  `touchstone-usecases` — an adapter that does not drive the use cases is a second implementation.
- **`touchstone-mcp-adapter` is generic over the ports**, never over concrete adapters. It cannot
  name `SqliteIndex` or `FsBundle`, which is what stops it drifting from the CLI.
- **Transports live in the composition root**, not the adapter. stdio is the default; Streamable
  HTTP is opt-in via `touchstone mcp --http <addr>`. Choosing *how* to serve is a wiring decision.
- **The rubric is a test, not a report.** `tests/ai_readiness.rs` asserts each of the six
  dimensions against the declared schemas.
- **Parity is a test.** `tests/parity.rs` implements the check ADR-019 specifies, parsing the CLI
  enum textually rather than importing the other adapter — which rule 5 forbids, and rightly: a
  parity check that can only run by violating the layering it defends is self-defeating.

### What the rubric bought, concretely

| Dimension | Decision |
|---|---|
| Response discipline | every list-shaped tool takes `limit` **with a declared default of 10** and a ceiling of 200, and returns `total`/`returned` rather than a bare array |
| Field selection | `fields` on every result-bearing tool |
| Retrieval shape | `type`/`tag`/`status`/`trust` filter inside the SQL query, never after |
| Self-description | recoverable failures are tool-execution errors carrying *what to do next*; only an unknown tool name is a protocol error, because clients render those opaquely and the caller never sees the message |
| Workflow atomicity | a concept path is the only identifier, obtainable in one call — there is no id to resolve before an id |
| Discovery | `touchstone_discover` describes the bundle and the surface in one argument-free call |

### The trust invariant crosses the boundary intact

No tool can write `verified`. `touchstone_new` emits `generated:` and nothing else, so a concept an
agent creates is `machine` — never `human`. The server's `instructions` say so explicitly, because
an agent that believes it can attest is more dangerous than one that cannot try.

## Consequences

**Positive:**
- One implementation, two skins — enforced by rule 4b and by the parity test rather than asserted.
- The rubric's statically-gradable majority is a gate; a schema regression fails the build.
- Reviving the use-case layer deleted a duplicate parser, a duplicate `slugify`, two extra copies
  of `split_frontmatter`, and four spellings of the trust-tier names.

**Negative:**
- `rmcp` is a young SDK tracking a spec that revises quarterly; a revision bump is a dependency
  bump plus whatever the model changes underneath.
- Streamable HTTP binds a socket that can read **and write** the bundle. There is no
  authentication in this surface — that is a deliberate omission, not an oversight, and it is why
  stdio is the default and the HTTP path prints a warning.
- Only `lint` currently routes through the use cases on the CLI side; `index`/`search`/`fmt`/
  `export`/`new` still carry command-local logic. Rule 4b checks the dependency, not every call
  site, so the rule is satisfied while the migration is incomplete.

**Mitigations:**
- The conformance suite drives the binary black-box, so the CLI cannot regress silently while the
  remaining commands migrate.
- `TOUCHSTONE_BIN` lets the same drills gate any implementation, including one built on a future
  spec revision.

## Implementation

| Phase | Description | Status | Verification |
|-------|------------|--------|--------------|
| P1 | Rule 4 splits into 4a/4b; 6b asserts wiring means *used* | Completed | test:cargo test -p touchstone-cli --test architecture |
| P2 | Tool surface at MCP 2026-07-28 with input/output schemas and annotations | Completed | code:touchstone-adapters/primary/mcp/src/tools.rs |
| P3 | Handler drives the use cases; recoverable errors carry recovery text | Completed | code:touchstone-adapters/primary/mcp/src/lib.rs |
| P4 | AI-readiness rubric as assertions | Completed | test:cargo test -p touchstone-mcp-adapter --test ai_readiness |
| P5 | CLI–MCP parity check (closes the gap ADR-019 left open) | Completed | test:cargo test -p touchstone-mcp-adapter --test parity |
| P6 | stdio + Streamable HTTP transports in the composition root | Completed | code:touchstone-cli/src/main.rs |
| P7 | Migrate the remaining CLI commands onto the use cases | Pending | test:cargo test -p touchstone-cli --test architecture |
| P8 | Authentication for the HTTP transport | Pending — gated on a real remote use case | — |

## References

- [MCP specification 2026-07-28](https://modelcontextprotocol.io/specification/2026-07-28)
- [api-ai-readiness](https://github.com/gaberger/api-ai-readiness) — the rubric and its SPEC
- [hex ADR-019](https://github.com/gaberger/hex/blob/main/docs/adrs/ADR-019-cli-mcp-parity.md) — CLI/MCP parity
- [ADR-2608010940](ADR-2608010940-rust-implementation-parser-as-port.md) — the crate layout this amends
- [ADR-2608010950](ADR-2608010950-conformance-suite.md) — the black-box gate that made the rewire safe
- [FINDINGS.md](../../FINDINGS.md) — E6, and the duplications the rewire surfaced
