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
#[ignore = "enabled in S11 Task 5 once resolve_trees is wired into buckify"]
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
