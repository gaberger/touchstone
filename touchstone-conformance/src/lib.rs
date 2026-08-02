//! The OKF conformance suite — the drills, as a black-box gate on a `touchstone` binary.
//!
//! These drills are not statements about Rust. They are statements about **OKF conformance**
//! (ADR-2608010950), and they were the Python prototype's job until the port reached parity.
//! They now live here, driving the binary through its CLI, so that:
//!
//! - the suite cannot cheat by importing the implementation it is testing, and
//! - a *different* implementation can be gated by setting `TOUCHSTONE_BIN`.
//!
//! | Drill | Asserts |
//! |---|---|
//! | T1   | `index.md` regenerates byte-identically after every derived artifact is deleted |
//! | T1b  | A second index run is a no-op |
//! | T2a  | Ingest → export is byte-exact (CRLF, unicode paths, anchors, multiline scalars) |
//! | T2b  | No key or value lost through canonical reserialization |
//! | T2c  | Unknown types and unknown keys preserved; broken links recorded, not rejected |
//! | T2d  | No temporal value coerced off ISO 8601 |
//! | T2e  | Every formatter rewrite is value-preserving; unsafe files are refused |
//! | T6   | The entire index recovers from files alone |
//!
//! **Every drill runs on a copy.** T1 and T6 are destructive by design — they delete every
//! derived artifact to prove the rebuild is exact. Run in place and they consume the corpus
//! they are testing: the first upstream run deleted a vendored `attesters/index.md`, which
//! both destroyed third-party data and erased the evidence for the defect it had just found.
//! That failure is self-erasing — the second run passes, because the missing file is no
//! longer there to be missing. Hence [`Bundle::checkout`].

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

// ── Locating the artifact under test ────────────────────────────────────────

/// Workspace root — this crate's manifest dir is `<root>/touchstone-conformance`.
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_path_buf()
}

/// The binary under test.
///
/// `TOUCHSTONE_BIN` overrides, which is how a non-Rust implementation gets gated by this
/// same suite. Otherwise prefer the release build, falling back to debug.
///
/// A missing binary is a hard error rather than a skip. A conformance gate that quietly
/// skips is worse than no gate: it reports green while asserting nothing.
pub fn touchstone_bin() -> PathBuf {
    if let Ok(p) = std::env::var("TOUCHSTONE_BIN") {
        let p = PathBuf::from(p);
        assert!(p.exists(), "TOUCHSTONE_BIN points at a missing file: {}", p.display());
        return p;
    }
    let root = workspace_root();
    for profile in ["release", "debug"] {
        let p = root.join("target").join(profile).join("touchstone");
        if p.exists() {
            return p;
        }
    }
    panic!(
        "no touchstone binary found under {}/target/{{release,debug}}.\n\
         build it first:  cargo build --release --bin touchstone\n\
         or point at another implementation:  TOUCHSTONE_BIN=/path/to/impl cargo test -p touchstone-conformance",
        root.display()
    );
}

/// Every bundle the suite runs against: the adversarial self-authored `_fixture`, plus every
/// vendored third-party bundle under `_upstream/`.
///
/// The upstream bundles are the stronger test and the reason they are included: `_fixture` is
/// adversarial but self-authored, which is exactly its weakness — it can only encode
/// assumptions we already hold. Real OKF written by people who never saw this code cannot.
pub fn all_bundles() -> Vec<PathBuf> {
    let root = workspace_root();
    let mut out = vec![root.join("_fixture")];
    if let Ok(entries) = fs::read_dir(root.join("_upstream")) {
        let mut ups: Vec<PathBuf> =
            entries.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        ups.sort();
        out.extend(ups);
    }
    out
}

// ── A disposable working copy ───────────────────────────────────────────────

/// A scratch copy of a bundle. Dropped (and deleted) with the `TempDir`.
pub struct Bundle {
    pub name: String,
    pub root: PathBuf,
    _tmp: tempfile::TempDir,
}

impl Bundle {
    /// Copy `src` into a temp dir, dropping any pre-existing derived state so the run starts
    /// from bare files — which is the only starting point the drills are allowed to assume.
    pub fn checkout(src: &Path) -> Bundle {
        let name = src.file_name().unwrap().to_string_lossy().into_owned();
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let root = tmp.path().join(&name);
        copy_tree(src, &root);
        let _ = fs::remove_dir_all(root.join(".touchstone"));
        Bundle { name, root, _tmp: tmp }
    }

    /// Run `touchstone --bundle <root> <args...>`.
    pub fn run(&self, args: &[&str]) -> Out {
        let out = Command::new(touchstone_bin())
            .arg("--bundle")
            .arg(&self.root)
            .args(args)
            .output()
            .expect("failed to spawn touchstone");
        Out {
            code: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    }

    /// Run and require exit code 0.
    pub fn ok(&self, args: &[&str]) -> Out {
        let o = self.run(args);
        assert_eq!(o.code, 0, "[{}] `{}` exited {}: {}", self.name, args.join(" "), o.code, o.stderr);
        o
    }

    /// Every generated `index.md`, as bundle-relative path → bytes.
    pub fn index_files(&self) -> BTreeMap<String, Vec<u8>> {
        let mut out = BTreeMap::new();
        collect(&self.root, &self.root, &mut |rel, full| {
            if rel == "index.md" || rel.ends_with("/index.md") {
                out.insert(rel.to_string(), fs::read(full).unwrap_or_default());
            }
        });
        out
    }

    /// Every concept file (i.e. every `.md` that is not a generated `index.md`),
    /// as bundle-relative path → bytes.
    pub fn concept_files(&self) -> BTreeMap<String, Vec<u8>> {
        let mut out = BTreeMap::new();
        collect(&self.root, &self.root, &mut |rel, full| {
            if rel != "index.md" && !rel.ends_with("/index.md") {
                out.insert(rel.to_string(), fs::read(full).unwrap_or_default());
            }
        });
        out
    }

    /// Delete every derived artifact: the `.touchstone/` dir and all generated `index.md`.
    /// This is the precondition for T1 and T6 — "derived and disposable" is only a real
    /// claim if something actually disposes of it.
    pub fn destroy_derived(&self) {
        let _ = fs::remove_dir_all(self.root.join(".touchstone"));
        for rel in self.index_files().keys() {
            let _ = fs::remove_file(self.root.join(rel));
        }
    }

    /// Concept count as the implementation reports it.
    pub fn concept_count(&self) -> usize {
        parse_stat(&self.ok(&["stats"]).stdout, "concepts:")
            .unwrap_or_else(|| panic!("[{}] stats printed no concept count", self.name))
    }

    /// Broken-link count as the implementation reports it.
    pub fn broken_links(&self) -> usize {
        let out = self.ok(&["stats"]).stdout;
        // `links: 14 (2 broken -- legal per spec)`
        let line = out.lines().find(|l| l.starts_with("links:")).unwrap_or("");
        line.split('(')
            .nth(1)
            .and_then(|r| r.split_whitespace().next())
            .and_then(|n| n.parse().ok())
            .unwrap_or_else(|| panic!("[{}] cannot read broken-link count from {line:?}", self.name))
    }

    /// The parsed frontmatter view of one concept, via `show --json`.
    pub fn show(&self, rel: &str) -> serde_json::Value {
        let out = self.ok(&["show", rel, "--json"]).stdout;
        serde_json::from_str(&out)
            .unwrap_or_else(|e| panic!("[{}] show {rel} emitted invalid JSON ({e}): {out}", self.name))
    }
}

pub struct Out {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

// ── Small helpers ───────────────────────────────────────────────────────────

/// `concepts: 25` → `Some(25)`.
fn parse_stat(text: &str, prefix: &str) -> Option<usize> {
    text.lines()
        .find(|l| l.starts_with(prefix))
        .and_then(|l| l[prefix.len()..].trim().parse().ok())
}

fn copy_tree(src: &Path, dst: &Path) {
    fs::create_dir_all(dst).expect("mkdir");
    for entry in fs::read_dir(src).expect("readdir").flatten() {
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_tree(&from, &to);
        } else {
            fs::copy(&from, &to).expect("copy");
        }
    }
}

/// Walk every `.md` file under `root`, applying the same directory rules the implementation
/// uses (skip dotted / underscored dirs and `node_modules`), and hand back bundle-relative
/// slash-separated paths.
fn collect(root: &Path, dir: &Path, f: &mut impl FnMut(&str, &Path)) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if name.starts_with('.') || name.starts_with('_') || name == "node_modules" {
                continue;
            }
            collect(root, &path, f);
        } else if name.ends_with(".md") {
            let rel = path.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/");
            f(&rel, &path);
        }
    }
}

/// The frontmatter block of a raw concept file, as text (empty when there is none).
pub fn frontmatter_text(raw: &[u8]) -> String {
    let s = String::from_utf8_lossy(raw);
    let s = s.strip_prefix('\u{feff}').unwrap_or(&s);
    let Some(rest) = s.strip_prefix("---\n").or_else(|| s.strip_prefix("---\r\n")) else {
        return String::new();
    };
    match rest.find("\n---") {
        Some(end) => rest[..end].to_string(),
        None => String::new(),
    }
}

/// Every ISO-8601-looking token in a chunk of text: `2026-01-01`, optionally with a
/// `T`-separated time and a zone. Used to prove no parser rewrote one.
pub fn iso8601_tokens(text: &str) -> Vec<String> {
    let bytes: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 10 <= bytes.len() {
        let is_date = bytes[i..i + 10].iter().enumerate().all(|(k, c)| match k {
            4 | 7 => *c == '-',
            _ => c.is_ascii_digit(),
        });
        // Must not be preceded by a digit or dash, or `12026-01-01` matches at offset 1.
        let clean_start = i == 0 || !(bytes[i - 1].is_ascii_digit() || bytes[i - 1] == '-');
        if is_date && clean_start {
            let mut end = i + 10;
            if end < bytes.len() && bytes[end] == 'T' {
                end += 1;
                while end < bytes.len()
                    && (bytes[end].is_ascii_digit()
                        || matches!(bytes[end], ':' | '.' | '+' | '-' | 'Z'))
                {
                    end += 1;
                }
            }
            out.push(bytes[i..end].iter().collect());
            i = end;
        } else {
            i += 1;
        }
    }
    out
}

/// True if `s` is *entirely* a timestamp that has been coerced off ISO 8601.
///
/// This is the exact shape of the PyYAML defect (FINDINGS E3a): the implicit timestamp
/// resolver rewrites `2026-01-01T00:00:00Z` into `2026-01-01 00:00:00+00:00` — a space where
/// the `T` belongs. Matching the *whole* value matters: prose legitimately mentions dates
/// ("last updated on 2022-11-25 and no longer maintained"), and flagging that would make the
/// drill fire on well-formed upstream content.
pub fn is_coerced_timestamp(s: &str) -> bool {
    let s = s.trim();
    let b: Vec<char> = s.chars().collect();
    if b.len() < 16 {
        return false;
    }
    let date_ok = b[..10].iter().enumerate().all(|(k, c)| match k {
        4 | 7 => *c == '-',
        _ => c.is_ascii_digit(),
    });
    // ` HH:MM` where ISO 8601 requires `THH:MM`.
    date_ok
        && b[10] == ' '
        && b[11].is_ascii_digit()
        && b[12].is_ascii_digit()
        && b[13] == ':'
        && b[14].is_ascii_digit()
        && b[15].is_ascii_digit()
}

/// Walk a JSON value, yielding `(json_pointer_ish_path, value)` for every leaf.
pub fn walk_json<'a>(v: &'a serde_json::Value, path: &str, out: &mut Vec<(String, &'a serde_json::Value)>) {
    match v {
        serde_json::Value::Object(m) => {
            for (k, vv) in m {
                walk_json(vv, &format!("{path}.{k}"), out);
            }
        }
        serde_json::Value::Array(a) => {
            for (i, vv) in a.iter().enumerate() {
                walk_json(vv, &format!("{path}[{i}]"), out);
            }
        }
        leaf => out.push((path.to_string(), leaf)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_tokens_finds_dates_and_datetimes() {
        let t = iso8601_tokens("stale_after: 2026-01-01T00:00:00Z\nborn: 1999-12-31\n");
        assert_eq!(t, vec!["2026-01-01T00:00:00Z", "1999-12-31"]);
    }

    #[test]
    fn iso_tokens_ignores_digit_runs_that_merely_contain_a_date() {
        // A version string or an id must not be mistaken for a timestamp.
        assert!(iso8601_tokens("id: 12026-01-011").is_empty());
    }

    #[test]
    fn coerced_timestamp_matches_the_pyyaml_shape_only() {
        assert!(is_coerced_timestamp("2026-01-01 00:00:00+00:00"));
        assert!(!is_coerced_timestamp("2026-01-01T00:00:00Z"), "correct ISO 8601 must not flag");
        assert!(!is_coerced_timestamp("2026-01-01"), "a bare date is valid ISO 8601");
    }

    #[test]
    fn prose_mentioning_a_date_is_not_a_coerced_timestamp() {
        // The false positive that made this drill fire on a well-formed upstream description.
        assert!(!is_coerced_timestamp(
            "It was last updated on 2022-11-25 and is no longer actively updated."
        ));
    }

    #[test]
    fn frontmatter_text_extracts_the_block_only() {
        let raw = b"---\ntype: Note\n---\n\nBody with --- inside.\n";
        assert_eq!(frontmatter_text(raw), "type: Note");
    }

    #[test]
    fn frontmatter_text_is_empty_without_a_block() {
        assert_eq!(frontmatter_text(b"no frontmatter here\n"), "");
    }
}
