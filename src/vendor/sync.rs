//! Sync-prune of stale wheels in `<third_party_dir>/vendor/`.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

/// Delete `*.whl` entries in `vendor_dir` not in `expected`. If `no_prune`,
/// returns `Ok(vec![])` without inspecting the directory. Returns the list of
/// removed filenames (sorted). Files without a `.whl` extension are left alone.
pub fn prune_stale(
    vendor_dir: &Path,
    expected: &BTreeSet<String>,
    no_prune: bool,
) -> std::io::Result<Vec<String>> {
    if no_prune {
        return Ok(vec![]);
    }
    if !vendor_dir.is_dir() {
        return Ok(vec![]);
    }
    let mut removed = Vec::new();
    for entry in fs::read_dir(vendor_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("whl") {
            continue;
        }
        let filename = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        if expected.contains(&filename) {
            continue;
        }
        fs::remove_file(&path)?;
        removed.push(filename);
    }
    removed.sort();
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_empty_whl(dir: &Path, name: &str) {
        fs::write(dir.join(name), b"").unwrap();
    }

    #[test]
    fn prune_deletes_stale_wheels() {
        let tmp = tempfile::tempdir().unwrap();
        write_empty_whl(tmp.path(), "keep-1.0-py3-none-any.whl");
        write_empty_whl(tmp.path(), "stale-1.0-py3-none-any.whl");
        let expected: BTreeSet<String> = ["keep-1.0-py3-none-any.whl".to_string()]
            .into_iter()
            .collect();
        let removed = prune_stale(tmp.path(), &expected, false).unwrap();
        assert_eq!(removed, vec!["stale-1.0-py3-none-any.whl"]);
        assert!(tmp.path().join("keep-1.0-py3-none-any.whl").exists());
        assert!(!tmp.path().join("stale-1.0-py3-none-any.whl").exists());
    }

    #[test]
    fn no_prune_skips_deletion() {
        let tmp = tempfile::tempdir().unwrap();
        write_empty_whl(tmp.path(), "stale-1.0-py3-none-any.whl");
        let removed = prune_stale(tmp.path(), &BTreeSet::new(), true).unwrap();
        assert!(removed.is_empty());
        assert!(tmp.path().join("stale-1.0-py3-none-any.whl").exists());
    }

    #[test]
    fn prune_leaves_non_whl_files_alone() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join(".gitignore"), b"*").unwrap();
        fs::write(tmp.path().join("notes.txt"), b"hi").unwrap();
        write_empty_whl(tmp.path(), "stale-1.0-py3-none-any.whl");
        let _ = prune_stale(tmp.path(), &BTreeSet::new(), false).unwrap();
        assert!(tmp.path().join(".gitignore").exists());
        assert!(tmp.path().join("notes.txt").exists());
        assert!(!tmp.path().join("stale-1.0-py3-none-any.whl").exists());
    }

    #[test]
    fn prune_missing_dir_is_ok() {
        let tmp = tempfile::tempdir().unwrap();
        let absent = tmp.path().join("does-not-exist");
        let removed = prune_stale(&absent, &BTreeSet::new(), false).unwrap();
        assert!(removed.is_empty());
    }
}
