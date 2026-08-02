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
use touchstone_cli_adapter::args::Command;
use std::path::Path;
use touchstone_cli_adapter::{Cli, CliStore};
use touchstone_fs_bundle::{find_bundle, FsBundle};
use touchstone_ports::Clock;
use touchstone_sqlite_index::SqliteIndex;
use touchstone_git_attest::GitAttest;
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
    let vc = GitAttest;

    // `mcp` is served here rather than dispatched as a command: it needs an async runtime and
    // the whole adapter set, neither of which belongs inside a command handler.
    if let Command::Mcp(args) = &cli.command {
        std::process::exit(mcp::serve(&bundle, args.http.as_deref()));
    }

    // Export writes to a directory that is not known until arguments are parsed, and the CLI
    // adapter cannot name a filesystem adapter (rule 5) -- so the composition root supplies the
    // constructor and the command supplies the destination.
    let make_sink = |out: &Path| -> Box<dyn touchstone_ports::ConceptSink> {
        Box::new(FsBundle::new(out))
    };

    let store: &mut dyn CliStore = &mut index;
    let exit_code =
        touchstone_cli_adapter::run(&cli, &bundle, store, &files, &parser, &clock, &vc, &make_sink);
    std::process::exit(exit_code);
}

// ── MCP transports ────────────────────────────────────────────────────────────

mod mcp {
    use super::SystemClock;
    use rmcp::transport::stdio;
    use rmcp::transport::streamable_http_server::{
        session::local::LocalSessionManager, StreamableHttpService,
    };
    use rmcp::ServiceExt;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use touchstone_fs_bundle::FsBundle;
    use touchstone_mcp_adapter::Surface;
    use touchstone_sqlite_index::SqliteIndex;
    use touchstone_git_attest::GitAttest;
use touchstone_yaml_serde::YamlSerde;

    type Ts = Surface<FsBundle, YamlSerde, SqliteIndex, SystemClock>;

    /// Build a surface over `bundle`. Called once for stdio, and once per session for HTTP --
    /// each session gets its own SQLite connection rather than sharing one across requests.
    fn build(bundle: &Path) -> Result<Ts, std::io::Error> {
        let index = SqliteIndex::open(bundle)
            .map_err(|e| std::io::Error::other(format!("cannot open index: {e}")))?;
        Ok(Surface::new(
            bundle.to_path_buf(),
            FsBundle::new(bundle),
            YamlSerde,
            index,
            SystemClock,
            Box::new(GitAttest),
        ))
    }

    pub fn serve(bundle: &Path, http: Option<&str>) -> i32 {
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!("cannot start async runtime: {e}");
                return 1;
            }
        };
        match http {
            Some(addr) => rt.block_on(serve_http(bundle.to_path_buf(), addr)),
            None => rt.block_on(serve_stdio(bundle.to_path_buf())),
        }
    }

    async fn serve_stdio(bundle: PathBuf) -> i32 {
        let surface = match build(&bundle) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("{e}");
                return 1;
            }
        };
        // NOTE: nothing may write to stdout but the protocol itself -- stdout IS the transport.
        // Diagnostics go to stderr.
        eprintln!("touchstone mcp: stdio, bundle {}", bundle.display());
        match surface.serve(stdio()).await {
            Ok(service) => {
                let _ = service.waiting().await;
                0
            }
            Err(e) => {
                eprintln!("mcp server error: {e}");
                1
            }
        }
    }

    async fn serve_http(bundle: PathBuf, addr: &str) -> i32 {
        let service = StreamableHttpService::new(
            move || build(&bundle),
            Arc::new(LocalSessionManager::default()),
            Default::default(),
        );
        let app = axum::Router::new().nest_service("/mcp", service);
        let listener = match tokio::net::TcpListener::bind(addr).await {
            Ok(l) => l,
            Err(e) => {
                eprintln!("cannot bind {addr}: {e}");
                return 1;
            }
        };
        eprintln!("touchstone mcp: streamable-http on http://{addr}/mcp");
        eprintln!("warning: this endpoint can read AND WRITE the bundle -- do not expose it \
                   without authentication in front.");
        match axum::serve(listener, app)
            .with_graceful_shutdown(async { let _ = tokio::signal::ctrl_c().await; })
            .await
        {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("http server error: {e}");
                1
            }
        }
    }
}
