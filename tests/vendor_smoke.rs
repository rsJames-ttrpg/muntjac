//! End-to-end smoke for `muntjac vendor`.
//!
//! Spawns a local httpmock, points the lockfile's sdist URL at it, runs
//! `muntjac vendor`, and verifies the manifest entry + wheel match the
//! committed `04-pure-python-sdist/` form.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/buck")
        .join(name)
}

fn uv_on_path() -> bool {
    std::process::Command::new("uv")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn vendor_prebakes_tomli_into_matching_manifest_and_wheel() {
    if !uv_on_path() {
        eprintln!("skipping: uv not on PATH");
        return;
    }

    let fix = fixture("04-pure-python-sdist-vendor-input");
    let golden = fixture("04-pure-python-sdist");
    let tmp = tempfile::tempdir().unwrap();

    // Copy fixture content (excluding seed/).
    for entry in std::fs::read_dir(&fix).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name() == "seed" {
            continue;
        }
        let dst = tmp.path().join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &dst);
        } else {
            std::fs::copy(entry.path(), &dst).unwrap();
        }
    }

    // Start httpmock serving the seed tarball.
    let server = httpmock::MockServer::start();
    let tarball = std::fs::read(fix.join("seed/tomli-2.0.1.tar.gz")).unwrap();
    server.mock(|when, then| {
        when.method(httpmock::Method::GET).path("/tomli-2.0.1.tar.gz");
        then.status(200).body(tarball.clone());
    });

    // Rewrite uv.lock to point at the mock.
    let lock_path = tmp.path().join("uv.lock");
    let lock_text = std::fs::read_to_string(&lock_path).unwrap();
    let lock_text = lock_text
        .replace("__MOCK_HOST__", "127.0.0.1")
        .replace("__MOCK_PORT__", &server.port().to_string());
    std::fs::write(&lock_path, lock_text).unwrap();

    // Run muntjac vendor against the workdir with --frozen so it doesn't
    // try to re-lock against the network.
    let out = Command::new(env!("CARGO_BIN_EXE_muntjac"))
        .arg("-C")
        .arg(tmp.path())
        .arg("--frozen")
        .arg("vendor")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "muntjac vendor failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    // Assertions.
    let manifest_path = tmp.path().join("third-party/python/prebake/.manifest.toml");
    assert!(manifest_path.is_file(), "manifest not written");

    let wheel_path = tmp
        .path()
        .join("third-party/python/prebake/tomli-2.0.1-py3-none-any.whl");
    assert!(wheel_path.is_file(), "wheel not written");

    // Compare wheel sha to the committed golden wheel.
    let actual_wheel = std::fs::read(&wheel_path).unwrap();
    let golden_wheel =
        std::fs::read(golden.join("third-party/python/prebake/tomli-2.0.1-py3-none-any.whl"))
            .unwrap();
    use sha2::{Digest, Sha256};
    let actual_sha = hex::encode(Sha256::digest(&actual_wheel));
    let golden_sha = hex::encode(Sha256::digest(&golden_wheel));
    assert_eq!(
        actual_sha, golden_sha,
        "freshly built wheel sha differs from committed golden; \
         either uv build is non-deterministic in this env, or the \
         committed wheel needs regenerating."
    );

    // Compare manifest (modulo formatting whitespace).
    let manifest = crate::common::read_manifest(&manifest_path);
    let golden_manifest =
        crate::common::read_manifest(&golden.join("third-party/python/prebake/.manifest.toml"));
    assert_eq!(manifest, golden_manifest);
}

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let p = entry.path();
        let d = dst.join(entry.file_name());
        if p.is_dir() {
            copy_dir(&p, &d);
        } else {
            std::fs::copy(&p, &d).unwrap();
        }
    }
}

mod common {
    use std::path::Path;
    pub fn read_manifest(p: &Path) -> String {
        let text = std::fs::read_to_string(p).unwrap();
        // Strip the `# @generated` header for comparison robustness, and
        // trim trailing blank lines (the writer emits a blank between
        // entries which appears as a trailing blank after the last one;
        // hand-edited goldens may not).
        let body = text
            .lines()
            .filter(|l| !l.starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n");
        body.trim_end().to_string()
    }
}
