//! Raw-layer drills — the pipeline that turns source material into cited knowledge.
//!
//! The pattern this project is shaped after has three layers: `raw/` immutable, concepts
//! compiled from it, and a schema. Touchstone had the second and third and not the first, which
//! meant a bundle could only hold knowledge somebody had already written in OKF — nobody's
//! starting position, and the reason an adoption test would have measured the wrong thing.
//!
//! These drills hold the properties that make the layer worth having, rather than that the
//! commands run.

use touchstone_conformance::*;
use std::fs;

fn empty_bundle() -> Bundle {
    Bundle::checkout(&workspace_root().join("_fixture"))
}

/// R1: ingest is byte-exact. It parses, converts and summarises nothing.
///
/// The raw layer is what every concept is later checked against, so anything that transformed
/// on the way in would destroy the only property that makes it worth keeping.
#[test]
fn r1_ingest_is_byte_exact() {
    let b = empty_bundle();
    let src = b.root.parent().unwrap().join("source.txt");
    // Deliberately hostile: CRLF, a BOM, and trailing whitespace a "helpful" importer would fix.
    let bytes: &[u8] = b"\xef\xbb\xbfline one\r\nline two   \r\n\r\n";
    fs::write(&src, bytes).unwrap();

    b.ok(&["ingest", src.to_str().unwrap()]);
    let stored = fs::read(b.root.join("raw/source.txt")).expect("ingested file");
    assert_eq!(stored, bytes, "ingest altered the bytes -- the raw layer is no longer a reference");
}

/// R2: raw documents are never concepts, whatever their extension.
///
/// A markdown file pasted in from elsewhere is source material. Indexing it would put
/// unverified third-party text into the same ranking pool as knowledge you wrote, with a trust
/// tier it never earned.
#[test]
fn r2_raw_markdown_is_not_indexed_as_a_concept() {
    let b = empty_bundle();
    let src = b.root.parent().unwrap().join("pasted.md");
    fs::write(&src, "---\ntype: Note\ntitle: Not mine\n---\n\nSomeone else wrote this.\n").unwrap();

    b.ok(&["index", "-q"]);
    let before = b.concept_count();
    b.ok(&["ingest", src.to_str().unwrap()]);
    b.ok(&["index", "-q"]);

    assert_eq!(
        b.concept_count(),
        before,
        "a raw .md was indexed as a concept -- source material must not rank as knowledge"
    );
    assert!(b.root.join("raw/pasted.md").exists(), "but it must still be stored");
}

/// R3: the work queue is derived, not tracked.
///
/// A raw document is processed exactly when some concept cites it. No state file means nothing
/// to fall out of sync, and blowing away the derived plane changes nothing — the same rule the
/// index follows.
#[test]
fn r3_citing_a_source_removes_it_from_the_queue() {
    let b = empty_bundle();
    let src = b.root.parent().unwrap().join("interview.txt");
    fs::write(&src, "Some source material.\n").unwrap();
    b.ok(&["ingest", src.to_str().unwrap()]);

    let queued = b.ok(&["unprocessed"]).stdout;
    assert!(queued.contains("raw/interview.txt"), "expected it queued:\n{queued}");

    fs::write(
        b.root.join("notes/compiled.md"),
        "---\ntype: Note\ntitle: Compiled\nsources:\n  - id: s1\n    resource: raw/interview.txt\n    title: Interview\n---\n\nWhat it said.\n",
    )
    .unwrap();

    let after = b.ok(&["unprocessed"]).stdout;
    assert!(!after.contains("raw/interview.txt"), "citing it should clear the queue:\n{after}");

    // And it survives a full derived-plane wipe, because it was never derived state.
    b.destroy_derived();
    b.ok(&["index", "-q"]);
    let rebuilt = b.ok(&["unprocessed"]).stdout;
    assert!(!rebuilt.contains("raw/interview.txt"), "queue state must not live in the index:\n{rebuilt}");
}

/// R4: sources travel with the bundle.
///
/// A concept citing `raw/interview.txt` in a bundle that does not carry it has a provenance
/// chain with a hole in it. The exported bundle is exactly what a stranger receives.
#[test]
fn r4_raw_sources_survive_export() {
    let b = empty_bundle();
    let src = b.root.parent().unwrap().join("evidence.txt");
    fs::write(&src, "The thing that was actually said.\n").unwrap();
    b.ok(&["ingest", src.to_str().unwrap()]);
    b.ok(&["index", "-q"]);

    let out = b.root.parent().unwrap().join("exported");
    b.ok(&["export", out.to_str().unwrap(), "--force"]);

    let carried = fs::read(out.join("raw/evidence.txt")).expect("raw source did not survive export");
    assert_eq!(carried, fs::read(b.root.join("raw/evidence.txt")).unwrap());
}

/// R5: ingest never overwrites.
///
/// Raw material is immutable by definition. If it changed, it is a different document and
/// deserves a different name — silently replacing it would rewrite the evidence a concept cites.
#[test]
fn r5_ingest_refuses_to_overwrite() {
    let b = empty_bundle();
    let src = b.root.parent().unwrap().join("doc.txt");
    fs::write(&src, "original\n").unwrap();
    b.ok(&["ingest", src.to_str().unwrap()]);

    fs::write(&src, "tampered\n").unwrap();
    let out = b.run(&["ingest", src.to_str().unwrap()]).stdout;
    assert!(out.contains("skipped"), "second ingest should be refused:\n{out}");
    assert_eq!(
        fs::read_to_string(b.root.join("raw/doc.txt")).unwrap(),
        "original\n",
        "ingest overwrote raw material -- a cited source changed underneath its concept"
    );
}

/// R6: a machine-written concept citing nothing is flagged.
///
/// The trust tier already says `machine`. Without a source there is no way to check what the
/// machine read, which is the difference between a provenance chain and a claim.
#[test]
fn r6_lint_flags_generated_concepts_with_no_source() {
    let b = empty_bundle();
    fs::write(
        b.root.join("notes/uncited.md"),
        "---\ntype: Note\ntitle: Uncited\ngenerated:\n  by: capture/agent\n  at: 2026-08-02T00:00:00Z\n---\n\nAsserted from nowhere.\n",
    )
    .unwrap();

    let out = b.run(&["lint"]).stdout;
    assert!(
        out.contains("uncited.md") && out.contains("no `sources`"),
        "a generated concept with no source must be flagged:\n{out}"
    );
}
