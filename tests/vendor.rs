//! S9 integration tests: vendor command in committed mode.

use std::process::Command;

fn target_exe() -> std::path::PathBuf {
    // Cargo sets CARGO_BIN_EXE_<name> to an absolute path to the test binary
    // under test. Using it avoids cwd-relative-path issues when the test
    // changes the working directory.
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_muntjac"))
}

#[test]
fn vendor_committed_with_no_network_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let toml_path = tmp.path().join("muntjac.toml");
    std::fs::write(
        &toml_path,
        r#"
manifest_path = "pyproject.toml"
third_party_dir = "third-party/python"
python_versions = ["3.12"]

[platforms]
linux-x86_64-gnu = { target = "x86_64-unknown-linux-gnu", manylinux = "2_17" }

[buck]
vendor = true
"#,
    )
    .unwrap();

    let output = Command::new(target_exe())
        .args(["vendor", "--mode", "committed", "--no-network"])
        .current_dir(tmp.path())
        .output()
        .expect("running muntjac vendor");
    assert!(!output.status.success(), "expected failure");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--mode=committed requires network access"),
        "stderr: {stderr}"
    );
}
