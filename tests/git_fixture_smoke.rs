mod common;

use common::git_fixture::init_bare_repo_from_source;
use std::fs;

#[test]
fn helper_produces_deterministic_sha() {
    let src = tempfile::TempDir::new().unwrap();
    fs::create_dir_all(src.path().join("packages/pkg-a")).unwrap();
    fs::write(
        src.path().join("packages/pkg-a/fixups.toml"),
        "extra_deps = [\"//x:y\"]\n",
    )
    .unwrap();

    let bare1 = tempfile::TempDir::new().unwrap();
    let sha1 = init_bare_repo_from_source(src.path(), &bare1.path().join("registry.git")).unwrap();

    let bare2 = tempfile::TempDir::new().unwrap();
    let sha2 = init_bare_repo_from_source(src.path(), &bare2.path().join("registry.git")).unwrap();

    assert_eq!(sha1, sha2, "same source → same SHA (deterministic)");
    assert_eq!(sha1.len(), 40);
}
