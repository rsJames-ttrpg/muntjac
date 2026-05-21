//! Integration tests for `muntjac debug pick-wheels`.

mod common;

use common::muntjac;
use std::path::Path;

fn run_pick_wheels(fixture_dir: &Path, extra_args: &[&str]) -> std::process::Output {
    muntjac()
        .arg("-C")
        .arg(fixture_dir)
        .args(["debug", "pick-wheels"])
        .args(extra_args)
        .output()
        .expect("run muntjac")
}

#[test]
fn fixture_04_pure_python_picks_any_wheel() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/wheel/04-pure-python");
    let out = run_pick_wheels(&fixture, &[]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&out.stdout)
        .expect("output is valid JSON");
    assert_eq!(json["schema_version"], 1);
    let selections = json["selections"]
        .as_array()
        .expect("selections is array");
    assert!(!selections.is_empty(), "no selections emitted");
    for s in selections {
        assert_eq!(s["outcome"], "picked", "expected picked for {s}");
    }
}

#[test]
fn fixture_01_numpy_matrix_golden() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/wheel/01-numpy-matrix");
    let out = run_pick_wheels(&fixture, &[]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let actual = String::from_utf8(out.stdout).unwrap();
    let expected = std::fs::read_to_string(fixture.join("expected.json")).unwrap();
    assert_eq!(actual.trim(), expected.trim(), "golden mismatch");
}
