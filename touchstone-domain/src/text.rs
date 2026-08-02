//! Pure text operations over concept bytes: link extraction and resolution, frontmatter
//! splitting, slugs, and the content digest.
//!
//! These live in the domain because they are the same in every implementation — a link is a
//! link whichever YAML library parsed the file above it. They were previously carried inside
//! the CLI adapter, which is why a second, subtly different `slugify` had grown in the
//! use-case layer: two implementations of one pure function, neither aware of the other. The
//! version here is the one the conformance drills exercise.

// ── Links ──────────────────────────────────────────────────────────────────────

/// Extract `(label, target)` pairs for every `[label](target)` link in text,
/// excluding image links `![...](...)`.
pub fn extract_links(text: &str) -> Vec<(String, String)> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'[' {
            i += 1;
            continue;
        }
        // Exclude image links: `!` before `[`
        if i > 0 && bytes[i - 1] == b'!' {
            i += 1;
            continue;
        }
        let label_start = i + 1;
        let Some(close_bracket) = text[label_start..].find(']') else {
            i += 1;
            continue;
        };
        let label_end = label_start + close_bracket;
        if label_end + 1 >= bytes.len() || bytes[label_end + 1] != b'(' {
            i = label_end + 1;
            continue;
        }
        let target_start = label_end + 2;
        let Some(close_paren) = text[target_start..].find(')') else {
            i = target_start;
            continue;
        };
        let target_raw = &text[target_start..target_start + close_paren];
        // Strip an optional link title: `(path "Title")`
        let target = match target_raw.find(' ') {
            Some(space) => &target_raw[..space],
            None => target_raw,
        };
        if !target.is_empty() {
            out.push((text[label_start..label_end].to_string(), target.to_string()));
        }
        i = target_start + close_paren + 1;
    }
    out
}

/// Resolve a bundle link target to a bundle-relative path.
///
/// Returns `None` for external (http/https/mailto/tel) or anchor-only links — those are not
/// bundle edges and must not be counted as broken ones.
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
        return Some(normalize_path(t.trim_start_matches('/')));
    }
    let base = match src_path.rsplit_once('/') {
        Some((dir, _)) => dir.to_string(),
        None => String::new(),
    };
    let joined = if base.is_empty() { t.to_string() } else { format!("{base}/{t}") };
    Some(normalize_path(&joined))
}

/// Collapse `./` and `../` segments.
pub fn normalize_path(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

/// POSIX relative path from `from_dir` to `path`. Both are bundle-relative.
pub fn relative_path(path: &str, from_dir: &str) -> String {
    let path_parts: Vec<&str> = path.split('/').collect();
    let dir_parts: Vec<&str> =
        if from_dir.is_empty() { vec![] } else { from_dir.split('/').collect() };

    let common = path_parts
        .iter()
        .zip(dir_parts.iter())
        .take_while(|(a, b)| a == b)
        .count();

    let up = dir_parts.len() - common;
    let mut result: Vec<&str> = std::iter::repeat("..").take(up).collect();
    result.extend_from_slice(&path_parts[common..]);
    if result.is_empty() { ".".to_string() } else { result.join("/") }
}

// ── Frontmatter ────────────────────────────────────────────────────────────────

/// Split raw text into `(frontmatter_text, body)`.
/// Returns `(None, full_text)` when there is no frontmatter block.
///
/// Byte-preserving on the body: raw bytes stay authoritative, so this only ever hands back
/// borrowed slices of the input and never rewrites what it did not understand.
pub fn split_frontmatter(raw: &str) -> (Option<&str>, &str) {
    let text = raw.trim_start_matches('\u{FEFF}');
    if !text.starts_with("---") {
        return (None, raw);
    }
    let rest = if let Some(s) = text.strip_prefix("---\n") {
        s
    } else if let Some(s) = text.strip_prefix("---\r\n") {
        s
    } else if text.trim() == "---" {
        return (Some(""), "");
    } else {
        return (None, raw);
    };

    let mut end = None;
    let mut body_start = rest.len();
    for line in rest.split_inclusive('\n') {
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

// ── Identity ───────────────────────────────────────────────────────────────────

/// FNV-1a fingerprint of raw bytes. Used for incremental indexing: a concept whose digest
/// is unchanged does not need reparsing.
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

/// Convert a title to a filename slug.
pub fn slugify(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        if ch.is_alphanumeric() {
            out.extend(ch.to_lowercase());
        } else {
            out.push('-');
        }
    }
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
    fn split_tolerates_a_bom_and_crlf() {
        let raw = "\u{FEFF}---\r\ntype: Note\r\n---\r\nBody.\r\n";
        let (fm, _) = split_frontmatter(raw);
        assert!(fm.is_some(), "BOM + CRLF frontmatter must still be recognised");
    }

    #[test]
    fn split_absent_returns_whole_text() {
        let raw = "no frontmatter here\n";
        assert_eq!(split_frontmatter(raw), (None, raw));
    }

    #[test]
    fn links_exclude_images() {
        let out = extract_links("see [a](x.md) and ![img](y.png)");
        assert_eq!(out, vec![("a".to_string(), "x.md".to_string())]);
    }

    #[test]
    fn links_strip_a_title_suffix() {
        let out = extract_links("[a](x.md \"Title\")");
        assert_eq!(out[0].1, "x.md");
    }

    #[test]
    fn external_targets_are_not_bundle_edges() {
        for t in ["https://example.com", "mailto:a@b.c", "#anchor"] {
            assert_eq!(resolve_link("notes/a.md", t), None, "{t} must not resolve");
        }
    }

    #[test]
    fn relative_targets_resolve_against_the_source_dir() {
        assert_eq!(resolve_link("notes/a.md", "b.md"), Some("notes/b.md".into()));
        assert_eq!(resolve_link("notes/a.md", "../t/c.md"), Some("t/c.md".into()));
        assert_eq!(resolve_link("notes/a.md", "/root.md"), Some("root.md".into()));
    }

    #[test]
    fn relative_path_round_trips() {
        assert_eq!(relative_path("notes/foo.md", "notes"), "foo.md");
        assert_eq!(relative_path("notes/foo.md", ""), "notes/foo.md");
        assert_eq!(relative_path("foo.md", "notes"), "../foo.md");
    }

    #[test]
    fn digest_is_stable_and_content_sensitive() {
        assert_eq!(fnv64(b"abc"), fnv64(b"abc"));
        assert_ne!(fnv64(b"abc"), fnv64(b"abd"));
    }

    #[test]
    fn slug_collapses_and_trims_separators() {
        assert_eq!(slugify("Hello, World!"), "hello-world");
        assert_eq!(slugify("  --  "), "untitled");
    }

    /// The use-case layer had grown its own `slugify` using `to_ascii_lowercase`, which leaves
    /// non-ASCII uppercase untouched. Unicode paths are part of the adversarial fixture, so the
    /// difference was reachable — this pins the correct behaviour.
    #[test]
    fn slug_lowercases_beyond_ascii() {
        assert_eq!(slugify("ÜNÏCODE"), "ünïcode");
    }
}
