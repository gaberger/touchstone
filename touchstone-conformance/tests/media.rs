//! Media drills — knowledge attached to things that are not markdown.
//!
//! A bundle is not only prose. OKF puts `resource:` in its canonical key order, and real
//! third-party bundles use it: `resource: https://console.cloud.google.com/bigquery?...`,
//! alongside vendored `viz.html` and `attesters/sql_equality.py`. The format anticipates
//! concepts that *describe an artifact* rather than contain the knowledge themselves.
//!
//! That makes the artifact case load-bearing rather than decorative: a photo, a PDF or a
//! recording is worth nothing without the concept that says what it is, who verified it, and
//! when it goes stale. **The describing concept IS the knowledge.** Lose it and you have a file
//! nobody can account for.
//!
//! These drills run against `_media`, a fixture carrying real bytes — an actual PNG, a valid
//! PDF, a binary blob standing in for video — because a test over `.md` files pretending to be
//! images would not exercise the property.

use okf_conformance_helpers::*;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

// The conformance crate's helpers, re-exported under a local name so this file reads as a
// drill rather than as plumbing.
mod okf_conformance_helpers {
    pub use touchstone_conformance::*;
}

fn media_bundle() -> PathBuf {
    workspace_root().join("_media")
}

/// Every file in the bundle that is not markdown and not derived state.
fn artifacts(root: &Path) -> BTreeSet<String> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeSet<String>) {
        let Ok(entries) = fs::read_dir(dir) else { return };
        for e in entries.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if p.is_dir() {
                if !name.starts_with('.') {
                    walk(root, &p, out);
                }
            } else if !name.ends_with(".md") {
                out.insert(p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let mut out = BTreeSet::new();
    walk(root, root, &mut out);
    out
}

/// Resolve a `resource:` value against the concept that declared it. `None` for anything
/// remote — a URL is not this bundle's to guarantee.
fn local_resource(concept_path: &str, resource: &str) -> Option<String> {
    if resource.starts_with("http://") || resource.starts_with("https://") {
        return None;
    }
    let base = concept_path.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let joined = if base.is_empty() { resource.to_string() } else { format!("{base}/{resource}") };
    let mut parts: Vec<&str> = Vec::new();
    for seg in joined.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

// ── M1 ──────────────────────────────────────────────────────────────────────

/// Non-markdown bytes survive a round-trip untouched.
///
/// T2a asserts this for concepts. A PNG is a harsher test of the same claim: any accidental
/// text handling — line-ending normalisation, UTF-8 lossy conversion — corrupts it silently,
/// and a corrupted image is not an error anyone sees until they open it.
#[test]
fn m1_binary_artifacts_survive_export_byte_for_byte() {
    let b = Bundle::checkout(&media_bundle());
    b.ok(&["index", "-q"]);
    let out = b.root.parent().unwrap().join("exported");
    b.ok(&["export", &out.to_string_lossy(), "--force"]);

    let originals = artifacts(&b.root);
    assert!(!originals.is_empty(), "the media fixture has no artifacts -- drill would be vacuous");

    let mut lost = Vec::new();
    let mut corrupted = Vec::new();
    for rel in &originals {
        let src = fs::read(b.root.join(rel)).expect("source artifact readable");
        match fs::read(out.join(rel)) {
            Err(_) => lost.push(rel.clone()),
            Ok(got) if got != src => corrupted.push(rel.clone()),
            Ok(_) => {}
        }
    }
    assert!(
        lost.is_empty() && corrupted.is_empty(),
        "export does not carry artifacts: {} lost {lost:?}, {} corrupted {corrupted:?}.\n\
         `export` is the portability guarantee -- a bundle whose PDFs do not come with it is \
         not portable, whatever the markdown does.",
        lost.len(),
        corrupted.len()
    );
}

// ── M2 ──────────────────────────────────────────────────────────────────────

/// Every local `resource:` target actually exists.
///
/// A concept pointing at a missing file is not a broken *link* — broken links are legal and
/// represent unwritten knowledge. This is a concept asserting an artifact exists when it does
/// not, which is a different and worse thing: the description survives, the evidence is gone.
#[test]
fn m2_every_local_resource_target_resolves() {
    let b = Bundle::checkout(&media_bundle());
    b.ok(&["index", "-q"]);

    let mut dangling = Vec::new();
    let mut checked = 0;
    for path in b.concept_files().into_keys() {
        let doc = b.show(&path);
        let Some(resource) = doc["frontmatter"]["resource"].as_str() else { continue };
        let Some(target) = local_resource(&path, resource) else { continue };
        checked += 1;
        if !b.root.join(&target).exists() {
            dangling.push(format!("{path} -> {resource}"));
        }
    }
    assert!(checked > 0, "no local `resource:` targets found -- the fixture is not exercising this");
    assert!(dangling.is_empty(), "concepts asserting artifacts that are not there: {dangling:?}");
}

// ── M3 ──────────────────────────────────────────────────────────────────────

/// **Every artifact keeps a describing concept across a rebuild.**
///
/// This is the drill the media case actually turns on, and the one that fails.
///
/// An artifact is accounted for if some surviving concept names it — via `resource:` or a body
/// link. `attesters/sql_equality.py` in the upstream `acme_retail` bundle is described only by
/// a hand-written `attesters/index.md`; `touchstone index` treats every `index.md` as generated,
/// deletes it, and cannot regenerate it because the directory holds no concepts. The artifact
/// survives; the knowledge about it does not.
///
/// That is E4b, which has been carried as an abstract falsification of A1 ("everything above
/// the bundle is derived and disposable"). It is not abstract. It is the first instance of
/// attaching knowledge to a file, and it loses the knowledge.
#[test]
fn m3_no_artifact_is_orphaned_by_a_rebuild() {
    let b = Bundle::checkout(&media_bundle());
    b.ok(&["index", "-q"]);

    let described = |b: &Bundle| -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        for (path, bytes) in b.concept_files() {
            let text = String::from_utf8_lossy(&bytes);
            for art in artifacts(&b.root) {
                let name = art.rsplit('/').next().unwrap_or(&art).to_string();
                if text.contains(&name) {
                    out.insert(art);
                }
            }
            let _ = path;
        }
        // A generated index.md counts only if it would survive a rebuild, so it is excluded:
        // the whole question is what remains when the derived plane is thrown away.
        out
    };

    let before = described(&b);
    b.destroy_derived();
    b.ok(&["index", "-q"]);
    let after = described(&b);

    let all = artifacts(&b.root);
    let orphaned: Vec<String> = all.iter().filter(|a| !after.contains(*a)).cloned().collect();

    // ── Recorded defect, not an accepted one ────────────────────────────────
    // E4b is undecided: either `index` reconstructs a directory that holds no concepts, or A1
    // narrows to directories that do. That is a reading of the spec, and this drill has no
    // business making it. So the KNOWN shape is asserted rather than tolerated -- a different
    // set of orphans fails, and so does an empty one, because a defect that quietly stops
    // reproducing means FINDINGS.md is asserting something false.
    const KNOWN_ORPHANS: [&str; 1] = ["orphan/attester.sql"];

    if orphaned.iter().map(String::as_str).eq(KNOWN_ORPHANS) {
        eprintln!(
            "  XFAIL  m3: {:?} orphaned by rebuild -- E4b, undecided.\n\
             \x20        Its only description is a hand-written index.md in a directory with no\n\
             \x20        concepts, so `index` deletes it and cannot regenerate it. Same shape as\n\
             \x20        _upstream/acme_retail/attesters/sql_equality.py.\n\
             \x20        {} of {} artifacts described before the rebuild.",
            orphaned, before.len(), all.len()
        );
        return;
    }

    assert!(
        orphaned.is_empty(),
        "\n{} artifact(s) orphaned by a rebuild, and NOT the recorded set:\n  got      {orphaned:?}\n  recorded {KNOWN_ORPHANS:?}\n\n\
         The bytes are still on disk. What was lost is the knowledge ABOUT them -- what the \
         file is, who verified it, when it goes stale -- which is the entire reason a knowledge \
         base attaches to media at all. An artifact nobody can account for is not an asset, it \
         is a liability.",
        orphaned.len()
    );

    panic!(
        "m3 no longer reproduces E4b: nothing was orphaned.\n\
         That is good news, but it means the recorded defect is fixed and FINDINGS.md still \
         says otherwise. Close E4b, then remove KNOWN_ORPHANS."
    );
}

// ── M4 ──────────────────────────────────────────────────────────────────────

/// An artifact is findable by what it is *about*, not by its filename.
///
/// The point of a describing concept is that "the escalation procedure" finds `handbook.pdf`
/// without anyone remembering it is called that. If search only matches filenames, the
/// concepts are an index nobody needed.
#[test]
fn m4_artifacts_are_findable_by_description() {
    let b = Bundle::checkout(&media_bundle());
    b.ok(&["index", "-q"]);

    for (query, expect) in [
        ("on-call escalation procedure", "handbook.md"),
        ("hexagonal layout drawn for the design review", "architecture-diagram.md"),
        ("screen recording of a first session", "walkthrough.md"),
    ] {
        let out = b.ok(&["search", query, "--limit", "5"]).stdout;
        assert!(
            out.contains(expect),
            "searching {query:?} did not surface {expect}.\n\
             Artifacts must be findable by what they are about, not by filename.\ngot:\n{out}"
        );
    }
}

// ── M5 ──────────────────────────────────────────────────────────────────────

/// The trust tier reaches the artifact through its describing concept.
///
/// A signed PDF and an unreviewed screen recording are not equally reliable, and the only
/// place that distinction can live is the concept. This is what makes "attach knowledge to
/// media" different from a filesystem with good names.
#[test]
fn m5_trust_tier_carries_to_the_artifact() {
    let b = Bundle::checkout(&media_bundle());
    b.ok(&["index", "-q"]);

    let handbook = b.show("media/handbook.md");
    assert_eq!(
        handbook["trust"], "human",
        "the handbook is human-verified; its concept must say so"
    );
    let walkthrough = b.show("media/walkthrough.md");
    assert_eq!(
        walkthrough["trust"], "unattributed",
        "an unreviewed recording must not inherit the handbook's standing"
    );
    assert_eq!(walkthrough["status"], "draft");
}
