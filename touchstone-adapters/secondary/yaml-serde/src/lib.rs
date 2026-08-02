//! Secondary adapter — the OKF frontmatter parser, backed by serde_yaml_ng.
//!
//! Imports `touchstone-ports` ONLY (ARCHITECTURE.md rule 4a). Cannot reach another adapter because
//! Cargo will not resolve it.
//!
//! **This adapter is the only place in the workspace that knows what YAML is.** That is the
//! point of making the parser a port (ADR-2608010940): two conformant YAML libraries disagree
//! on the same bytes — `serde_yaml_ng` does not implement merge keys, so a concept carrying
//! `verified: [{<<: *defaults}]` reads as human-verified under PyYAML and unattributed here —
//! and a divergence that lives behind a port is a contract test rather than a latent bug in
//! ranking.
//!
//! Key guarantee (FINDINGS E3a / RUST-PATH §1): temporal values (`at:`, `stale_after:`,
//! `usage_window:`) stay ISO 8601 strings. serde_yaml_ng deserialises them as `Value::String`
//! with no timestamp coercion, so the silent-rewrite defect PyYAML had is structurally absent.
//!
//! Raw bytes remain authoritative throughout: `ParsedConcept.raw` is a verbatim copy, and the
//! CRLF normalisation below applies only to the derived text view. That asymmetry is what makes
//! byte-exact export (T2a) possible at all.

use touchstone_ports::{
    split_frontmatter, Concept, ConceptParser, FrontmatterParser, NewConceptRequest,
    ParsedConcept, Trust, VerifiedEntry,
};
use serde_yaml_ng::{Mapping, Value};

pub struct YamlSerde;

/// OKF canonical frontmatter key order. Known keys are emitted first in this order; unknown
/// keys follow in their authored order, because the spec requires consumers to preserve what
/// they do not recognise rather than tidy it away.
const KEY_ORDER: &[&str] = &[
    "id", "type", "title", "description", "resource", "tags", "aliases", "status", "stale_after",
    "generated", "verified", "sources",
];

// ── Constructs the formatter must refuse ───────────────────────────────────────

/// True when the frontmatter text contains constructs `fmt` cannot safely reproduce:
/// anchors (`&`), aliases (`*`), merge keys (`<<:`), or block scalars (`: |`, `: >`).
///
/// This predicate exists because building the formatter revealed the danger (FINDINGS E3b):
/// the naive canonicalizer resolved merge keys, invented an `&id001` anchor on an unrelated
/// timestamp, and flattened a shell script under `script: |` into a quoted string. Refusing is
/// the only safe answer, so the detector is deliberately over-eager.
pub fn is_risky(fm_text: &str) -> bool {
    for line in fm_text.lines() {
        let trimmed = line.trim();
        if let Some(pos) = line.find(':') {
            let after = line[pos + 1..].trim();
            if after.starts_with('|') || after.starts_with('>') {
                return true;
            }
        }
        if trimmed.contains("<<:") {
            return true;
        }
        if trimmed.starts_with('&') || trimmed.starts_with('*') {
            return true;
        }
        for (i, ch) in line.char_indices() {
            if (ch == '&' || ch == '*') && i > 0 {
                let prev = line[..i].chars().last();
                if prev == Some(' ') || prev == Some('\t') {
                    return true;
                }
            }
        }
    }
    false
}

// ── YAML → JSON ────────────────────────────────────────────────────────────────

/// Serialise a mapping to a JSON object string.
///
/// Hand-rolled rather than routed through a typed intermediate so that scalars survive exactly
/// as YAML read them: a timestamp that arrived as a string leaves as a string. That is the
/// property drill T2d asserts, and a re-encode is precisely where it would be lost.
fn fm_to_json(fm: &Mapping) -> String {
    value_to_json(&Value::Mapping(fm.clone()))
}

fn value_to_json(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string()),
        Value::Sequence(seq) => {
            let items: Vec<String> = seq.iter().map(value_to_json).collect();
            format!("[{}]", items.join(","))
        }
        Value::Mapping(m) => {
            let pairs: Vec<String> = m
                .iter()
                .map(|(k, val)| {
                    let key = match k {
                        Value::String(s) => serde_json::to_string(s).unwrap_or_default(),
                        _ => "\"?\"".to_string(),
                    };
                    format!("{}:{}", key, value_to_json(val))
                })
                .collect();
            format!("{{{}}}", pairs.join(","))
        }
        Value::Tagged(t) => value_to_json(&t.value),
    }
}

// ── Canonical emission ─────────────────────────────────────────────────────────

/// Deterministic YAML dump: known keys in `KEY_ORDER`, unknown keys appended in authored order.
pub fn canonical_frontmatter(fm: &Mapping) -> String {
    let mut ordered = Mapping::new();
    for &k in KEY_ORDER {
        if let Some(v) = fm.get(k) {
            ordered.insert(Value::String(k.to_string()), v.clone());
        }
    }
    for (k, v) in fm {
        match k {
            Value::String(ks) if KEY_ORDER.contains(&ks.as_str()) => {}
            _ => {
                ordered.insert(k.clone(), v.clone());
            }
        }
    }
    serde_yaml_ng::to_string(&Value::Mapping(ordered))
        .unwrap_or_default()
        .trim_start_matches("---\n")
        .to_string()
}

// ── Parsing ────────────────────────────────────────────────────────────────────

struct Frontmatter {
    text: String,
    map: Mapping,
    body: String,
    error: Option<String>,
}

fn read_frontmatter(raw: &[u8]) -> Result<Frontmatter, String> {
    let text = std::str::from_utf8(raw).map_err(|_| "invalid UTF-8".to_string())?;
    // Normalise line endings for the DERIVED view only. `raw` is untouched.
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let (fm_opt, body) = split_frontmatter(&text);
    let body = body.to_string();

    let Some(fm_str) = fm_opt else {
        return Ok(Frontmatter {
            text: String::new(),
            map: Mapping::new(),
            body,
            error: Some("no frontmatter".to_string()),
        });
    };

    let fm_text = fm_str.to_string();
    match serde_yaml_ng::from_str::<Value>(fm_str) {
        Err(e) => Ok(Frontmatter {
            text: fm_text,
            map: Mapping::new(),
            body,
            error: Some(format!("invalid YAML: {}", e.to_string().lines().next().unwrap_or(""))),
        }),
        Ok(Value::Null) => Ok(Frontmatter {
            text: fm_text,
            map: Mapping::new(),
            body,
            error: Some("missing or empty `type`".to_string()),
        }),
        Ok(Value::Mapping(m)) => {
            let error = match m.get("type") {
                Some(Value::String(s)) if !s.trim().is_empty() => None,
                _ => Some("missing or empty `type`".to_string()),
            };
            Ok(Frontmatter { text: fm_text, map: m, body, error })
        }
        Ok(_) => Ok(Frontmatter {
            text: fm_text,
            map: Mapping::new(),
            body,
            error: Some("frontmatter is not a mapping".to_string()),
        }),
    }
}

fn str_field<'a>(fm: &'a Mapping, key: &str) -> &'a str {
    match fm.get(key) {
        Some(Value::String(s)) => s.as_str(),
        _ => "",
    }
}

/// Title falls back to a humanised filename, so an untitled concept is still addressable.
fn derive_title(fm: &Mapping, path: &str) -> String {
    match fm.get("title") {
        Some(Value::String(s)) if !s.trim().is_empty() => s.clone(),
        _ => {
            let stem = path.rsplit('/').next().unwrap_or(path);
            let stem = stem.strip_suffix(".md").unwrap_or(stem);
            stem.replace('-', " ").replace('_', " ")
        }
    }
}

fn derive_tags(fm: &Mapping) -> Vec<String> {
    match fm.get("tags") {
        Some(Value::Sequence(seq)) => {
            seq.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()
        }
        Some(Value::String(s)) => vec![s.clone()],
        _ => vec![],
    }
}

fn verified_entries(fm: &Mapping) -> Vec<VerifiedEntry> {
    let entries: Vec<&Mapping> = match fm.get("verified") {
        Some(Value::Sequence(seq)) => seq.iter().filter_map(|v| v.as_mapping()).collect(),
        Some(Value::Mapping(m)) => vec![m],
        _ => vec![],
    };
    entries
        .into_iter()
        .map(|e| VerifiedEntry {
            by: match e.get("by") {
                Some(Value::String(s)) => Some(s.clone()),
                _ => None,
            },
        })
        .collect()
}

/// Spec-derived trust tier. Never authored — see ARCHITECTURE.md "The trust invariant".
///
/// `verified[].by` starting with `human:` → Verified; `generated` present without a human
/// `verified` → Generated; neither → Unknown. Note `Trust::Attested` is unreachable from this
/// derivation: it is a documented tier with no rule behind it (FINDINGS E6).
fn derive_trust(fm: &Mapping) -> Trust {
    for entry in verified_entries(fm) {
        if entry.by.as_deref().is_some_and(|by| by.starts_with("human:")) {
            return Trust::Verified;
        }
    }
    if fm.contains_key("generated") {
        return Trust::Generated;
    }
    Trust::Unknown
}

fn source_missing_resource(fm: &Mapping) -> bool {
    match fm.get("sources") {
        Some(Value::Sequence(seq)) => seq.iter().any(|s| match s {
            Value::Mapping(m) => !m.contains_key("resource"),
            _ => true,
        }),
        _ => false,
    }
}

impl YamlSerde {
    /// The full parse. Inherent so the composition root can call it without a trait object.
    pub fn parse_concept(&self, path: &str, raw: &[u8]) -> ParsedConcept {
        let fmres = match read_frontmatter(raw) {
            Ok(f) => f,
            Err(e) => {
                return ParsedConcept {
                    path: path.to_string(),
                    raw: raw.to_vec(),
                    error: Some(e),
                    ..Default::default()
                }
            }
        };

        let fm = &fmres.map;
        let status = {
            let s = str_field(fm, "status");
            if s.is_empty() { "stable".to_string() } else { s.to_string() }
        };

        // Unformattable when there is nothing to format, when it did not parse, or when it
        // carries constructs that cannot be reproduced.
        let format_skip_reason = if fm.is_empty() {
            Some("no frontmatter".to_string())
        } else if let Some(ref e) = fmres.error {
            Some(e.clone())
        } else if is_risky(&fmres.text) {
            Some("contains anchors, aliases, merge keys or block scalars".to_string())
        } else {
            None
        };

        ParsedConcept {
            path: path.to_string(),
            concept_type: str_field(fm, "type").to_string(),
            title: derive_title(fm, path),
            description: str_field(fm, "description").to_string(),
            body: fmres.body,
            tags: derive_tags(fm),
            trust: derive_trust(fm),
            status,
            raw: raw.to_vec(),
            error: fmres.error,
            format_skip_reason,
            verified_entries: verified_entries(fm),
            has_source_missing_resource: source_missing_resource(fm),
            has_wikilinks: String::from_utf8_lossy(raw).contains("[["),
            frontmatter_json: if fm.is_empty() { String::new() } else { fm_to_json(fm) },
        }
    }
}

impl ConceptParser for YamlSerde {
    fn parse(&self, path: &str, raw: &[u8]) -> ParsedConcept {
        self.parse_concept(path, raw)
    }

    /// Canonical rewrite, or `None` when the concept must be refused.
    ///
    /// Re-parses `parsed.raw` rather than trusting a cached mapping: the raw bytes are the
    /// authoritative value, so re-reading them is what makes this safe to call on a concept
    /// some other layer handed along.
    fn canonicalize(&self, parsed: &ParsedConcept) -> Option<Vec<u8>> {
        if parsed.format_skip_reason.is_some() {
            return None;
        }
        let fmres = read_frontmatter(&parsed.raw).ok()?;
        if fmres.error.is_some() || fmres.map.is_empty() {
            return None;
        }
        let canon = canonical_frontmatter(&fmres.map);

        // Exactly one leading and one trailing newline around the body. Pinning both ends is
        // what makes `fmt` idempotent — without it a second run keeps adding or trimming
        // whitespace and the formatter never reaches a fixed point, so `--check` is never clean.
        let b = &fmres.body;
        let b = if b.starts_with('\n') { b.clone() } else { format!("\n{b}") };
        let b = b.trim_end_matches('\n');
        Some(format!("---\n{canon}---\n{b}\n").into_bytes())
    }

    fn emit_new(&self, req: &NewConceptRequest) -> Vec<u8> {
        // Emitted through the YAML dumper, never string concatenation: FINDINGS E3d found that
        // hand-crafted frontmatter carrying `: ` inside a value is the single most likely
        // authoring error, and a dumper cannot make it.
        let mut fm = Mapping::new();
        fn put(m: &mut Mapping, k: &str, v: Value) {
            m.insert(Value::String(k.to_string()), v);
        }
        put(&mut fm, "id", Value::String(req.id.clone()));
        put(&mut fm, "type", Value::String(req.concept_type.clone()));
        put(&mut fm, "title", Value::String(req.title.clone()));
        put(&mut fm, "description", Value::String(req.description.clone()));
        put(
            &mut fm,
            "tags",
            Value::Sequence(req.tags.iter().map(|t| Value::String(t.clone())).collect()),
        );
        put(&mut fm, "status", Value::String("draft".to_string()));

        if let Some(ref by) = req.generated_by {
            let mut g = Mapping::new();
            put(&mut g, "by", Value::String(by.clone()));
            put(&mut g, "at", Value::String(req.now_iso8601.clone()));
            put(&mut fm, "generated", Value::Mapping(g));
        }
        // NOTE: no `verified` key, ever. An agent may not author a human verification claim
        // (ARCHITECTURE.md "The trust invariant"); scaffolding one would launder a
        // machine-written concept into a trusted one.

        let body = format!("\n# {}\n\n", req.title);
        format!("---\n{}---\n{}", canonical_frontmatter(&fm), body).into_bytes()
    }
}

impl FrontmatterParser for YamlSerde {
    /// The minimal query view, derived from the same parse as everything else so the two
    /// cannot disagree about what a concept is.
    fn parse(&self, raw: &[u8]) -> Result<Concept, String> {
        let parsed = self.parse_concept("", raw);
        match parsed.error {
            Some(e) => Err(e),
            None => Ok(parsed.as_concept()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(raw: &str) -> ParsedConcept {
        YamlSerde.parse_concept("notes/x.md", raw.as_bytes())
    }

    #[test]
    fn temporal_values_stay_iso8601_strings() {
        let c = parse("---\ntype: Note\nstale_after: 2026-01-01T00:00:00Z\n---\n\nB.\n");
        assert!(
            c.frontmatter_json.contains("\"2026-01-01T00:00:00Z\""),
            "E3a regression -- timestamp coerced: {}",
            c.frontmatter_json
        );
    }

    #[test]
    fn unknown_keys_and_unknown_types_are_preserved() {
        let c = parse("---\ntype: ThreatModel\nretention: 7y\n---\n\nB.\n");
        assert_eq!(c.concept_type, "ThreatModel", "unknown type must survive");
        assert!(c.frontmatter_json.contains("\"retention\""), "unknown key dropped");
        assert!(c.conformant(), "an unknown type is still conformant per spec");
    }

    #[test]
    fn missing_type_is_non_conformant_but_still_parsed() {
        let c = parse("---\ntitle: No type\n---\n\nB.\n");
        assert!(!c.conformant());
        assert_eq!(c.error.as_deref(), Some("missing or empty `type`"));
        assert_eq!(c.title, "No type", "the rest of the parse must still be usable");
    }

    #[test]
    fn absent_frontmatter_is_an_error_not_a_panic() {
        let c = parse("just a body\n");
        assert_eq!(c.error.as_deref(), Some("no frontmatter"));
        assert_eq!(c.title, "x", "title falls back to the filename stem");
    }

    #[test]
    fn status_defaults_to_stable() {
        assert_eq!(parse("---\ntype: Note\n---\n\nB.\n").status, "stable");
    }

    #[test]
    fn trust_follows_the_spec_actor_convention() {
        let human = parse("---\ntype: Note\nverified:\n  - by: human:gary\n---\n\nB.\n");
        assert_eq!(human.trust, Trust::Verified);
        let machine = parse("---\ntype: Note\ngenerated:\n  by: agent:x\n---\n\nB.\n");
        assert_eq!(machine.trust, Trust::Generated);
        assert_eq!(parse("---\ntype: Note\n---\n\nB.\n").trust, Trust::Unknown);
    }

    /// A non-`human:` verifier must NOT reach the trusted tier. The whole ranking model rests
    /// on this, so it is asserted directly rather than inferred from the case above.
    #[test]
    fn a_machine_verifier_is_not_human_verified() {
        let c = parse("---\ntype: Note\nverified:\n  - by: agent:claude\n---\n\nB.\n");
        assert_eq!(c.trust, Trust::Unknown, "only `human:` grants the trusted tier");
    }

    #[test]
    fn risky_constructs_are_refused_by_the_formatter() {
        for raw in [
            "---\ntype: Note\nscript: |\n  echo hi\n---\n\nB.\n",
            "---\ntype: Note\ndefaults: &d\n  a: 1\n---\n\nB.\n",
            "---\ntype: Note\nv:\n  - <<: *d\n---\n\nB.\n",
        ] {
            let c = parse(raw);
            assert!(c.format_skip_reason.is_some(), "must refuse to reformat: {raw:?}");
            assert!(YamlSerde.canonicalize(&c).is_none());
        }
    }

    #[test]
    fn canonical_rewrite_preserves_values_and_reaches_a_fixed_point() {
        let c = parse("---\ntitle: Hello\ntype: Note\n---\n\nBody.\n");
        let once = YamlSerde.canonicalize(&c).expect("formattable");
        let c2 = YamlSerde.parse_concept("notes/x.md", &once);
        assert_eq!(c2.title, c.title);
        assert_eq!(c2.concept_type, c.concept_type);
        let twice = YamlSerde.canonicalize(&c2).expect("still formattable");
        assert_eq!(once, twice, "fmt must be idempotent or --check is never clean");
    }

    #[test]
    fn canonical_order_puts_type_before_title() {
        let c = parse("---\ntitle: Hello\ntype: Note\n---\n\nB.\n");
        let out = String::from_utf8(YamlSerde.canonicalize(&c).unwrap()).unwrap();
        assert!(out.find("type:").unwrap() < out.find("title:").unwrap());
    }

    #[test]
    fn unknown_keys_survive_canonicalisation() {
        let c = parse("---\ntype: Note\nretention: 7y\n---\n\nB.\n");
        let out = String::from_utf8(YamlSerde.canonicalize(&c).unwrap()).unwrap();
        assert!(out.contains("retention"), "canonicalise dropped an unknown key: {out}");
    }

    #[test]
    fn scaffolding_never_emits_a_verified_claim() {
        let req = NewConceptRequest {
            concept_type: "Note".into(),
            title: "A Thing".into(),
            generated_by: Some("capture/claude".into()),
            now_iso8601: "2026-08-02T00:00:00Z".into(),
            id: "abc123abc123".into(),
            ..Default::default()
        };
        let out = String::from_utf8(YamlSerde.emit_new(&req)).unwrap();
        assert!(!out.contains("verified"), "`new` must never author a verified claim: {out}");
        assert!(out.contains("generated:"));
        let back = YamlSerde.parse_concept("notes/a-thing.md", out.as_bytes());
        assert_eq!(back.trust, Trust::Generated);
        assert!(back.conformant());
    }

    #[test]
    fn raw_bytes_are_carried_verbatim_including_crlf() {
        let raw = b"---\r\ntype: Note\r\n---\r\n\r\nBody.\r\n";
        let c = YamlSerde.parse_concept("x.md", raw);
        assert_eq!(c.raw, raw.to_vec(), "raw must be byte-identical -- T2a depends on it");
        assert_eq!(c.concept_type, "Note", "but the derived view still parses");
    }
}
