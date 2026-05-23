//! Smoke test for `muntjac fixups show <pkg>`.

use std::fs;
use tempfile::TempDir;

fn muntjac_bin() -> std::path::PathBuf {
    let exe = env!("CARGO_BIN_EXE_muntjac");
    std::path::PathBuf::from(exe)
}

#[test]
fn fixups_show_prints_round_trippable_toml() {
    let tmp = TempDir::new().unwrap();

    fs::write(
        tmp.path().join("muntjac.toml"),
        r#"manifest_path = "pyproject.toml"
third_party_dir = "third-party/python"
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target = "x86_64-unknown-linux-gnu"
manylinux = "2_17"
"#,
    )
    .unwrap();

    fs::create_dir_all(tmp.path().join("third-party/python/fixups/pillow")).unwrap();
    let fixup_body = r#"extra_deps = ["//third-party/c:libjpeg"]
omit_deps = ["useless"]
"#;
    fs::write(
        tmp.path()
            .join("third-party/python/fixups/pillow/fixups.toml"),
        fixup_body,
    )
    .unwrap();

    let out = std::process::Command::new(muntjac_bin())
        .args([
            "-C",
            tmp.path().to_str().unwrap(),
            "fixups",
            "show",
            "pillow",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(
        stdout.contains("//third-party/c:libjpeg"),
        "got:\n{}",
        stdout
    );
    assert!(stdout.contains("useless"), "got:\n{}", stdout);
}

#[test]
fn fixups_show_missing_package_exits_nonzero() {
    let tmp = TempDir::new().unwrap();
    fs::write(
        tmp.path().join("muntjac.toml"),
        r#"manifest_path = "pyproject.toml"
third_party_dir = "third-party/python"
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target = "x86_64-unknown-linux-gnu"
manylinux = "2_17"
"#,
    )
    .unwrap();
    fs::create_dir_all(tmp.path().join("third-party/python")).unwrap();

    let out = std::process::Command::new(muntjac_bin())
        .args([
            "-C",
            tmp.path().to_str().unwrap(),
            "fixups",
            "show",
            "nonexistent",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("no fixup for package"), "got:\n{}", stderr);
}
