//! The hexagonal rules of ARCHITECTURE.md, asserted against the dependency graph itself.
//!
//! The rules are already enforced by Cargo: a crate can only import what its `[dependencies]`
//! declare, so `touchstone-domain` naming `touchstone_ports` is `error[E0432]`, not a lint. Cargo additionally
//! forbids dependency cycles outright, which no path-based linter can guarantee.
//!
//! So why this test? Because the compiler enforces the graph, and nothing enforces the *graph*.
//! Adding one line to a `Cargo.toml` silently relaxes a rule and everything still builds. This
//! guards the enforcement mechanism, which is the part a reviewer would not notice weakening.
//!
//! Deliberately parses `Cargo.toml` by hand: `touchstone-domain` must keep zero dependencies (rule 1), and
//! pulling in a TOML crate to check that rule would be its own small irony. std only.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is touchstone-cli/; the workspace is its parent.
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

/// All dependency key names declared by a crate (internal and external alike).
fn deps_of(rel: &str) -> BTreeSet<String> {
    let manifest = workspace_root().join(rel).join("Cargo.toml");
    let text = fs::read_to_string(&manifest)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", manifest.display()));
    let mut out = BTreeSet::new();
    let mut in_deps = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            // Only the plain [dependencies] table counts. dev-/build-dependencies do not ship in
            // the artifact and so cannot violate a runtime layering rule.
            in_deps = t == "[dependencies]";
            continue;
        }
        if !in_deps || t.is_empty() || t.starts_with('#') {
            continue;
        }
        if let Some((name, _)) = t.split_once('=') {
            out.insert(name.trim().to_string());
        }
    }
    out
}

/// Internal (touchstone-*) deps only — used for layering checks.
///
/// Adapters may freely depend on external crates (e.g. `serde_yaml_ng`, `rusqlite`).
/// The layering rule only governs *internal* workspace crates: an adapter must not bypass
/// `touchstone-ports` to reach `touchstone-domain` or `touchstone-usecases` directly, and must never import
/// a sibling adapter. External crates are invisible to the layer model.
fn internal_deps_of(rel: &str) -> BTreeSet<String> {
    deps_of(rel)
        .into_iter()
        .filter(|name| name.starts_with("touchstone-"))
        .collect()
}

const SECONDARY: [&str; 6] = ["yaml-serde", "fs-bundle", "sqlite-index", "git-attest", "crdt-sync", "embed-local"];
const PRIMARY: [&str; 2] = ["cli", "mcp"];

fn adapter_crate_names() -> BTreeSet<String> {
    let mut s: BTreeSet<String> = SECONDARY.iter().map(|a| format!("touchstone-{a}")).collect();
    s.extend(PRIMARY.iter().map(|a| format!("touchstone-{a}-adapter")));
    s
}

#[test]
fn rule_1_domain_depends_on_nothing() {
    // The strongest rule and the cheapest to keep: an empty [dependencies] is what makes every
    // "domain must not import X" violation a compile error, for every possible X, forever.
    assert_eq!(deps_of("touchstone-domain"), BTreeSet::new(), "touchstone-domain must stay dependency-free");
}

#[test]
fn rule_2_ports_depend_on_domain_only() {
    assert_eq!(deps_of("touchstone-ports"), BTreeSet::from(["touchstone-domain".to_string()]));
}

#[test]
fn rule_3_usecases_never_name_an_adapter() {
    let deps = deps_of("touchstone-usecases");
    assert!(deps.is_subset(&BTreeSet::from(["touchstone-domain".to_string(), "touchstone-ports".to_string()])),
        "usecases may depend only on domain + ports, found {deps:?}");
    for a in adapter_crate_names() {
        assert!(!deps.contains(&a), "usecases must not depend on adapter {a}");
    }
}

#[test]
fn rule_4a_secondary_adapters_import_ports_only() {
    // Driven adapters IMPLEMENT ports. They are called by the use-case layer and must never
    // call back into it, so `touchstone-ports` is the whole of their internal surface.
    //
    // Note this is only satisfiable because touchstone-ports RE-EXPORTS the domain value types. Without
    // that, every adapter would need its own touchstone-domain dependency and the rule would be aspirational.
    //
    // Only *internal* (touchstone-*) deps are checked here: adapters may freely depend on external
    // crates (serde_yaml_ng, rusqlite, gix, …). The layering rule governs which workspace
    // crates an adapter may reach — external crates are invisible to the layer model.
    for a in SECONDARY {
        let rel = format!("touchstone-adapters/secondary/{a}");
        let deps = internal_deps_of(&rel);
        assert_eq!(deps, BTreeSet::from(["touchstone-ports".to_string()]),
            "touchstone-{a} must depend on touchstone-ports only (internal deps), found {deps:?}");
    }
}

#[test]
fn rule_4b_primary_adapters_drive_the_use_cases() {
    // Driving adapters CALL use cases. This is the half of rule 4 that was missing, and its
    // absence was not cosmetic: forbidding `touchstone-usecases` here is precisely why the CLI adapter
    // reimplemented the entire use-case layer, leaving 1,074 tested lines unreachable. The gate
    // did not merely fail to catch that duplication -- it required it.
    //
    // A primary adapter may name touchstone-usecases and touchstone-ports, and nothing else internal. It must
    // NOT reach past the use cases into touchstone-domain directly: the domain types it needs are
    // re-exported through touchstone-ports, and letting an adapter bind to the domain would put a second
    // door into the hexagon.
    let allowed = BTreeSet::from(["touchstone-usecases".to_string(), "touchstone-ports".to_string()]);
    let deferred: BTreeSet<&str> = DEFERRED.iter().map(|(n, _)| *n).collect();
    for a in PRIMARY {
        // A declared stub cannot drive anything yet. It is exempt only while it is listed in
        // DEFERRED with a reason -- removing it from that list is what makes this rule bite.
        if deferred.contains(format!("touchstone-{a}-adapter").as_str()) {
            continue;
        }
        let rel = format!("touchstone-adapters/primary/{a}");
        let deps = internal_deps_of(&rel);
        assert!(deps.is_subset(&allowed),
            "touchstone-{a}-adapter may depend only on touchstone-usecases + touchstone-ports, found {deps:?}");
        assert!(deps.contains("touchstone-usecases"),
            "touchstone-{a}-adapter must DRIVE the use cases, not reimplement them -- \
             a primary adapter that does not depend on touchstone-usecases is a second implementation");

        // ...and the dependency must be USED. A declared-but-unimported crate is the same
        // vacuous pass as `_touch_adapters()`: the rule reads green while the adapter carries
        // its own copy of everything. Counted, not merely present, so deleting the last real
        // call site fails the gate instead of silently weakening it.
        let src_dir = workspace_root().join(&rel).join("src");
        // One `use touchstone_usecases::{...}` legitimately imports many functions, so the
        // threshold is presence, not frequency. This catches the vacuous case -- a declared
        // dependency nothing imports -- and nothing subtler. What actually proves the two
        // adapters share behaviour is the byte-identity drill in the conformance suite, which
        // runs both and diffs the result; grep cannot substitute for that.
        let uses = count_uses(&src_dir, "touchstone_usecases");
        assert!(
            uses >= 1,
            "touchstone-{a}-adapter names touchstone-usecases in Cargo.toml but never imports \
             it. Declaring the dependency is not driving the use cases."
        );
    }
}

/// Count references to `needle` across every `.rs` file under `dir`.
fn count_uses(dir: &Path, needle: &str) -> usize {
    let mut n = 0;
    let Ok(entries) = fs::read_dir(dir) else { return 0 };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            n += count_uses(&p, needle);
        } else if p.extension().is_some_and(|e| e == "rs") {
            n += fs::read_to_string(&p).map(|s| s.matches(needle).count()).unwrap_or(0);
        }
    }
    n
}

#[test]
fn rule_5_no_adapter_depends_on_another_adapter() {
    // This is the rule that makes "one agent = one adapter = one worktree = one merge unit" a
    // compiler-backed property rather than a coordination agreement between agents.
    let adapters = adapter_crate_names();
    for (rel, name) in SECONDARY.iter().map(|a| (format!("touchstone-adapters/secondary/{a}"), format!("touchstone-{a}")))
        .chain(PRIMARY.iter().map(|a| (format!("touchstone-adapters/primary/{a}"), format!("touchstone-{a}-adapter"))))
    {
        for other in &adapters {
            if *other == name { continue; }
            assert!(!deps_of(&rel).contains(other), "{name} must not depend on adapter {other}");
        }
    }
}

#[test]
fn rule_6_cli_is_the_only_crate_that_imports_adapters() {
    let adapters = adapter_crate_names();
    let non_cli = ["touchstone-domain", "touchstone-ports", "touchstone-usecases"];
    for c in non_cli {
        for a in &adapters {
            assert!(!deps_of(c).contains(a), "{c} must not import adapter {a} -- only touchstone-cli may");
        }
    }
    // ...and the composition root must actually wire every adapter, or one is dead weight nobody
    // instantiates -- the "orphan adapter" smell, caught here without needing an external analyzer.
    let cli = deps_of("touchstone-cli");
    for a in &adapters {
        assert!(cli.contains(a), "touchstone-cli does not wire adapter {a}");
    }
}

/// Adapters that are deliberately not on the execution path yet, each gated on a named
/// untested assumption. Being listed here is a claim that the crate is a stub by decision,
/// not by neglect -- so the list is short, and every entry cites its gate.
const DEFERRED: [(&str, &str); 2] = [
    ("touchstone-crdt-sync", "A7 -- CRDT sync is unproven; git remains the write path (ADR-2608010930)"),
    ("touchstone-embed-local", "A4 -- hybrid retrieval is unmeasured; BM25 alone until it is"),
];

#[test]
fn rule_6b_wiring_means_used_not_merely_declared() {
    // Guards the guard, again. Rule 6 above checks that touchstone-cli *declares* each adapter, and
    // that is trivially satisfiable by a function which constructs each one and throws it away:
    //
    //     fn _touch_adapters() {                       // <- what used to live in main.rs
    //         let _ = touchstone_fs_bundle::FsBundle::new(".");
    //         let _: Option<touchstone_sqlite_index::SqliteIndex> = None;   // not even constructed
    //     }
    //
    // With that present the gate reported a fully wired hexagon while the binary actually ran on
    // a `FullStore` hand-rolled over raw rusqlite inside main.rs, and the entire adapter and
    // use-case layer -- some 2,500 tested lines -- was unreachable. The rule was true and useless.
    //
    // So: a non-deferred adapter must be named somewhere in the composition root OUTSIDE any
    // such touch function, and no touch function may exist at all.
    let main_rs = fs::read_to_string(workspace_root().join("touchstone-cli/src/main.rs")).expect("main.rs");
    // Match the DEFINITION, not any mention: main.rs documents why the touch function was
    // removed, and a bare substring check flagged its own explanation. A gate that fires on
    // the comment describing the fix is a gate nobody will keep.
    assert!(
        !main_rs.contains("fn _touch_adapters"),
        "touchstone-cli/src/main.rs still defines a touch function -- adapters must be wired, not touched"
    );

    let deferred: BTreeSet<&str> = DEFERRED.iter().map(|(n, _)| *n).collect();
    for a in adapter_crate_names() {
        if deferred.contains(a.as_str()) {
            continue;
        }
        let ident = a.replace('-', "_");
        assert!(
            main_rs.contains(&ident),
            "{a} is declared but never referenced in the composition root -- \
             either wire it or add it to DEFERRED with the assumption it is gated on"
        );
    }
}

#[test]
fn rule_7_conformance_names_no_internal_crate() {
    // The conformance suite must not be able to reach the implementation it is testing. A suite
    // that imports `touchstone-domain` could assert against the same code that produced the answer, and
    // would pass by construction; one that imports any adapter could only ever gate THIS build.
    //
    // Naming nothing internal is what makes the suite a black-box gate over the `touchstone`
    // binary, so the same drills can be pointed at a future rewrite -- or at the upstream
    // reference_agent -- via TOUCHSTONE_BIN (ADR-2608010950).
    let internal = internal_deps_of("touchstone-conformance");
    assert!(
        internal.is_empty(),
        "touchstone-conformance must name no touchstone-* crate -- it gates the binary as a black box, found {internal:?}"
    );
}

#[test]
fn every_workspace_member_is_covered_by_a_rule() {
    // Guards the guard: a new crate added to the workspace without a rule would otherwise be
    // silently unchecked, which is exactly how a layering gate rots.
    let text = fs::read_to_string(workspace_root().join("Cargo.toml")).expect("workspace manifest");
    let declared: BTreeSet<String> = text
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with('"') && l.ends_with("\","))
        .map(|l| l.trim_matches(|c| c == '"' || c == ',').to_string())
        .collect();
    let mut known: BTreeSet<String> =
        ["touchstone-domain", "touchstone-ports", "touchstone-usecases", "touchstone-cli", "touchstone-conformance"]
            .iter().map(|s| s.to_string()).collect();
    known.extend(SECONDARY.iter().map(|a| format!("touchstone-adapters/secondary/{a}")));
    known.extend(PRIMARY.iter().map(|a| format!("touchstone-adapters/primary/{a}")));
    assert_eq!(declared, known, "workspace members and architecture rules have diverged");
}
