//! Primary adapter — CLI.
//!
//! This crate PARSES and DISPATCHES; it does not wire concrete adapters (that
//! is `touchstone-cli`, the composition root, rule 6). It exposes:
//!
//! - `Cli` — the full clap argument tree (import and call `Cli::parse()` in
//!   `touchstone-cli/src/main.rs` to obtain parsed arguments).
//! - `CliStore` — the additional trait the CLI needs from the storage adapter
//!   (beyond the minimal `SearchIndex` port).  The composition root implements
//!   this by wrapping the concrete `SqliteIndex`.
//! - `run(cli, bundle, store, clock) -> i32` — dispatch to the right command.
//!
//! Architecture: imports `touchstone-ports` for `Clock` (and value-type re-exports).
//! All other internal deps are forbidden by the `[dependencies]` table and
//! enforced by Cargo (ARCHITECTURE.md rule 4).

pub mod args;
pub mod cmd;
pub mod store;

pub use args::Cli;
pub use store::CliStore;

use args::Command;
use touchstone_ports::{Clock, ConceptParser, ConceptRepository, ConceptSink, RawStore};
use std::path::Path;

/// Dispatch parsed CLI arguments to the appropriate command handler.
///
/// `bundle` — the resolved bundle root directory.
/// `store`  — index + search + stats (implemented by the composition root).
/// `clock`  — UTC timestamp source (`Clock` from `touchstone-ports`).
///
/// Returns a POSIX exit code: 0 = success, 1 = command-level error, 2 = usage.
pub fn run<F, P>(
    cli: &Cli,
    bundle: &Path,
    store: &mut dyn CliStore,
    files: &F,
    parser: &P,
    clock: &dyn Clock,
    vc: &dyn touchstone_ports::VersionControl,
    make_sink: &dyn Fn(&std::path::Path) -> Box<dyn ConceptSink>,
) -> i32
where
    F: ConceptRepository + RawStore + ConceptSink + touchstone_ports::EventLog,
    P: ConceptParser,
{
    match &cli.command {
        Command::Index(a) => cmd::index::run(bundle, store, files, parser, a.quiet),
        Command::Search(a) => cmd::search::run(a, store),
        Command::New(a) => cmd::new::run(a, parser, files, clock),
        Command::Fmt(a) => cmd::fmt::run(a.check, files, parser),
        Command::Lint => cmd::lint::run(bundle, files, parser),
        Command::Export(a) => cmd::export::run(a, files, make_sink),
        Command::Stats => cmd::stats::run(bundle, store),
        Command::Show(a) => cmd::show::run(a, files, parser),
        // Served by the composition root: it needs an async runtime and the full adapter set,
        // neither of which belongs in a command handler.
        Command::Mcp(_) => 0,
        Command::Verify => cmd::verify::run(files, parser, vc),
        Command::Attest(a) => cmd::attest::run(a, files, parser, vc, clock),
    }
}
