//! Discover overlay files: walk `<fixup_dir>/<overlay>/` recursively,
//! return `(path_in_wheel, path_relative_to_third_party_dir)` pairs.
//!
//! Rejects symlinks that escape the fixup directory. Returns
//! `OverlayEmpty` if the overlay directory is missing or empty.

use std::path::Path;

use walkdir::WalkDir;

use crate::fixup::error::FixupError;

/// Walk `<fixup_dir>/<overlay_rel>/` and return `(path_in_wheel,
/// src_path_relative_to_third_party_dir)` pairs.
///
/// `pkg_name`: only used for error messages.
/// `third_party_dir`: the tree's third-party dir (e.g. `third-party/python`).
/// `fixup_dir`: the package's fixup directory (e.g. `third-party/python/fixups/pillow`).
pub fn walk_overlay(
    pkg_name: &str,
    third_party_dir: &Path,
    fixup_dir: &Path,
    overlay_rel: &Path,
) -> Result<Vec<(String, String)>, FixupError> {
    let root = fixup_dir.join(overlay_rel);
    if !root.is_dir() {
        return Err(FixupError::OverlayEmpty {
            pkg: pkg_name.to_string(),
            path: overlay_rel.display().to_string(),
        });
    }

    let canonical_root = std::fs::canonicalize(&root).map_err(|e| FixupError::Io {
        path: root.clone(),
        source: e,
    })?;

    let mut files = Vec::new();
    for entry in WalkDir::new(&root).follow_links(false) {
        let entry = entry.map_err(|e| FixupError::Io {
            path: root.clone(),
            source: std::io::Error::other(e.to_string()),
        })?;
        let ft = entry.file_type();
        if !ft.is_file() && !ft.is_symlink() {
            continue;
        }

        // Symlink escape: canonicalize the file path and confirm it's
        // still under canonical_root.
        let canon = std::fs::canonicalize(entry.path()).map_err(|e| FixupError::Io {
            path: entry.path().to_path_buf(),
            source: e,
        })?;
        if !canon.starts_with(&canonical_root) {
            return Err(FixupError::OverlayPathOutsideTree {
                file: entry.path().to_path_buf(),
            });
        }

        // Path inside the wheel: relative path from overlay root.
        let in_wheel = entry
            .path()
            .strip_prefix(&root)
            .map_err(|e| FixupError::Io {
                path: entry.path().to_path_buf(),
                source: std::io::Error::other(e.to_string()),
            })?
            .to_string_lossy()
            .replace('\\', "/")
            .to_string();

        // Path used as a Buck `srcs` label — relative to third_party_dir
        // (Buck file paths are relative to the BUCK file, which lives in
        // third_party_dir).
        let src_rel = entry
            .path()
            .strip_prefix(third_party_dir)
            .map_err(|e| FixupError::Io {
                path: entry.path().to_path_buf(),
                source: std::io::Error::other(e.to_string()),
            })?
            .to_string_lossy()
            .replace('\\', "/")
            .to_string();

        files.push((in_wheel, src_rel));
    }

    if files.is_empty() {
        return Err(FixupError::OverlayEmpty {
            pkg: pkg_name.to_string(),
            path: overlay_rel.display().to_string(),
        });
    }

    // Sort for determinism.
    files.sort();
    Ok(files)
}

/// Wrapper that returns the discovered files plus the path-in-wheel set
/// as a single Vec<(String, String)>. Provided as the public API used
/// by `build_emit_input`.
pub fn discover_overlay_files(
    pkg_name: &str,
    third_party_dir: &Path,
    fixup_dir: &Path,
    overlay_rel: &Path,
) -> Result<Vec<(String, String)>, FixupError> {
    walk_overlay(pkg_name, third_party_dir, fixup_dir, overlay_rel)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_file(p: &Path, contents: &str) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, contents).unwrap();
    }

    #[test]
    fn discovers_overlay_files() {
        let tmp = TempDir::new().unwrap();
        let tpd = tmp.path();
        let fixup_dir = tpd.join("fixups/pillow");
        let overlay = fixup_dir.join("overlay");
        write_file(&overlay.join("PIL/_imaging.py"), "print('hi')");
        write_file(&overlay.join("PIL/extra.py"), "x = 1");

        let files = walk_overlay("pillow", tpd, &fixup_dir, Path::new("overlay")).unwrap();
        let in_wheel: Vec<&str> = files.iter().map(|(w, _)| w.as_str()).collect();
        assert_eq!(in_wheel, vec!["PIL/_imaging.py", "PIL/extra.py"]);
        let src_paths: Vec<&str> = files.iter().map(|(_, s)| s.as_str()).collect();
        assert_eq!(
            src_paths,
            vec![
                "fixups/pillow/overlay/PIL/_imaging.py",
                "fixups/pillow/overlay/PIL/extra.py",
            ]
        );
    }

    #[test]
    fn empty_overlay_errors() {
        let tmp = TempDir::new().unwrap();
        let tpd = tmp.path();
        let fixup_dir = tpd.join("fixups/pillow");
        fs::create_dir_all(fixup_dir.join("overlay")).unwrap();

        let err = walk_overlay("pillow", tpd, &fixup_dir, Path::new("overlay")).unwrap_err();
        match err {
            FixupError::OverlayEmpty { pkg, .. } => assert_eq!(pkg, "pillow"),
            other => panic!("expected OverlayEmpty, got {:?}", other),
        }
    }

    #[test]
    fn missing_overlay_errors() {
        let tmp = TempDir::new().unwrap();
        let tpd = tmp.path();
        let fixup_dir = tpd.join("fixups/pillow");
        fs::create_dir_all(&fixup_dir).unwrap();

        let err = walk_overlay("pillow", tpd, &fixup_dir, Path::new("overlay")).unwrap_err();
        match err {
            FixupError::OverlayEmpty { pkg, .. } => assert_eq!(pkg, "pillow"),
            other => panic!("expected OverlayEmpty, got {:?}", other),
        }
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_escape() {
        let tmp = TempDir::new().unwrap();
        let tpd = tmp.path();
        let fixup_dir = tpd.join("fixups/pillow");
        let overlay = fixup_dir.join("overlay");
        fs::create_dir_all(&overlay).unwrap();

        // Create a file outside the fixup dir and symlink to it from overlay.
        let outside = tpd.join("outside_target.py");
        fs::write(&outside, "import os").unwrap();
        std::os::unix::fs::symlink(&outside, overlay.join("escape.py")).unwrap();

        let err = walk_overlay("pillow", tpd, &fixup_dir, Path::new("overlay")).unwrap_err();
        match err {
            FixupError::OverlayPathOutsideTree { .. } => {}
            other => panic!("expected OverlayPathOutsideTree, got {:?}", other),
        }
    }
}
