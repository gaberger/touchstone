//! The AI-readiness scorecard, as assertions.
//!
//! `gaberger/api-ai-readiness` grades an API on six dimensions for how well it serves an
//! autonomous agent. Its own SPEC notes that roughly 70% is statically gradable from the schema
//! — so that 70% can be a test rather than a report, which is what this file makes it.
//!
//! Each test names the dimension it defends and the gap the rubric would otherwise emit.

use serde_json::Value;
use touchstone_mcp_adapter::tools::{all, DEFAULT_LIMIT, MAX_LIMIT};

/// Tools that return a list and so are subject to the list-shaped dimensions.
const LIST_SHAPED: &[&str] = &["touchstone_search", "touchstone_lint", "touchstone_fmt"];

fn schema_of(tool: &str) -> Value {
    let t = all().into_iter().find(|t| t.name.as_ref() == tool).expect("tool exists");
    Value::Object((*t.input_schema).clone())
}

fn props(tool: &str) -> Value {
    schema_of(tool).get("properties").cloned().unwrap_or(Value::Null)
}

/// Dimension 1 — response discipline.
///
/// Rubric gap: "list endpoint has NO limit/page-size param — unbounded response blows the
/// context window." This is the single largest constraint on an agent, so it is checked hardest:
/// the parameter must exist AND declare a default, because a limit an agent has to know to pass
/// is a limit that will be forgotten.
#[test]
fn every_list_shaped_tool_bounds_its_response_by_default() {
    for tool in LIST_SHAPED {
        let limit = props(tool).get("limit").cloned().unwrap_or(Value::Null);
        assert!(!limit.is_null(), "{tool}: no `limit` -- responses would be unbounded");
        assert_eq!(
            limit.get("default").and_then(Value::as_u64),
            Some(DEFAULT_LIMIT as u64),
            "{tool}: `limit` has no default; an agent that omits it gets everything"
        );
        assert_eq!(
            limit.get("maximum").and_then(Value::as_u64),
            Some(MAX_LIMIT as u64),
            "{tool}: `limit` has no ceiling"
        );
    }
}

/// Dimension 1, second signal: the result carries a count, not a bare array. An agent needs to
/// know whether it saw everything.
#[test]
fn list_results_report_totals_rather_than_returning_a_bare_array() {
    for tool in LIST_SHAPED {
        let t = all().into_iter().find(|t| t.name.as_ref() == *tool).unwrap();
        let out = Value::Object((**t.output_schema.as_ref().expect("outputSchema")).clone());
        assert_eq!(
            out.get("type").and_then(Value::as_str),
            Some("object"),
            "{tool}: a bare array tells the agent nothing about what it did not see"
        );
        let keys: Vec<&str> = out["properties"].as_object().unwrap().keys().map(String::as_str).collect();
        assert!(
            keys.iter().any(|k| ["total", "returned"].contains(k)),
            "{tool}: result has no count field, only {keys:?}"
        );
    }
}

/// Dimension 2 — field selection.
///
/// Rubric gap: "no field-selection param — the agent must pull every column."
#[test]
fn result_bearing_tools_allow_field_selection() {
    for tool in ["touchstone_search", "touchstone_show", "touchstone_lint"] {
        let fields = props(tool).get("fields").cloned().unwrap_or(Value::Null);
        assert!(!fields.is_null(), "{tool}: no `fields` -- the agent must take every column");
        assert_eq!(fields.get("type").and_then(Value::as_str), Some("array"));
    }
}

/// Dimension 3 — retrieval shape.
///
/// Rubric gap: "no server-side filter params — the agent must over-fetch, then filter
/// in-context." Filtering in context is the expensive failure: it costs the tokens the filter
/// was supposed to save.
#[test]
fn search_filters_server_side() {
    let p = props("touchstone_search");
    for filter in ["type", "tag", "status", "trust"] {
        assert!(p.get(filter).is_some(), "touchstone_search: no server-side `{filter}` filter");
    }
    // Enumerated where the value set is closed, so an agent can see the legal values without
    // guessing and without a round-trip.
    for enumerated in ["status", "trust"] {
        assert!(
            p[enumerated].get("enum").is_some(),
            "touchstone_search: `{enumerated}` should enumerate its legal values"
        );
    }
}

/// Dimension 4 — self-description.
///
/// Rubric gap: "no documented 4xx error response — the agent can't recover from a bad call."
/// MCP has no status codes, so the equivalent is that every recoverable failure is a
/// tool-execution error whose text says what to do next. Asserted behaviourally in
/// `surface.rs`; here we check the schemas describe their own constraints well enough that a
/// bad call is avoidable in the first place.
#[test]
fn schemas_describe_themselves_well_enough_to_avoid_a_bad_call() {
    for t in all() {
        let name = t.name.as_ref();
        let desc = t.description.as_ref().expect("description");
        assert!(desc.len() > 80, "{name}: description too thin to act on ({} chars)", desc.len());

        let schema = Value::Object((*t.input_schema).clone());
        if let Some(props) = schema.get("properties").and_then(Value::as_object) {
            for (key, spec) in props {
                assert!(
                    spec.get("description").is_some() || spec.get("enum").is_some(),
                    "{name}.{key}: neither described nor enumerated -- an agent must guess"
                );
            }
        }
        // Closed schemas: an unknown argument should be rejected, not silently ignored.
        assert_eq!(
            schema.get("additionalProperties").and_then(Value::as_bool),
            Some(false),
            "{name}: schema is open, so a typo'd argument fails silently"
        );
    }
}

/// Dimension 5 — workflow atomicity.
///
/// Rubric gap: "chains N resource ids — multi-stage: each must come from a prior call."
/// A concept path is the only identifier in this surface, and it is obtainable in one call from
/// search, stats or discover. There is no id to resolve before an id.
#[test]
fn no_tool_requires_a_chain_of_resource_ids() {
    for t in all() {
        let name = t.name.as_ref();
        let schema = Value::Object((*t.input_schema).clone());
        let required: Vec<String> = schema
            .get("required")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
            .unwrap_or_default();

        let id_like: Vec<&String> = required
            .iter()
            .filter(|r| r.ends_with("_id") || r.ends_with("Id") || *r == "path")
            .collect();
        assert!(
            id_like.len() <= 1,
            "{name}: requires {} identifiers ({id_like:?}) -- each would need its own prior call",
            id_like.len()
        );
    }
}

/// Dimension 6 — discovery.
///
/// Rubric signal: a capability-discovery endpoint exists. Scored once per API.
#[test]
fn a_single_call_describes_the_whole_surface() {
    let d = all().into_iter().find(|t| t.name.as_ref() == "touchstone_discover");
    let d = d.expect("no discovery tool -- the rubric scores this once per API and it is cheap");
    let out = Value::Object((**d.output_schema.as_ref().unwrap()).clone());
    let props = out["properties"].as_object().unwrap();
    for key in ["tools", "filters", "concepts"] {
        assert!(props.contains_key(key), "discovery omits `{key}`");
    }
    assert_eq!(
        Value::Object((*d.input_schema).clone()).get("required"),
        None,
        "discovery must be callable with no arguments -- it is what you call when you know nothing"
    );
}

/// Annotations are how a client decides what needs human confirmation. A tool that writes but
/// claims to be read-only would get auto-approved.
#[test]
fn write_tools_are_not_labelled_read_only() {
    let writers =
        ["touchstone_index", "touchstone_new", "touchstone_fmt", "touchstone_export", "touchstone_ingest"];
    for t in all() {
        let name = t.name.as_ref().to_string();
        let a = t.annotations.as_ref().unwrap_or_else(|| panic!("{name}: no annotations"));
        let read_only = a.read_only_hint.unwrap_or(false);
        if writers.contains(&name.as_str()) {
            assert!(!read_only, "{name} writes but is annotated read-only");
        } else {
            assert!(read_only, "{name} does not write but is not annotated read-only");
        }
    }
}
