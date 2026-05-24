mod common;

use common::git_fixture::init_bare_repo_from_source;
use muntjac::fixup::fetch_into_cache;

/// Mutex serializing the integration tests (they all mutate MUNTJAC_CACHE_HOME).
static ENV_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn setup_cache_isolated_tempdir() -> tempfile::TempDir {
    let tmp = tempfile::TempDir::new().unwrap();
    unsafe {
        std::env::set_var("MUNTJAC_CACHE_HOME", tmp.path());
    }
    tmp
}

#[test]
fn fetch_default_branch_succeeds() {
    let _g = ENV_GUARD.lock().unwrap();
    let _cache_tmp = setup_cache_isolated_tempdir();

    let src = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(src.path().join("packages/pkg-a")).unwrap();
    std::fs::write(
        src.path().join("packages/pkg-a/fixups.toml"),
        "extra_deps = [\"//x:y\"]\n",
    )
    .unwrap();

    let bare_tmp = tempfile::TempDir::new().unwrap();
    let bare_path = bare_tmp.path().join("registry.git");
    let sha = init_bare_repo_from_source(src.path(), &bare_path).unwrap();

    let url = format!("file://{}", bare_path.display());
    let result = fetch_into_cache(&url, None, false).expect("fetch succeeds");
    assert_eq!(result.sha, sha);
    assert!(
        result
            .working_tree
            .join("packages/pkg-a/fixups.toml")
            .is_file()
    );

    unsafe {
        std::env::remove_var("MUNTJAC_CACHE_HOME");
    }
}

#[test]
fn fetch_explicit_sha_succeeds_and_subsequent_call_hits_cache() {
    let _g = ENV_GUARD.lock().unwrap();
    let _cache_tmp = setup_cache_isolated_tempdir();

    let src = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(src.path().join("packages/pkg-a")).unwrap();
    std::fs::write(
        src.path().join("packages/pkg-a/fixups.toml"),
        "extra_deps = [\"//x:y\"]\n",
    )
    .unwrap();

    let bare_tmp = tempfile::TempDir::new().unwrap();
    let bare_path = bare_tmp.path().join("registry.git");
    let sha = init_bare_repo_from_source(src.path(), &bare_path).unwrap();

    let url = format!("file://{}", bare_path.display());

    let first = fetch_into_cache(&url, Some(&sha), false).unwrap();
    assert_eq!(first.sha, sha);

    // Second call with the same SHA should hit cache (no network).
    // We verify by deleting the bare repo and re-fetching — should still work.
    drop(bare_tmp);
    let second = fetch_into_cache(&url, Some(&sha), false).unwrap();
    assert_eq!(second.sha, sha);
    assert_eq!(first.working_tree, second.working_tree);

    unsafe {
        std::env::remove_var("MUNTJAC_CACHE_HOME");
    }
}

#[test]
fn fetch_nonexistent_rev_errors_with_git_fetch() {
    let _g = ENV_GUARD.lock().unwrap();
    let _cache_tmp = setup_cache_isolated_tempdir();

    let src = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(src.path().join("packages")).unwrap();
    std::fs::write(src.path().join("packages/.keep"), "").unwrap();

    let bare_tmp = tempfile::TempDir::new().unwrap();
    let bare_path = bare_tmp.path().join("registry.git");
    let _sha = init_bare_repo_from_source(src.path(), &bare_path).unwrap();

    let url = format!("file://{}", bare_path.display());
    let err = fetch_into_cache(&url, Some("nonexistent-branch"), false).unwrap_err();
    match err {
        muntjac::fixup::FixupError::GitFetch { .. } => {}
        other => panic!("expected GitFetch, got {:?}", other),
    }

    unsafe {
        std::env::remove_var("MUNTJAC_CACHE_HOME");
    }
}
