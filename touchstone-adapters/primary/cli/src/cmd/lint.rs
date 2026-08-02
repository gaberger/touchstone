//! `touchstone lint` — conformance floor plus duplicate checks.
//!
//! Rules from FINDINGS E2 / E3d: the duplicate checks are what git's 16% CLEAN_DEFECT rate
//! actually produces; the conformance floor catches the authoring errors `new` is designed to
//! prevent.
//!
//! **This command owns no rules.** It calls `touchstone_usecases::lint_bundle` and formats the
//! result. That is the whole distinction between a driving adapter and a second implementation:
//! when the MCP surface exposes `touchstone_lint`, it will call the same function and cannot
//! drift from this output, because there is only one set of rules to drift from.

use std::path::Path;
use touchstone_ports::{ConceptParser, ConceptRepository, RawStore};
use touchstone_usecases::lint_bundle;

pub fn run<F, P>(_bundle: &Path, files: &F, parser: &P) -> i32
where
    F: ConceptRepository + RawStore,
    P: ConceptParser,
{
    let report = lint_bundle(files, parser);

    // Group by path for display. The use case returns a flat list because that is the useful
    // shape for a machine; grouping is a presentation choice and belongs here.
    let mut current = "";
    for problem in &report.problems {
        if problem.path != current {
            println!("{}", problem.path);
            current = &problem.path;
        }
        println!("  - {}", problem.message);
    }

    let total = report.total();
    println!("\n{total} problem(s)");
    if total > 0 { 1 } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use touchstone_ports::{ParsedConcept, VerifiedEntry};
    use std::collections::BTreeMap;

    /// An in-memory bundle. The command is being tested, not the filesystem.
    struct Files(BTreeMap<String, Vec<u8>>);

    impl Files {
        fn new(entries: &[(&str, &str)]) -> Self {
            Files(entries.iter().map(|(p, r)| (p.to_string(), r.as_bytes().to_vec())).collect())
        }
    }

    impl ConceptRepository for Files {
        fn paths(&self) -> Vec<String> {
            self.0.keys().cloned().collect()
        }
        /// Lint reads concepts only; this fake carries no artifacts and no raw sources.
        fn artifact_paths(&self) -> Vec<String> {
            Vec::new()
        }
        fn raw_paths(&self) -> Vec<String> {
            Vec::new()
        }
    }
    impl RawStore for Files {
        fn raw_bytes(&self, path: &str) -> Option<Vec<u8>> {
            self.0.get(path).cloned()
        }
    }

    /// Just enough parser to exercise the lint rules, without depending on an adapter.
    struct Parser;
    impl ConceptParser for Parser {
        fn parse(&self, path: &str, raw: &[u8]) -> ParsedConcept {
            let text = String::from_utf8_lossy(raw).to_string();
            let tags: Vec<String> = text
                .lines()
                .find_map(|l| l.strip_prefix("tags: ["))
                .map(|l| {
                    l.trim_end_matches(']')
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect()
                })
                .unwrap_or_default();
            ParsedConcept {
                path: path.to_string(),
                concept_type: if text.contains("type: Note") { "Note".into() } else { String::new() },
                tags,
                status: "stable".into(),
                raw: raw.to_vec(),
                error: (!text.contains("type:")).then(|| "missing or empty `type`".to_string()),
                verified_entries: text
                    .contains("verified:")
                    .then(|| vec![VerifiedEntry { by: None }])
                    .unwrap_or_default(),
                has_wikilinks: text.contains("[["),
                ..Default::default()
            }
        }
        fn canonicalize(&self, _: &ParsedConcept) -> Option<Vec<u8>> { None }
        fn emit_new(&self, _: &touchstone_ports::NewConceptRequest) -> Vec<u8> { vec![] }
    }

    #[test]
    fn clean_bundle_returns_0() {
        let files = Files::new(&[("clean.md", "---\ntype: Note\ntags: []\n---\n\nBody.\n")]);
        assert_eq!(run(Path::new("/x"), &files, &Parser), 0);
    }

    #[test]
    fn duplicate_tags_return_1() {
        let files = Files::new(&[("bad.md", "---\ntype: Note\ntags: [a, b, a]\n---\n\nBody.\n")]);
        assert_eq!(run(Path::new("/x"), &files, &Parser), 1);
    }

    #[test]
    fn a_verified_entry_without_by_is_a_problem() {
        let files = Files::new(&[("v.md", "---\ntype: Note\ntags: []\nverified:\n---\n\nB.\n")]);
        assert_eq!(run(Path::new("/x"), &files, &Parser), 1);
    }

    #[test]
    fn wikilinks_are_flagged_as_not_okf() {
        let files = Files::new(&[("w.md", "---\ntype: Note\ntags: []\n---\n\nSee [[other]].\n")]);
        assert_eq!(run(Path::new("/x"), &files, &Parser), 1);
    }
}
