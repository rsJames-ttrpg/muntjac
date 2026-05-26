//! `muntjac vendor` — prebake pure-python sdists into wheels; in committed
//! mode, also download all referenced registry wheels into vendor/.
//!
//! See specs/2026-05-26-muntjac-s9-vendor-mode-design.md.

use std::collections::BTreeSet;
use std::io::Read;
use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use url::Url;

use crate::cli::{Globals, VendorArgs, resolve_vendor_mode};
use crate::config::{Config, Tree};
use crate::error::VendorError;
use crate::lock::types::{Package, Source};
use crate::sdist::{
    AllowlistedBackend, Classification, Manifest, ManifestClassification, ManifestEntry,
    NativeReason, NativeSourceHit, classify,
};

pub fn run(globals: &Globals, args: VendorArgs) -> Result<()> {
    let workdir = globals.workdir().context("resolving working directory")?;
    let config_path = workdir.join("muntjac.toml");
    let config_text = std::fs::read_to_string(&config_path)
        .with_context(|| format!("reading {}", config_path.display()))?;
    let config = Config::from_str(&config_text)
        .with_context(|| format!("parsing {}", config_path.display()))?;

    let vendor_mode = resolve_vendor_mode(&config, args.mode);
    if vendor_mode && globals.no_network {
        return Err(VendorError::ModeNetworkConflict.into());
    }

    for tree in crate::cli::resolve_trees(&config, globals.tree.as_deref())? {
        vendor_tree(
            globals,
            &workdir,
            &config_path,
            tree,
            vendor_mode,
            args.no_prune,
        )
        .with_context(|| format!("vendoring tree '{}'", tree.name))?;
    }
    Ok(())
}

fn vendor_tree(
    globals: &Globals,
    workdir: &Path,
    config_path: &Path,
    tree: &Tree,
    vendor_mode: bool,
    no_prune: bool,
) -> Result<()> {
    let third_party_dir = workdir.join(&tree.third_party_dir);

    // Lock freshness. Resolve uv.lock relative to the tree's manifest dir.
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

    let lockfile_text = std::fs::read_to_string(&lockfile_path)
        .with_context(|| format!("reading {}", lockfile_path.display()))?;
    let lockfile = crate::lock::parser::parse(&lockfile_text)
        .with_context(|| format!("parsing {}", lockfile_path.display()))?;

    if vendor_mode {
        vendor_tree_committed(workdir, &third_party_dir, tree, &lockfile, no_prune)
    } else {
        vendor_tree_prebake_only(workdir, &third_party_dir, &lockfile)
    }
}

/// Committed mode: download all registry wheels + prebake pure-python sdists,
/// all into `<third_party_dir>/vendor/`. Sync-prune stale wheels unless `no_prune`.
/// No manifest is written.
fn vendor_tree_committed(
    workdir: &Path,
    third_party_dir: &Path,
    tree: &Tree,
    lockfile: &crate::lock::types::Lockfile,
    no_prune: bool,
) -> Result<()> {
    let vendor_dir = third_party_dir.join("vendor");
    std::fs::create_dir_all(&vendor_dir)
        .with_context(|| format!("creating {}", vendor_dir.display()))?;

    let mut expected: BTreeSet<String> = BTreeSet::new();

    // 1. Download registry wheels needed by configured (python × platform) cells.
    for pkg in &lockfile.packages {
        if !matches!(pkg.source, Source::Registry { .. }) {
            continue;
        }
        for wheel in &pkg.wheels {
            // Determine whether this wheel is needed by ANY (py, platform) of this tree.
            // Cheap pre-filter: keep all wheels for now; the picker will narrow at emit time.
            // We must vendor the full set the emitter could pick, so include all.
            let expected_sha = wheel.hash.trim_start_matches("sha256:").to_string();
            let dest = vendor_dir.join(&wheel.filename);
            if !skip_existing(&dest, &expected_sha)? {
                eprintln!(
                    "muntjac vendor: downloading {} {} → vendor/{}",
                    pkg.name.as_ref(),
                    pkg.version,
                    wheel.filename
                );
                crate::vendor::download_wheel(
                    &wheel.url,
                    &dest,
                    pkg.name.as_ref(),
                    &pkg.version.to_string(),
                    &expected_sha,
                )?;
            }
            expected.insert(wheel.filename.clone());
        }
    }

    // 2. Prebake pure-python sdists directly into vendor/.
    for pkg in &lockfile.packages {
        if !is_sdist_only(pkg) {
            continue;
        }
        let sdist = pkg.sdist.as_ref().unwrap();
        let pkg_name = pkg.name.as_ref().to_string();
        let pkg_version = pkg.version.to_string();
        let expected_sha = sdist.hash.trim_start_matches("sha256:").to_string();
        let computed_filename = crate::pep427::pure_python_filename(&pkg_name, &pkg_version);

        // Idempotence: if the target file already exists, assume good.
        if vendor_dir.join(&computed_filename).is_file() {
            expected.insert(computed_filename);
            continue;
        }

        let tmp = tempfile::tempdir().context("creating tempdir for tarball")?;
        let tarball_path = tmp.path().join("sdist.tar.gz");
        download_sdist(&sdist.url, &tarball_path, &pkg_name, &pkg_version)?;
        verify_sha256(&tarball_path, &expected_sha, &pkg_name, &pkg_version)?;

        let extract_dir = tmp.path().join("extracted");
        std::fs::create_dir_all(&extract_dir)
            .with_context(|| format!("creating {}", extract_dir.display()))?;
        extract_tarball(&tarball_path, &extract_dir, &pkg_name, &pkg_version)?;
        let sdist_root = find_sdist_root(&extract_dir)?;

        match classify(&sdist_root)? {
            Classification::PurePython { backend } => {
                eprintln!(
                    "muntjac vendor: prebaking {} {} → vendor/{} ({})",
                    pkg_name,
                    pkg_version,
                    computed_filename,
                    backend_str(backend)
                );
                let staging = tmp.path().join("staging");
                std::fs::create_dir_all(&staging)
                    .with_context(|| format!("creating {}", staging.display()))?;
                let result =
                    crate::sdist::build_wheel(&sdist_root, &staging, &pkg_name, &pkg_version)?;
                if result.wheel_filename != computed_filename {
                    eprintln!(
                        "muntjac vendor: warning: uv built `{}` but expected `{}` (PEP 427); using uv's name",
                        result.wheel_filename, computed_filename
                    );
                }
                let final_path = vendor_dir.join(&result.wheel_filename);
                if final_path.is_file() {
                    std::fs::remove_file(&final_path)
                        .with_context(|| format!("removing {}", final_path.display()))?;
                }
                std::fs::rename(&result.wheel_path, &final_path)
                    .or_else(|_| {
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
                expected.insert(result.wheel_filename);
            }
            Classification::Native { reason } => {
                eprintln!(
                    "muntjac vendor: skipping native sdist {} {} ({})",
                    pkg_name,
                    pkg_version,
                    render_native_reason(&reason)
                );
                // Native skipped: nothing in vendor/; downstream emit will error
                // if any cfg references it.
            }
        }
    }

    // 3. Sync-prune.
    let removed = crate::vendor::prune_stale(&vendor_dir, &expected, no_prune)
        .with_context(|| format!("pruning {}", vendor_dir.display()))?;
    for f in &removed {
        eprintln!("muntjac vendor: pruned {}", f);
    }

    // Note: no manifest is written in committed mode (see spec §2.3).
    // Note: prebake/ is left untouched in committed mode.

    let _ = (workdir, tree); // silence unused-variable until referenced elsewhere
    Ok(())
}

/// Prebake-only mode: today's behavior. Wheels build to <third_party_dir>/prebake/,
/// manifest written, .gitignore for prebake/. No registry-wheel downloads.
fn vendor_tree_prebake_only(
    workdir: &Path,
    third_party_dir: &Path,
    lockfile: &crate::lock::types::Lockfile,
) -> Result<()> {
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

        let tmp = tempfile::tempdir().context("creating tempdir for tarball")?;
        let tarball_path = tmp.path().join("sdist.tar.gz");
        download_sdist(&sdist.url, &tarball_path, &pkg_name, &pkg_version)?;
        verify_sha256(&tarball_path, &expected_sha, &pkg_name, &pkg_version)?;

        let extract_dir = tmp.path().join("extracted");
        std::fs::create_dir_all(&extract_dir)
            .with_context(|| format!("creating {}", extract_dir.display()))?;
        extract_tarball(&tarball_path, &extract_dir, &pkg_name, &pkg_version)?;
        let sdist_root = find_sdist_root(&extract_dir)?;

        match classify(&sdist_root)? {
            Classification::PurePython { backend } => {
                eprintln!(
                    "muntjac vendor: prebaking {} {} ({})",
                    pkg_name,
                    pkg_version,
                    backend_str(backend)
                );
                let staging = tmp.path().join("staging");
                std::fs::create_dir_all(&staging)
                    .with_context(|| format!("creating {}", staging.display()))?;
                let result =
                    crate::sdist::build_wheel(&sdist_root, &staging, &pkg_name, &pkg_version)?;
                let final_path = prebake_dir.join(&result.wheel_filename);
                if final_path.is_file() {
                    std::fs::remove_file(&final_path)
                        .with_context(|| format!("removing {}", final_path.display()))?;
                }
                std::fs::rename(&result.wheel_path, &final_path)
                    .or_else(|_| {
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

    let manifest = Manifest {
        version: 1,
        entries,
    };
    let manifest_path = prebake_dir.join(".manifest.toml");
    manifest.save(&manifest_path)?;
    Ok(())
}

fn is_sdist_only(pkg: &Package) -> bool {
    matches!(pkg.source, Source::Registry { .. }) && pkg.sdist.is_some() && pkg.wheels.is_empty()
}

fn skip_existing(dest: &Path, expected_sha: &str) -> Result<bool> {
    if !dest.is_file() {
        return Ok(false);
    }
    // Hash existing file; reuse if matches.
    let mut f = std::fs::File::open(dest)
        .with_context(|| format!("opening existing {}", dest.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f
            .read(&mut buf)
            .with_context(|| format!("reading {}", dest.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let got = hex::encode(hasher.finalize());
    let want = expected_sha.trim_start_matches("sha256:");
    Ok(got == want)
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

fn download_sdist(url: &Url, dest: &Path, package: &str, version: &str) -> Result<()> {
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
