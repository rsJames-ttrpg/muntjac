mod common;

use assert_cmd::prelude::*;
use common::muntjac;
use predicates::str::contains;
use std::fs;
use tempfile::tempdir;

#[test]
fn init_creates_starter_in_empty_dir() {
    let dir = tempdir().unwrap();
    muntjac()
        .arg("-C")
        .arg(dir.path())
        .arg("init")
        .assert()
        .success();

    let cfg = fs::read_to_string(dir.path().join("muntjac.toml")).unwrap();
    assert!(cfg.contains("manifest_path"));
    assert!(cfg.contains("TODO: muntjac init could not auto-detect"));
    assert!(cfg.contains("[platforms.linux-x86_64-gnu]"));
    assert!(cfg.contains("[fixups]"));
    assert!(cfg.contains("# Uncomment to include PEP 735 dependency groups"));
    assert!(cfg.contains("# [lockfile]"));

    assert!(dir.path().join("third-party/python/BUCK").exists());
    assert!(dir.path().join("third-party/python/.gitignore").exists());
    assert!(
        dir.path()
            .join("third-party/python/fixups/.gitkeep")
            .exists()
    );
}

#[test]
fn init_detects_existing_pyproject() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("pyproject.toml"),
        "[project]\nname = \"app\"\nrequires-python = \">=3.11,<3.13\"\n",
    )
    .unwrap();

    muntjac()
        .arg("-C")
        .arg(dir.path())
        .arg("init")
        .assert()
        .success();

    let cfg = fs::read_to_string(dir.path().join("muntjac.toml")).unwrap();
    assert!(!cfg.contains("TODO: muntjac init could not auto-detect"));
    assert!(cfg.contains("python_versions = [\"3.11\", \"3.12\"]"));
}

#[test]
fn init_refuses_to_overwrite() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("muntjac.toml"), "# existing\n").unwrap();

    muntjac()
        .arg("-C")
        .arg(dir.path())
        .arg("init")
        .assert()
        .failure()
        .stderr(contains("already exists"));
}

#[test]
fn init_with_force_overwrites() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("muntjac.toml"), "# old\n").unwrap();

    muntjac()
        .arg("-C")
        .arg(dir.path())
        .arg("init")
        .arg("--force")
        .assert()
        .success();

    let cfg = fs::read_to_string(dir.path().join("muntjac.toml")).unwrap();
    assert!(!cfg.contains("# old"));
    assert!(cfg.contains("[platforms"));
}
