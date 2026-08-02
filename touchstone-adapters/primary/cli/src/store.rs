//! Re-exports of the index contract, which now lives in `touchstone-ports`.
//!
//! This module used to *define* `CliStore`, `IndexRecord`, `SearchHit` and friends — a driving
//! adapter dictating the shape its own index must have, implemented by a `FullStore` hand-rolled
//! inside the composition root while `touchstone-sqlite-index` sat unreachable.
//!
//! The contract belongs to the ports layer, so the CLI and the MCP surface are held to the same
//! index rather than each getting the one its author happened to build. These aliases exist so
//! the command modules keep reading naturally; there is no second definition behind them.

pub use touchstone_ports::{
    BundleIndex as CliStore, BundleStats, IndexRecord, SearchHit, SearchQuery as CliSearchFilter,
    SearchVia, Trust,
};
