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
                // Skip `expected/` always. Skip `third-party/` *unless* it
                // contains a `prebake/` subtree (committed for fixtures that
                // start the buckify run with a pre-existing manifest).
                if name == "expected" {
                    continue;
                }
                if name == "third-party" {
                    let prebake_src = path.join("python/prebake");
                    if prebake_src.is_dir() {
                        let prebake_dst = dst.join("third-party/python/prebake");
                        copy_dir(&prebake_src, &prebake_dst);
                    }
                    let fixups_src = path.join("python/fixups");
                    if fixups_src.is_dir() {
                        let fixups_dst = dst.join("third-party/python/fixups");
                        copy_dir(&fixups_src, &fixups_dst);
                    }
                    // S9: in vendor mode, third-party/python/vendor/*.whl is
                    // committed and must be present at buckify time (the
                    // emit-time existence check will otherwise abort).
                    let vendor_src = path.join("python/vendor");
                    if vendor_src.is_dir() {
                        let vendor_dst = dst.join("third-party/python/vendor");
                        copy_dir(&vendor_src, &vendor_dst);
                    }
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
    assert!(
        !tmp.path().join("third-party/python/PACKAGE").exists(),
        "third-party/python/PACKAGE should not exist (wiring.bzl replaces it)"
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
fn fixture_03_musllinux_golden() {
    let fix = fixture("03-musllinux");
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
    // Negative assertion: NO PACKAGE in the third-party dir.
    assert!(
        !tmp.path().join("third-party/python/PACKAGE").exists(),
        "third-party/python/PACKAGE should not exist (wiring.bzl replaces it)"
    );
    // Sanity: at least one URL in BUCK contains a musllinux substring.
    let buck = std::fs::read_to_string(tmp.path().join("third-party/python/BUCK")).unwrap();
    assert!(
        buck.contains("musllinux"),
        "expected at least one musllinux wheel URL in generated BUCK"
    );
}

#[test]
fn fixture_04_pure_python_sdist_golden() {
    let fix = fixture("04-pure-python-sdist");
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
fn fixture_05_local_fixup_golden() {
    let fix = fixture("05-local-fixup");
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
    // Negative assertion: NO PACKAGE in the third-party dir.
    assert!(
        !tmp.path().join("third-party/python/PACKAGE").exists(),
        "third-party/python/PACKAGE should not exist (wiring.bzl replaces it)"
    );
    // Sanity: tomli rendered with the overlay + fixup-derived kwargs.
    let buck = std::fs::read_to_string(tmp.path().join("third-party/python/BUCK")).unwrap();
    assert!(
        buck.contains("overlay_files = ["),
        "expected overlay_files kwarg in BUCK for tomli"
    );
    assert!(
        buck.contains("//third-party/c:libjpeg"),
        "expected extra_deps libjpeg in BUCK"
    );
    assert!(buck.contains("labels = ["), "expected labels kwarg in BUCK");
    assert!(
        buck.contains("security-sensitive"),
        "expected security-sensitive label in BUCK"
    );
}

#[test]
fn fixture_06_community_fixup_golden() {
    let fix = fixture("06-community-fixup");
    let tmp = tempfile::tempdir().unwrap();
    copy_fixture_to(&fix, tmp.path());

    // Rewrite the file:// registry placeholder to the absolute path inside
    // the tempdir copy so buckify can resolve the local registry.
    let abs_registry = tmp.path().join("registry");
    let muntjac_toml_path = tmp.path().join("muntjac.toml");
    let toml_src = std::fs::read_to_string(&muntjac_toml_path).unwrap();
    let toml_src = toml_src.replace(
        "file:///REPLACED_AT_TEST_TIME",
        &format!("file://{}", abs_registry.display()),
    );
    std::fs::write(&muntjac_toml_path, toml_src).unwrap();

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

    // Sanity: layering correctness assertions.
    let buck = std::fs::read_to_string(tmp.path().join("third-party/python/BUCK")).unwrap();
    // pkg-a must have both community and local deps.
    assert!(
        buck.contains("//community:base"),
        "missing community base in BUCK for pkg-a"
    );
    assert!(
        buck.contains("//local:base"),
        "missing local base in BUCK for pkg-a"
    );
    // pkg-a linux-only (community) and x86-only (local) are merged into deps.
    assert!(
        buck.contains("//community:linux-only"),
        "missing community linux-only dep"
    );
    assert!(
        buck.contains("//local:x86-only"),
        "missing local x86-only dep"
    );
    // pkg-b community-only.
    assert!(
        buck.contains("//community:b-base"),
        "missing pkg-b community dep"
    );
    assert!(
        buck.contains("community-tag"),
        "missing community-tag label for pkg-b"
    );
    // pkg-c local-only.
    assert!(buck.contains("//local:c-base"), "missing pkg-c local dep");
}

#[test]
fn fixture_07_allow_local_overrides_false_golden() {
    let fixture_src = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/buck/07-allow-local-overrides-false");
    let tmp = tempfile::TempDir::new().unwrap();
    copy_fixture_to(&fixture_src, tmp.path());

    let abs_registry = tmp.path().join("registry");
    let muntjac_toml_path = tmp.path().join("muntjac.toml");
    let mut toml_bytes = std::fs::read_to_string(&muntjac_toml_path).unwrap();
    toml_bytes = toml_bytes.replace(
        "file:///REPLACED_AT_TEST_TIME",
        &format!("file://{}", abs_registry.display()),
    );
    std::fs::write(&muntjac_toml_path, toml_bytes).unwrap();

    let status = std::process::Command::new(env!("CARGO_BIN_EXE_muntjac"))
        .args(["-C", tmp.path().to_str().unwrap(), "buckify"])
        .status()
        .unwrap();
    assert!(status.success());

    let generated_buck =
        std::fs::read_to_string(tmp.path().join("third-party/python/BUCK")).unwrap();
    let expected_buck = std::fs::read_to_string(fixture_src.join("expected/BUCK")).unwrap();
    assert_eq!(generated_buck, expected_buck);

    // Sanity: local visibility was suppressed.
    assert!(
        !generated_buck.contains("//local:..."),
        "local visibility leaked: {}",
        generated_buck
    );
    assert!(
        generated_buck.contains("//community:..."),
        "community visibility missing"
    );
}

#[test]
fn fixture_08_replace_community_golden() {
    let fixture_src = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/buck/08-replace-community");
    let tmp = tempfile::TempDir::new().unwrap();
    copy_fixture_to(&fixture_src, tmp.path());

    let abs_registry = tmp.path().join("registry");
    let muntjac_toml_path = tmp.path().join("muntjac.toml");
    let mut toml_bytes = std::fs::read_to_string(&muntjac_toml_path).unwrap();
    toml_bytes = toml_bytes.replace(
        "file:///REPLACED_AT_TEST_TIME",
        &format!("file://{}", abs_registry.display()),
    );
    std::fs::write(&muntjac_toml_path, toml_bytes).unwrap();

    let status = std::process::Command::new(env!("CARGO_BIN_EXE_muntjac"))
        .args(["-C", tmp.path().to_str().unwrap(), "buckify"])
        .status()
        .unwrap();
    assert!(status.success());

    let generated_buck =
        std::fs::read_to_string(tmp.path().join("third-party/python/BUCK")).unwrap();
    let expected_buck = std::fs::read_to_string(fixture_src.join("expected/BUCK")).unwrap();
    assert_eq!(generated_buck, expected_buck);

    // Community must be fully suppressed.
    assert!(generated_buck.contains("//local:only"));
    assert!(
        !generated_buck.contains("//community:"),
        "community fields leaked: {}",
        generated_buck
    );
}

#[test]
fn fixture_09_native_sdist_error_message_pins_canonical_text() {
    let fix = fixture("09-native-sdist-error");
    let tmp = tempfile::tempdir().unwrap();
    copy_fixture_to(&fix, tmp.path());
    let out = run_buckify(tmp.path());

    assert!(
        !out.status.success(),
        "expected buckify to fail; got success"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    let expected = std::fs::read_to_string(fix.join("expected-error.txt")).unwrap();
    let expected = expected.trim_end();
    assert!(
        stderr.contains(expected),
        "stderr did not contain the canonical native-sdist error.\n\
         expected:\n{expected}\n\nactual stderr:\n{stderr}"
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

fn build_bare_repo_inline(source_dir: &std::path::Path, bare_dest: &std::path::Path) -> String {
    use std::process::Command;
    let work = tempfile::TempDir::new().unwrap();
    for entry in walkdir::WalkDir::new(source_dir) {
        let entry = entry.unwrap();
        let rel = entry.path().strip_prefix(source_dir).unwrap();
        let dst = work.path().join(rel);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&dst).unwrap();
        } else if entry.file_type().is_file() {
            if let Some(p) = dst.parent() {
                std::fs::create_dir_all(p).unwrap();
            }
            std::fs::copy(entry.path(), &dst).unwrap();
        }
    }
    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .current_dir(work.path())
            .env("GIT_AUTHOR_NAME", "muntjac-test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_AUTHOR_DATE", "1970-01-01T00:00:00Z")
            .env("GIT_COMMITTER_NAME", "muntjac-test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_DATE", "1970-01-01T00:00:00Z")
            .output()
            .unwrap()
    };
    assert!(git(&["init", "-q", "-b", "main"]).status.success());
    assert!(git(&["add", "-A"]).status.success());
    assert!(git(&["commit", "-q", "-m", "initial"]).status.success());
    let sha = String::from_utf8(git(&["rev-parse", "HEAD"]).stdout)
        .unwrap()
        .trim()
        .to_string();
    assert!(
        git(&["clone", "-q", "--bare", ".", bare_dest.to_str().unwrap()])
            .status
            .success()
    );
    sha
}

#[test]
fn fixture_09_git_registry_golden() {
    let fixture_src = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/buck/09-git-registry");
    let tmp = tempfile::TempDir::new().unwrap();
    copy_fixture_to(&fixture_src, tmp.path());

    // Build the bare repo at the placeholder location.
    let bare_path = tmp.path().join("registry.git");
    let sha = build_bare_repo_inline(&tmp.path().join("registry-source"), &bare_path);

    // Substitute placeholders in muntjac.toml.
    let muntjac_toml_path = tmp.path().join("muntjac.toml");
    let mut bytes = std::fs::read_to_string(&muntjac_toml_path).unwrap();
    bytes = bytes.replace(
        "/REPLACED_AT_TEST_TIME/registry.git",
        &bare_path.display().to_string(),
    );
    bytes = bytes.replace(
        "registry_rev = \"REPLACED_AT_TEST_TIME\"",
        &format!("registry_rev = \"{}\"", sha),
    );
    std::fs::write(&muntjac_toml_path, bytes).unwrap();

    // Isolate the cache.
    let cache_home = tmp.path().join("cache");
    std::fs::create_dir_all(&cache_home).unwrap();

    let status = std::process::Command::new(env!("CARGO_BIN_EXE_muntjac"))
        .env("MUNTJAC_CACHE_HOME", &cache_home)
        .args(["-C", tmp.path().to_str().unwrap(), "buckify"])
        .status()
        .unwrap();
    assert!(status.success());

    let generated = std::fs::read_to_string(tmp.path().join("third-party/python/BUCK")).unwrap();
    let expected = std::fs::read_to_string(fixture_src.join("expected/BUCK")).unwrap();
    assert_eq!(generated, expected, "BUCK byte-diff");

    // Sanity: cache was populated at the resolved SHA.
    assert!(
        cache_home
            .join("fixups")
            .join(&sha)
            .join("packages")
            .is_dir()
    );

    // Sanity: BUCK has both community and local extra_deps.
    assert!(generated.contains("//community:base"));
    assert!(generated.contains("//local:base"));
}

#[test]
fn fixture_09_offline_cache_hit() {
    let fixture_src = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/buck/09-git-registry");
    let tmp = tempfile::TempDir::new().unwrap();
    copy_fixture_to(&fixture_src, tmp.path());

    let bare_path = tmp.path().join("registry.git");
    let sha = build_bare_repo_inline(&tmp.path().join("registry-source"), &bare_path);

    let muntjac_toml_path = tmp.path().join("muntjac.toml");
    let mut bytes = std::fs::read_to_string(&muntjac_toml_path).unwrap();
    bytes = bytes.replace(
        "/REPLACED_AT_TEST_TIME/registry.git",
        &bare_path.display().to_string(),
    );
    bytes = bytes.replace(
        "registry_rev = \"REPLACED_AT_TEST_TIME\"",
        &format!("registry_rev = \"{}\"", sha),
    );
    std::fs::write(&muntjac_toml_path, bytes).unwrap();

    let cache_home = tmp.path().join("cache");
    std::fs::create_dir_all(&cache_home).unwrap();

    // First run: populates cache.
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_muntjac"))
        .env("MUNTJAC_CACHE_HOME", &cache_home)
        .args(["-C", tmp.path().to_str().unwrap(), "buckify"])
        .status()
        .unwrap();
    assert!(status.success(), "first buckify run must succeed");

    // Delete the bare repo (proves the second run doesn't need network).
    std::fs::remove_dir_all(&bare_path).unwrap();

    // Second run: --no-network, expect success from cache.
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_muntjac"))
        .env("MUNTJAC_CACHE_HOME", &cache_home)
        .args([
            "-C",
            tmp.path().to_str().unwrap(),
            "--no-network",
            "buckify",
        ])
        .status()
        .unwrap();
    assert!(
        status.success(),
        "--no-network buckify with cached fetch must succeed"
    );
}

#[test]
fn fixture_10_multi_tree_golden() {
    let fix = fixture("10-multi-tree");
    let tmp = tempfile::tempdir().unwrap();
    copy_fixture_to(&fix, tmp.path());
    let out = run_buckify(tmp.path());
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // expected/ mirrors the emitted `third-party/python` subtree: shared cfg
    // (config/BUCK + wiring.bzl) once + per-tree modern/ + legacy/ packages.
    assert_files_match(
        &tmp.path().join("third-party/python"),
        &fix.join("expected"),
    );
}

#[test]
fn fixture_11_vendor_golden() {
    // 11-vendor commits idna (downloaded) + iniconfig (downloaded) wheels
    // under third-party/python/vendor/. buckify emits `vendor:<filename>`
    // URLs in BUCK + the conditional `elif vendor:` arm in muntjac.bzl.
    let fix = fixture("11-vendor");
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
