//! Prebake manifest TOML serde.
//!
//! See `docs/superpowers/specs/2026-05-22-muntjac-s5-sdist-prebake-design.md` §4.

use std::path::Path;

use super::classifier::AllowlistedBackend;
use super::error::SdistError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub version: u32,
    pub entries: Vec<ManifestEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestEntry {
    pub package: String,
    pub version: String,
    pub sdist_sha256: String,
    pub classification: ManifestClassification,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestClassification {
    PurePython {
        backend: AllowlistedBackend,
        wheel_filename: String,
        wheel_sha256: String,
    },
    Native {
        reason: String,
    },
}

impl Manifest {
    pub fn empty() -> Self {
        Self { version: 1, entries: Vec::new() }
    }

    pub fn load(_path: &Path) -> Result<Self, SdistError> {
        todo!("Task 3 implements this")
    }

    pub fn save(&self, _path: &Path) -> Result<(), SdistError> {
        todo!("Task 3 implements this")
    }
}
