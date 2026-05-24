mod common;

use common::git_fixture::init_bare_repo_from_source;
use std::process::Command;

/// Serialize tests that mutate MUNTJAC_CACHE_HOME — they share global state
/// via the subprocess env (`Command::env`), but the parent process's env
/// determines what the spawned muntjac binary sees if env() isn't passed.
/// Using a Mutex avoids cross-test contamination.
static ENV_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn setup_fixture(cwd: &std::path::Path, bare_url: &str, initial_rev: Option<&str>) {
    let muntjac_toml = format!(
        r#"
manifest_path   = "pyproject.toml"
third_party_dir = "third-party/python"
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target    = "x86_64-unknown-linux-gnu"
manylinux = "2_17"

[fixups]
registry = "{}"
{}
"#,
        bare_url,
        match initial_rev {
            Some(r) => format!("registry_rev = \"{}\"", r),
            None => String::new(),
        },
    );
    std::fs::write(cwd.join("muntjac.toml"), muntjac_toml).unwrap();
    std::fs::create_dir_all(cwd.join("third-party/python")).unwrap();
}

fn run_muntjac(
    cwd: &std::path::Path,
    cache_home: &std::path::Path,
    args: &[&str],
) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_muntjac"))
        .env("MUNTJAC_CACHE_HOME", cache_home)
        .args(["-C", cwd.to_str().unwrap()])
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn update_pins_initial_sha() {
    let _g = ENV_GUARD.lock().unwrap();
    let cwd_tmp = tempfile::TempDir::new().unwrap();
    let cache_tmp = tempfile::TempDir::new().unwrap();

    // Build a bare repo with one synthetic package.
    let src_tmp = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(src_tmp.path().join("packages/pkg-a")).unwrap();
    std::fs::write(
        src_tmp.path().join("packages/pkg-a/fixups.toml"),
        "extra_deps = [\"//x:y\"]\n",
    )
    .unwrap();

    let bare_tmp = tempfile::TempDir::new().unwrap();
    let bare = bare_tmp.path().join("registry.git");
    let sha = init_bare_repo_from_source(src_tmp.path(), &bare).unwrap();
    let bare_url = format!("file://{}", bare.display());

    setup_fixture(cwd_tmp.path(), &bare_url, None);

    let out = run_muntjac(cwd_tmp.path(), cache_tmp.path(), &["fixups", "update"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("Initial pin"), "stdout: {}", stdout);
    assert!(stdout.contains(&sha), "expected SHA in stdout: {}", stdout);

    // Verify muntjac.toml was updated.
    let after = std::fs::read_to_string(cwd_tmp.path().join("muntjac.toml")).unwrap();
    assert!(after.contains(&format!("registry_rev = \"{}\"", sha)));
}

#[test]
fn update_rejects_none_registry() {
    let _g = ENV_GUARD.lock().unwrap();
    let cwd_tmp = tempfile::TempDir::new().unwrap();
    let cache_tmp = tempfile::TempDir::new().unwrap();

    std::fs::write(
        cwd_tmp.path().join("muntjac.toml"),
        r#"
manifest_path   = "pyproject.toml"
third_party_dir = "third-party/python"
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target    = "x86_64-unknown-linux-gnu"
manylinux = "2_17"

[fixups]
registry = "none"
"#,
    )
    .unwrap();
    std::fs::create_dir_all(cwd_tmp.path().join("third-party/python")).unwrap();

    let out = run_muntjac(cwd_tmp.path(), cache_tmp.path(), &["fixups", "update"]);
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("requires a git-based registry"),
        "got: {}",
        stderr
    );
    assert!(stderr.contains("\"none\""), "got: {}", stderr);
}

#[test]
fn update_preserves_existing_toml_formatting() {
    let _g = ENV_GUARD.lock().unwrap();
    let cwd_tmp = tempfile::TempDir::new().unwrap();
    let cache_tmp = tempfile::TempDir::new().unwrap();

    let src_tmp = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(src_tmp.path().join("packages")).unwrap();
    std::fs::write(src_tmp.path().join("packages/.keep"), "").unwrap();

    let bare_tmp = tempfile::TempDir::new().unwrap();
    let bare = bare_tmp.path().join("registry.git");
    let _sha = init_bare_repo_from_source(src_tmp.path(), &bare).unwrap();
    let bare_url = format!("file://{}", bare.display());

    let initial_toml = format!(
        r#"# My muntjac config
manifest_path   = "pyproject.toml"
third_party_dir = "third-party/python"
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
# Comment about Linux
target    = "x86_64-unknown-linux-gnu"
manylinux = "2_17"

[fixups]
# Pinned registry for reproducibility
registry = "{}"
allow_local_overrides = true
"#,
        bare_url
    );
    std::fs::write(cwd_tmp.path().join("muntjac.toml"), &initial_toml).unwrap();
    std::fs::create_dir_all(cwd_tmp.path().join("third-party/python")).unwrap();

    let out = run_muntjac(cwd_tmp.path(), cache_tmp.path(), &["fixups", "update"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let after = std::fs::read_to_string(cwd_tmp.path().join("muntjac.toml")).unwrap();
    // Comments preserved
    assert!(after.contains("# My muntjac config"));
    assert!(after.contains("# Comment about Linux"));
    assert!(after.contains("# Pinned registry for reproducibility"));
    // Key insertion ordering: registry stays first; registry_rev added after registry.
    let reg_pos = after.find("registry = ").unwrap();
    let rev_pos = after.find("registry_rev = ").unwrap();
    assert!(
        reg_pos < rev_pos,
        "registry should come before registry_rev"
    );
}
