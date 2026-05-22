//! Thin wrappers around the `uv` CLI.
//!
//! Two shellout sites per design spec §5 invariants: `uv lock` (when stale)
//! and `uv build` (for pure-python sdist prebake). All other uv usage is
//! out-of-scope for muntjac.

use std::path::{Path, PathBuf};
use std::process::Output;

use crate::sdist::error::SdistError;

/// Probe for `uv` on PATH. Returns the version string from `uv --version`.
pub fn uv_version() -> Result<String, SdistError> {
    let output = std::process::Command::new("uv")
        .arg("--version")
        .output()
        .map_err(|_| SdistError::UvNotFound)?;
    if !output.status.success() {
        return Err(SdistError::UvNotFound);
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Shell out `uv build --wheel --out-dir <out_dir> <sdist_root>`.
/// Returns the full process Output; callers inspect status + stderr.
pub fn uv_build_wheel(sdist_root: &Path, out_dir: &Path) -> Result<Output, SdistError> {
    std::process::Command::new("uv")
        .arg("build")
        .arg("--wheel")
        .arg("--out-dir")
        .arg(out_dir)
        .arg(sdist_root)
        .output()
        .map_err(|_| SdistError::UvNotFound)
}

/// Shell out `uv lock` in the given project root. Inherits stdio so the
/// user sees uv's progress + errors directly. Returns the exit status.
pub fn uv_lock(project_root: &Path) -> Result<std::process::ExitStatus, SdistError> {
    std::process::Command::new("uv")
        .arg("lock")
        .current_dir(project_root)
        .status()
        .map_err(|_| SdistError::UvNotFound)
}

/// Locate the `uv` binary; returns the resolved path or `UvNotFound`.
#[allow(dead_code)]
pub fn uv_path() -> Result<PathBuf, SdistError> {
    let output = std::process::Command::new("uv")
        .arg("--version")
        .output()
        .map_err(|_| SdistError::UvNotFound)?;
    if !output.status.success() {
        return Err(SdistError::UvNotFound);
    }
    // `which`-style lookup via PATH; on success uv was on PATH.
    Ok(PathBuf::from("uv"))
}
