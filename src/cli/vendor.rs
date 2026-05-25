//! `muntjac vendor` — prebake pure-python sdists into wheels.
//!
//! See spec §5 for the full pipeline.

use std::io::Read;
use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use url::Url;

use crate::cli::Globals;
use crate::config::{Config, Tree};
use crate::lock::types::Package;
use crate::sdist::{
    AllowlistedBackend, Classification, Manifest, ManifestClassification, ManifestEntry,
    NativeReason, NativeSourceHit, classify,
};

pub fn run(globals: &Globals) -> Result<()> {
    let workdir = globals.workdir().context("resolving working directory")?;
    let config_path = workdir.join("muntjac.toml");
    let config_text = std::fs::read_to_string(&config_path)
        .with_context(|| format!("reading {}", config_path.display()))?;
    let config = Config::from_str(&config_text)
        .with_context(|| format!("parsing {}", config_path.display()))?;

    for tree in crate::cli::resolve_trees(&config, globals.tree.as_deref())? {
        vendor_tree(globals, &workdir, &config_path, tree)
            .with_context(|| format!("vendoring tree '{}'", tree.name))?;
    }
    Ok(())
}

fn vendor_tree(globals: &Globals, workdir: &Path, config_path: &Path, tree: &Tree) -> Result<()> {
    let third_party_dir = workdir.join(&tree.third_party_dir);

    // Step 1: lock freshness. Resolve uv.lock relative to the tree's manifest
    // directory (same convention buckify uses).
    let cfg_dir = config_path.parent().unwrap_or(Path::new("."));
    let manifest_dir = cfg_dir.join(tree.manifest_path.parent().unwrap_or(Path::new("")));
    let pyproject = cfg_dir.join(&tree.manifest_path);
    let lockfile_path = manifest_dir.join("uv.lock");

    if pyproject.is_file() && lockfile_path.is_file() && !globals.frozen && !globals.no_network {
        let py_mtime = std::fs::metadata(&pyproject)
            .with_context(|| format!("reading metadata of {}", pyproject.display()))?
            .modified()
            .with_context(|| format!("reading mtime of {}", pyproject.display()))?;
        let lock_mtime = std::fs::metadata(&lockfile_path)
            .with_context(|| format!("reading metadata of {}", lockfile_path.display()))?
            .modified()
            .with_context(|| format!("reading mtime of {}", lockfile_path.display()))?;
        if py_mtime > lock_mtime {
            eprintln!("muntjac vendor: pyproject.toml is newer than uv.lock; running `uv lock`");
            let status = crate::uv::uv_lock(&manifest_dir)?;
            if !status.success() {
                anyhow::bail!("`uv lock` failed with status {status}");
            }
        }
    }

    // Step 2: parse uv.lock
    let lockfile_text = std::fs::read_to_string(&lockfile_path)
        .with_context(|| format!("reading {}", lockfile_path.display()))?;
    let lockfile = crate::lock::parser::parse(&lockfile_text)
        .with_context(|| format!("parsing {}", lockfile_path.display()))?;

    // Step 3+4: identify sdist-only packages; process each.
    let mut entries: Vec<ManifestEntry> = Vec::new();
    let prebake_dir = third_party_dir.join("prebake");
    std::fs::create_dir_all(&prebake_dir)
        .with_context(|| format!("creating {}", prebake_dir.display()))?;
    write_gitignore(&prebake_dir)?;

    for pkg in &lockfile.packages {
        if !is_sdist_only(pkg) {
            continue;
        }
        let sdist = pkg.sdist.as_ref().unwrap();
        let pkg_name = pkg.name.as_ref().to_string();
        let pkg_version = pkg.version.to_string();
        let expected_sha = sdist.hash.trim_start_matches("sha256:").to_string();

        // Step 4a: download to tempdir
        let tmp = tempfile::tempdir().context("creating tempdir for tarball")?;
        let tarball_path = tmp.path().join("sdist.tar.gz");
        download(&sdist.url, &tarball_path, &pkg_name, &pkg_version)?;
        verify_sha256(&tarball_path, &expected_sha, &pkg_name, &pkg_version)?;

        // Step 4b: extract
        let extract_dir = tmp.path().join("extracted");
        std::fs::create_dir_all(&extract_dir)
            .with_context(|| format!("creating {}", extract_dir.display()))?;
        extract_tarball(&tarball_path, &extract_dir, &pkg_name, &pkg_version)?;
        let sdist_root = find_sdist_root(&extract_dir)?;

        // Step 4c: classify
        match classify(&sdist_root)? {
            Classification::PurePython { backend } => {
                eprintln!(
                    "muntjac vendor: prebaking {} {} ({})",
                    pkg_name,
                    pkg_version,
                    backend_str(backend)
                );
                // Step 4d: prebake
                let staging = tmp.path().join("staging");
                std::fs::create_dir_all(&staging)
                    .with_context(|| format!("creating {}", staging.display()))?;
                let result =
                    crate::sdist::build_wheel(&sdist_root, &staging, &pkg_name, &pkg_version)?;
                // Step 4e: move into prebake/
                let final_path = prebake_dir.join(&result.wheel_filename);
                if final_path.is_file() {
                    std::fs::remove_file(&final_path)
                        .with_context(|| format!("removing {}", final_path.display()))?;
                }
                std::fs::rename(&result.wheel_path, &final_path)
                    .or_else(|_| {
                        // rename across filesystems fails; fall back to copy + remove.
                        std::fs::copy(&result.wheel_path, &final_path)?;
                        std::fs::remove_file(&result.wheel_path)?;
                        Ok::<_, std::io::Error>(())
                    })
                    .with_context(|| {
                        format!(
                            "moving {} → {}",
                            result.wheel_path.display(),
                            final_path.display()
                        )
                    })?;
                eprintln!(
                    "prebaked: {} {} → {}",
                    pkg_name,
                    pkg_version,
                    pathdiff::diff_paths(&final_path, workdir)
                        .unwrap_or_else(|| final_path.clone())
                        .display()
                );
                entries.push(ManifestEntry {
                    package: pkg_name,
                    version: pkg_version,
                    sdist_sha256: expected_sha,
                    classification: ManifestClassification::PurePython {
                        backend,
                        wheel_filename: result.wheel_filename,
                        wheel_sha256: result.sha256,
                    },
                });
            }
            Classification::Native { reason } => {
                eprintln!(
                    "skipped: {} {} (native; will error at buckify if no wheel matches)",
                    pkg_name, pkg_version
                );
                entries.push(ManifestEntry {
                    package: pkg_name,
                    version: pkg_version,
                    sdist_sha256: expected_sha,
                    classification: ManifestClassification::Native {
                        reason: render_native_reason(&reason),
                    },
                });
            }
        }
    }

    // Step 5: write manifest
    let manifest = Manifest {
        version: 1,
        entries,
    };
    let manifest_path = prebake_dir.join(".manifest.toml");
    manifest.save(&manifest_path)?;
    Ok(())
}

fn is_sdist_only(pkg: &Package) -> bool {
    use crate::lock::types::Source;
    matches!(pkg.source, Source::Registry { .. }) && pkg.sdist.is_some() && pkg.wheels.is_empty()
}

fn backend_str(b: AllowlistedBackend) -> &'static str {
    match b {
        AllowlistedBackend::FlitCore => "flit-core",
        AllowlistedBackend::Hatchling => "hatchling",
        AllowlistedBackend::Setuptools => "setuptools",
        AllowlistedBackend::PoetryCore => "poetry-core",
        AllowlistedBackend::PdmBackend => "pdm-backend",
    }
}

fn render_native_reason(reason: &NativeReason) -> String {
    match reason {
        NativeReason::UnknownBackend { build_backend } => {
            format!("UnknownBackend:{}", build_backend)
        }
        NativeReason::MissingPyprojectToml => "MissingPyprojectToml".to_string(),
        NativeReason::SetuptoolsWithExtModules => "SetuptoolsWithExtModules".to_string(),
        NativeReason::AdjacentNativeSource { hit } => {
            let (tag, p) = match hit {
                NativeSourceHit::CargoToml(p) => ("CargoToml", p),
                NativeSourceHit::MesonBuild(p) => ("MesonBuild", p),
                NativeSourceHit::CMakeLists(p) => ("CMakeLists", p),
                NativeSourceHit::CExt(p) => ("CExt", p),
                NativeSourceHit::CppExt(p) => ("CppExt", p),
                NativeSourceHit::PyxExt(p) => ("PyxExt", p),
            };
            format!("AdjacentNativeSource:{}@{}", tag, p.display())
        }
    }
}

fn write_gitignore(prebake_dir: &Path) -> Result<()> {
    let gi = prebake_dir.join(".gitignore");
    if !gi.exists() {
        std::fs::write(&gi, "*\n").with_context(|| format!("writing {}", gi.display()))?;
    }
    Ok(())
}

fn download(url: &Url, dest: &Path, package: &str, version: &str) -> Result<()> {
    use crate::sdist::SdistError;
    let response = reqwest::blocking::get(url.clone()).map_err(|e| SdistError::Download {
        package: package.into(),
        version: version.into(),
        url: url.to_string(),
        source: e,
    })?;
    let bytes = response.bytes().map_err(|e| SdistError::Download {
        package: package.into(),
        version: version.into(),
        url: url.to_string(),
        source: e,
    })?;
    std::fs::write(dest, &bytes).with_context(|| format!("writing {}", dest.display()))?;
    Ok(())
}

fn verify_sha256(path: &Path, expected: &str, package: &str, version: &str) -> Result<()> {
    use crate::sdist::SdistError;
    let mut f = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = f
            .read(&mut buf)
            .with_context(|| format!("reading {}", path.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let actual = hex::encode(hasher.finalize());
    if actual != expected {
        return Err(SdistError::HashMismatch {
            package: package.into(),
            version: version.into(),
            expected: expected.into(),
            actual,
        }
        .into());
    }
    Ok(())
}

fn extract_tarball(tarball: &Path, dest: &Path, package: &str, version: &str) -> Result<()> {
    use crate::sdist::SdistError;
    use flate2::read::GzDecoder;
    use tar::Archive;

    let f =
        std::fs::File::open(tarball).with_context(|| format!("opening {}", tarball.display()))?;
    let gz = GzDecoder::new(f);
    let mut archive = Archive::new(gz);
    archive.set_preserve_permissions(false);

    for entry in archive.entries().map_err(|e| SdistError::Extract {
        package: package.into(),
        version: version.into(),
        source: e,
    })? {
        let mut entry = entry.map_err(|e| SdistError::Extract {
            package: package.into(),
            version: version.into(),
            source: e,
        })?;
        let path = entry
            .path()
            .map_err(|e| SdistError::Extract {
                package: package.into(),
                version: version.into(),
                source: e,
            })?
            .into_owned();
        // Path traversal hardening.
        for comp in path.components() {
            if matches!(
                comp,
                std::path::Component::ParentDir | std::path::Component::RootDir
            ) {
                return Err(SdistError::PathTraversal {
                    package: package.into(),
                    version: version.into(),
                    member: path.display().to_string(),
                }
                .into());
            }
        }
        let out = dest.join(&path);
        // Some sdists (e.g. tomli 2.0.1) ship file entries without preceding
        // directory entries. `Entry::unpack` does not create parent dirs, so
        // ensure they exist before unpacking.
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| SdistError::Extract {
                package: package.into(),
                version: version.into(),
                source: e,
            })?;
        }
        // tar-rs only honours the ustar header `mtime` field; many sdists
        // (built with GNU tar or setuptools) record the real mtime in a
        // PAX extension with a `0` ustar mtime, which silently becomes
        // 1970 on disk. Build backends like flit-core then reject the
        // resulting <1980 ZIP timestamp. Read PAX `mtime` ahead of unpack
        // so we can restore it explicitly below.
        let pax_mtime: Option<i64> = match entry.pax_extensions() {
            Ok(Some(exts)) => {
                let mut found = None;
                for ext in exts {
                    if let Ok(ext) = ext
                        && let Ok(key) = ext.key()
                        && key == "mtime"
                        && let Ok(val) = ext.value()
                    {
                        // Value is decimal seconds, optionally fractional.
                        let secs = val.split('.').next().unwrap_or(val);
                        if let Ok(s) = secs.parse::<i64>() {
                            found = Some(s);
                        }
                    }
                }
                found
            }
            _ => None,
        };
        entry.unpack(&out).map_err(|e| SdistError::Extract {
            package: package.into(),
            version: version.into(),
            source: e,
        })?;
        if let Some(secs) = pax_mtime {
            let ft = filetime::FileTime::from_unix_time(secs, 0);
            // Best-effort: failures here are not fatal — they only affect
            // build determinism, not correctness of file content.
            let _ = filetime::set_file_mtime(&out, ft);
        }
    }
    Ok(())
}

fn find_sdist_root(extract_dir: &Path) -> Result<std::path::PathBuf> {
    // Most sdists extract to a single top-level directory `<name>-<version>/`.
    let mut entries: Vec<_> = std::fs::read_dir(extract_dir)
        .with_context(|| format!("reading {}", extract_dir.display()))?
        .filter_map(|r| r.ok())
        .filter(|e| e.path().is_dir())
        .collect();
    if entries.len() == 1 {
        Ok(entries.pop().unwrap().path())
    } else {
        // Fall back to the extract dir itself (some sdists don't nest).
        Ok(extract_dir.to_path_buf())
    }
}
