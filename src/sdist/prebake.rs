//! `uv build`-based pure-python sdist → wheel prebake.

use std::path::{Path, PathBuf};

use super::error::SdistError;

#[derive(Debug, Clone)]
pub struct PrebakeOutput {
    pub wheel_path: PathBuf,
    pub wheel_filename: String,
    pub sha256: String,
}

pub fn build_wheel(
    _sdist_root: &Path,
    _out_dir: &Path,
    _package: &str,
    _version: &str,
) -> Result<PrebakeOutput, SdistError> {
    todo!("Task 5 implements this")
}
