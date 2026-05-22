//! Pure-python vs native sdist classification.
//!
//! See `docs/superpowers/specs/2026-05-22-muntjac-s5-sdist-prebake-design.md` §4.

use std::path::{Path, PathBuf};

use super::error::ClassifyError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Classification {
    PurePython { backend: AllowlistedBackend },
    Native { reason: NativeReason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllowlistedBackend {
    FlitCore,
    Hatchling,
    Setuptools,
    PoetryCore,
    PdmBackend,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeReason {
    UnknownBackend { build_backend: String },
    MissingPyprojectToml,
    SetuptoolsWithExtModules,
    AdjacentNativeSource { hit: NativeSourceHit },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeSourceHit {
    CargoToml(PathBuf),
    MesonBuild(PathBuf),
    CMakeLists(PathBuf),
    CExt(PathBuf),
    CppExt(PathBuf),
    PyxExt(PathBuf),
}

pub fn classify(_sdist_root: &Path) -> Result<Classification, ClassifyError> {
    todo!("Task 2 implements this")
}
