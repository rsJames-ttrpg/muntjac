mod common;

use assert_cmd::prelude::*;
use common::muntjac;
use predicates::str::contains;
use std::fs;
use tempfile::tempdir;

const GOOD_CONFIG: &str = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target    = "x86_64-unknown-linux-gnu"
manylinux = "2_17"
"#;

const BAD_TRIPLE: &str = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.bogus]
target = "not-a-triple"
"#;

#[test]
fn config_check_passes_on_good_config() {
    let dir = tempdir().unwrap();
    let cfg = dir.path().join("muntjac.toml");
    let manifest = dir.path().join("pyproject.toml");
    fs::write(&cfg, GOOD_CONFIG.replace("../pyproject.toml", manifest.to_str().unwrap())).unwrap();
    fs::write(&manifest, "[project]\nname = \"x\"\n").unwrap();

    muntjac()
        .arg("-C")
        .arg(dir.path())
        .args(["config", "check"])
        .assert()
        .success()
        .stdout(contains("muntjac.toml ok"));
}

#[test]
fn config_check_rejects_bad_triple() {
    let dir = tempdir().unwrap();
    let cfg = dir.path().join("muntjac.toml");
    let manifest = dir.path().join("pyproject.toml");
    fs::write(&cfg, BAD_TRIPLE.replace("../pyproject.toml", manifest.to_str().unwrap())).unwrap();
    fs::write(&manifest, "[project]\nname = \"x\"\n").unwrap();

    muntjac()
        .arg("-C")
        .arg(dir.path())
        .args(["config", "check"])
        .assert()
        .failure()
        .stderr(contains("unknown target triple"));
}

#[test]
fn config_check_rejects_missing_file() {
    let dir = tempdir().unwrap();
    muntjac()
        .arg("-C")
        .arg(dir.path())
        .args(["config", "check"])
        .assert()
        .failure()
        .stderr(contains("muntjac.toml"));
}
