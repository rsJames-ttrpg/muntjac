//! Integration tests for `muntjac buckify`.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/buck")
        .join(name)
}

fn copy_fixture_to(src: &Path, dst: &Path) {
    fn copy_dir(src: &Path, dst: &Path) {
        std::fs::create_dir_all(dst).unwrap();
        for entry in std::fs::read_dir(src).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let name = entry.file_name();
            if path.is_dir() {
                // Skip `expected/` and `third-party/` — they're test outputs.
                if name == "expected" || name == "third-party" {
                    continue;
                }
                copy_dir(&path, &dst.join(&name));
            } else {
                std::fs::copy(&path, dst.join(&name)).unwrap();
            }
        }
    }
    copy_dir(src, dst);
}

fn run_buckify(workdir: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_muntjac"))
        .arg("-C")
        .arg(workdir)
        .arg("buckify")
        .output()
        .expect("run muntjac buckify")
}

fn assert_files_match(out_dir: &Path, golden_dir: &Path) {
    for entry in walkdir::WalkDir::new(golden_dir) {
        let entry = entry.unwrap();
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry.path().strip_prefix(golden_dir).unwrap();
        let out_path = out_dir.join(rel);
        assert!(
            out_path.exists(),
            "output file missing: {}",
            out_path.display()
        );
        let actual = std::fs::read_to_string(&out_path).unwrap();
        let expected = std::fs::read_to_string(entry.path()).unwrap();
        assert_eq!(actual, expected, "diff at {}", rel.display());
    }
}

#[test]
fn fixture_01_pure_python_golden() {
    let fix = fixture("01-pure-python");
    let tmp = tempfile::tempdir().unwrap();
    copy_fixture_to(&fix, tmp.path());
    let out = run_buckify(tmp.path());
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_files_match(
        &tmp.path().join("third-party/python"),
        &fix.join("expected"),
    );
}

#[test]
fn fixture_02_numpy_pandas_golden() {
    let fix = fixture("02-numpy-pandas");
    let tmp = tempfile::tempdir().unwrap();
    copy_fixture_to(&fix, tmp.path());
    let out = run_buckify(tmp.path());
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_files_match(
        &tmp.path().join("third-party/python"),
        &fix.join("expected"),
    );
    assert!(
        !tmp.path().join("third-party/python/PACKAGE").exists(),
        "third-party/python/PACKAGE should not exist (wiring.bzl replaces it)"
    );
}

#[test]
fn fixture_10_determinism_two_runs_byte_identical() {
    let fix = fixture("01-pure-python");

    let tmp_a = tempfile::tempdir().unwrap();
    let tmp_b = tempfile::tempdir().unwrap();
    copy_fixture_to(&fix, tmp_a.path());
    copy_fixture_to(&fix, tmp_b.path());

    let out_a = run_buckify(tmp_a.path());
    assert!(
        out_a.status.success(),
        "run a failed: {}",
        String::from_utf8_lossy(&out_a.stderr)
    );
    let out_b = run_buckify(tmp_b.path());
    assert!(
        out_b.status.success(),
        "run b failed: {}",
        String::from_utf8_lossy(&out_b.stderr)
    );

    let tpd_a = tmp_a.path().join("third-party/python");
    let tpd_b = tmp_b.path().join("third-party/python");

    for rel in ["BUCK", "muntjac.bzl", "wiring.bzl", "config/BUCK"] {
        let bytes_a = std::fs::read(tpd_a.join(rel)).unwrap();
        let bytes_b = std::fs::read(tpd_b.join(rel)).unwrap();
        assert_eq!(bytes_a, bytes_b, "{} differs across runs", rel);
    }
}
