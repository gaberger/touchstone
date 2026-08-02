//! clap argument types — mirrors Python argparse surface exactly.
use clap::{Args, Parser, Subcommand};

const TEMPLATE_TYPES: [&str; 10] = [
    "Note", "Source", "Person", "Project", "Decision",
    "Meeting", "Term", "Runbook", "System", "Metric",
];

#[derive(Parser, Debug)]
#[command(
    name = "touchstone",
    about = "Touchstone -- provenance and portability for machine-written knowledge"
)]
pub struct Cli {
    #[arg(long, default_value = ".", help = "bundle root (default: discover)")]
    pub bundle: String,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Rebuild the derived index and index.md files.
    Index(IndexArgs),
    /// Search the bundle.
    Search(SearchArgs),
    /// Scaffold a conformant concept.
    New(NewArgs),
    /// Canonicalize frontmatter (the cheap CRDT alternative).
    Fmt(FmtArgs),
    /// Conformance + duplicate checks.
    Lint,
    /// Write raw bytes back out.
    Export(ExportArgs),
    /// Bundle summary.
    Stats,
    /// Print one concept's derived view (parsed frontmatter with `--json`).
    Show(ShowArgs),
    /// Serve the MCP tool surface (stdio by default).
    Mcp(McpArgs),
}

#[derive(Args, Debug)]
pub struct IndexArgs {
    #[arg(short, long)]
    pub quiet: bool,
}

#[derive(Args, Debug)]
pub struct SearchArgs {
    pub query: String,
    #[arg(long = "type")]
    pub concept_type: Option<String>,
    #[arg(long)]
    pub tag: Option<String>,
    #[arg(long)]
    pub status: Option<String>,
    #[arg(long)]
    pub trust: Option<String>,
    #[arg(long, default_value = "10")]
    pub limit: usize,
    #[arg(long)]
    pub no_expand: bool,
}

#[derive(Args, Debug)]
pub struct NewArgs {
    /// OKF concept type (e.g. Note, Decision, Metric).
    #[arg(value_parser = TEMPLATE_TYPES)]
    pub concept_type: String,
    pub title: String,
    #[arg(long)]
    pub dir: Option<String>,
    #[arg(long)]
    pub description: Option<String>,
    #[arg(long = "tag", action = clap::ArgAction::Append)]
    pub tags: Vec<String>,
    #[arg(long, help = "agent id, e.g. capture/claude-opus-5")]
    pub generated: Option<String>,
}

#[derive(Args, Debug)]
pub struct FmtArgs {
    #[arg(long)]
    pub check: bool,
}

#[derive(Args, Debug)]
pub struct McpArgs {
    /// Serve Streamable HTTP on this address instead of stdio, e.g. 127.0.0.1:8765.
    ///
    /// stdio is the default because it is how local agents attach and it has no network
    /// surface. HTTP binds a socket that can read AND WRITE this bundle -- bind it to
    /// loopback unless you have put authentication in front of it.
    #[arg(long, value_name = "ADDR")]
    pub http: Option<String>,
}

#[derive(Args, Debug)]
pub struct ShowArgs {
    /// Bundle-relative concept path, e.g. `notes/why-files-win.md`.
    pub path: String,
    /// Emit machine-readable JSON, with frontmatter verbatim.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct ExportArgs {
    pub out: String,
    #[arg(long)]
    pub force: bool,
}
