//! Composition root — the ONLY crate that imports adapters (ARCHITECTURE.md rule 6).
//!
//! Its whole job is to choose concrete adapters and hand them to the driving adapter. It
//! contains no logic of its own, and that is the point: anything implemented here is
//! unreachable from every test that drives a port, and invisible to any other primary adapter.
//!
//! It did contain logic. A `FullStore` hand-rolled over raw rusqlite lived here — schema and
//! all — while `touchstone-sqlite-index` sat unreachable, and a `_touch_adapters()` function
//! constructed each adapter and threw it away purely so the architecture test's "the CLI wires
//! every adapter" rule would pass. The gate reported a fully wired hexagon over a binary that
//! used almost none of it. Both are gone; `rule_6b` now checks that wiring means *used*.

use clap::Parser;
use std::path::Path;
use touchstone_cli_adapter::{Cli, CliStore};
use touchstone_fs_bundle::{find_bundle, FsBundle};
use touchstone_ports::Clock;
use touchstone_sqlite_index::SqliteIndex;
use touchstone_yaml_serde::YamlSerde;

// ── SystemClock ───────────────────────────────────────────────────────────────

/// The `Clock` port, over `std::time`.
///
/// Hand-rolled Gregorian arithmetic rather than a date crate, for the same reason the domain
/// has no dependencies: the timestamp format is a spec obligation here, not a convenience.
/// Accurate 1970–2099, no leap seconds — well beyond the life of any concept it stamps.
struct SystemClock;

impl Clock for SystemClock {
    fn now_iso8601(&self) -> String {
        use std::time::{SystemTime, UNIX_EPOCH};
        let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let s = secs % 60;
        let m = (secs / 60) % 60;
        let h = (secs / 3600) % 24;
        let days = secs / 86400;
        let z = days + 719468;
        let era = z / 146097;
        let doe = z - era * 146097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let year = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = if month <= 2 { year + 1 } else { year };
        format!("{year:04}-{month:02}-{day:02}T{h:02}:{m:02}:{s:02}Z")
    }
}

// ── main ──────────────────────────────────────────────────────────────────────

fn main() {
    let cli = Cli::parse();
    let bundle = find_bundle(Path::new(&cli.bundle));

    // The concrete adapter set. Swapping any line here swaps an implementation without another
    // crate noticing — which is the only reason the layering is worth enforcing at all.
    let mut index = match SqliteIndex::open(&bundle) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot open index: {e}");
            std::process::exit(1);
        }
    };
    let files = FsBundle::new(&bundle);
    let parser = YamlSerde;
    let clock = SystemClock;

    let store: &mut dyn CliStore = &mut index;
    let exit_code = touchstone_cli_adapter::run(&cli, &bundle, store, &files, &parser, &clock);
    std::process::exit(exit_code);
}
