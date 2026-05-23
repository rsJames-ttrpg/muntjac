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

const ALLOWLIST: &[(&str, AllowlistedBackend)] = &[
    ("flit_core.buildapi", AllowlistedBackend::FlitCore),
    ("flit_core.api", AllowlistedBackend::FlitCore),
    ("hatchling.build", AllowlistedBackend::Hatchling),
    ("setuptools.build_meta", AllowlistedBackend::Setuptools),
    (
        "setuptools.build_meta:__legacy__",
        AllowlistedBackend::Setuptools,
    ),
    ("poetry.core.masonry.api", AllowlistedBackend::PoetryCore),
    ("pdm.backend", AllowlistedBackend::PdmBackend),
];

const WALK_DEPTH_LIMIT: usize = 4;
const EXCLUDE_DIRS: &[&str] = &[".git", "__pycache__", ".venv", "node_modules"];

fn read_build_backend(pyproject: &Path) -> Result<Option<String>, ClassifyError> {
    let bytes = std::fs::read(pyproject).map_err(|e| ClassifyError::Io {
        path: pyproject.to_path_buf(),
        source: e,
    })?;
    let text = String::from_utf8_lossy(&bytes);
    let parsed: toml::Value = toml::from_str(&text).map_err(|e| ClassifyError::BadToml {
        path: pyproject.to_path_buf(),
        source: e,
    })?;
    let backend = parsed
        .get("build-system")
        .and_then(|v| v.get("build-backend"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    Ok(backend)
}

fn match_backend(s: &str) -> Option<AllowlistedBackend> {
    ALLOWLIST
        .iter()
        .find(|(name, _)| *name == s)
        .map(|(_, b)| *b)
}

fn setup_py_has_ext_modules(sdist_root: &Path) -> Result<bool, ClassifyError> {
    let setup = sdist_root.join("setup.py");
    if !setup.is_file() {
        return Ok(false);
    }
    let bytes = std::fs::read(&setup).map_err(|e| ClassifyError::Io {
        path: setup.clone(),
        source: e,
    })?;
    let text = String::from_utf8_lossy(&bytes);
    Ok(text.contains("ext_modules"))
}

fn walk_for_native_sources(sdist_root: &Path) -> Result<Option<NativeSourceHit>, ClassifyError> {
    fn is_excluded(name: &str) -> bool {
        if EXCLUDE_DIRS.contains(&name) {
            return true;
        }
        name.ends_with(".egg-info")
    }

    fn walk(
        root: &Path,
        cur: &Path,
        depth: usize,
    ) -> Result<Option<NativeSourceHit>, ClassifyError> {
        if depth > WALK_DEPTH_LIMIT {
            return Ok(None);
        }
        let mut entries: Vec<_> = std::fs::read_dir(cur)
            .map_err(|e| ClassifyError::Io {
                path: cur.to_path_buf(),
                source: e,
            })?
            .filter_map(|r| r.ok())
            .collect();
        // Sort by filename for determinism.
        entries.sort_by_key(|e| e.file_name());

        // First pass: check files at this level for native-source hits.
        for entry in &entries {
            let path = entry.path();
            let file_name = entry.file_name();
            let name = file_name.to_string_lossy();
            let ft = entry.file_type().map_err(|e| ClassifyError::Io {
                path: path.clone(),
                source: e,
            })?;
            if ft.is_dir() {
                continue;
            }
            let rel = pathdiff::diff_paths(&path, root).unwrap_or(path.clone());
            if name == "Cargo.toml" {
                return Ok(Some(NativeSourceHit::CargoToml(rel)));
            }
            if name == "meson.build" {
                return Ok(Some(NativeSourceHit::MesonBuild(rel)));
            }
            if name == "CMakeLists.txt" {
                return Ok(Some(NativeSourceHit::CMakeLists(rel)));
            }
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                match ext {
                    "c" => return Ok(Some(NativeSourceHit::CExt(rel))),
                    "cpp" | "cc" | "cxx" => return Ok(Some(NativeSourceHit::CppExt(rel))),
                    "pyx" => return Ok(Some(NativeSourceHit::PyxExt(rel))),
                    _ => {}
                }
            }
        }

        // Second pass: recurse into subdirs.
        for entry in &entries {
            let path = entry.path();
            let file_name = entry.file_name();
            let name = file_name.to_string_lossy();
            let ft = entry.file_type().map_err(|e| ClassifyError::Io {
                path: path.clone(),
                source: e,
            })?;
            if !ft.is_dir() {
                continue;
            }
            if is_excluded(&name) {
                continue;
            }
            if let Some(hit) = walk(root, &path, depth + 1)? {
                return Ok(Some(hit));
            }
        }
        Ok(None)
    }

    walk(sdist_root, sdist_root, 0)
}

pub fn classify(sdist_root: &Path) -> Result<Classification, ClassifyError> {
    if !sdist_root.is_dir() {
        return Err(ClassifyError::NotADir(sdist_root.to_path_buf()));
    }

    // Step 1: pyproject.toml at root?
    let pyproject = sdist_root.join("pyproject.toml");
    if !pyproject.is_file() {
        return Ok(Classification::Native {
            reason: NativeReason::MissingPyprojectToml,
        });
    }

    // Step 2: read build-backend; reject if missing or not in allowlist.
    let backend_str = read_build_backend(&pyproject)?.unwrap_or_default();
    let Some(backend) = match_backend(&backend_str) else {
        return Ok(Classification::Native {
            reason: NativeReason::UnknownBackend {
                build_backend: backend_str,
            },
        });
    };

    // Step 3: setuptools ext_modules carve-out.
    if backend == AllowlistedBackend::Setuptools && setup_py_has_ext_modules(sdist_root)? {
        return Ok(Classification::Native {
            reason: NativeReason::SetuptoolsWithExtModules,
        });
    }

    // Step 4: adjacent native-source walk.
    if let Some(hit) = walk_for_native_sources(sdist_root)? {
        return Ok(Classification::Native {
            reason: NativeReason::AdjacentNativeSource { hit },
        });
    }

    // Step 5: pure-python.
    Ok(Classification::PurePython { backend })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/sdist/classify")
            .join(name)
    }

    #[test]
    fn flit_core_pure() {
        let result = classify(&fixture("flit_core_pure")).unwrap();
        assert_eq!(
            result,
            Classification::PurePython {
                backend: AllowlistedBackend::FlitCore
            }
        );
    }

    #[test]
    fn hatchling_pure() {
        let result = classify(&fixture("hatchling_pure")).unwrap();
        assert_eq!(
            result,
            Classification::PurePython {
                backend: AllowlistedBackend::Hatchling
            }
        );
    }

    #[test]
    fn setuptools_pure() {
        let result = classify(&fixture("setuptools_pure")).unwrap();
        assert_eq!(
            result,
            Classification::PurePython {
                backend: AllowlistedBackend::Setuptools
            }
        );
    }

    #[test]
    fn poetry_core_pure() {
        let result = classify(&fixture("poetry_core_pure")).unwrap();
        assert_eq!(
            result,
            Classification::PurePython {
                backend: AllowlistedBackend::PoetryCore
            }
        );
    }

    #[test]
    fn pdm_backend_pure() {
        let result = classify(&fixture("pdm_backend_pure")).unwrap();
        assert_eq!(
            result,
            Classification::PurePython {
                backend: AllowlistedBackend::PdmBackend
            }
        );
    }

    #[test]
    fn missing_pyproject_is_native() {
        let result = classify(&fixture("missing_pyproject")).unwrap();
        assert_eq!(
            result,
            Classification::Native {
                reason: NativeReason::MissingPyprojectToml
            }
        );
    }

    #[test]
    fn unknown_backend_is_native() {
        let result = classify(&fixture("unknown_backend")).unwrap();
        match result {
            Classification::Native {
                reason: NativeReason::UnknownBackend { build_backend },
            } => assert_eq!(build_backend, "scikit_build_core.build"),
            other => panic!("expected UnknownBackend, got {other:?}"),
        }
    }

    #[test]
    fn setuptools_ext_modules_is_native() {
        let result = classify(&fixture("setuptools_ext_modules")).unwrap();
        assert_eq!(
            result,
            Classification::Native {
                reason: NativeReason::SetuptoolsWithExtModules
            }
        );
    }

    #[test]
    fn cargo_toml_via_unknown_backend() {
        // maturin is not in the allowlist, so this fires UnknownBackend before
        // the adjacent-source walk runs. Documents the priority order from §4.
        let result = classify(&fixture("cargo_toml")).unwrap();
        match result {
            Classification::Native {
                reason: NativeReason::UnknownBackend { build_backend },
            } => assert_eq!(build_backend, "maturin"),
            other => panic!("expected UnknownBackend(maturin), got {other:?}"),
        }
    }

    #[test]
    fn meson_build_is_native() {
        let result = classify(&fixture("meson_build")).unwrap();
        match result {
            Classification::Native {
                reason:
                    NativeReason::AdjacentNativeSource {
                        hit: NativeSourceHit::MesonBuild(_),
                    },
            } => {}
            other => panic!("expected MesonBuild hit, got {other:?}"),
        }
    }

    #[test]
    fn cmakelists_is_native() {
        let result = classify(&fixture("cmakelists")).unwrap();
        match result {
            Classification::Native {
                reason:
                    NativeReason::AdjacentNativeSource {
                        hit: NativeSourceHit::CMakeLists(_),
                    },
            } => {}
            other => panic!("expected CMakeLists hit, got {other:?}"),
        }
    }

    #[test]
    fn c_ext_is_native() {
        let result = classify(&fixture("c_ext")).unwrap();
        match result {
            Classification::Native {
                reason:
                    NativeReason::AdjacentNativeSource {
                        hit: NativeSourceHit::CExt(_),
                    },
            } => {}
            other => panic!("expected CExt hit, got {other:?}"),
        }
    }

    #[test]
    fn cpp_ext_is_native() {
        let result = classify(&fixture("cpp_ext")).unwrap();
        match result {
            Classification::Native {
                reason:
                    NativeReason::AdjacentNativeSource {
                        hit: NativeSourceHit::CppExt(_),
                    },
            } => {}
            other => panic!("expected CppExt hit, got {other:?}"),
        }
    }

    #[test]
    fn pyx_ext_is_native() {
        let result = classify(&fixture("pyx_ext")).unwrap();
        match result {
            Classification::Native {
                reason:
                    NativeReason::AdjacentNativeSource {
                        hit: NativeSourceHit::PyxExt(_),
                    },
            } => {}
            other => panic!("expected PyxExt hit, got {other:?}"),
        }
    }

    #[test]
    fn excluded_dir_does_not_trigger_native() {
        // The `.git/Cargo.toml` decoy can't be tracked in git (git refuses to
        // version anything inside a literal `.git/` path), so we materialize
        // it at test time. The classifier should still ignore it because
        // `.git` is in the EXCLUDE_DIRS list.
        let root = fixture("excluded_dir_has_native_source");
        let git_dir = root.join(".git");
        std::fs::create_dir_all(&git_dir).unwrap();
        std::fs::write(
            git_dir.join("Cargo.toml"),
            "# Should be ignored by the classifier — .git/ is in the exclude list.\n",
        )
        .unwrap();

        let result = classify(&root).unwrap();
        assert_eq!(
            result,
            Classification::PurePython {
                backend: AllowlistedBackend::FlitCore
            }
        );
    }

    #[test]
    fn root_does_not_exist() {
        let err = classify(std::path::Path::new("/nonexistent/dir/should/fail")).unwrap_err();
        match err {
            ClassifyError::NotADir(_) => {}
            other => panic!("expected NotADir, got {other:?}"),
        }
    }
}
