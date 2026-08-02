//! Drills that assert against *known content* — the adversarial `_fixture`, and the one
//! upstream bundle that carries an adjudicated defect.
//!
//! The universal drills in `drills.rs` check properties that must hold for any bundle. These
//! check that specific hostile constructs survive specific handling, which needs a corpus whose
//! contents are known. `_fixture` is built for that: unknown types, unknown keys, YAML anchors,
//! merge keys, block scalars, flow style, CRLF, unicode paths, empty `type`, absent
//! frontmatter, and deliberately broken links.

use touchstone_conformance::*;
use std::path::Path;

fn fixture() -> Bundle {
    let b = Bundle::checkout(&workspace_root().join("_fixture"));
    b.ok(&["index", "-q"]);
    b
}

// ── T2c ─────────────────────────────────────────────────────────────────────

/// T2c — unknown types and unknown keys are preserved, not normalised away.
///
/// The spec is explicit that consumers MUST NOT reject what they do not recognise. The
/// tempting failure is not rejection but *tidying*: dropping the keys you have no column for.
#[test]
fn t2c_unknown_type_and_unknown_keys_survive() {
    let b = fixture();
    let doc = b.show("adversarial/unknown-type.md");

    assert_eq!(
        doc["type"], "ThreatModel",
        "unknown `type` was not preserved -- got {}",
        doc["type"]
    );
    for key in ["retention", "classification"] {
        assert!(
            doc["frontmatter"].get(key).is_some(),
            "unknown key `{key}` was dropped: {}",
            doc["frontmatter"]
        );
    }
}

/// Broken links are legal per spec — they represent not-yet-written knowledge, which is the
/// normal state of a growing brain. They must be *recorded*, not silently discarded.
#[test]
fn t2c_broken_links_are_recorded_not_rejected() {
    let b = fixture();
    assert!(
        b.broken_links() > 0,
        "fixture contains deliberately broken links but none were recorded"
    );
}

/// A concept with no frontmatter at all, and one with an empty `type`, are both non-conformant
/// — and both must still be indexed. Refusing them would narrow the spec's tolerance, which is
/// the failure mode this project treats as a defect rather than strictness.
#[test]
fn non_conformant_concepts_are_still_indexed() {
    let b = fixture();
    for rel in ["adversarial/no-frontmatter.md", "adversarial/empty-type.md"] {
        let doc = b.show(rel);
        assert_eq!(doc["conformant"], false, "{rel} should be flagged non-conformant");
        assert_eq!(doc["path"], rel, "{rel} was not indexed");
    }
    // ...and `lint` must actually say so, or the floor is unenforced. `lint` exits non-zero
    // when it finds problems, and this fixture is built to contain them, so the exit code is
    // deliberately not asserted to be 0 here.
    let out = b.run(&["lint"]).stdout;
    assert!(out.contains("no frontmatter"), "lint missed the frontmatter-less concept: {out}");
    assert!(out.contains("`type`"), "lint missed the empty type: {out}");
}

// ── Trust ───────────────────────────────────────────────────────────────────

/// The trust invariant, at the only place it is observable from outside.
///
/// This field is the only thing separating a curated brain from a pile of plausible text, and
/// every ranking decision in retrieval depends on it. A tier that drifts with the parser is
/// the failure RUST-PATH §1 measured: `verified: [{<<: *defaults}]` reads as human-verified
/// under one conformant YAML library and unattributed under another — same bytes, different tier.
///
/// Asserted against the three tiers the invariant actually derives. Note that the domain's
/// `Trust::Attested` variant is never produced by this derivation; `adversarial/attested.md`
/// is a `type: Attested Computation` *concept*, which is a different thing from an attested
/// *trust tier*, and it correctly reads as unattributed because it carries neither `verified`
/// nor `generated`.
#[test]
fn trust_tiers_are_derived_from_the_spec_convention() {
    let b = fixture();
    let cases = [
        // `verified[].by` starts with `human:`
        ("adversarial/verified-chain.md", "human"),
        // `generated` present, no human `verified`
        ("systems/retrieval.md", "machine"),
        // neither
        ("adversarial/unknown-type.md", "unattributed"),
        ("adversarial/attested.md", "unattributed"),
    ];
    for (rel, want) in cases {
        if !Path::new(&b.root).join(rel).exists() {
            continue;
        }
        let got = b.show(rel);
        assert_eq!(
            got["trust"], want,
            "{rel}: trust tier is {} but the spec convention says {want}",
            got["trust"]
        );
    }
}

/// No agent may ever write `verified: {by: human:...}` — in a database row that is a claim, in
/// git it is a signed commit. The scaffolder is the one place the tool itself authors
/// frontmatter, so it is the one place that rule can be broken by accident.
#[test]
fn new_never_scaffolds_a_human_verified_claim() {
    let b = fixture();
    b.ok(&["new", "Note", "Scaffolded By The Suite", "--generated", "capture/conformance"]);
    let created: Vec<String> = b
        .concept_files()
        .into_keys()
        .filter(|k| k.contains("scaffolded-by-the-suite"))
        .collect();
    assert_eq!(created.len(), 1, "`new` did not create exactly one concept: {created:?}");

    let doc = b.show(&created[0]);
    assert_eq!(doc["trust"], "machine", "a scaffolded concept must not claim human verification");
    assert!(
        doc["frontmatter"].get("verified").is_none(),
        "`new` wrote a `verified` field: {}",
        doc["frontmatter"]
    );
}

// ── Search ──────────────────────────────────────────────────────────────────

#[test]
fn search_finds_a_known_concept() {
    let b = fixture();
    let out = b.ok(&["search", "revocation", "--limit", "3"]).stdout;
    assert!(
        out.to_lowercase().contains("revocation"),
        "search for a term that exists in the fixture returned nothing useful: {out}"
    );
}

#[test]
fn search_applies_the_structured_prefilter() {
    let b = fixture();
    let out = b.ok(&["search", "index", "--type", "Decision", "--limit", "5"]).stdout;
    assert!(out.contains("decisions/"), "type=Decision filter returned no decisions: {out}");
    // Result lines name a concept path; the trailing legend also starts with `*`, so match on
    // the path rather than the bullet.
    let stray: Vec<&str> = out
        .lines()
        .filter(|l| l.trim_start().starts_with("* ") && l.contains(".md"))
        .filter(|l| !l.contains("decisions/"))
        .collect();
    assert!(stray.is_empty(), "type=Decision filter leaked non-decisions: {stray:?}");
}

// ── E4a, promoted from a differential divergence to an assertion ────────────

/// `log.md` is a concept, not a reserved filename.
///
/// This was found by running the drills against a third-party bundle: the Python oracle
/// reserved `log.md` alongside `index.md` and so dropped a legitimate `type: Log` concept from
/// `acme_retail`. For as long as both implementations existed this lived in the acceptance
/// gate as a KNOWN_DIVERGENCE — "rust 10, python 9, and rust is right".
///
/// With the oracle gone there is nothing to diverge *from*, so the finding is asserted
/// directly. Deleting an implementation must not delete what it taught us.
#[test]
fn e4a_log_md_is_a_concept_not_a_reserved_name() {
    let src = workspace_root().join("_upstream/acme_retail");
    if !src.exists() {
        return; // upstream bundles are vendored; absence is not a failure
    }
    let b = Bundle::checkout(&src);
    b.ok(&["index", "-q"]);

    assert!(
        Path::new(&b.root).join("log.md").exists(),
        "the bundle this drill is about no longer contains log.md"
    );
    let doc = b.show("log.md");
    assert_eq!(doc["path"], "log.md", "log.md was excluded from the index");
    assert_eq!(
        b.concept_count(),
        10,
        "acme_retail must index 10 concepts; 9 means log.md was reserved away again (E4a)"
    );
}

// ── The gate cannot pass vacuously ─────────────────────────────────────────

/// Guards the guard. Every assertion above is worthless if the fixture quietly loses the
/// adversarial files it is built from — the suite would go green while testing nothing, which
/// is exactly how the architecture check used to lie before it counted recognised layers.
#[test]
fn the_adversarial_corpus_is_actually_present() {
    let b = fixture();
    let files = b.concept_files();
    let required = [
        "adversarial/anchors.md",
        "adversarial/crlf.md",
        "adversarial/empty-type.md",
        "adversarial/flow-style.md",
        "adversarial/multiline.md",
        "adversarial/no-frontmatter.md",
        "adversarial/unknown-type.md",
        "adversarial/verified-chain.md",
    ];
    for rel in required {
        assert!(files.contains_key(rel), "adversarial fixture lost {rel} -- the suite is now weaker than it claims");
    }
    assert!(
        files.keys().any(|k| !k.is_ascii()),
        "fixture lost its unicode path -- CRLF/unicode round-trip is no longer being tested"
    );
}

// ── The two primary adapters must agree on bytes ────────────────────────────

/// `touchstone index` and the MCP `touchstone_index` tool must leave the bundle in the same
/// state, byte for byte.
///
/// This drill exists because they did not. The index pipeline lived inside the CLI command, so
/// the MCP tool did everything except regenerate the `index.md` files — same name, same
/// description, different effect on disk. Nothing caught it: the parity test compares tool
/// *names*, and names were never the problem.
///
/// A name-level parity check cannot see this class of divergence. Only running both and
/// diffing can, which is why this lives in the black-box suite rather than beside the unit
/// tests of either adapter.
#[test]
fn cli_index_and_mcp_index_produce_identical_bytes() {
    let src = workspace_root().join("_fixture");
    let via_cli = Bundle::checkout(&src);
    let via_mcp = Bundle::checkout(&src);

    via_cli.ok(&["index", "-q"]);

    // Drive the MCP surface over stdio: initialize, then call the tool.
    let script = concat!(
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2026-07-28","capabilities":{},"clientInfo":{"name":"conformance","version":"0"}}}"#,
        "\n",
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        "\n",
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"touchstone_index","arguments":{}}}"#,
        "\n",
    );
    let mut child = std::process::Command::new(touchstone_bin())
        .arg("--bundle")
        .arg(&via_mcp.root)
        .arg("mcp")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn mcp server");
    {
        use std::io::Write;
        child.stdin.as_mut().unwrap().write_all(script.as_bytes()).unwrap();
    }
    let out = child.wait_with_output().expect("mcp server exits when stdin closes");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("\"indexed\""),
        "the MCP index tool did not report a result; got: {stdout}"
    );

    // Concept files are untouched by indexing, and every generated index.md must match.
    let cli_files = via_cli.index_files();
    let mcp_files = via_mcp.index_files();
    assert!(!cli_files.is_empty(), "no index.md generated -- the comparison would be vacuous");
    assert_eq!(
        cli_files.keys().collect::<Vec<_>>(),
        mcp_files.keys().collect::<Vec<_>>(),
        "the two adapters generated different SETS of index.md files"
    );
    let differing: Vec<&String> = cli_files
        .iter()
        .filter(|(k, v)| mcp_files.get(*k) != Some(v))
        .map(|(k, _)| k)
        .collect();
    assert!(
        differing.is_empty(),
        "CLI and MCP index disagree on {} file(s): {:?}\n\
         Same name, same description, different bytes -- that is a parity failure the \
         name-level check cannot see.",
        differing.len(),
        &differing[..differing.len().min(3)]
    );
}
