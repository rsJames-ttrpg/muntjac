mod common;

use assert_cmd::prelude::*;
use common::muntjac;
use predicates::str::contains;
use std::fs;
use tempfile::tempdir;

const TWO_TREE_TOML: &str = r#"
[platforms]
macos-arm64 = { target = "aarch64-apple-darwin", macos_min = "11.0" }
[tree.modern]
manifest_path = "modern/pyproject.toml"
third_party_dir = "tp/modern"
python_versions = ["3.12"]
[tree.legacy]
manifest_path = "legacy/pyproject.toml"
third_party_dir = "tp/legacy"
python_versions = ["3.12"]
"#;

#[test]
fn buckify_unknown_tree_errors_with_available_names() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("muntjac.toml"), TWO_TREE_TOML).unwrap();
    muntjac()
        .arg("-C")
        .arg(dir.path())
        .arg("--tree")
        .arg("ghost")
        .arg("buckify")
        .assert()
        .failure()
        .stderr(contains("tree `ghost` not found"))
        .stderr(contains("modern"))
        .stderr(contains("legacy"));
}

#[test]
fn fixups_show_multi_tree_prints_per_tree_blocks() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("muntjac.toml"), TWO_TREE_TOML).unwrap();
    // fixups show reads each tree's <third_party_dir>/fixups/ — create them so
    // EffectiveFixups::load finds an (empty) local dir rather than a bare path.
    fs::create_dir_all(dir.path().join("tp/modern/fixups")).unwrap();
    fs::create_dir_all(dir.path().join("tp/legacy/fixups")).unwrap();

    // `somepkg` has no fixup in either tree. With >1 tree, each tree gets a
    // header + a per-tree "no fixup" note and the command does NOT abort.
    muntjac()
        .arg("-C")
        .arg(dir.path())
        .arg("fixups")
        .arg("show")
        .arg("somepkg")
        .assert()
        .success()
        .stdout(contains("# ===== tree: modern ====="))
        .stdout(contains("# ===== tree: legacy ====="))
        .stdout(contains("# (no fixup for 'somepkg' in tree 'modern')"))
        .stdout(contains("# (no fixup for 'somepkg' in tree 'legacy')"));
}
