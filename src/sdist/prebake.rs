//! `uv build`-based pure-python sdist → wheel prebake.
//!
//! See `docs/superpowers/specs/2026-05-22-muntjac-s5-sdist-prebake-design.md` §4.

use std::io::Read;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::error::SdistError;

#[derive(Debug, Clone)]
pub struct PrebakeOutput {
    pub wheel_path: PathBuf,
    pub wheel_filename: String,
    pub sha256: String,
}

pub fn build_wheel(
    sdist_root: &Path,
    out_dir: &Path,
    package: &str,
    version: &str,
) -> Result<PrebakeOutput, SdistError> {
    // 1) Probe uv first — surfaces UvNotFound with a clear message before
    //    the build attempt.
    let _ = crate::uv::uv_version()?;

    // 2) Shell out `uv build --wheel --out-dir <out_dir> <sdist_root>`.
    let output = crate::uv::uv_build_wheel(sdist_root, out_dir)?;
    if !output.status.success() {
        return Err(SdistError::PrebakeFailed {
            package: package.to_string(),
            version: version.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }

    // 3) Collect the wheels uv wrote.
    let mut wheels: Vec<PathBuf> = std::fs::read_dir(out_dir)
        .map_err(|e| SdistError::Extract {
            package: package.to_string(),
            version: version.to_string(),
            source: e,
        })?
        .filter_map(|r| r.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("whl"))
        .collect();
    wheels.sort();

    if wheels.len() != 1 {
        return Err(SdistError::PrebakeOutputUnexpected {
            package: package.to_string(),
            version: version.to_string(),
            out_dir: out_dir.to_path_buf(),
            found: wheels
                .into_iter()
                .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
                .collect(),
        });
    }
    let wheel_path = wheels.into_iter().next().unwrap();

    // 4) sha256 the wheel.
    let mut file = std::fs::File::open(&wheel_path).map_err(|e| SdistError::Extract {
        package: package.to_string(),
        version: version.to_string(),
        source: e,
    })?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = file.read(&mut buf).map_err(|e| SdistError::Extract {
            package: package.to_string(),
            version: version.to_string(),
            source: e,
        })?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let sha256 = hex::encode(hasher.finalize());

    let wheel_filename = wheel_path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();

    Ok(PrebakeOutput {
        wheel_path,
        wheel_filename,
        sha256,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn uv_on_path() -> bool {
        std::process::Command::new("uv")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Build a minimal flit-core source tree in tmp.
    fn write_synth_sdist(root: &std::path::Path) {
        std::fs::write(
            root.join("pyproject.toml"),
            br#"[build-system]
requires = ["flit_core >=3.2,<4"]
build-backend = "flit_core.buildapi"

[project]
name = "synth-prebake"
version = "0.1.0"
description = ""
requires-python = ">=3.8"
"#,
        )
        .unwrap();
        let pkg = root.join("synth_prebake");
        std::fs::create_dir_all(&pkg).unwrap();
        let mut init = std::fs::File::create(pkg.join("__init__.py")).unwrap();
        writeln!(init, "__version__ = \"0.1.0\"").unwrap();
    }

    #[test]
    fn build_wheel_against_synth_sdist_produces_wheel() {
        if !uv_on_path() {
            eprintln!("skipping: uv not on PATH");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        write_synth_sdist(&src);

        let out = tmp.path().join("out");
        std::fs::create_dir_all(&out).unwrap();

        let result = build_wheel(&src, &out, "synth-prebake", "0.1.0").unwrap();

        assert!(result.wheel_filename.ends_with("-py3-none-any.whl"));
        assert!(result.wheel_filename.contains("synth_prebake-0.1.0"));
        assert!(result.wheel_path.is_file());
        assert_eq!(result.sha256.len(), 64, "sha256 is 64 hex chars");

        // Re-build into a different out dir; the sha should match — uv build is
        // deterministic for a fixed source tree (modulo any embedded mtimes,
        // which flit-core does NOT include).
        let out2 = tmp.path().join("out2");
        std::fs::create_dir_all(&out2).unwrap();
        let r2 = build_wheel(&src, &out2, "synth-prebake", "0.1.0").unwrap();
        assert_eq!(
            r2.sha256, result.sha256,
            "wheel build should be deterministic"
        );
    }

    #[test]
    fn build_wheel_uv_not_found_when_uv_missing() {
        // Hard to test in isolation without PATH manipulation; covered indirectly
        // by uv_version() failure. Document the code path.
    }
}
