//! Primary adapter — MCP, at specification revision 2026-07-28.
//!
//! Drives the use cases from outside, exactly as the CLI does (hex ADR-019: every capability in
//! one primary adapter must exist in the other, calling the same code). It imports
//! `touchstone-usecases` and `touchstone-ports` and nothing else — architecture rule 4b — so it
//! cannot become a second implementation. `tests/parity.rs` holds it to that.
//!
//! The tool surface is graded against `gaberger/api-ai-readiness`; see `tools.rs` for how each
//! of the six dimensions is satisfied, and `tests/ai_readiness.rs` for the assertions.
//!
//! ## Errors
//!
//! MCP distinguishes protocol errors from tool-execution errors, and the distinction is not
//! cosmetic: clients render protocol errors opaquely, so the caller never sees the message.
//! Anything an agent could recover from — a path that does not exist, an empty query, an unbuilt
//! index — is returned as `Ok(CallToolResult::error(...))` carrying **what to do next**. Only an
//! unknown tool name is a protocol error. That is the rubric's "self-description" dimension, and
//! it is the difference between an agent retrying successfully and an agent giving up.

pub mod tools;

use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    ListToolsResult, PaginatedRequestParams, ProtocolVersion, ServerCapabilities, ServerInfo, Tool,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer};
use serde_json::{json, Map, Value};
use std::path::PathBuf;
use std::sync::Mutex;
use touchstone_ports::{
    BundleIndex, Clock, ConceptParser, ConceptRepository, ConceptSink, EventLog, IndexRecord, RawStore,
    SearchQuery, SearchVia, Trust,
};
use touchstone_usecases::{
    capture_concept_logged, export_bundle, format_bundle, ingest_raw, lint_bundle, reindex_bundle,
    unprocessed_raw, verify_bundle, AttestStatus, CaptureRequest,
};

/// Everything the surface needs, chosen by the composition root.
///
/// Generic over the ports rather than over concrete adapters: this crate cannot name
/// `SqliteIndex` or `FsBundle`, which is what makes the same surface reusable over a different
/// adapter set — and what stops it drifting from the CLI.
pub struct Surface<F, P, I, C> {
    bundle: PathBuf,
    /// Verification only. There is no signing path through this surface, by design.
    vc: Box<dyn touchstone_ports::VersionControl + Send + Sync>,
    files: F,
    parser: P,
    index: Mutex<I>,
    clock: C,
}

impl<F, P, I, C> Surface<F, P, I, C>
where
    F: ConceptRepository + RawStore + ConceptSink + EventLog + Send + Sync + 'static,
    P: ConceptParser + Send + Sync + 'static,
    // NOT Sync: the index lives behind a Mutex, which is what makes `Surface` Sync. Demanding
    // Sync here would exclude any single-threaded connection handle -- rusqlite's among them.
    I: BundleIndex + Send + 'static,
    C: Clock + Send + Sync + 'static,
{
    pub fn new(
        bundle: PathBuf,
        files: F,
        parser: P,
        index: I,
        clock: C,
        vc: Box<dyn touchstone_ports::VersionControl + Send + Sync>,
    ) -> Self {
        Self { bundle, files, parser, index: Mutex::new(index), clock, vc }
    }

    // ── argument helpers ────────────────────────────────────────────────────

    fn str_arg(args: &Map<String, Value>, key: &str) -> Option<String> {
        args.get(key).and_then(Value::as_str).map(str::to_string)
    }

    fn bool_arg(args: &Map<String, Value>, key: &str, default: bool) -> bool {
        args.get(key).and_then(Value::as_bool).unwrap_or(default)
    }

    /// `limit`, defaulted and clamped. Never unbounded — that is the whole point.
    fn limit_arg(args: &Map<String, Value>) -> usize {
        args.get("limit")
            .and_then(Value::as_u64)
            .unwrap_or(tools::DEFAULT_LIMIT as u64)
            .clamp(1, tools::MAX_LIMIT as u64) as usize
    }

    /// Project an object down to the requested `fields`. Absent or empty means everything.
    fn project(value: Value, args: &Map<String, Value>) -> Value {
        let Some(fields) = args.get("fields").and_then(Value::as_array) else { return value };
        let keep: Vec<&str> = fields.iter().filter_map(Value::as_str).collect();
        if keep.is_empty() {
            return value;
        }
        match value {
            Value::Object(m) => {
                Value::Object(m.into_iter().filter(|(k, _)| keep.contains(&k.as_str())).collect())
            }
            Value::Array(a) => Value::Array(a.into_iter().map(|v| Self::project(v, args)).collect()),
            other => other,
        }
    }

    /// Success: structured content plus the same JSON as text, which MCP recommends so clients
    /// that ignore `structuredContent` still see the result.
    fn ok(value: Value) -> CallToolResult {
        let text = serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".into());
        let mut result = CallToolResult::success(vec![ContentBlock::text(text)]);
        result.structured_content = Some(value);
        result
    }

    /// A recoverable failure. `next` must say what the caller should actually do.
    fn fail(what: &str, next: &str) -> CallToolResult {
        CallToolResult::error(vec![ContentBlock::text(format!("{what}\n\nWhat to do next: {next}"))])
    }

    fn require_index(&self) -> Result<(), CallToolResult> {
        let empty = self.index.lock().map(|i| i.all_paths().is_empty()).unwrap_or(true);
        if empty && !self.files.paths().is_empty() {
            return Err(Self::fail(
                "The derived index is empty but the bundle contains concept files.",
                "Call touchstone_index first; it is idempotent and safe to call at any time.",
            ));
        }
        Ok(())
    }

    fn links_of(path: &str, body: &str) -> Vec<String> {
        touchstone_ports::extract_links(body)
            .into_iter()
            .filter_map(|(_, target)| touchstone_ports::resolve_link(path, &target))
            .collect()
    }

    // ── tools ───────────────────────────────────────────────────────────────

    fn t_index(&self) -> CallToolResult {
        // Same use case the CLI calls. Previously this did everything EXCEPT regenerate the
        // index.md files, so `touchstone_index` and `touchstone index` had the same name, the
        // same description, and different effects on disk.
        let Ok(mut index) = self.index.lock() else {
            return Self::fail("The index lock is poisoned.", "Restart the server.");
        };
        let r = reindex_bundle(&self.files, &self.parser, &mut *index);
        Self::ok(json!({
            "indexed": r.total,
            "new": r.new,
            "changed": r.changed,
            "removed": r.removed,
            "indexes_written": r.indexes_written,
            "broken_links": r.broken_links,
            "errors": r.non_conformant.iter()
                .map(|(p, e)| json!({ "path": p, "error": e }))
                .collect::<Vec<_>>(),
        }))
    }

    fn t_search(&self, args: &Map<String, Value>) -> CallToolResult {
        let Some(query) = Self::str_arg(args, "query").filter(|q| !q.trim().is_empty()) else {
            return Self::fail(
                "`query` is required and must not be empty.",
                "Pass a search term, e.g. {\"query\": \"revocation\"}. To see what is in the \
                 bundle without searching, call touchstone_stats or touchstone_discover.",
            );
        };
        if let Err(e) = self.require_index() {
            return e;
        }

        let trust = match args.get("trust").and_then(Value::as_str) {
            Some(t) => match Trust::from_label(t) {
                Some(t) => Some(t),
                None => {
                    return Self::fail(
                        &format!("`{t}` is not a trust tier."),
                        "Use one of: human, attested, machine, unattributed.",
                    )
                }
            },
            None => None,
        };

        let limit = Self::limit_arg(args);
        let q = SearchQuery {
            text: query,
            concept_type: Self::str_arg(args, "type"),
            tag: Self::str_arg(args, "tag"),
            status: Self::str_arg(args, "status"),
            trust,
            limit,
            expand: Self::bool_arg(args, "expand", true),
        };

        let hits = match self.index.lock() {
            Ok(i) => match i.search(&q) {
                Ok(h) => h,
                Err(e) => {
                    return Self::fail(
                        &format!("The search query could not be executed: {e}"),
                        "FTS5 syntax applies -- try plain words, or quote a phrase.",
                    )
                }
            },
            Err(_) => return Self::fail("The index lock is poisoned.", "Restart the server."),
        };

        let list: Vec<Value> = hits
            .iter()
            .map(|h| {
                Self::project(
                    json!({
                        "path": h.path,
                        "title": h.title,
                        "type": h.concept_type,
                        "description": h.description,
                        "trust": h.trust.label(),
                        "via": if h.via == SearchVia::Direct { "direct" } else { "link" },
                    }),
                    args,
                )
            })
            .collect();

        Self::ok(json!({ "returned": list.len(), "limit": limit, "hits": list }))
    }

    fn t_show(&self, args: &Map<String, Value>) -> CallToolResult {
        let Some(path) = Self::str_arg(args, "path") else {
            return Self::fail(
                "`path` is required.",
                "Pass a bundle-relative concept path. Get one from touchstone_search.",
            );
        };
        let want = path.trim_start_matches("./").to_string();
        if !self.files.paths().iter().any(|p| *p == want) {
            return Self::fail(
                &format!("No concept at `{want}`."),
                "Paths are bundle-relative and end in .md. Use touchstone_search to find the \
                 right one; note that index.md files are generated and are not concepts.",
            );
        }
        let Some(raw) = self.files.raw_bytes(&want) else {
            return Self::fail(
                &format!("`{want}` is listed but could not be read."),
                "Check file permissions, then retry.",
            );
        };
        let c = self.parser.parse(&want, &raw);
        let frontmatter: Value = serde_json::from_str(&c.frontmatter_json).unwrap_or(json!({}));

        Self::ok(Self::project(
            json!({
                "path": c.path,
                "type": c.concept_type,
                "title": c.title,
                "status": c.status,
                "trust": c.trust.label(),
                "conformant": c.conformant(),
                "tags": c.tags,
                "links": Self::links_of(&want, &c.body),
                "frontmatter": frontmatter,
            }),
            args,
        ))
    }

    fn t_verify(&self, vc: &dyn touchstone_ports::VersionControl) -> CallToolResult {
        let r = verify_bundle(&self.files, &self.parser, vc);
        let problems: Vec<Value> = r
            .problems
            .iter()
            .map(|(p, s)| {
                let kind = match s {
                    AttestStatus::Unbacked => "unbacked",
                    AttestStatus::Stale => "stale",
                    AttestStatus::BadSignature => "bad_signature",
                    _ => "ok",
                };
                json!({ "path": p, "status": kind })
            })
            .collect();
        Self::ok(json!({
            "checked": r.checked,
            "backed": r.backed,
            "clean": r.is_clean(),
            "problems": problems,
        }))
    }

    fn t_init(&self) -> CallToolResult {
        let mut created = Vec::new();
        for rel in ["raw/.gitkeep", "attest/.gitkeep"] {
            if !self.files.exists(rel) && self.files.write(rel, b"").is_ok() {
                created.push(rel.to_string());
            }
        }
        Self::ok(json!({ "created": created, "concepts": self.files.paths().len() }))
    }

    fn t_ingest(&self, args: &Map<String, Value>) -> CallToolResult {
        let (Some(name), Some(content)) =
            (Self::str_arg(args, "name"), Self::str_arg(args, "content"))
        else {
            return Self::fail(
                "`name` and `content` are both required.",
                "e.g. {\"name\": \"interview.txt\", \"content\": \"...\"}",
            );
        };
        let report = ingest_raw(&[(name, content.into_bytes())], &self.files, &self.files);
        Self::ok(json!({
            "ingested": report.ingested,
            "skipped": report.skipped.iter().map(|(p, w)| json!({ "path": p, "reason": w })).collect::<Vec<_>>(),
        }))
    }

    fn t_unprocessed(&self, args: &Map<String, Value>) -> CallToolResult {
        let pending = unprocessed_raw(&self.files, &self.parser);
        let limit = Self::limit_arg(args);
        let with_content = Self::bool_arg(args, "content", false);
        let docs: Vec<Value> = pending
            .iter()
            .take(limit)
            .map(|p| {
                if with_content {
                    let body = self
                        .files
                        .raw_bytes(p)
                        .map(|b| String::from_utf8_lossy(&b).to_string())
                        .unwrap_or_default();
                    json!({ "path": p, "content": body })
                } else {
                    json!({ "path": p })
                }
            })
            .collect();
        Self::ok(json!({
            "total": self.files.raw_paths().len(),
            "uncited": pending.len(),
            "documents": docs,
        }))
    }

    fn t_stats(&self) -> CallToolResult {
        let Ok(index) = self.index.lock() else {
            return Self::fail("The index lock is poisoned.", "Restart the server.");
        };
        let s = index.stats();
        Self::ok(json!({
            "concepts": s.total,
            "by_type": s.by_type.iter().map(|(k, n)| json!({ "type": k, "count": n })).collect::<Vec<_>>(),
            "by_trust": s.by_trust.iter().map(|(k, n)| json!({ "trust": k, "count": n })).collect::<Vec<_>>(),
            "by_status": s.by_status.iter().map(|(k, n)| json!({ "status": k, "count": n })).collect::<Vec<_>>(),
            "links": s.link_count,
            "broken_links": s.broken_link_count,
        }))
    }

    fn t_lint(&self, args: &Map<String, Value>) -> CallToolResult {
        let report = lint_bundle(&self.files, &self.parser);
        let limit = Self::limit_arg(args);
        let total = report.total();
        let problems: Vec<Value> = report
            .problems
            .iter()
            .take(limit)
            .map(|p| Self::project(json!({ "path": p.path, "message": p.message }), args))
            .collect();
        Self::ok(json!({ "total": total, "returned": problems.len(), "problems": problems }))
    }

    fn t_fmt(&self, args: &Map<String, Value>) -> CallToolResult {
        let check = Self::bool_arg(args, "check", false);
        let report = format_bundle(&self.files, &self.parser, &self.files, check);
        let limit = Self::limit_arg(args);
        let mut files: Vec<Value> = report
            .changed
            .iter()
            .take(limit)
            .map(|p| {
                json!({ "path": p, "action": if check { "would-reformat" } else { "formatted" } })
            })
            .collect();
        files.extend(
            report
                .skipped
                .iter()
                .take(limit.saturating_sub(files.len()))
                .map(|(p, why)| json!({ "path": p, "action": "skipped", "reason": why })),
        );
        Self::ok(json!({
            "changed": report.changed.len(),
            "skipped": report.skipped.len(),
            "total": report.changed.len() + report.skipped.len(),
            "files": files,
        }))
    }

    fn t_new(&self, args: &Map<String, Value>) -> CallToolResult {
        let (Some(concept_type), Some(title)) =
            (Self::str_arg(args, "type"), Self::str_arg(args, "title"))
        else {
            return Self::fail(
                "`type` and `title` are both required.",
                "e.g. {\"type\": \"Note\", \"title\": \"Why files win\"}.",
            );
        };
        let req = CaptureRequest {
            concept_type,
            title,
            description: Self::str_arg(args, "description").unwrap_or_default(),
            tags: args
                .get("tags")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
                .unwrap_or_default(),
            subdir: Self::str_arg(args, "dir"),
            generated_by: Self::str_arg(args, "generated_by"),
        };
        // surface = "mcp": an agent wrote this, and A3 must not count it as adoption.
        match capture_concept_logged(
            &req, &self.parser, &self.files, &self.clock, &self.files, "mcp",
        ) {
            Ok(path) => {
                let trust = self
                    .files
                    .raw_bytes(&path)
                    .map(|raw| self.parser.parse(&path, &raw).trust)
                    .unwrap_or(Trust::Unknown);
                Self::ok(json!({ "path": path, "trust": trust.label() }))
            }
            Err(e) => Self::fail(
                &format!("Could not create the concept: {e}"),
                "A concept with that title may already exist in that directory; try a different \
                 title or pass `dir`.",
            ),
        }
    }

    fn t_export(&self, args: &Map<String, Value>, sink: &F) -> CallToolResult {
        let Some(out) = Self::str_arg(args, "out_dir") else {
            return Self::fail("`out_dir` is required.", "Pass a destination directory path.");
        };
        match export_bundle(&self.files, sink) {
            Ok(stats) => Self::ok(json!({ "exported": stats.count, "out_dir": out })),
            Err(e) => Self::fail(
                &format!("Export failed: {e}"),
                "Check that the destination is writable and retry.",
            ),
        }
    }

    fn t_discover(&self) -> CallToolResult {
        let s = self.index.lock().map(|i| i.stats()).unwrap_or_default();
        let on_disk = self.files.paths().len();
        Self::ok(json!({
            "bundle": self.bundle.display().to_string(),
            "concepts": s.total,
            "concept_files_on_disk": on_disk,
            "indexed": !(s.total == 0 && on_disk > 0),
            "types": s.by_type.iter().map(|(k, n)| json!({ "type": k, "count": n })).collect::<Vec<_>>(),
            "trust_tiers": s.by_trust.iter().map(|(k, n)| json!({ "trust": k, "count": n })).collect::<Vec<_>>(),
            "statuses": s.by_status.iter().map(|(k, n)| json!({ "status": k, "count": n })).collect::<Vec<_>>(),
            "filters": ["type", "tag", "status", "trust"],
            "default_limit": tools::DEFAULT_LIMIT,
            "max_limit": tools::MAX_LIMIT,
            "tools": tools::all().iter().map(|t| json!({
                "name": t.name,
                "title": t.title,
                "read_only": t.annotations.as_ref().and_then(|a| a.read_only_hint).unwrap_or(false),
            })).collect::<Vec<_>>(),
        }))
    }

    /// Dispatch by tool name. `None` means the name is unknown, which is the caller's cue to
    /// raise a protocol error. Public so the transports and tests can drive the surface without
    /// standing up a live MCP client.
    pub fn dispatch(&self, name: &str, args: &Map<String, Value>) -> Option<CallToolResult> {
        Some(match name {
            "touchstone_discover" => self.t_discover(),
            "touchstone_export" => self.t_export(args, &self.files),
            "touchstone_fmt" => self.t_fmt(args),
            "touchstone_index" => self.t_index(),
            "touchstone_ingest" => self.t_ingest(args),
            "touchstone_init" => self.t_init(),
            "touchstone_unprocessed" => self.t_unprocessed(args),
            "touchstone_lint" => self.t_lint(args),
            "touchstone_new" => self.t_new(args),
            "touchstone_search" => self.t_search(args),
            "touchstone_show" => self.t_show(args),
            "touchstone_stats" => self.t_stats(),
            "touchstone_verify" => self.t_verify(self.vc.as_ref()),
            _ => return None,
        })
    }
}

impl<F, P, I, C> ServerHandler for Surface<F, P, I, C>
where
    F: ConceptRepository + RawStore + ConceptSink + EventLog + Send + Sync + 'static,
    P: ConceptParser + Send + Sync + 'static,
    // NOT Sync: the index lives behind a Mutex, which is what makes `Surface` Sync. Demanding
    // Sync here would exclude any single-threaded connection handle -- rusqlite's among them.
    I: BundleIndex + Send + 'static,
    C: Clock + Send + Sync + 'static,
{
    fn get_info(&self) -> ServerInfo {
        // Built field-by-field: both ServerInfo and Implementation are #[non_exhaustive], so a
        // struct literal would break on any rmcp minor release that adds a field.
        let mut info = ServerInfo::default();
        info.protocol_version = ProtocolVersion::default();
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info.server_info = {
            let mut imp = Implementation::default();
            imp.name = "touchstone".into();
            imp.version = env!("CARGO_PKG_VERSION").into();
            imp
        };
        info.instructions = Some(
            "Touchstone exposes an Open Knowledge Format bundle: a directory of markdown \
             concepts with YAML frontmatter, where raw bytes are authoritative and everything \
             else is derived.\n\n\
             Start with touchstone_discover to see what is in the bundle and which filters are \
             worth using. Search returns concept PATHS, not chunks -- read whole files with \
             touchstone_show.\n\n\
             Trust tiers are DERIVED, never authored: `human` means a person signed it, \
             `machine` means an agent generated it, `unattributed` means neither. You cannot \
             write a `verified` claim through this surface, and should not try -- that \
             distinction is the only thing separating curated knowledge from plausible text."
                .into(),
        );
        info
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult { tools: tools::all(), ..Default::default() })
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        tools::all().into_iter().find(|t| t.name.as_ref() == name)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let args = request.arguments.unwrap_or_default();
        match self.dispatch(request.name.as_ref(), &args) {
            Some(result) => Ok(CallToolResponse::Complete(result)),
            // The one genuine protocol error: the request is unroutable. Everything else is a
            // tool-execution error, because the caller can act on those and cannot act on this.
            None => Err(McpError::invalid_params(
                format!(
                    "Unknown tool `{}`. Available: {}",
                    request.name,
                    tools::TOOL_NAMES.join(", ")
                ),
                None,
            )),
        }
    }
}
