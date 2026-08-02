//! `touchstone show <path> [--json]` — print one concept's derived view.
//!
//! Read-only, and the only command that exposes *parsed frontmatter* rather than a
//! summary of it. That exists for a reason: without it, three conformance drills
//! (T2b semantic round-trip, T2d ISO 8601 non-coercion, T2e formatter safety) can only
//! be checked by importing the implementation as a library. A suite that imports the
//! implementation cannot gate a *different* implementation, which is the whole point of
//! `okf-conformance` (ADR-2608010950). One read-only command is the price of a black-box gate.
//!
//! `--json` is the machine contract. Frontmatter is emitted verbatim — unknown keys
//! included, temporal values as the strings they were authored as — because that is
//! exactly the property under test.

use crate::args::ShowArgs;
use crate::parse::parse_concept;
use crate::walk::iter_concept_files;
use std::fs;
use std::path::Path;

pub fn run(args: &ShowArgs, bundle: &Path) -> i32 {
    // Resolve against the walker rather than joining the path directly: that way `show`
    // sees exactly the file set every other command sees, including the E4a rule that
    // `log.md` is a concept and `index.md` is not.
    let want = args.path.trim_start_matches("./");
    let files = iter_concept_files(bundle);
    let Some((rel, full)) = files.into_iter().find(|(rel, _)| rel == want) else {
        eprintln!("no such concept: {want}");
        return 1;
    };

    let raw = match fs::read(&full) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("cannot read {want}: {e}");
            return 1;
        }
    };
    let c = parse_concept(&rel, raw);

    if args.json {
        // Hand-assembled rather than derived: `fm_json()` is already the canonical
        // serialisation used by the index, and re-encoding it through a struct would
        // introduce a second frontmatter encoder that could drift from the first.
        let esc = |s: &str| serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into());
        let formattable = match c.formattable() {
            Some(reason) => esc(&reason),
            None => "null".to_string(),
        };
        let links: Vec<String> = c.resolved_links().iter().map(|l| esc(l)).collect();
        let tags: Vec<String> = c.tags().iter().map(|t| esc(t)).collect();
        println!("{{");
        println!("  \"path\": {},", esc(&c.path));
        println!("  \"type\": {},", esc(c.concept_type()));
        println!("  \"title\": {},", esc(&c.title()));
        println!("  \"status\": {},", esc(c.status()));
        println!("  \"trust\": {},", esc(c.trust()));
        println!("  \"conformant\": {},", c.conformant());
        println!("  \"formattable_refusal\": {formattable},");
        println!("  \"tags\": [{}],", tags.join(", "));
        println!("  \"links\": [{}],", links.join(", "));
        println!("  \"frontmatter\": {}", c.fm_json());
        println!("}}");
        return 0;
    }

    println!("path:   {}", c.path);
    println!("type:   {}", c.concept_type());
    println!("title:  {}", c.title());
    println!("status: {}", c.status());
    println!("trust:  {}", c.trust());
    let tags = c.tags();
    if !tags.is_empty() {
        println!("tags:   {}", tags.join(", "));
    }
    if let Some(reason) = c.formattable() {
        println!("fmt:    refused -- {reason}");
    }
    let links = c.resolved_links();
    if !links.is_empty() {
        println!("\nlinks:");
        for l in &links {
            println!("  -> {l}");
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> &'static Path {
        Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../_fixture"))
    }

    #[test]
    fn missing_concept_returns_1() {
        let args = ShowArgs { path: "nope/does-not-exist.md".into(), json: true };
        assert_eq!(run(&args, fixture()), 1);
    }

    #[test]
    fn known_concept_returns_0() {
        let args = ShowArgs { path: "adversarial/unknown-type.md".into(), json: true };
        assert_eq!(run(&args, fixture()), 0);
    }

    #[test]
    fn leading_dot_slash_is_tolerated() {
        let args = ShowArgs { path: "./adversarial/unknown-type.md".into(), json: false };
        assert_eq!(run(&args, fixture()), 0);
    }

    /// The drill this command exists to make possible: unknown keys survive parsing.
    /// A `show` that quietly dropped them would make T2c vacuous.
    #[test]
    fn json_carries_unknown_keys_verbatim() {
        let raw = fs::read(fixture().join("adversarial/unknown-type.md")).unwrap();
        let c = parse_concept("adversarial/unknown-type.md", raw);
        let json = c.fm_json();
        assert!(json.contains("\"retention\""), "unknown key dropped: {json}");
        assert!(json.contains("\"classification\""), "unknown key dropped: {json}");
        assert_eq!(c.concept_type(), "ThreatModel", "unknown type not preserved");
    }

    /// T2d, at the level `show` reports it: a temporal value must still be the string
    /// it was authored as, not a parser's idea of a datetime.
    #[test]
    fn json_keeps_temporal_values_as_iso8601_strings() {
        let raw = b"---\ntype: Note\nstale_after: 2026-01-01T00:00:00Z\n---\n\nBody.\n".to_vec();
        let c = parse_concept("x.md", raw);
        let json = c.fm_json();
        assert!(
            json.contains("\"2026-01-01T00:00:00Z\""),
            "temporal value coerced off ISO 8601: {json}"
        );
    }
}
