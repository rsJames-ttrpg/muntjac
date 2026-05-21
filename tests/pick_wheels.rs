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
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wheel/04-pure-python");
    let out = run_pick_wheels(&fixture, &[]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("output is valid JSON");
    assert_eq!(json["schema_version"], 1);
    let selections = json["selections"].as_array().expect("selections is array");
    assert!(!selections.is_empty(), "no selections emitted");
    for s in selections {
        assert_eq!(s["outcome"], "picked", "expected picked for {s}");
    }
}

#[test]
fn fixture_01_numpy_matrix_golden() {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wheel/01-numpy-matrix");
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

#[test]
fn fixture_02_musllinux_only() {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wheel/02-musllinux-only");
    let out = run_pick_wheels(&fixture, &[]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let actual = String::from_utf8(out.stdout).unwrap();
    let expected = std::fs::read_to_string(fixture.join("expected.json")).unwrap();
    assert_eq!(actual.trim(), expected.trim(), "golden mismatch");

    // Sanity: musl picks, gnu does not — confirms the asymmetric outcome.
    let json: serde_json::Value = serde_json::from_str(&actual).unwrap();
    let by_platform: std::collections::BTreeMap<String, String> = json["selections"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            (
                s["platform"].as_str().unwrap().to_string(),
                s["outcome"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert_eq!(
        by_platform.get("linux-x86_64-musl").map(|s| s.as_str()),
        Some("picked")
    );
    assert_eq!(
        by_platform.get("linux-x86_64-gnu").map(|s| s.as_str()),
        Some("no_wheel")
    );
}

#[test]
fn fixture_03_no_wheel() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wheel/03-no-wheel");
    let out = run_pick_wheels(&fixture, &[]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let actual = String::from_utf8(out.stdout).unwrap();
    let expected = std::fs::read_to_string(fixture.join("expected.json")).unwrap();
    assert_eq!(actual.trim(), expected.trim(), "golden mismatch");

    // Sanity: every selection should be no_wheel with wheels_considered > 0
    // (distinguishes "wheels exist but none compatible" from "nothing to consider").
    let json: serde_json::Value = serde_json::from_str(&actual).unwrap();
    let selections = json["selections"].as_array().unwrap();
    assert!(!selections.is_empty(), "no selections emitted");
    for sel in selections {
        assert_eq!(sel["outcome"], "no_wheel", "expected no_wheel for {sel}");
        assert!(
            sel["wheels_considered"].as_u64().unwrap() > 0,
            "wheels_considered should be >0 to distinguish from parse-fail"
        );
    }
}

#[test]
fn fixture_05_determinism_two_runs_byte_identical() {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wheel/01-numpy-matrix");
    let out1 = run_pick_wheels(&fixture, &[]);
    let out2 = run_pick_wheels(&fixture, &[]);
    assert!(
        out1.status.success() && out2.status.success(),
        "stderr1: {}\nstderr2: {}",
        String::from_utf8_lossy(&out1.stderr),
        String::from_utf8_lossy(&out2.stderr)
    );
    assert_eq!(
        out1.stdout, out2.stdout,
        "two runs produced different output (non-deterministic)"
    );
}
