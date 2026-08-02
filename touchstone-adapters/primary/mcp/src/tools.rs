//! The tool surface, as data.
//!
//! Declared separately from the handler for two reasons. The parity test compares this list
//! against the CLI subcommand set (hex ADR-019: every capability in one primary adapter must
//! exist in the other), and the AI-readiness scorecard asserts against these schemas directly.
//! Both would be untestable if the definitions were buried in a `match`.
//!
//! **The schemas here are graded.** `gaberger/api-ai-readiness` scores an API on six dimensions
//! for how well it serves an autonomous agent, and each is a deliberate choice below:
//!
//! | Dimension | How this surface satisfies it |
//! |---|---|
//! | Response discipline | every list-shaped tool takes `limit` **with a default**, and returns a count plus a bounded array — never a bare unbounded list |
//! | Field selection | `fields` on the result-bearing tools, so an agent can ask for paths alone |
//! | Retrieval shape | `type` / `tag` / `status` / `trust` filter server-side, so the agent never over-fetches to filter in context |
//! | Self-description | every failure is a tool-execution error carrying what to do next, not an opaque protocol error |
//! | Workflow atomicity | a concept path is the only identifier, and it comes from `search`/`stats` — there is no chain of ids to resolve |
//! | Discovery | `touchstone_discover` describes the bundle and this surface in one call |

use rmcp::model::{Tool, ToolAnnotations};
use serde_json::{json, Map, Value};
use std::borrow::Cow;
use std::sync::Arc;

/// Default page size for every list-shaped tool.
///
/// The single most important number here. An agent that asks for "everything" and gets it will
/// blow its context window on a bundle of any size, so the surface never returns unbounded
/// results — it returns this many and tells the caller the total.
pub const DEFAULT_LIMIT: u32 = 10;

/// Upper bound on `limit`. A caller asking for more gets this, and is told so.
pub const MAX_LIMIT: u32 = 200;

/// Every tool name, in a stable order.
///
/// `tools/list` must be deterministic (MCP 2026-07-28 §Tools: "Servers SHOULD return tools in a
/// deterministic order") so clients can cache the list and LLM prompt caches keep hitting.
pub const TOOL_NAMES: &[&str] = &[
    "touchstone_discover",
    "touchstone_export",
    "touchstone_fmt",
    "touchstone_index",
    "touchstone_ingest",
    "touchstone_lint",
    "touchstone_new",
    "touchstone_search",
    "touchstone_show",
    "touchstone_stats",
    "touchstone_unprocessed",
    "touchstone_verify",
];

fn obj(v: Value) -> Arc<Map<String, Value>> {
    match v {
        Value::Object(m) => Arc::new(m),
        _ => Arc::new(Map::new()),
    }
}

/// A read-only tool: safe to call speculatively, no side effects.
fn read_only(title: &str) -> ToolAnnotations {
    ToolAnnotations::with_title(title).read_only(true).destructive(false).idempotent(true).open_world(false)
}

/// A writing tool. `destructive` distinguishes "adds things" from "rewrites existing bytes",
/// which is exactly the distinction a human approving the call needs to see.
fn writes(title: &str, destructive: bool, idempotent: bool) -> ToolAnnotations {
    ToolAnnotations::with_title(title)
        .read_only(false)
        .destructive(destructive)
        .idempotent(idempotent)
        .open_world(false)
}

/// The `limit` property, with its default stated in the schema so an agent can see the bound
/// without calling first.
fn limit_prop() -> Value {
    json!({
        "type": "integer",
        "minimum": 1,
        "maximum": MAX_LIMIT,
        "default": DEFAULT_LIMIT,
        "description": format!(
            "Maximum results to return. Defaults to {DEFAULT_LIMIT}; values above {MAX_LIMIT} are \
             clamped to {MAX_LIMIT}. Responses are always bounded -- check `total` in the result \
             to see whether more exist."
        )
    })
}

fn fields_prop(available: &str) -> Value {
    json!({
        "type": "array",
        "items": { "type": "string" },
        "description": format!(
            "Subset of fields to return for each result, e.g. [\"path\"] to get paths only and \
             nothing else. Omit for all fields. Available: {available}."
        )
    })
}

fn tool(
    name: &'static str,
    title: &str,
    description: &str,
    input: Value,
    output: Value,
    annotations: ToolAnnotations,
) -> Tool {
    Tool::new(Cow::Borrowed(name), Cow::Owned(description.to_string()), obj(input))
        .with_title(title)
        .with_raw_output_schema(obj(output))
        .annotate(annotations)
}

/// The hit shape returned by `search`, reused in its output schema.
fn hit_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "Bundle-relative concept path. This is the identifier every other tool accepts." },
            "title": { "type": "string", "description": "Concept title, or a humanised filename when untitled." },
            "type": { "type": "string", "description": "OKF concept type. May be any string -- unknown types are preserved, not rejected." },
            "description": { "type": "string" },
            "trust": { "type": "string", "enum": ["human", "attested", "machine", "unattributed"],
                       "description": "Derived trust tier, never authored. `human` outranks `machine` outranks `unattributed` in ranking." },
            "via": { "type": "string", "enum": ["direct", "link"],
                     "description": "`direct` = matched the query; `link` = reached by one graph hop from a match." }
        },
        "required": ["path", "title", "type", "trust", "via"]
    })
}

/// Build the full tool list. Order matches [`TOOL_NAMES`].
pub fn all() -> Vec<Tool> {
    vec![
        tool(
            "touchstone_discover",
            "Discover capabilities",
            "Describe this bundle and this tool surface in a single call: concept counts by type, \
             trust tier and status, the available filters, and every tool with its purpose. Call \
             this first when you do not know what is in the bundle -- it is cheaper than listing \
             concepts and tells you which filters are worth using.",
            json!({ "type": "object", "additionalProperties": false }),
            json!({
                "type": "object",
                "properties": {
                    "bundle": { "type": "string" },
                    "concepts": { "type": "integer" },
                    "types": { "type": "array", "items": { "type": "object" } },
                    "trust_tiers": { "type": "array", "items": { "type": "object" } },
                    "filters": { "type": "array", "items": { "type": "string" } },
                    "tools": { "type": "array", "items": { "type": "object" } },
                    "indexed": { "type": "boolean", "description": "False when the derived index has not been built yet -- call touchstone_index." }
                },
                "required": ["bundle", "concepts", "tools", "indexed"]
            }),
            read_only("Discover capabilities"),
        ),
        tool(
            "touchstone_export",
            "Export raw bytes",
            "Write every concept's raw bytes to a directory, byte-for-byte identical to the \
             originals. This is the portability guarantee: no serializer sits in the write path, \
             so nothing can be dropped or reformatted on the way out.",
            json!({
                "type": "object",
                "properties": {
                    "out_dir": { "type": "string", "description": "Destination directory. Created if absent; existing files are overwritten." }
                },
                "required": ["out_dir"],
                "additionalProperties": false
            }),
            json!({
                "type": "object",
                "properties": { "exported": { "type": "integer" }, "out_dir": { "type": "string" } },
                "required": ["exported", "out_dir"]
            }),
            writes("Export raw bytes", false, true),
        ),
        tool(
            "touchstone_fmt",
            "Canonicalize frontmatter",
            "Canonicalize frontmatter key order. REFUSES any file it cannot reproduce exactly \
             (YAML anchors, aliases, merge keys, block scalars) rather than risk rewriting \
             authored content, and reports what it skipped and why. Use `check: true` to see what \
             would change without writing.",
            json!({
                "type": "object",
                "properties": {
                    "check": { "type": "boolean", "default": false,
                               "description": "Report what would change without modifying any file." },
                    "limit": limit_prop()
                },
                "additionalProperties": false
            }),
            json!({
                "type": "object",
                "properties": {
                    "changed": { "type": "integer" }, "skipped": { "type": "integer" },
                    "total": { "type": "integer" },
                    "files": { "type": "array", "items": { "type": "object" } }
                },
                "required": ["changed", "skipped", "total", "files"]
            }),
            writes("Canonicalize frontmatter", true, true),
        ),
        tool(
            "touchstone_index",
            "Rebuild the index",
            "Rebuild the derived index and every generated index.md. Idempotent and incremental \
             on content hash. Everything it produces is disposable -- deleting the derived plane \
             and re-running yields byte-identical output.",
            json!({ "type": "object", "additionalProperties": false }),
            json!({
                "type": "object",
                "properties": {
                    "indexed": { "type": "integer" },
                    "new": { "type": "integer" }, "changed": { "type": "integer" },
                    "removed": { "type": "integer" },
                    "indexes_written": { "type": "integer", "description": "Generated index.md files rewritten." },
                    "broken_links": { "type": "integer", "description": "Unresolved links. Legal per spec -- not-yet-written knowledge." },
                    "errors": { "type": "array", "items": { "type": "object" } }
                },
                "required": ["indexed", "errors"]
            }),
            writes("Rebuild the index", true, true),
        ),
        tool(
            "touchstone_ingest",
            "Ingest source material",
            "Copy a source document into the immutable `raw/` layer, verbatim. Nothing is parsed, \
             converted or summarised on the way in -- raw material is what every concept is later \
             checked against, so it must come back out byte-identical. After ingesting, compile \
             concepts from it with touchstone_new and cite it in `sources`; that citation is what \
             moves it off the unprocessed queue.",
            json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Filename to store it under in raw/, e.g. interview-2026-08.txt" },
                    "content": { "type": "string", "description": "The document's text content, verbatim." }
                },
                "required": ["name", "content"],
                "additionalProperties": false
            }),
            json!({
                "type": "object",
                "properties": {
                    "ingested": { "type": "array", "items": { "type": "string" } },
                    "skipped": { "type": "array", "items": { "type": "object" } }
                },
                "required": ["ingested", "skipped"]
            }),
            writes("Ingest source material", false, false),
        ),
        tool(
            "touchstone_lint",
            "Check conformance",
            "Check every concept against the OKF conformance floor plus the duplicate rules that \
             matter in practice: duplicate tags, duplicate verified principals, `verified` entries \
             missing `by`, sources missing `resource`, invalid status, and [[wikilinks]] (which are \
             not OKF and will not resolve). Broken links are NOT problems -- they are legal and \
             represent knowledge not yet written.",
            json!({
                "type": "object",
                "properties": { "limit": limit_prop(), "fields": fields_prop("path, message") },
                "additionalProperties": false
            }),
            json!({
                "type": "object",
                "properties": {
                    "total": { "type": "integer", "description": "Total problems found, which may exceed the number returned." },
                    "returned": { "type": "integer" },
                    "problems": { "type": "array", "items": { "type": "object" } }
                },
                "required": ["total", "returned", "problems"]
            }),
            read_only("Check conformance"),
        ),
        tool(
            "touchstone_new",
            "Scaffold a concept",
            "Scaffold a conformant concept. Frontmatter is emitted through a YAML dumper, never \
             string concatenation. NOTE: this can never write a `verified` claim -- an agent may \
             not assert human verification, and the trust tier is derived, not authored. Pass \
             `generated_by` to record which agent wrote it.",
            json!({
                "type": "object",
                "properties": {
                    "type": { "type": "string", "description": "OKF concept type, e.g. Note, Decision, Term, Runbook." },
                    "title": { "type": "string", "description": "Human-readable title. Also determines the filename slug." },
                    "description": { "type": "string", "description": "One-line summary, indexed for search." },
                    "tags": { "type": "array", "items": { "type": "string" }, "description": "Free-form tags, filterable via touchstone_search." },
                    "dir": { "type": "string", "description": "Subdirectory. Defaults to the lowercased type plus 's'." },
                    "generated_by": { "type": "string", "description": "Agent identifier, e.g. capture/claude-opus-5." }
                },
                "required": ["type", "title"],
                "additionalProperties": false
            }),
            json!({
                "type": "object",
                "properties": { "path": { "type": "string" }, "trust": { "type": "string" } },
                "required": ["path", "trust"]
            }),
            writes("Scaffold a concept", false, false),
        ),
        tool(
            "touchstone_search",
            "Search the bundle",
            "Search concepts: structured prefilter, then BM25, then one graph hop, then trust rank. \
             Filters are applied server-side inside the query -- always pass them rather than \
             fetching broadly and filtering yourself. Returns concept PATHS, not chunks: read the \
             whole file with touchstone_show when you need its content.",
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Full-text query. Required and non-empty." },
                    "type": { "type": "string", "description": "Filter by OKF concept type." },
                    "tag": { "type": "string", "description": "Filter to concepts carrying this tag. See touchstone_discover for what exists." },
                    "status": { "type": "string", "enum": ["draft", "stable", "deprecated"] },
                    "trust": { "type": "string", "enum": ["human", "attested", "machine", "unattributed"] },
                    "limit": limit_prop(),
                    "fields": fields_prop("path, title, type, description, trust, via"),
                    "expand": { "type": "boolean", "default": true,
                                "description": "Include concepts one graph hop from a match. Set false for exact matches only." }
                },
                "required": ["query"],
                "additionalProperties": false
            }),
            json!({
                "type": "object",
                "properties": {
                    "returned": { "type": "integer" },
                    "limit": { "type": "integer" },
                    "hits": { "type": "array", "items": hit_schema() }
                },
                "required": ["returned", "limit", "hits"]
            }),
            read_only("Search the bundle"),
        ),
        tool(
            "touchstone_show",
            "Show one concept",
            "Return one concept's derived view: type, title, trust tier, status, tags, resolved \
             links, and its parsed frontmatter verbatim -- unknown keys included, temporal values \
             as the strings they were authored as. The `path` comes from touchstone_search or \
             touchstone_stats; there is no id to resolve first.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Bundle-relative concept path, e.g. notes/why-files-win.md" },
                    "fields": fields_prop("path, type, title, status, trust, tags, links, frontmatter")
                },
                "required": ["path"],
                "additionalProperties": false
            }),
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" }, "type": { "type": "string" }, "title": { "type": "string" },
                    "status": { "type": "string" }, "trust": { "type": "string" },
                    "conformant": { "type": "boolean" },
                    "tags": { "type": "array", "items": { "type": "string" } },
                    "links": { "type": "array", "items": { "type": "string" } },
                    "frontmatter": { "type": "object" }
                },
                "required": ["path", "type", "title", "trust"]
            }),
            read_only("Show one concept"),
        ),
        tool(
            "touchstone_stats",
            "Summarise the bundle",
            "Concept counts by type, trust tier and status, plus link and broken-link counts. A \
             cheap way to understand the shape of a bundle before searching it.",
            json!({ "type": "object", "additionalProperties": false }),
            json!({
                "type": "object",
                "properties": {
                    "concepts": { "type": "integer" },
                    "by_type": { "type": "array", "items": { "type": "object" } },
                    "by_trust": { "type": "array", "items": { "type": "object" } },
                    "by_status": { "type": "array", "items": { "type": "object" } },
                    "links": { "type": "integer" }, "broken_links": { "type": "integer" }
                },
                "required": ["concepts", "links", "broken_links"]
            }),
            read_only("Summarise the bundle"),
        ),
        tool(
            "touchstone_unprocessed",
            "List uncompiled sources",
            "Raw documents that no concept cites yet -- your work queue. Read one, write concepts \
             capturing what it says, and cite it in each concept's `sources`; it then leaves this \
             list. Derived rather than tracked: a document counts as processed exactly when some \
             concept names it, so there is no separate state to go stale.",
            json!({
                "type": "object",
                "properties": { "limit": limit_prop() },
                "additionalProperties": false
            }),
            json!({
                "type": "object",
                "properties": {
                    "total": { "type": "integer" }, "uncited": { "type": "integer" },
                    "documents": { "type": "array", "items": { "type": "string" } }
                },
                "required": ["total", "uncited", "documents"]
            }),
            read_only("List uncompiled sources"),
        ),
        tool(
            "touchstone_verify",
            "Verify signed claims",
            "Check that every concept claiming human verification is backed by a valid signature \
             over its CURRENT bytes. Reports three distinct failures: `unbacked` (claimed, never \
             signed), `stale` (signed, then edited -- the verified bytes are not these bytes), and \
             `bad_signature` (forged, or signed by a key this bundle does not list). Use this \
             before relying on a `human` trust tier from a bundle you did not author. NOTE: you \
             cannot create attestations -- signing is deliberately unavailable to agents.",
            json!({ "type": "object", "additionalProperties": false }),
            json!({
                "type": "object",
                "properties": {
                    "checked": { "type": "integer" }, "backed": { "type": "integer" },
                    "clean": { "type": "boolean" },
                    "problems": { "type": "array", "items": { "type": "object" } }
                },
                "required": ["checked", "backed", "clean", "problems"]
            }),
            read_only("Verify signed claims"),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_declared_name_is_built_exactly_once() {
        let built: Vec<&str> = all().iter().map(|t| t.name.as_ref().to_string()).collect::<Vec<_>>()
            .leak()
            .iter()
            .map(|s| s.as_str())
            .collect();
        assert_eq!(built, TOOL_NAMES, "TOOL_NAMES and all() have diverged");
    }

    #[test]
    fn tool_order_is_deterministic() {
        let a: Vec<String> = all().iter().map(|t| t.name.to_string()).collect();
        let b: Vec<String> = all().iter().map(|t| t.name.to_string()).collect();
        assert_eq!(a, b);
        let mut sorted = a.clone();
        sorted.sort();
        assert_eq!(a, sorted, "a sorted list is the cheapest way to stay deterministic");
    }

    #[test]
    fn names_are_legal_mcp_tool_names() {
        for t in all() {
            let n = t.name.as_ref();
            assert!((1..=128).contains(&n.len()), "{n}: bad length");
            assert!(
                n.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')),
                "{n}: illegal character"
            );
        }
    }

    #[test]
    fn every_tool_declares_an_output_schema() {
        for t in all() {
            assert!(t.output_schema.is_some(), "{}: no outputSchema", t.name);
            assert!(t.description.is_some(), "{}: no description", t.name);
            assert!(t.annotations.is_some(), "{}: no annotations", t.name);
        }
    }
}
