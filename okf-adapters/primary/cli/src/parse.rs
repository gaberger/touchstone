//! Rich concept parsing from raw OKF markdown bytes.
//!
//! This is the full parse view the CLI needs, not just the minimal `Concept`
//! from `okf-ports`. The raw bytes remain authoritative; everything here is
//! a query-time view derived from them.
//!
//! Key guarantees (FINDINGS.md E3a, RUST-PATH.md §1):
//! - Temporal values stay ISO 8601 strings — serde_yaml_ng does NOT coerce them.
//! - Unknown frontmatter keys and unknown `type` values are preserved.
//! - Anchors / merge keys / block scalars cause the concept to be refused by
//!   `fmt` but are otherwise preserved and indexed (the raw bytes are truth).

use serde_yaml_ng::Value;
use std::collections::HashMap;

/// OKF canonical frontmatter key order (matches Python okf.py KEY_ORDER).
const KEY_ORDER: &[&str] = &[
    "id", "type", "title", "description", "resource", "tags", "aliases",
    "status", "stale_after", "generated", "verified", "sources",
];

// ── YAML construct detectors ───────────────────────────────────────────────

/// True if the frontmatter text contains constructs `fmt` cannot safely rewrite:
/// anchors (`&`), aliases (`*`), merge keys (`<<:`), or block scalars (`: |`, `: >`).
/// Mirrors Python's `_RISKY` pattern.
pub fn is_risky(fm_text: &str) -> bool {
    for line in fm_text.lines() {
        let trimmed = line.trim();
        // block scalar: value starts with | or >
        if let Some(pos) = line.find(':') {
            let after = line[pos + 1..].trim();
            if after.starts_with('|') || after.starts_with('>') {
                return true;
            }
        }
        // merge key
        if trimmed.starts_with("<<:") || trimmed.contains("<<:") {
            return true;
        }
        // anchor or alias: & or * preceded by whitespace or start of line
        if trimmed.starts_with('&') || trimmed.starts_with('*') {
            return true;
        }
        // inline anchor/alias after a value
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

// ── Link extraction ───────────────────────────────────────────────────────────

/// Extract `(label, target)` pairs for every `[label](target)` link in text,
/// excluding image links `![...](...) `. Mirrors Python's `_LINK` pattern.
pub fn extract_links(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Look for `[`
        if bytes[i] != b'[' {
            i += 1;
            continue;
        }
        // Exclude image links: `!` before `[`
        if i > 0 && bytes[i - 1] == b'!' {
            i += 1;
            continue;
        }
        // Find closing `]`
        let label_start = i + 1;
        let Some(close_bracket) = text[label_start..].find(']') else {
            i += 1;
            continue;
        };
        let label_end = label_start + close_bracket;
        // Next must be `(`
        if label_end + 1 >= bytes.len() || bytes[label_end + 1] != b'(' {
            i = label_end + 1;
            continue;
        }
        let target_start = label_end + 2;
        // Find closing `)`
        let Some(close_paren) = text[target_start..].find(')') else {
            i = target_start;
            continue;
        };
        let target_raw = &text[target_start..target_start + close_paren];
        // Strip optional title `"..."` from target
        let target = if let Some(space) = target_raw.find(' ') {
            &target_raw[..space]
        } else {
            target_raw
        };
        if !target.is_empty() {
            out.push((
                text[label_start..label_end].to_string(),
                target.to_string(),
            ));
        }
        i = target_start + close_paren + 1;
    }
    out
}

/// Resolve a bundle link target to a bundle-relative path.
/// Returns `None` for external (http/https/mailto/tel) or anchor-only links.
/// Mirrors Python's `store.resolve_link`.
pub fn resolve_link(src_path: &str, target: &str) -> Option<String> {
    let t = match target.split_once('#') {
        Some((left, _)) => left,
        None => target,
    };
    if t.is_empty() {
        return None;
    }
    if t.starts_with("http://")
        || t.starts_with("https://")
        || t.starts_with("mailto:")
        || t.starts_with("tel:")
    {
        return None;
    }
    if t.starts_with('/') {
        // Bundle-absolute
        return Some(normalize_path(t.trim_start_matches('/')));
    }
    // Relative — resolve against the src directory
    let base = match src_path.rsplit_once('/') {
        Some((dir, _)) => dir.to_string(),
        None => String::new(),
    };
    let joined = if base.is_empty() {
        t.to_string()
    } else {
        format!("{}/{}", base, t)
    };
    Some(normalize_path(&joined))
}

/// Simple path normaliser: collapses `./` and `../` segments.
fn normalize_path(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => { parts.pop(); }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

// ── Frontmatter split ──────────────────────────────────────────────────────

/// Split raw text into `(frontmatter_text, body)`.
/// Returns `(None, full_text)` if no frontmatter block is found.
pub fn split_frontmatter(raw: &str) -> (Option<&str>, &str) {
    let text = raw.trim_start_matches('\u{FEFF}');
    let norm_start = if text.starts_with("---\n") || text == "---" || text.starts_with("---\r\n") {
        text
    } else {
        return (None, raw);
    };
    let _ = norm_start; // used via `text` already trimmed

    if !text.starts_with("---") {
        return (None, raw);
    }
    // Must be exactly `---` on the first line
    let rest = if let Some(s) = text.strip_prefix("---\n") {
        s
    } else if let Some(s) = text.strip_prefix("---\r\n") {
        s
    } else if text.trim() == "---" {
        return (Some(""), "");
    } else {
        return (None, raw);
    };

    // Find the closing `---`
    let mut end = None;
    let mut body_start = rest.len();
    for (i, line) in rest.split_inclusive('\n').enumerate() {
        let _ = i;
        let trimmed = line.trim_end_matches(|c| c == '\r' || c == '\n');
        if trimmed == "---" {
            let offset = rest.find(line).unwrap_or(rest.len());
            end = Some(offset);
            body_start = offset + line.len();
            break;
        }
    }

    match end {
        Some(e) => (Some(&rest[..e]), &rest[body_start..]),
        None => (None, raw),
    }
}

// ── Rich Concept ──────────────────────────────────────────────────────────────

/// Full parsed concept — the view the CLI commands work with.
pub struct RichConcept {
    pub path: String,
    pub raw: Vec<u8>,
    pub fm_text: String,
    pub fm: serde_yaml_ng::Mapping,
    pub body: String,
    pub error: Option<String>,
}

impl RichConcept {
    pub fn concept_type(&self) -> &str {
        match self.fm.get("type") {
            Some(Value::String(s)) => s.as_str(),
            _ => "",
        }
    }

    pub fn title(&self) -> String {
        match self.fm.get("title") {
            Some(Value::String(s)) if !s.trim().is_empty() => s.clone(),
            _ => {
                // Derive from filename
                let stem = self.path.rsplit('/').next().unwrap_or(&self.path);
                let stem = stem.strip_suffix(".md").unwrap_or(stem);
                stem.replace('-', " ").replace('_', " ")
            }
        }
    }

    pub fn description(&self) -> &str {
        match self.fm.get("description") {
            Some(Value::String(s)) => s.as_str(),
            _ => "",
        }
    }

    pub fn tags(&self) -> Vec<String> {
        match self.fm.get("tags") {
            Some(Value::Sequence(seq)) => seq
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect(),
            Some(Value::String(s)) => vec![s.clone()],
            _ => vec![],
        }
    }

    pub fn status(&self) -> &str {
        match self.fm.get("status") {
            Some(Value::String(s)) => s.as_str(),
            _ => "stable",
        }
    }

    pub fn stale_after(&self) -> Option<String> {
        match self.fm.get("stale_after") {
            Some(Value::String(s)) => Some(s.clone()),
            _ => None,
        }
    }

    /// Spec-derived trust tier (ARCHITECTURE.md "The trust invariant").
    /// `verified[].by` starting with `human:` → "human"
    /// `generated` present without human verified → "machine"
    /// neither → "unattributed"
    pub fn trust(&self) -> &'static str {
        if let Some(ver) = self.fm.get("verified") {
            let entries: Vec<&serde_yaml_ng::Mapping> = match ver {
                Value::Sequence(seq) => seq.iter().filter_map(|v| v.as_mapping()).collect(),
                Value::Mapping(m) => vec![m],
                _ => vec![],
            };
            for entry in entries {
                if let Some(Value::String(by)) = entry.get("by") {
                    if by.starts_with("human:") {
                        return "human";
                    }
                }
            }
        }
        if self.fm.contains_key("generated") {
            return "machine";
        }
        "unattributed"
    }

    pub fn conformant(&self) -> bool {
        self.error.is_none() && !self.concept_type().trim().is_empty()
    }

    /// Simple FNV-1a fingerprint of the raw bytes (used for incremental indexing).
    pub fn digest(&self) -> String {
        fnv64(&self.raw)
    }

    /// Serialise frontmatter as JSON for the `fm_json` index column.
    pub fn fm_json(&self) -> String {
        fm_to_json(&self.fm)
    }

    /// All resolved bundle-relative link targets from the body.
    pub fn resolved_links(&self) -> Vec<String> {
        extract_links(&self.body)
            .into_iter()
            .filter_map(|(_, target)| resolve_link(&self.path, &target))
            .collect()
    }

    /// None if safe to format; Some(reason) if not.
    /// Mirrors Python's `formattable()`.
    pub fn formattable(&self) -> Option<String> {
        if self.fm.is_empty() {
            return Some("no frontmatter".to_string());
        }
        if let Some(ref e) = self.error {
            return Some(e.clone());
        }
        if is_risky(&self.fm_text) {
            return Some("contains anchors, aliases, merge keys or block scalars".to_string());
        }
        None
    }

    /// Canonical rewrite. Only valid after `formattable()` returns `None`.
    pub fn format(&self) -> String {
        let canon = canonical_frontmatter(&self.fm);
        let body = {
            let b = &self.body;
            let b = if b.starts_with('\n') { b.as_str() } else { &format!("\n{b}") };
            let b = b.trim_end_matches('\n');
            format!("{b}\n")
        };
        format!("---\n{}---\n{}", canon, body)
    }
}

// ── Parsing entry point ────────────────────────────────────────────────────────

/// Parse a raw concept file into the rich view.
/// Never fails: error is stored in `RichConcept.error`.
pub fn parse_concept(path: &str, raw: Vec<u8>) -> RichConcept {
    let text = match std::str::from_utf8(&raw) {
        Ok(s) => s.to_string(),
        Err(_) => {
            return RichConcept {
                path: path.to_string(),
                raw,
                fm_text: String::new(),
                fm: serde_yaml_ng::Mapping::new(),
                body: String::new(),
                error: Some("invalid UTF-8".to_string()),
            };
        }
    };
    // Normalise CRLF
    let text = text.replace("\r\n", "\n").replace('\r', "\n");

    let (fm_text_opt, body) = split_frontmatter(&text);

    let (fm_text, fm_text_owned, fm, error) = match fm_text_opt {
        None => (
            "",
            String::new(),
            serde_yaml_ng::Mapping::new(),
            Some("no frontmatter".to_string()),
        ),
        Some(fm_str) => {
            let fm_owned = fm_str.to_string();
            match serde_yaml_ng::from_str::<Value>(fm_str) {
                Err(e) => (
                    fm_str,
                    fm_owned,
                    serde_yaml_ng::Mapping::new(),
                    Some(format!("invalid YAML: {}", e.to_string().lines().next().unwrap_or(""))),
                ),
                Ok(Value::Null) => (
                    fm_str,
                    fm_owned,
                    serde_yaml_ng::Mapping::new(),
                    Some("missing or empty `type`".to_string()),
                ),
                Ok(Value::Mapping(m)) => {
                    let error = match m.get("type") {
                        Some(Value::String(s)) if !s.trim().is_empty() => None,
                        _ => Some("missing or empty `type`".to_string()),
                    };
                    (fm_str, fm_owned, m, error)
                }
                Ok(_) => (
                    fm_str,
                    fm_owned,
                    serde_yaml_ng::Mapping::new(),
                    Some("frontmatter is not a mapping".to_string()),
                ),
            }
        }
    };
    let _ = fm_text;

    RichConcept {
        path: path.to_string(),
        raw,
        fm_text: fm_text_owned,
        fm,
        body: body.to_string(),
        error,
    }
}

// ── Canonical frontmatter ──────────────────────────────────────────────────────

/// Deterministic YAML dump matching Python's `canonical_frontmatter()`.
/// Known keys first in KEY_ORDER; unknown keys appended in original order.
pub fn canonical_frontmatter(fm: &serde_yaml_ng::Mapping) -> String {
    let mut ordered = serde_yaml_ng::Mapping::new();
    for &k in KEY_ORDER {
        if let Some(v) = fm.get(k) {
            ordered.insert(Value::String(k.to_string()), v.clone());
        }
    }
    for (k, v) in fm {
        if let Value::String(ks) = k {
            if !KEY_ORDER.contains(&ks.as_str()) {
                ordered.insert(k.clone(), v.clone());
            }
        } else {
            ordered.insert(k.clone(), v.clone());
        }
    }
    serde_yaml_ng::to_string(&Value::Mapping(ordered))
        .unwrap_or_default()
        .trim_start_matches("---\n")
        .to_string()
}

// ── Lint rules ────────────────────────────────────────────────────────────────

/// Lint an already-parsed concept. Returns human-readable problem strings.
/// Mirrors Python's `okf.lint()` — rules E2 showed are actually needed.
pub fn lint(c: &RichConcept) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();

    if let Some(ref e) = c.error {
        out.push(format!("error: {e}"));
        if c.fm.is_empty() {
            return out;
        }
    }

    // Duplicate tags
    let tags = c.tags();
    let mut seen_tags: HashMap<&str, usize> = HashMap::new();
    for t in &tags {
        *seen_tags.entry(t.as_str()).or_insert(0) += 1;
    }
    let mut dupes: Vec<&str> = seen_tags.into_iter().filter(|(_, n)| *n > 1).map(|(t, _)| t).collect();
    dupes.sort();
    if !dupes.is_empty() {
        out.push(format!("duplicate tags: {}", dupes.join(", ")));
    }

    // Verified entries
    let entries: Vec<&serde_yaml_ng::Mapping> = match c.fm.get("verified") {
        Some(Value::Sequence(seq)) => seq.iter().filter_map(|v| v.as_mapping()).collect(),
        Some(Value::Mapping(m)) => vec![m],
        _ => vec![],
    };
    let bys: Vec<&str> = entries
        .iter()
        .filter_map(|e| e.get("by").and_then(|v| v.as_str()))
        .collect();
    let mut seen_by: HashMap<&str, usize> = HashMap::new();
    for by in &bys {
        *seen_by.entry(by).or_insert(0) += 1;
    }
    let mut dup_by: Vec<&str> = seen_by.into_iter().filter(|(_, n)| *n > 1).map(|(b, _)| b).collect();
    dup_by.sort();
    if !dup_by.is_empty() {
        out.push(format!("duplicate verified principals: {}", dup_by.join(", ")));
    }
    for entry in &entries {
        if entry.get("by").is_none() {
            out.push("verified entry missing required `by`".to_string());
        }
    }

    // Sources
    if let Some(Value::Sequence(srcs)) = c.fm.get("sources") {
        for s in srcs {
            if let Some(m) = s.as_mapping() {
                if m.get("resource").is_none() {
                    let id = m.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                    out.push(format!("source `{id}` missing required `resource`"));
                }
            }
        }
    }

    // Status validity
    if let Some(Value::String(st)) = c.fm.get("status") {
        if !matches!(st.as_str(), "draft" | "stable" | "deprecated") {
            out.push(format!("status `{st}` is not draft|stable|deprecated"));
        }
    }

    // Wikilinks
    for (_, tgt) in extract_links(&c.body) {
        if tgt.contains("[[") {
            out.push("wikilink syntax is not OKF -- use a markdown link".to_string());
        }
    }
    if c.body.contains("[[") {
        out.push("body contains [[wikilinks]] -- not OKF, will not resolve".to_string());
    }

    out
}

// ── Utilities ─────────────────────────────────────────────────────────────────

/// FNV-1a 64-bit hash of raw bytes — stable fingerprint for incremental indexing.
pub fn fnv64(data: &[u8]) -> String {
    const OFFSET: u64 = 14695981039346656037;
    const PRIME: u64 = 1099511628211;
    let mut h = OFFSET;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(PRIME);
    }
    format!("{h:016x}")
}

/// Serialise a serde_yaml_ng Mapping to JSON (for `fm_json` index column).
fn fm_to_json(fm: &serde_yaml_ng::Mapping) -> String {
    yaml_value_to_json(&Value::Mapping(fm.clone()))
}

fn yaml_value_to_json(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string()),
        Value::Sequence(seq) => {
            let items: Vec<String> = seq.iter().map(yaml_value_to_json).collect();
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
                    format!("{}:{}", key, yaml_value_to_json(val))
                })
                .collect();
            format!("{{{}}}", pairs.join(","))
        }
        Value::Tagged(t) => yaml_value_to_json(&t.value),
    }
}

// ── Slugify ───────────────────────────────────────────────────────────────────

/// Convert a title to a filename slug. Mirrors Python's `slugify()`.
pub fn slugify(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        if ch.is_alphanumeric() {
            out.push(ch.to_lowercase().next().unwrap());
        } else {
            out.push('-');
        }
    }
    // Collapse repeated dashes
    let mut result = String::new();
    let mut last_dash = false;
    for ch in out.chars() {
        if ch == '-' {
            if !last_dash {
                result.push('-');
            }
            last_dash = true;
        } else {
            result.push(ch);
            last_dash = false;
        }
    }
    let result = result.trim_matches('-').to_string();
    if result.is_empty() { "untitled".to_string() } else { result }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_basic() {
        let raw = "---\ntype: Note\ntitle: Hello\n---\n\nBody.\n";
        let (fm, body) = split_frontmatter(raw);
        assert_eq!(fm, Some("type: Note\ntitle: Hello\n"));
        assert!(body.contains("Body."));
    }

    #[test]
    fn parse_basic_concept() {
        let raw = b"---\ntype: Note\ntitle: Hello\n---\n\nBody.\n";
        let c = parse_concept("notes/hello.md", raw.to_vec());
        assert_eq!(c.concept_type(), "Note");
        assert_eq!(c.title(), "Hello");
        assert!(c.error.is_none());
        assert!(c.conformant());
    }

    #[test]
    fn parse_temporal_stays_string() {
        let raw = b"---\ntype: Metric\nat: 2026-01-01T00:00:00Z\nstale_after: 2026-09-23\n---\n";
        let c = parse_concept("m.md", raw.to_vec());
        // The fm_json should contain ISO 8601 strings, not coerced timestamps
        let json = c.fm_json();
        assert!(json.contains("2026-01-01T00:00:00Z"), "temporal must stay ISO 8601 string");
    }

    #[test]
    fn parse_unknown_type_preserved() {
        let raw = b"---\ntype: ThreatModel\ntitle: Unknown\n---\n";
        let c = parse_concept("x.md", raw.to_vec());
        assert_eq!(c.concept_type(), "ThreatModel");
        assert!(c.error.is_none());
    }

    #[test]
    fn trust_human() {
        let raw = b"---\ntype: Note\nverified:\n  - by: human:alice\n    at: 2026-01-01\n---\n";
        let c = parse_concept("x.md", raw.to_vec());
        assert_eq!(c.trust(), "human");
    }

    #[test]
    fn trust_machine() {
        let raw = b"---\ntype: Note\ngenerated:\n  by: agent:x\n  at: 2026-01-01\n---\n";
        let c = parse_concept("x.md", raw.to_vec());
        assert_eq!(c.trust(), "machine");
    }

    #[test]
    fn trust_unattributed() {
        let raw = b"---\ntype: Note\n---\n";
        let c = parse_concept("x.md", raw.to_vec());
        assert_eq!(c.trust(), "unattributed");
    }

    #[test]
    fn link_extraction() {
        let body = "See [this](notes/foo.md) and [that](bar.md). Not an ![image](img.png).";
        let links = extract_links(body);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].1, "notes/foo.md");
        assert_eq!(links[1].1, "bar.md");
    }

    #[test]
    fn resolve_link_relative() {
        let resolved = resolve_link("notes/foo.md", "bar.md").unwrap();
        assert_eq!(resolved, "notes/bar.md");
    }

    #[test]
    fn resolve_link_absolute() {
        let resolved = resolve_link("notes/foo.md", "/concepts/bar.md").unwrap();
        assert_eq!(resolved, "concepts/bar.md");
    }

    #[test]
    fn resolve_link_external_none() {
        assert!(resolve_link("notes/foo.md", "https://example.com").is_none());
    }

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("Hello World"), "hello-world");
        assert_eq!(slugify("Hello  --  World"), "hello-world");
        assert_eq!(slugify(""), "untitled");
    }

    #[test]
    fn is_risky_detects_block_scalar() {
        assert!(is_risky("script: |\n  echo hello\n"));
    }

    #[test]
    fn is_risky_detects_anchor() {
        assert!(is_risky("defaults: &defaults\n  by: human:x\n"));
    }

    #[test]
    fn is_risky_detects_merge_key() {
        assert!(is_risky("verified:\n  - <<: *defaults\n"));
    }

    #[test]
    fn is_risky_plain_safe() {
        assert!(!is_risky("type: Note\ntitle: Hello\n"));
    }

    #[test]
    fn fnv64_deterministic() {
        let h1 = fnv64(b"hello");
        let h2 = fnv64(b"hello");
        assert_eq!(h1, h2);
        assert_ne!(h1, fnv64(b"world"));
    }

    #[test]
    fn lint_duplicate_tags() {
        let raw = b"---\ntype: Note\ntags: [a, b, a]\n---\n";
        let c = parse_concept("x.md", raw.to_vec());
        let issues = lint(&c);
        assert!(issues.iter().any(|s| s.contains("duplicate tags")));
    }

    #[test]
    fn lint_clean_concept() {
        let raw = b"---\ntype: Note\ntitle: Clean\ntags: [a, b]\nstatus: stable\n---\n";
        let c = parse_concept("x.md", raw.to_vec());
        assert!(lint(&c).is_empty());
    }

    #[test]
    fn formattable_risky() {
        let raw = b"---\ntype: Note\nscript: |\n  echo hi\n---\n";
        let c = parse_concept("x.md", raw.to_vec());
        assert!(c.formattable().is_some());
    }

    #[test]
    fn formattable_plain() {
        let raw = b"---\ntype: Note\ntitle: Hello\n---\n";
        let c = parse_concept("x.md", raw.to_vec());
        assert!(c.formattable().is_none());
    }

    #[test]
    fn format_roundtrip() {
        let raw = b"---\ntype: Note\ntitle: Hello\n---\n\nBody.\n";
        let c = parse_concept("x.md", raw.to_vec());
        let formatted = c.format();
        let c2 = parse_concept("x.md", formatted.into_bytes());
        assert_eq!(c2.concept_type(), c.concept_type());
        assert_eq!(c2.title(), c.title());
    }
}
