//! The hexagonal rules of ARCHITECTURE.md, asserted against the dependency graph itself.
//!
//! The rules are already enforced by Cargo: a crate can only import what its `[dependencies]`
//! declare, so `okf-domain` naming `okf_ports` is `error[E0432]`, not a lint. Cargo additionally
//! forbids dependency cycles outright, which no path-based linter can guarantee.
//!
//! So why this test? Because the compiler enforces the graph, and nothing enforces the *graph*.
//! Adding one line to a `Cargo.toml` silently relaxes a rule and everything still builds. This
//! guards the enforcement mechanism, which is the part a reviewer would not notice weakening.
//!
//! Deliberately parses `Cargo.toml` by hand: `okf-domain` must keep zero dependencies (rule 1), and
//! pulling in a TOML crate to check that rule would be its own small irony. std only.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is okf-cli/; the workspace is its parent.
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

/// Internal (okf-*) deps only — used for layering checks.
///
/// Adapters may freely depend on external crates (e.g. `serde_yaml_ng`, `rusqlite`).
/// The layering rule only governs *internal* workspace crates: an adapter must not bypass
/// `okf-ports` to reach `okf-domain` or `okf-usecases` directly, and must never import
/// a sibling adapter. External crates are invisible to the layer model.
fn internal_deps_of(rel: &str) -> BTreeSet<String> {
    deps_of(rel)
        .into_iter()
        .filter(|name| name.starts_with("okf-"))
        .collect()
}

const SECONDARY: [&str; 6] = ["yaml-serde", "fs-bundle", "sqlite-index", "git-attest", "crdt-sync", "embed-local"];
const PRIMARY: [&str; 2] = ["cli", "mcp"];

fn adapter_crate_names() -> BTreeSet<String> {
    let mut s: BTreeSet<String> = SECONDARY.iter().map(|a| format!("okf-{a}")).collect();
    s.extend(PRIMARY.iter().map(|a| format!("okf-{a}-adapter")));
    s
}

#[test]
fn rule_1_domain_depends_on_nothing() {
    // The strongest rule and the cheapest to keep: an empty [dependencies] is what makes every
    // "domain must not import X" violation a compile error, for every possible X, forever.
    assert_eq!(deps_of("okf-domain"), BTreeSet::new(), "okf-domain must stay dependency-free");
}

#[test]
fn rule_2_ports_depend_on_domain_only() {
    assert_eq!(deps_of("okf-ports"), BTreeSet::from(["okf-domain".to_string()]));
}

#[test]
fn rule_3_usecases_never_name_an_adapter() {
    let deps = deps_of("okf-usecases");
    assert!(deps.is_subset(&BTreeSet::from(["okf-domain".to_string(), "okf-ports".to_string()])),
        "usecases may depend only on domain + ports, found {deps:?}");
    for a in adapter_crate_names() {
        assert!(!deps.contains(&a), "usecases must not depend on adapter {a}");
    }
}

#[test]
fn rule_4_adapters_import_ports_only() {
    // Note this is only satisfiable because okf-ports RE-EXPORTS the domain value types. Without
    // that, every adapter would need its own okf-domain dependency and rule 4 would be aspirational.
    //
    // Only *internal* (okf-*) deps are checked here: adapters may freely depend on external
    // crates (serde_yaml_ng, rusqlite, gix, …). The layering rule governs which workspace
    // crates an adapter may reach — external crates are invisible to the layer model.
    for (rel, name) in SECONDARY.iter().map(|a| (format!("okf-adapters/secondary/{a}"), format!("okf-{a}")))
        .chain(PRIMARY.iter().map(|a| (format!("okf-adapters/primary/{a}"), format!("okf-{a}-adapter"))))
    {
        let deps = internal_deps_of(&rel);
        assert_eq!(deps, BTreeSet::from(["okf-ports".to_string()]),
            "{name} must depend on okf-ports only (internal deps), found {deps:?}");
    }
}

#[test]
fn rule_5_no_adapter_depends_on_another_adapter() {
    // This is the rule that makes "one agent = one adapter = one worktree = one merge unit" a
    // compiler-backed property rather than a coordination agreement between agents.
    let adapters = adapter_crate_names();
    for (rel, name) in SECONDARY.iter().map(|a| (format!("okf-adapters/secondary/{a}"), format!("okf-{a}")))
        .chain(PRIMARY.iter().map(|a| (format!("okf-adapters/primary/{a}"), format!("okf-{a}-adapter"))))
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
    let non_cli = ["okf-domain", "okf-ports", "okf-usecases"];
    for c in non_cli {
        for a in &adapters {
            assert!(!deps_of(c).contains(a), "{c} must not import adapter {a} -- only okf-cli may");
        }
    }
    // ...and the composition root must actually wire every adapter, or one is dead weight nobody
    // instantiates -- the "orphan adapter" smell, caught here without needing an external analyzer.
    let cli = deps_of("okf-cli");
    for a in &adapters {
        assert!(cli.contains(a), "okf-cli does not wire adapter {a}");
    }
}

#[test]
fn rule_7_conformance_names_no_internal_crate() {
    // The conformance suite must not be able to reach the implementation it is testing. A suite
    // that imports `okf-domain` could assert against the same code that produced the answer, and
    // would pass by construction; one that imports any adapter could only ever gate THIS build.
    //
    // Naming nothing internal is what makes the suite a black-box gate over the `touchstone`
    // binary, so the same drills can be pointed at a future rewrite -- or at the upstream
    // reference_agent -- via TOUCHSTONE_BIN (ADR-2608010950).
    let internal = internal_deps_of("okf-conformance");
    assert!(
        internal.is_empty(),
        "okf-conformance must name no okf-* crate -- it gates the binary as a black box, found {internal:?}"
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
        ["okf-domain", "okf-ports", "okf-usecases", "okf-cli", "okf-conformance"]
            .iter().map(|s| s.to_string()).collect();
    known.extend(SECONDARY.iter().map(|a| format!("okf-adapters/secondary/{a}")));
    known.extend(PRIMARY.iter().map(|a| format!("okf-adapters/primary/{a}")));
    assert_eq!(declared, known, "workspace members and architecture rules have diverged");
}
