mod common;

use assert_cmd::prelude::*;
use common::muntjac;
use predicates::str::contains;
use std::fs;
use tempfile::tempdir;

const TWO_TREE_TOML: &str = r#"
[platforms]
macos-arm64 = { target = "aarch64-apple-darwin", macos_min = "11.0" }
[tree.modern]
manifest_path = "modern/pyproject.toml"
third_party_dir = "tp/modern"
python_versions = ["3.12"]
[tree.legacy]
manifest_path = "legacy/pyproject.toml"
third_party_dir = "tp/legacy"
python_versions = ["3.12"]
"#;

#[test]
fn buckify_unknown_tree_errors_with_available_names() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("muntjac.toml"), TWO_TREE_TOML).unwrap();
    muntjac()
        .arg("-C")
        .arg(dir.path())
        .arg("--tree")
        .arg("ghost")
        .arg("buckify")
        .assert()
        .failure()
        .stderr(contains("tree `ghost` not found"))
        .stderr(contains("modern"))
        .stderr(contains("legacy"));
}

/// S9: exercises the per-tree `vendor:` emission path at the buckify boundary.
/// Each tree has its own `vendor/` dir pre-populated with a placeholder wheel
/// file so the emit-time existence check passes; the test only asserts that
/// each tree's BUCK contains a `vendor:idna-3.10-py3-none-any.whl` reference.
/// No buck2 build is needed — this is purely an emit-correctness check.
#[test]
fn buckify_committed_multi_tree_emits_per_tree_vendor_refs() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join("muntjac.toml"),
        r#"
[platforms]
linux-x86_64-gnu = { target = "x86_64-unknown-linux-gnu", manylinux = "2_17" }

[buck]
vendor = true

[tree.modern]
manifest_path = "modern/pyproject.toml"
third_party_dir = "tp/modern"
python_versions = ["3.12"]

[tree.legacy]
manifest_path = "legacy/pyproject.toml"
third_party_dir = "tp/legacy"
python_versions = ["3.12"]
"#,
    )
    .unwrap();

    for tree in &["modern", "legacy"] {
        let tree_dir = dir.path().join(tree);
        fs::create_dir_all(&tree_dir).unwrap();
        fs::write(
            tree_dir.join("pyproject.toml"),
            format!(
                r#"[project]
name = "{tree}-app"
version = "0.0.0"
requires-python = ">=3.12"
dependencies = ["idna==3.10"]
"#
            ),
        )
        .unwrap();
        fs::write(
            tree_dir.join("uv.lock"),
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
        // Pre-populate vendor/ with an empty placeholder so the emit-time
        // existence check passes. The test asserts on emit output only.
        let vendor = dir.path().join("tp").join(tree).join("vendor");
        fs::create_dir_all(&vendor).unwrap();
        fs::write(vendor.join("idna-3.10-py3-none-any.whl"), b"").unwrap();
    }

    muntjac()
        .arg("-C")
        .arg(dir.path())
        .arg("buckify")
        .assert()
        .success();

    for tree in &["modern", "legacy"] {
        let buck = fs::read_to_string(dir.path().join("tp").join(tree).join("BUCK")).unwrap();
        assert!(
            buck.contains("vendor:idna-3.10-py3-none-any.whl"),
            "tree `{tree}` BUCK missing vendor: reference. content:\n{buck}"
        );
    }
}

#[test]
fn fixups_show_multi_tree_prints_per_tree_blocks() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("muntjac.toml"), TWO_TREE_TOML).unwrap();
    // fixups show reads each tree's <third_party_dir>/fixups/ — create them so
    // EffectiveFixups::load finds an (empty) local dir rather than a bare path.
    fs::create_dir_all(dir.path().join("tp/modern/fixups")).unwrap();
    fs::create_dir_all(dir.path().join("tp/legacy/fixups")).unwrap();

    // `somepkg` has no fixup in either tree. With >1 tree, each tree gets a
    // header + a per-tree "no fixup" note and the command does NOT abort.
    muntjac()
        .arg("-C")
        .arg(dir.path())
        .arg("fixups")
        .arg("show")
        .arg("somepkg")
        .assert()
        .success()
        .stdout(contains("# ===== tree: modern ====="))
        .stdout(contains("# ===== tree: legacy ====="))
        .stdout(contains("# (no fixup for 'somepkg' in tree 'modern')"))
        .stdout(contains("# (no fixup for 'somepkg' in tree 'legacy')"));
}
