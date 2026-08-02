//! `touchstone show <path>` — print one concept's derived view.
//!
//! Read-only, and the only command that exposes *parsed frontmatter* rather than a summary of
//! it. That exists so the conformance drills can observe what the parser saw without importing
//! the implementation — a suite that imports the implementation cannot gate a different one
//! (ADR-2608010950).

use crate::args::ShowArgs;
use touchstone_ports::{ConceptParser, ConceptRepository, RawStore};

pub fn run<F, P>(args: &ShowArgs, files: &F, parser: &P) -> i32
where
    F: ConceptRepository + RawStore,
    P: ConceptParser,
{
    let want = args.path.trim_start_matches("./");
    if !files.paths().iter().any(|p| p == want) {
        eprintln!("no such concept: {want}");
        return 1;
    }
    let Some(raw) = files.raw_bytes(want) else {
        eprintln!("cannot read {want}");
        return 1;
    };
    let c = parser.parse(want, &raw);
    let links: Vec<String> = touchstone_ports::extract_links(&c.body)
        .into_iter()
        .filter_map(|(_, t)| touchstone_ports::resolve_link(want, &t))
        .collect();

    if args.json {
        let esc = |s: &str| serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into());
        let fm = if c.frontmatter_json.is_empty() { "{}" } else { &c.frontmatter_json };
        println!("{{");
        println!("  \"path\": {},", esc(&c.path));
        println!("  \"type\": {},", esc(&c.concept_type));
        println!("  \"title\": {},", esc(&c.title));
        println!("  \"status\": {},", esc(&c.status));
        println!("  \"trust\": {},", esc(c.trust.label()));
        println!("  \"conformant\": {},", c.conformant());
        println!(
            "  \"formattable_refusal\": {},",
            c.format_skip_reason.as_deref().map(esc).unwrap_or_else(|| "null".into())
        );
        println!(
            "  \"tags\": [{}],",
            c.tags.iter().map(|t| esc(t)).collect::<Vec<_>>().join(", ")
        );
        println!(
            "  \"links\": [{}],",
            links.iter().map(|l| esc(l)).collect::<Vec<_>>().join(", ")
        );
        println!("  \"frontmatter\": {fm}");
        println!("}}");
        return 0;
    }

    println!("path:   {}", c.path);
    println!("type:   {}", c.concept_type);
    println!("title:  {}", c.title);
    println!("status: {}", c.status);
    println!("trust:  {}", c.trust.label());
    if !c.tags.is_empty() {
        println!("tags:   {}", c.tags.join(", "));
    }
    if let Some(reason) = &c.format_skip_reason {
        println!("fmt:    refused -- {reason}");
    }
    if !links.is_empty() {
        println!("\nlinks:");
        for l in &links {
            println!("  -> {l}");
        }
    }
    0
}
