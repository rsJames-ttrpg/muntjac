//! Test helpers: build a bare git repo from a source directory.
//!
//! Used by S7b tests that exercise `fetch_into_cache` against a hermetic
//! bare repo (no network).

use std::fs;
use std::path::Path;
use std::process::Command;

/// Initialize a bare git repo at `bare_dest` and seed it with a single
/// commit containing every file under `source_dir`. Returns the commit SHA.
///
/// Author/committer are deterministic: ("muntjac-test",
/// "test@example.com", timestamp 0). This guarantees the SHA is stable
/// across runs given the same source content — useful for test
/// assertions and for the content-addressed cache.
pub fn init_bare_repo_from_source(
    source_dir: &Path,
    bare_dest: &Path,
) -> Result<String, Box<dyn std::error::Error>> {
    // Create a temp working dir; init non-bare repo; commit; clone to bare.
    let work_dir = tempfile::TempDir::new()?;

    // Copy source_dir contents into work_dir
    for entry in walkdir::WalkDir::new(source_dir) {
        let entry = entry?;
        let rel = entry.path().strip_prefix(source_dir)?;
        let dst = work_dir.path().join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&dst)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry.path(), &dst)?;
        }
    }

    let git = |args: &[&str]| -> Result<String, Box<dyn std::error::Error>> {
        let out = Command::new("git")
            .args(args)
            .current_dir(work_dir.path())
            .env("GIT_AUTHOR_NAME", "muntjac-test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_AUTHOR_DATE", "1970-01-01T00:00:00Z")
            .env("GIT_COMMITTER_NAME", "muntjac-test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_DATE", "1970-01-01T00:00:00Z")
            .output()?;
        if !out.status.success() {
            return Err(format!(
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&out.stderr)
            )
            .into());
        }
        Ok(String::from_utf8(out.stdout)?.trim().to_string())
    };

    git(&["init", "-q", "-b", "main"])?;
    git(&["add", "-A"])?;
    git(&["commit", "-q", "-m", "initial"])?;
    let sha = git(&["rev-parse", "HEAD"])?;

    fs::create_dir_all(bare_dest)?;
    let bare_str = bare_dest.to_str().ok_or("non-utf8 bare path")?;
    git(&["clone", "-q", "--bare", ".", bare_str])?;

    Ok(sha)
}
