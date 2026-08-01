//! Secondary adapter -- one adapter, one crate, one merge unit (hex ADR-001).
//! Imports `okf-ports` ONLY: it cannot reach another adapter, because Cargo will not resolve it.
//!
//! Shells out to the `git` binary rather than linking gix/libgit2. The surface area is
//! tiny (add + commit), gix would add a large dependency tree, and the binary is always
//! present on the platforms this targets. See ARCHITECTURE.md §git-as-adapter.
use std::path::Path;
use std::process::Command;

use okf_ports::VersionControl;

pub struct GitAttest;

impl GitAttest {
    fn run(dir: &Path, args: &[&str]) -> Result<std::process::Output, String> {
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .map_err(|e| format!("failed to spawn git: {e}"))
    }
}

impl VersionControl for GitAttest {
    /// Stage `path` and commit it as an attestation record.
    ///
    /// Returns `Err` with a clear message when:
    /// - `path` is not inside a git repository
    /// - `git add` or `git commit` fails for any other reason
    ///
    /// Returns `Ok(())` without creating a commit when `path` has not changed
    /// since the last commit (idempotent -- the file is already attested).
    fn attest(&self, path: &str) -> Result<(), String> {
        let p = Path::new(path);

        // Pick a directory inside the repo so `git -C dir` works regardless of
        // whether `path` itself exists yet.
        let work_dir = p
            .parent()
            .filter(|d| !d.as_os_str().is_empty())
            .unwrap_or(Path::new("."));

        // Fail clearly when path is not inside a git repository.
        let out = Self::run(work_dir, &["rev-parse", "--show-toplevel"])?;
        if !out.status.success() {
            let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
            return Err(format!("not a git repository ({path}): {msg}"));
        }

        // Stage the target file.
        let out = Self::run(work_dir, &["add", path])?;
        if !out.status.success() {
            let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
            return Err(format!("git add failed: {msg}"));
        }

        // `git diff --cached --quiet` exits 0 when nothing is staged.
        // If nothing is staged, the file is already attested -- return early.
        let out = Self::run(work_dir, &["diff", "--cached", "--quiet"])?;
        if out.status.success() {
            return Ok(());
        }

        // Commit the staged change as an attestation record.
        let commit_msg = format!("attest: {path}");
        let out = Self::run(work_dir, &["commit", "-m", &commit_msg])?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
            return Err(format!("git commit failed: {stderr} {stdout}").trim().to_string());
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;
    use tempfile::TempDir;

    /// Initialise a throwaway git repo with a user identity so commits work.
    fn init_repo() -> TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        let d = dir.path();

        run_git(d, &["init"]);
        run_git(d, &["config", "user.email", "test@example.com"]);
        run_git(d, &["config", "user.name", "Test"]);

        dir
    }

    fn run_git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .expect("git");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    fn commit_count(dir: &Path) -> usize {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["rev-list", "--count", "HEAD"])
            .output()
            .expect("git rev-list");
        if !out.status.success() {
            return 0;
        }
        String::from_utf8_lossy(&out.stdout)
            .trim()
            .parse()
            .unwrap_or(0)
    }

    #[test]
    fn attest_creates_commit() {
        let repo = init_repo();
        let concept = repo.path().join("concept.md");
        fs::write(&concept, "# Hello\ntype: Note\n").unwrap();

        let adapter = GitAttest;
        adapter
            .attest(concept.to_str().unwrap())
            .expect("attest should succeed");

        assert_eq!(commit_count(repo.path()), 1, "expected exactly one commit");
    }

    #[test]
    fn attest_not_a_git_repo_returns_err() {
        let dir = tempfile::tempdir().expect("tempdir");
        let concept = dir.path().join("concept.md");
        fs::write(&concept, "type: Note\n").unwrap();

        let adapter = GitAttest;
        let result = adapter.attest(concept.to_str().unwrap());

        assert!(result.is_err(), "expected Err for non-repo path");
        let msg = result.unwrap_err();
        assert!(
            msg.contains("not a git repository"),
            "error should say 'not a git repository', got: {msg}"
        );
    }

    #[test]
    fn attest_is_idempotent() {
        let repo = init_repo();
        let concept = repo.path().join("concept.md");
        fs::write(&concept, "type: Note\n").unwrap();

        let adapter = GitAttest;
        adapter.attest(concept.to_str().unwrap()).expect("first attest");
        assert_eq!(commit_count(repo.path()), 1);

        // Second call with no changes must not error and must not create another commit.
        adapter
            .attest(concept.to_str().unwrap())
            .expect("second attest (idempotent)");
        assert_eq!(
            commit_count(repo.path()),
            1,
            "idempotent attest must not create an extra commit"
        );
    }

    #[test]
    fn attest_records_updated_content() {
        let repo = init_repo();
        let concept = repo.path().join("concept.md");
        fs::write(&concept, "type: Note\n").unwrap();

        let adapter = GitAttest;
        adapter.attest(concept.to_str().unwrap()).expect("first attest");

        fs::write(&concept, "type: Note\ntitle: Updated\n").unwrap();
        adapter.attest(concept.to_str().unwrap()).expect("second attest");

        assert_eq!(
            commit_count(repo.path()),
            2,
            "each distinct version should produce its own attestation commit"
        );
    }

    #[test]
    fn attest_nonexistent_file_returns_err() {
        let repo = init_repo();
        // Deliberately do NOT create the file.
        let missing = repo.path().join("missing.md");

        let adapter = GitAttest;
        let result = adapter.attest(missing.to_str().unwrap());

        assert!(result.is_err(), "attesting a nonexistent file should fail");
    }
}
