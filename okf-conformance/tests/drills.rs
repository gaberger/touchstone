//! The universal drills: every one runs against every bundle.
//!
//! Each has a pre-registered kill criterion from PROTOTYPE.md §3. These are the assertions the
//! architecture rests on — if one fails, a claim in ARCHITECTURE.md's evidence table is false,
//! not merely a test being red.
//!
//! Failures accumulate across bundles and are reported together. A gate that stops at the first
//! bundle tells you one thing is broken; this one tells you whether it is broken *everywhere*,
//! which is usually the question you actually have.

use okf_conformance::*;

/// Defects FINDINGS.md has already adjudicated and nobody has yet decided how to fix.
///
/// `(drill, bundle, substring the failure must contain, why)`.
///
/// A recorded defect is not an excuse — it is a claim with two edges, and both are checked.
/// If the failure changes shape, that is a regression. If it *stops happening*, that is also
/// reported, because it means someone fixed a defect the record still calls open and FINDINGS
/// is now lying. Silence in either direction is how a known-issues list rots into an ignore-list.
const KNOWN_DEFECTS: &[(&str, &str, &str, &str)] = &[(
    "T1 rebuild",
    "acme_retail",
    "attesters/index.md",
    "E4b -- `attesters/` holds no concepts, so the index.md upstream ships for it cannot be \
     regenerated. A1 (\"everything above the bundle is derived\") is falsified as stated: either \
     index rebuilds concept-free directories, or A1 narrows to directories that contain concepts. \
     Undecided -- see FINDINGS.md E4b.",
)];

fn known_defect(drill: &str, bundle: &str) -> Option<(&'static str, &'static str)> {
    KNOWN_DEFECTS
        .iter()
        .find(|(d, b, _, _)| *d == drill && *b == bundle)
        .map(|(_, _, needle, why)| (*needle, *why))
}

/// Run `f` over every bundle, collecting failures rather than panicking on the first.
///
/// Reporting every bundle matters: a gate that stops at the first failure tells you something is
/// broken; this one tells you whether it is broken *everywhere*, which is usually the question
/// you actually have.
fn for_each_bundle(drill: &str, f: impl Fn(&Bundle) -> Result<String, String>) {
    let mut failures = Vec::new();
    let mut notes = Vec::new();
    let bundles = all_bundles();
    assert!(!bundles.is_empty(), "no bundles found -- the suite would be vacuous");

    for src in &bundles {
        let b = Bundle::checkout(src);
        let expected = known_defect(drill, &b.name);
        match (f(&b), expected) {
            (Ok(note), None) => notes.push(format!("  PASS   {:<24} {note}", b.name)),
            (Ok(_), Some((_, why))) => {
                let msg = format!(
                    "recorded defect no longer reproduces -- FINDINGS.md must be updated to close it.\n         {why}"
                );
                failures.push(format!("  FIXED  {:<24} {msg}", b.name));
                notes.push(format!("  FIXED  {:<24} {msg}", b.name));
            }
            (Err(why), Some((needle, note))) if why.contains(needle) => {
                notes.push(format!("  XFAIL  {:<24} {why}\n         {note}", b.name));
            }
            (Err(why), Some((needle, _))) => {
                let msg = format!("known defect changed shape (expected it to mention `{needle}`): {why}");
                failures.push(format!("  FAIL   {:<24} {msg}", b.name));
                notes.push(format!("  FAIL   {:<24} {msg}", b.name));
            }
            (Err(why), None) => {
                failures.push(format!("  FAIL   {:<24} {why}", b.name));
                notes.push(format!("  FAIL   {:<24} {why}", b.name));
            }
        }
    }
    println!("{drill}\n{}", notes.join("\n"));
    assert!(failures.is_empty(), "\n{drill}\n{}\n", failures.join("\n"));
}

// ── T1 ──────────────────────────────────────────────────────────────────────

/// KILL: any generated `index.md` differing by one byte after a full rebuild.
///
/// This is the drill the whole "derived and disposable" claim rests on. If it fails, the
/// derived plane is not derived — it holds state nothing else can reconstruct.
#[test]
fn t1_rebuild_is_byte_identical() {
    for_each_bundle("T1 rebuild", |b| {
        b.ok(&["index", "-q"]);
        let before = b.index_files();
        // Comparing two empty sets is a pass that asserts nothing. An implementation that
        // generates no index.md at all would otherwise sail through this drill.
        if before.is_empty() {
            return Err("no index.md generated -- nothing to compare, drill would pass vacuously".into());
        }

        b.destroy_derived();
        b.ok(&["index", "-q"]);
        let after = b.index_files();

        if before.keys().ne(after.keys()) {
            let lost: Vec<_> = before.keys().filter(|k| !after.contains_key(*k)).collect();
            let gained: Vec<_> = after.keys().filter(|k| !before.contains_key(*k)).collect();
            return Err(format!("index.md set changed -- lost {lost:?}, gained {gained:?}"));
        }
        let diff: Vec<&String> = before.iter().filter(|(k, v)| after.get(*k) != Some(v)).map(|(k, _)| k).collect();
        if diff.is_empty() {
            Ok(format!("{} index.md byte-identical across full rebuild", before.len()))
        } else {
            Err(format!("{} file(s) differ: {:?}", diff.len(), &diff[..diff.len().min(3)]))
        }
    });
}

/// A second index run must change nothing. Non-idempotence means the index is a function of
/// its own prior state, not of the files — which would quietly break T1 on someone else's machine.
#[test]
fn t1b_index_is_idempotent() {
    for_each_bundle("T1b idempotence", |b| {
        // Destroy first, so the files being compared were definitely produced by THIS run.
        // Without it the drill reads the `index.md` files the upstream bundles vendor, and an
        // implementation that does nothing at all looks perfectly idempotent.
        b.destroy_derived();
        b.ok(&["index", "-q"]);
        let a = b.index_files();
        if a.is_empty() {
            return Err("no index.md generated -- idempotence over nothing is not idempotence".into());
        }
        b.ok(&["index", "-q"]);
        let c = b.index_files();
        if a == c {
            Ok(format!("second run is a no-op ({} index.md unchanged)", a.len()))
        } else {
            let diff: Vec<&String> = a.iter().filter(|(k, v)| c.get(*k) != Some(v)).map(|(k, _)| k).collect();
            Err(format!("{} file(s) changed on re-run: {:?}", diff.len(), diff))
        }
    });
}

// ── T2 ──────────────────────────────────────────────────────────────────────

/// KILL: a single non-identical byte through ingest → export.
///
/// The claim being defended is that `export` writes *raw bytes*, so no serializer sits in the
/// write path where it could drop an unknown key. That makes the failure mode structurally
/// impossible rather than merely untested — but only if this drill actually holds.
#[test]
fn t2a_round_trip_is_byte_exact() {
    for_each_bundle("T2a raw round-trip", |b| {
        b.ok(&["index", "-q"]);
        let out = b.root.parent().unwrap().join("export");
        let out_s = out.to_string_lossy().into_owned();
        b.ok(&["export", &out_s, "--force"]);

        let mut bad = Vec::new();
        for (rel, bytes) in b.concept_files() {
            let exported = out.join(&rel);
            match std::fs::read(&exported) {
                Err(_) => bad.push(format!("{rel} (missing)")),
                Ok(got) if got != bytes => bad.push(rel),
                Ok(_) => {}
            }
        }
        if bad.is_empty() {
            Ok("byte-identical incl. CRLF, unicode paths, anchors".into())
        } else {
            Err(format!("{} differ: {:?}", bad.len(), &bad[..bad.len().min(3)]))
        }
    });
}

/// T2b — no key or value lost through canonical reserialization.
///
/// `fmt` is the only command that rewrites a concept, so it is the only place a value could be
/// silently lost. Parse → canonicalize → reparse, and compare the PARSED VALUES: formatting is
/// allowed to change, data is not. This is the diagnostic for the silent-truncation failure mode.
#[test]
fn t2b_reserialization_loses_nothing() {
    for_each_bundle("T2b semantic round-trip", |b| {
        b.ok(&["index", "-q"]);
        let paths: Vec<String> = b.concept_files().into_keys().collect();
        let before: Vec<serde_json::Value> = paths.iter().map(|p| b.show(p)).collect();

        b.ok(&["fmt"]);

        let mut lost = Vec::new();
        for (rel, was) in paths.iter().zip(&before) {
            let now = b.show(rel);
            if was["frontmatter"] != now["frontmatter"] {
                lost.push(format!("{rel}: frontmatter changed"));
            } else if was["trust"] != now["trust"] {
                // The tier is derived from frontmatter, so this cannot drift on its own --
                // if it does, the derivation is reading something the round-trip destroyed.
                lost.push(format!("{rel}: trust tier flipped {} -> {}", was["trust"], now["trust"]));
            }
        }
        if lost.is_empty() {
            Ok(format!("{} concepts survive canonical rewrite unchanged", paths.len()))
        } else {
            Err(format!("{} loss(es): {:?}", lost.len(), &lost[..lost.len().min(3)]))
        }
    });
}

/// T2d — no temporal value coerced off ISO 8601.
///
/// REGRESSION GUARD (FINDINGS E3a). PyYAML's implicit timestamp resolver silently rewrites
/// `2026-01-01T00:00:00Z` into `2026-01-01 00:00:00+00:00`, which is *not* ISO 8601 — a
/// space where the `T` belongs. That defect is why the frontmatter parser is a port at all.
///
/// Checked two ways. The first is universal and catches the exact PyYAML shape. The second is
/// exact but only safe on concepts the implementation considers reproducible: inside anchors,
/// merge keys and block scalars a token in the raw text legitimately need not appear verbatim
/// in the parsed view.
#[test]
fn t2d_temporal_values_stay_iso8601() {
    for_each_bundle("T2d no timestamp coercion", |b| {
        b.ok(&["index", "-q"]);
        let mut bad = Vec::new();
        let mut checked = 0usize;

        for (rel, raw) in b.concept_files() {
            let doc = b.show(&rel);
            let fm = &doc["frontmatter"];
            let mut leaves = Vec::new();
            walk_json(fm, "", &mut leaves);

            // (a) universal: no value may be a timestamp rewritten off ISO 8601. Matched on the
            // whole value, not on any date appearing inside it -- prose mentions dates.
            for (path, v) in &leaves {
                if let Some(s) = v.as_str() {
                    if is_coerced_timestamp(s) {
                        bad.push(format!("{rel}{path}: `{s}` is not ISO 8601 (space separator)"));
                    }
                }
            }

            // (b) exact, where the file is safely reproducible.
            if doc["formattable_refusal"].is_null() {
                checked += 1;
                let fm_str = serde_json::to_string(fm).unwrap_or_default();
                for tok in iso8601_tokens(&frontmatter_text(&raw)) {
                    if !fm_str.contains(&tok) {
                        bad.push(format!("{rel}: `{tok}` did not survive parsing verbatim"));
                    }
                }
            }
        }
        if bad.is_empty() {
            Ok(format!("all temporal values remain ISO 8601 strings ({checked} verbatim-checked)"))
        } else {
            Err(format!("{} coerced: {:?}", bad.len(), &bad[..bad.len().min(3)]))
        }
    });
}

/// T2e — the formatter is safe.
///
/// `fmt` is the only command that rewrites a file, so it is the only one that can destroy
/// authored content. Two obligations: it must REFUSE files whose structure it cannot reproduce,
/// and having refused them it must leave them byte-untouched.
///
/// This drill exists because building the formatter revealed the danger (FINDINGS E3b): the
/// naive canonicalizer resolved merge keys, invented an `&id001` anchor on an unrelated
/// timestamp, and flattened a shell script under `script: |` into a quoted string. Without the
/// drill that damage ships silently.
#[test]
fn t2e_formatter_refuses_what_it_cannot_reproduce() {
    for_each_bundle("T2e fmt safety", |b| {
        b.ok(&["index", "-q"]);

        // Which files does the implementation itself say it must refuse?
        let refused: Vec<String> = b
            .concept_files()
            .into_keys()
            .filter(|rel| !b.show(rel)["formattable_refusal"].is_null())
            .collect();
        let before = b.concept_files();

        b.ok(&["fmt"]);
        let after = b.concept_files();

        let mut violations = Vec::new();
        for rel in &refused {
            if before.get(rel) != after.get(rel) {
                violations.push(format!("{rel}: refused by fmt but rewritten anyway"));
            }
        }
        // `fmt --check` must now be clean: a formatter that cannot reach a fixed point would
        // make every CI run dirty and the check meaningless.
        let check = b.run(&["fmt", "--check"]);
        if check.code != 0 {
            let would: Vec<&str> = check.stdout.lines().filter(|l| l.starts_with("would reformat")).collect();
            violations.push(format!("fmt is not a fixed point: {:?}", &would[..would.len().min(3)]));
        }

        if violations.is_empty() {
            Ok(format!("{} file(s) correctly refused and left untouched", refused.len()))
        } else {
            Err(violations.join("; "))
        }
    });
}

// ── T6 ──────────────────────────────────────────────────────────────────────

/// KILL: concept count differs by more than 0 after destroying everything derived.
///
/// The asymmetry the whole design leans on: losing the derived plane costs time, not knowledge.
/// This is the drill that makes that a measurement rather than a slogan.
#[test]
fn t6_index_recovers_from_files_alone() {
    for_each_bundle("T6 service-death", |b| {
        b.ok(&["index", "-q"]);
        let before = b.concept_count();

        b.destroy_derived();
        b.ok(&["index", "-q"]);
        let after = b.concept_count();

        if before == after {
            Ok(format!("{after} concepts recovered from files alone"))
        } else {
            Err(format!("{after} recovered, expected {before}"))
        }
    });
}

// ── Conformance floor ───────────────────────────────────────────────────────

/// The spec requires consumers not to reject unknown types, unknown keys, or broken links.
/// Rejection is not the only way to fail that: silently dropping a concept fails it too, which
/// is exactly the defect E4a found. Every concept file present must be indexed.
#[test]
fn every_concept_file_is_indexed() {
    for_each_bundle("conformance floor", |b| {
        b.ok(&["index", "-q"]);
        let on_disk = b.concept_files().len();
        let indexed = b.concept_count();
        if on_disk == indexed {
            Ok(format!("{indexed} concepts, none dropped"))
        } else {
            Err(format!("{on_disk} concept files on disk but {indexed} indexed -- {} dropped", on_disk as i64 - indexed as i64))
        }
    });
}
