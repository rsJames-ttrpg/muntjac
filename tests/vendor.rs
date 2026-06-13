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

#[test]
fn buckify_committed_aborts_on_missing_wheels() {
    let tmp = tempfile::tempdir().unwrap();

    std::fs::write(
        tmp.path().join("muntjac.toml"),
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

    std::fs::write(
        tmp.path().join("pyproject.toml"),
        r#"[project]
name = "stub"
version = "0.0.0"
requires-python = ">=3.12"
dependencies = ["idna==3.10"]
"#,
    )
    .unwrap();

    std::fs::write(
        tmp.path().join("uv.lock"),
        r#"version = 1
revision = 1
requires-python = ">=3.12"

[[package]]
name = "stub"
version = "0.0.0"
source = { virtual = "." }
dependencies = [{ name = "idna" }]

[[package]]
name = "idna"
version = "3.10"
source = { registry = "https://pypi.org/simple" }
wheels = [{ url = "https://files.pythonhosted.org/idna-3.10-py3-none-any.whl", hash = "sha256:cafef00dcafef00dcafef00dcafef00dcafef00dcafef00dcafef00dcafef00d", size = 1, filename = "idna-3.10-py3-none-any.whl" }]
"#,
    )
    .unwrap();

    let exe = env!("CARGO_BIN_EXE_muntjac");
    let output = std::process::Command::new(exe)
        .args(["buckify"])
        .current_dir(tmp.path())
        .output()
        .expect("running muntjac buckify");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("missing") && stderr.contains("idna-3.10-py3-none-any.whl"),
        "expected missing-wheel error mentioning idna, got: {stderr}"
    );
}
