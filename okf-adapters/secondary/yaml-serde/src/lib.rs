//! Secondary adapter — FrontmatterParser port backed by serde_yaml_ng.
//!
//! Imports `okf-ports` ONLY (hex ADR-001). Cannot reach another adapter because
//! Cargo will not resolve it.
//!
//! Key guarantee (FINDINGS.md E3a / RUST-PATH.md §1):
//!   Temporal values (`at:`, `stale_after:`, `usage_window:`) stay ISO 8601 strings.
//!   serde_yaml_ng deserialises them as Value::String without any timestamp coercion,
//!   so the silent-rewrite bug PyYAML had is structurally absent here.
use okf_ports::{Concept, FrontmatterParser};
use serde_yaml_ng::Value;

pub struct YamlSerde;

const DELIM: &str = "---";

/// Extract the text between the opening and closing `---` delimiters.
/// Returns `Err` when no frontmatter block is present or it is unterminated.
fn split_frontmatter(text: &str) -> Result<String, String> {
    // Tolerate UTF-8 BOM (U+FEFF).
    let text = text.trim_start_matches('\u{FEFF}');
    // Normalise CRLF so delimiter matching works on Windows-style files.
    let norm = text.replace("\r\n", "\n");

    if !norm.starts_with("---") {
        return Err("no frontmatter".to_string());
    }

    // The very first line must be exactly `---`.
    let after_first = match norm.strip_prefix("---\n") {
        Some(s) => s,
        None => {
            if norm.trim() == "---" { "" } else {
                return Err("no frontmatter".to_string());
            }
        }
    };

    // Collect lines between the two delimiters.
    let mut fm_lines: Vec<&str> = Vec::new();
    for line in after_first.split('\n') {
        if line.trim() == DELIM {
            return Ok(fm_lines.join("\n"));
        }
        fm_lines.push(line);
    }

    Err("unterminated frontmatter".to_string())
}

impl FrontmatterParser for YamlSerde {
    /// Parse raw concept bytes into the minimal query view.
    ///
    /// Behaviour that matches the Python oracle (okf.py):
    /// - `type` must be present and non-empty; any string value is accepted including
    ///   unknown types not in the OKF vocabulary (spec: consumers MUST NOT reject them).
    /// - Unknown frontmatter keys are silently accepted; they are not visible in the
    ///   minimal `Concept` view but they do not cause an error.
    /// - Temporal field values (`at:`, `stale_after:`, `usage_window:` etc.) remain
    ///   ISO 8601 strings — serde_yaml_ng does not coerce them to datetime objects.
    /// - `path` is set to `""` because raw bytes carry no path information; the caller
    ///   (ConceptRepository) is responsible for filling it in.
    fn parse(&self, raw: &[u8]) -> Result<Concept, String> {
        let text = std::str::from_utf8(raw).map_err(|e| format!("invalid UTF-8: {e}"))?;

        let fm_text = split_frontmatter(text)?;

        let value: Value = serde_yaml_ng::from_str(&fm_text)
            .map_err(|e| format!("invalid YAML: {}", first_line(&e.to_string())))?;

        let mapping = match value {
            Value::Mapping(m) => m,
            // Empty frontmatter (`---\n---`) deserialises as Null.
            Value::Null => {
                return Err("missing or empty `type`".to_string());
            }
            _ => return Err("frontmatter is not a mapping".to_string()),
        };

        // `type` is the only REQUIRED field in OKF v0.2.
        // Any non-empty string value is preserved — unknown types must not be rejected.
        let concept_type = match mapping.get("type") {
            Some(Value::String(s)) if !s.trim().is_empty() => s.clone(),
            Some(_) | None => return Err("missing or empty `type`".to_string()),
        };

        // `title` is optional; derive from filename when absent (caller's responsibility
        // since we have no path here — return empty and let the repository layer fill it).
        let title = match mapping.get("title") {
            Some(Value::String(s)) => s.clone(),
            _ => String::new(),
        };

        Ok(Concept {
            path: String::new(),
            concept_type,
            title,
        })
    }
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use okf_ports::FrontmatterParser;

    fn parse(raw: &str) -> Result<Concept, String> {
        YamlSerde.parse(raw.as_bytes())
    }

    // ── happy-path ────────────────────────────────────────────────────────────

    #[test]
    fn basic_concept_parsed() {
        let raw = "---\ntype: Note\ntitle: Hello world\n---\n\nBody.\n";
        let c = parse(raw).unwrap();
        assert_eq!(c.concept_type, "Note");
        assert_eq!(c.title, "Hello world");
    }

    #[test]
    fn title_absent_returns_empty_string() {
        let raw = "---\ntype: Decision\n---\n\nBody.\n";
        let c = parse(raw).unwrap();
        assert_eq!(c.concept_type, "Decision");
        assert_eq!(c.title, "");
    }

    // ── E3a: temporal values must stay ISO 8601 strings ───────────────────────
    //
    // This is the single most important behavioural requirement (FINDINGS.md E3a,
    // RUST-PATH.md §1).  PyYAML coerces `at: 2026-01-01T00:00:00Z` to a Python
    // datetime and re-emits it as `2026-01-01 00:00:00+00:00` — breaking the spec.
    // serde_yaml_ng does NOT coerce: temporal scalars remain Value::String.

    #[test]
    fn temporal_values_stay_iso8601_strings() {
        // Parse the YAML directly through serde_yaml_ng::Value to inspect every field,
        // not just those exposed by the minimal Concept view.
        let fm = "\
type: Metric
title: Temporal test
at: 2026-01-01T00:00:00Z
stale_after: 2026-09-23
usage_window: 2026-06
generated:
  at: 2026-03-15T12:00:00Z
  by: agent:test
";
        let value: Value = serde_yaml_ng::from_str(fm).unwrap();
        let m = value.as_mapping().unwrap();

        assert!(
            matches!(m.get("at"), Some(Value::String(_))),
            "`at:` must stay a string, not be coerced to a datetime"
        );
        assert!(
            matches!(m.get("stale_after"), Some(Value::String(_))),
            "`stale_after:` must stay a string"
        );
        assert!(
            matches!(m.get("usage_window"), Some(Value::String(_))),
            "`usage_window:` must stay a string"
        );

        // Nested temporal inside `generated.at`
        let gen = m.get("generated").and_then(Value::as_mapping).unwrap();
        assert!(
            matches!(gen.get("at"), Some(Value::String(_))),
            "`generated.at` must stay a string"
        );
    }

    // ── unknown keys must be preserved (not dropped, not cause an error) ──────

    #[test]
    fn unknown_frontmatter_keys_do_not_cause_error() {
        // `retention`, `classification`, `custom_field` are not in the OKF vocabulary.
        let raw = "---\ntype: Note\ntitle: Retained\nretention: 7y\nclassification: internal\ncustom_field: whatever\n---\n\nBody.\n";
        let c = parse(raw).expect("unknown keys must not cause a parse error");
        assert_eq!(c.concept_type, "Note");
        // The raw bytes are authoritative; the Concept is a query view.
        // Unknown keys are visible in the underlying Value but not in Concept.
    }

    // ── unknown type values must be preserved (spec: consumers MUST NOT reject) ─

    #[test]
    fn unknown_type_value_is_preserved_not_rejected() {
        // `ThreatModel` is not in the standard OKF type vocabulary.
        let raw = "---\ntype: ThreatModel\ntitle: Unknown type survives\n---\n\nBody.\n";
        let c = parse(raw).expect("unknown type must not be rejected");
        assert_eq!(c.concept_type, "ThreatModel");
    }

    // ── combined: unknown key + unknown type + ISO 8601 timestamp in one doc ──
    //
    // This is the FINDINGS.md E3a gate case: all three requirements in one fixture.

    #[test]
    fn combined_unknown_key_unknown_type_and_iso8601_timestamp() {
        let raw = "\
---
type: ThreatModel
title: Combined gate test
retention: 7y
at: 2026-01-01T00:00:00Z
stale_after: 2026-09-23
classification: internal
---

Body with a [broken link](/does/not/exist.md).
";
        // Parse succeeds despite unknown type and unknown keys.
        let c = parse(raw).expect("combined case must parse without error");
        assert_eq!(c.concept_type, "ThreatModel");
        assert_eq!(c.title, "Combined gate test");

        // Also verify via serde_yaml_ng::Value that temporal values are strings.
        let fm_text = split_frontmatter(raw).unwrap();
        let value: Value = serde_yaml_ng::from_str(&fm_text).unwrap();
        let m = value.as_mapping().unwrap();

        assert_eq!(m.get("type").and_then(Value::as_str), Some("ThreatModel"));
        assert_eq!(m.get("retention").and_then(Value::as_str), Some("7y"),
            "unknown key `retention` must round-trip through the parser");
        assert!(
            matches!(m.get("at"), Some(Value::String(_))),
            "`at:` in combined fixture must stay a string"
        );
        assert!(
            matches!(m.get("stale_after"), Some(Value::String(_))),
            "`stale_after:` in combined fixture must stay a string"
        );
    }

    // ── fixture files: _fixture/ ──────────────────────────────────────────────

    #[test]
    fn fixture_unknown_type_md() {
        // _fixture/adversarial/unknown-type.md: ThreatModel type + two unknown keys.
        let raw = include_str!("../../../../_fixture/adversarial/unknown-type.md");
        let c = parse(raw).unwrap();
        assert_eq!(c.concept_type, "ThreatModel");
        assert_eq!(c.title, "Unknown type survives");
    }

    #[test]
    fn fixture_verified_chain_md_temporal_fields() {
        // _fixture/adversarial/verified-chain.md has `at:` fields inside verified[].
        let raw = include_str!("../../../../_fixture/adversarial/verified-chain.md");
        let c = parse(raw).unwrap();
        assert_eq!(c.concept_type, "Metric");

        let fm_text = split_frontmatter(raw).unwrap();
        let value: Value = serde_yaml_ng::from_str(&fm_text).unwrap();
        let m = value.as_mapping().unwrap();

        // usage_window is an inline mapping, not a string — just verify parsing works.
        assert!(m.contains_key("usage_window"), "usage_window must be preserved");

        // verified[].at must stay strings.
        if let Some(Value::Sequence(entries)) = m.get("verified") {
            for entry in entries {
                if let Some(entry_map) = entry.as_mapping() {
                    if let Some(at_val) = entry_map.get("at") {
                        assert!(
                            matches!(at_val, Value::String(_)),
                            "verified[].at must stay a string, got {at_val:?}"
                        );
                    }
                }
            }
        }
    }

    // ── _upstream/: third-party OKF, read-only oracle ─────────────────────────

    #[test]
    fn upstream_acme_retail_log_md() {
        // acme_retail/log.md uses `type: Log` — an upstream-defined type, not our vocab.
        let raw = include_str!("../../../../_upstream/acme_retail/log.md");
        let c = parse(raw).unwrap();
        assert_eq!(c.concept_type, "Log");
    }

    // ── error cases ───────────────────────────────────────────────────────────

    #[test]
    fn no_frontmatter_returns_error() {
        let raw = "Just a markdown file with no frontmatter.\n";
        assert!(parse(raw).is_err(), "no frontmatter must return Err");
    }

    #[test]
    fn empty_type_returns_error() {
        let raw = "---\ntype: \ntitle: Empty type\n---\n\nBody.\n";
        assert!(parse(raw).is_err(), "empty type must return Err");
    }

    #[test]
    fn missing_type_returns_error() {
        let raw = "---\ntitle: No type field\n---\n\nBody.\n";
        assert!(parse(raw).is_err(), "missing type must return Err");
    }

    #[test]
    fn invalid_yaml_returns_error() {
        let raw = "---\ntype: Note\nbad: [unclosed\n---\n";
        assert!(parse(raw).is_err(), "invalid YAML must return Err");
    }

    #[test]
    fn crlf_line_endings_handled() {
        let raw = "---\r\ntype: Note\r\ntitle: CRLF\r\n---\r\n\r\nBody.\r\n";
        let c = parse(raw).unwrap();
        assert_eq!(c.concept_type, "Note");
    }
}
