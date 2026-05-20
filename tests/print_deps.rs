mod common;

use common::muntjac;
use std::fs;
use std::path::Path;

fn run_print_deps(fixture: &str) -> (String, std::process::Output) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/lock")
        .join(fixture);
    let output = muntjac()
        .arg("-C")
        .arg(&dir)
        .args(["debug", "print-deps", "--pretty"])
        .output()
        .expect("run print-deps");
    let stdout = String::from_utf8(output.stdout.clone()).expect("utf-8");
    (stdout, output)
}

fn assert_golden(fixture: &str) {
    let (actual, output) = run_print_deps(fixture);
    assert!(
        output.status.success(),
        "print-deps failed for fixture {fixture}: stderr=\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let golden_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/lock")
        .join(fixture)
        .join("expected-print-deps.json");
    let expected = fs::read_to_string(&golden_path)
        .unwrap_or_else(|_| panic!("read golden {}", golden_path.display()));
    assert_eq!(
        actual.trim_end(),
        expected.trim_end(),
        "mismatch for {fixture}\n--- actual ---\n{actual}\n--- expected ---\n{expected}"
    );
}

#[test]
fn fixture_01_pure_python() {
    assert_golden("01-pure-python");
}

#[test]
fn fixture_02_env_markers() {
    assert_golden("02-env-markers");
}

#[test]
fn fixture_03_workspace() {
    assert_golden("03-workspace");
}

#[test]
fn fixture_04_extras() {
    assert_golden("04-extras");
}

#[test]
fn fixture_05_dev_deps() {
    assert_golden("05-dev-deps");
}

#[test]
fn fixture_08_multi_platform_marker() {
    assert_golden("08-multi-platform-marker");
}
