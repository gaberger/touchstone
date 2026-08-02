//! `touchstone new <Type> <Title>` — scaffold a conformant concept.
//!
//! Emits frontmatter through `canonical_frontmatter()`, never string
//! concatenation (FINDINGS.md E3d: hand-crafted frontmatter with `: ` in a
//! value is the single most likely authoring error).

use crate::args::NewArgs;
use crate::parse::{canonical_frontmatter, slugify};
use touchstone_ports::Clock;
use serde_yaml_ng::{Mapping, Value};
use std::fs;
use std::path::Path;

pub fn run(args: &NewArgs, bundle: &Path, clock: &dyn Clock) -> i32 {
    let sub = args
        .dir
        .clone()
        .unwrap_or_else(|| format!("{}s", args.concept_type.to_lowercase()));
    let rel = format!("{}/{}.md", sub, slugify(&args.title));
    let target = bundle.join(&rel);

    if target.exists() {
        eprintln!("exists: {rel}");
        return 1;
    }

    let now = clock.now_iso8601();

    let mut fm = Mapping::new();

    // Generate a short UUID-like id without pulling in the uuid crate:
    // use the first 12 hex chars of a FNV-64 hash of (now + title).
    let id_input = format!("{now}{}", args.title);
    let id = &crate::parse::fnv64(id_input.as_bytes())[..12];
    fm.insert(Value::String("id".into()), Value::String(id.to_string()));
    fm.insert(
        Value::String("type".into()),
        Value::String(args.concept_type.clone()),
    );
    fm.insert(
        Value::String("title".into()),
        Value::String(args.title.clone()),
    );
    fm.insert(
        Value::String("description".into()),
        Value::String(args.description.clone().unwrap_or_default()),
    );

    let tags_seq: Value = Value::Sequence(
        args.tags
            .iter()
            .map(|t| Value::String(t.clone()))
            .collect(),
    );
    fm.insert(Value::String("tags".into()), tags_seq);
    fm.insert(
        Value::String("status".into()),
        Value::String("draft".into()),
    );

    if let Some(ref gen) = args.generated {
        let mut g = Mapping::new();
        g.insert(Value::String("by".into()), Value::String(gen.clone()));
        g.insert(Value::String("at".into()), Value::String(now));
        fm.insert(Value::String("generated".into()), Value::Mapping(g));
    }

    let body = format!("\n# {}\n\n", args.title);
    let text = format!("---\n{}---\n{}", canonical_frontmatter(&fm), body);

    if let Some(parent) = target.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            eprintln!("cannot create directory: {e}");
            return 1;
        }
    }

    if let Err(e) = fs::write(&target, &text) {
        eprintln!("cannot write {rel}: {e}");
        return 1;
    }

    println!("{rel}");
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FixedClock;
    impl Clock for FixedClock {
        fn now_iso8601(&self) -> String {
            "2026-01-01T00:00:00Z".to_string()
        }
    }

    fn make_args(concept_type: &str, title: &str) -> NewArgs {
        NewArgs {
            concept_type: concept_type.to_string(),
            title: title.to_string(),
            dir: None,
            description: None,
            tags: vec![],
            generated: None,
        }
    }

    #[test]
    fn creates_concept_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let args = make_args("Note", "Hello World");
        let rc = run(&args, tmp.path(), &FixedClock);
        assert_eq!(rc, 0);
        let path = tmp.path().join("notes/hello-world.md");
        assert!(path.exists());
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("type: Note"));
        assert!(text.contains("title: Hello World"));
    }

    #[test]
    fn duplicate_returns_1() {
        let tmp = tempfile::TempDir::new().unwrap();
        let args = make_args("Note", "Hello");
        run(&args, tmp.path(), &FixedClock);
        let rc = run(&args, tmp.path(), &FixedClock);
        assert_eq!(rc, 1);
    }

    #[test]
    fn generated_field_written() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut args = make_args("Note", "Gen");
        args.generated = Some("agent:test".into());
        run(&args, tmp.path(), &FixedClock);
        let path = tmp.path().join("notes/gen.md");
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("generated:"));
        assert!(text.contains("agent:test"));
    }
}
