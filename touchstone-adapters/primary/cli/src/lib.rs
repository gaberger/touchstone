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
pub mod parse;
pub mod render;
pub mod store;
pub mod walk;

pub use args::Cli;
pub use store::CliStore;

use args::Command;
use touchstone_ports::{Clock, ConceptParser, ConceptRepository, RawStore};
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
) -> i32
where
    F: ConceptRepository + RawStore,
    P: ConceptParser,
{
    match &cli.command {
        Command::Index(a) => cmd::index::run(bundle, store, a.quiet),
        Command::Search(a) => cmd::search::run(a, store),
        Command::New(a) => cmd::new::run(a, bundle, clock),
        Command::Fmt(a) => cmd::fmt::run(a, bundle),
        Command::Lint => cmd::lint::run(bundle, files, parser),
        Command::Export(a) => cmd::export::run(a, bundle, store),
        Command::Stats => cmd::stats::run(bundle, store),
        Command::Show(a) => cmd::show::run(a, bundle),
    }
}
