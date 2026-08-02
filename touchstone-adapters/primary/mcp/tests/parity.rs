//! CLI–MCP parity (hex ADR-019).
//!
//! > Every capability exposed through one primary adapter MUST have an equivalent in the other.
//! > This is a first-principles rule, not a nice-to-have.
//!
//! ADR-019 specifies this check — "A CI check SHALL compare the set of CLI subcommands against
//! the tool registry and fail if they diverge" — and records it as unimplemented in hex itself.
//! It is implemented here.
//!
//! Why it matters beyond tidiness: without it, feature work drifts toward whichever adapter its
//! author tests in. A human ends up able to do something an agent cannot, or the reverse. In a
//! system whose whole claim is that human and agent interaction are interchangeable, that
//! asymmetry is a design failure, not a gap in coverage.
//!
//! The CLI subcommand list is parsed out of `args.rs` rather than imported: importing it would
//! mean this crate depends on the *other* primary adapter, which rule 5 forbids — and rightly,
//! since two adapters that can see each other can quietly share a definition.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

/// CLI subcommands that intentionally have no MCP tool, each with the reason it is exempt.
///
/// A count limit used to guard this list. That was a proxy for the thing actually worth
/// guarding — that exemptions are principled rather than convenient — and it started to bite
/// as soon as legitimate ones arrived. So the check is now on the REASON: every exemption must
/// cite one of three, and a fourth category cannot be invented without editing the rule.
///
/// - `long-running`: a process, not a request/response capability. MCP has no shape for
///   "runs until interrupted", and a tool that blocked forever would hang the caller.
/// - `human-only`: the trust invariant forbids an agent having it. `attest` is the whole
///   reason that invariant is worth anything — signing needs a private key a human holds, and
///   exposing it over MCP would hand a model exactly the capability the design denies it.
/// - `ergonomic-alias`: the capability already exists on the MCP surface under another name.
///   `capture` is `touchstone_new` with the title inferred and no editor — a shorthand for
///   someone typing at a terminal. An agent already supplies a title, so a second tool doing
///   the same thing worse would be surface bloat, not parity.
const CLI_ONLY: &[(&str, &str)] = &[
    ("mcp", "long-running"),
    ("watch", "long-running"),
    ("attest", "human-only"),
    ("capture", "ergonomic-alias"),
];

/// The only reasons an exemption may cite.
const EXEMPTION_REASONS: &[&str] = &["long-running", "human-only", "ergonomic-alias"];

/// MCP tools that intentionally have no CLI subcommand.
///
/// ADR-019 explicitly permits this direction ("MCP has extra JSON variant"): agents need
/// capability discovery, humans read `--help`.
const MCP_ONLY: &[&str] = &["discover"];

/// Parse `Command` variants out of the CLI adapter's `args.rs`.
///
/// Deliberately textual. The alternative — depending on `touchstone-cli-adapter` — would break
/// rule 5, and a parity check that can only run by violating the layering it is defending would
/// be self-defeating.
fn cli_subcommands() -> BTreeSet<String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../cli/src/args.rs");
    let src = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));

    let start = src.find("pub enum Command {").expect("Command enum not found");
    let body = &src[start..];
    let end = body.find("\n}").expect("unterminated Command enum");

    body[..end]
        .lines()
        .map(str::trim)
        // Variant lines look like `Index(IndexArgs),` or `Lint,` — skip attributes and docs.
        .filter(|l| !l.starts_with("//") && !l.starts_with('#') && !l.starts_with("pub enum"))
        .filter_map(|l| {
            let name: String =
                l.chars().take_while(|c| c.is_ascii_alphanumeric()).collect();
            (!name.is_empty()).then(|| name.to_lowercase())
        })
        .collect()
}

fn mcp_capabilities() -> BTreeSet<String> {
    touchstone_mcp_adapter::tools::TOOL_NAMES
        .iter()
        .map(|n| n.trim_start_matches("touchstone_").to_string())
        .collect()
}

#[test]
fn the_cli_enum_actually_parsed() {
    // Guards the guard. If `args.rs` moves or its shape changes, the parse silently returns an
    // empty set and every parity assertion below becomes vacuously true.
    let cli = cli_subcommands();
    assert!(cli.len() >= 5, "parsed only {cli:?} -- the args.rs parse has broken");
    for expected in ["index", "search", "lint"] {
        assert!(cli.contains(expected), "expected `{expected}` among {cli:?}");
    }
}

#[test]
fn every_cli_command_has_an_mcp_tool() {
    let cli = cli_subcommands();
    let mcp = mcp_capabilities();
    let missing: Vec<&String> = cli
        .iter()
        .filter(|c| !CLI_ONLY.iter().any(|(n, _)| n == c))
        .filter(|c| !mcp.contains(*c))
        .collect();
    assert!(
        missing.is_empty(),
        "CLI commands with no MCP equivalent: {missing:?}.\n\
         Add a touchstone_<name> tool, or add the command to CLI_ONLY with a reason.\n\
         hex ADR-019: add both or add neither."
    );
}

#[test]
fn every_mcp_tool_has_a_cli_command() {
    let cli = cli_subcommands();
    let mcp = mcp_capabilities();
    let missing: Vec<&String> = mcp
        .iter()
        .filter(|t| !MCP_ONLY.contains(&t.as_str()))
        .filter(|t| !cli.contains(*t))
        .collect();
    assert!(
        missing.is_empty(),
        "MCP tools with no CLI equivalent: {missing:?}.\n\
         A human must be able to do everything an agent can. Add the subcommand, or add the \
         tool to MCP_ONLY with a reason."
    );
}

#[test]
fn the_exemption_lists_are_not_hiding_a_real_gap() {
    // An exemption list is one careless commit from becoming a place to bury divergence. Both
    // lists stay tiny, and every entry must name something that actually exists.
    let cli = cli_subcommands();
    for (c, reason) in CLI_ONLY {
        assert!(cli.contains(*c), "CLI_ONLY lists `{c}`, which is not a CLI command");
        assert!(
            EXEMPTION_REASONS.contains(reason),
            "`{c}` cites reason `{reason}`, which is not one of {EXEMPTION_REASONS:?}.\n\
             Adding a new category means changing the rule, deliberately -- which is the point."
        );
    }
    let mcp = mcp_capabilities();
    for t in MCP_ONLY {
        assert!(mcp.contains(*t), "MCP_ONLY lists `{t}`, which is not an MCP tool");
    }
}

#[test]
fn tool_names_follow_the_adr_019_convention() {
    // "CLI uses kebab-case subcommands. MCP uses snake_case with a `<product>_` prefix."
    for name in touchstone_mcp_adapter::tools::TOOL_NAMES {
        assert!(name.starts_with("touchstone_"), "{name}: missing the touchstone_ prefix");
        assert_eq!(name.to_lowercase(), **name, "{name}: must be snake_case");
        assert!(!name.contains('-'), "{name}: kebab-case is the CLI convention, not MCP's");
    }
}
