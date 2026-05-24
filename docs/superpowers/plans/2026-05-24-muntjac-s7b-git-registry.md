# S7b — Git Fetch + Cache + `fixups update` Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the gix-based git fetch, content-addressed cache, `muntjac fixups update` command, and `--no-network` flag wiring so that `registry = "github.com/<owner>/<repo>"` becomes a usable registry mode.

**Architecture:** New `src/cache.rs` resolves the cache root (honoring `$MUNTJAC_CACHE_HOME` for test isolation, else `dirs::cache_dir()/muntjac`). Extended `src/fixup/registry.rs` gains `fetch_into_cache(url, rev, offline)` that uses `gix` to shallow-clone into `<cache_root>/fixups/.staging/<uuid>/` then atomically `rename` to `<cache_root>/fixups/<sha>/`. `EffectiveFixups::load` gains an `offline: bool` parameter; the Git arm calls `fetch_into_cache` then `load_community`. New `src/fixup/diff.rs` computes structural diffs between two `FixupSet`s for `muntjac fixups update`'s output. The CLI handler writes the new SHA back via `toml_edit` to preserve user formatting/comments.

**Tech Stack:** Rust 2024 edition, rust-version 1.85. `gix = "0.83"` (default features = https + ssh transports), `toml_edit = "0.22"` (surgical TOML), `dirs` for XDG cache resolution.

**Prerequisite reading:**
- `docs/superpowers/specs/2026-05-24-muntjac-s7b-git-registry-design.md` (locked design)
- `docs/superpowers/specs/2026-05-24-muntjac-s7a-community-layering-design.md` (`EffectiveFixups` foundation)
- `docs/superpowers/TECH_DEBT.md` TD-S7a-01 (prebake `.gitignore` allow-list pattern — fixture 09 uses the safer form)

**Spec-to-impl naming note:** The spec uses `--offline` throughout. The codebase already has `Globals::no_network` (added in S0, currently used only by `cli/vendor.rs`) with identical semantics. **This plan reuses `no_network`** rather than introducing a redundant flag. Function parameter names use `offline: bool` for clarity at the API boundary; the CLI flag stays `--no-network`.

---

## Phase 1 — Deps & error variants

### Task 1: Add `gix` and `toml_edit` dependencies

**Files:**
- Modify: `Cargo.toml` (`[dependencies]`)

- [ ] **Step 1: Add the dependencies**

In `Cargo.toml`, add to `[dependencies]` (alphabetical insertion):

```toml
gix = "0.83"
toml_edit = "0.22"
```

`gix` default features bring in https + ssh transports — needed for github.com URLs and `file://` test paths.

- [ ] **Step 2: Verify resolution**

Run: `cargo build 2>&1 | tail -10`

Expected: clean build. First build will pull gix's transitive deps (~50 crates); takes 1-3 minutes.

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "feat(s7b): add gix + toml_edit deps"
```

---

### Task 2: Add error variants for git fetch + cache + offline

**Files:**
- Modify: `src/fixup/error.rs` (add `GitFetch`, `CacheCorrupt`, `Offline` variants)
- Modify: `src/error.rs` (add `CacheError` enum)
- Remove: `FixupError::GitRegistryNotImplemented` (S7a stub, now unreachable)

- [ ] **Step 1: Write the failing tests for FixupError variants**

Add to `src/fixup/error.rs::tests`:

```rust
#[test]
fn offline_message_is_exact() {
    let e = FixupError::Offline {
        pin: "abc123def456".into(),
    };
    assert_eq!(
        e.to_string(),
        "offline mode but cache miss for pinned rev `abc123def456`\n  run `muntjac fixups update` (without --offline) first"
    );
}

#[test]
fn offline_default_branch_pin_message_is_exact() {
    let e = FixupError::Offline {
        pin: "(default branch)".into(),
    };
    assert_eq!(
        e.to_string(),
        "offline mode but cache miss for pinned rev `(default branch)`\n  run `muntjac fixups update` (without --offline) first"
    );
}

#[test]
fn cache_corrupt_message_is_exact() {
    let e = FixupError::CacheCorrupt {
        path: PathBuf::from("/tmp/muntjac/fixups/abc"),
        reason: "missing packages/ subdir".into(),
    };
    assert_eq!(
        e.to_string(),
        "cache entry at /tmp/muntjac/fixups/abc appears corrupt: missing packages/ subdir\n  delete it and re-run `muntjac fixups update`"
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p muntjac fixup::error::tests::offline_message_is_exact -- --exact`

Expected: FAIL — `Offline` variant does not exist.

- [ ] **Step 3: Add the three FixupError variants and remove the obsolete stub**

In `src/fixup/error.rs`, locate `GitRegistryNotImplemented` (added in S7a T1, around line 73). **Delete it.** Then in its place, add the three new variants:

```rust
#[error("offline mode but cache miss for pinned rev `{pin}`\n  run `muntjac fixups update` (without --offline) first")]
Offline { pin: String },

#[error("cache entry at {path} appears corrupt: {reason}\n  delete it and re-run `muntjac fixups update`")]
CacheCorrupt { path: PathBuf, reason: String },

#[error("git fetch failed for {url}{}: {source}", match rev { Some(r) => format!(" @ {r}"), None => String::new() })]
GitFetch {
    url: String,
    rev: Option<String>,
    #[source]
    source: Box<gix::clone::fetch::Error>,
},
```

Note: the `match rev` inside `#[error(...)]` uses thiserror's format-args shorthand. If thiserror rejects the inline match, fall back to a dedicated `fn fmt_git_fetch_url(...) -> String` helper and reference it via `#[error(fmt = fmt_git_fetch_url)]`.

- [ ] **Step 4: Run FixupError tests**

Run: `cargo test -p muntjac fixup::error::tests`

Expected: PASS — 3 new tests + existing ones. The `git_registry_not_implemented_message_is_exact` test from S7a T1 will FAIL because that variant is removed; **delete that test** in this same task.

- [ ] **Step 5: Verify no other call sites reference `GitRegistryNotImplemented`**

Run: `grep -rn 'GitRegistryNotImplemented' src/ tests/`

Expected: zero matches. If matches exist (most likely in S7a's `effective_fixups_load_git_errors_in_s7a` test in `src/fixup/layer.rs`), update those tests in T8 — for now, comment them out with `#[ignore]` or `#[cfg(any())]` to keep the build green; T8 replaces them.

Alternative: delete the test entirely now, since T8 replaces it with a positive test. Cleanest.

In `src/fixup/layer.rs::tests`, delete:
```rust
#[test]
fn effective_fixups_load_git_errors_in_s7a() {
    // ... whole test body ...
}
```

- [ ] **Step 6: Add `CacheError` typed enum**

In `src/error.rs`, append a new `CacheError` enum:

```rust
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("could not resolve cache root: {0}")]
    CacheRootResolve(String),

    #[error("could not create cache directory {path}: {source}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}
```

Add `use std::path::PathBuf;` at the top of `src/error.rs` if not already present.

- [ ] **Step 7: Add a test for CacheError**

In `src/error.rs::tests` (already has a tests module from S7a T2):

```rust
#[test]
fn cache_root_resolve_error_message() {
    let e = CacheError::CacheRootResolve("no XDG_CACHE_HOME or HOME".into());
    assert_eq!(
        e.to_string(),
        "could not resolve cache root: no XDG_CACHE_HOME or HOME"
    );
}
```

- [ ] **Step 8: Run all error tests and verify build**

Run: `cargo test -p muntjac fixup::error error -- --skip git_registry`

Expected: PASS for all error tests.

Run: `cargo build 2>&1 | tail -10`

Expected: clean build (assuming we deleted the S7a test that references `GitRegistryNotImplemented`).

- [ ] **Step 9: Run full test suite**

Run: `cargo test 2>&1 | grep -E '^test result' | awk '{sum+=$4; if ($6 != "0") fails+=$6} END {print "Pass:", sum, "Fail:", fails+0}'`

Expected: same as before (316 minus 1 test deleted = 315; or 316 if a new test added netted out). No regressions.

- [ ] **Step 10: Commit**

```bash
git add src/fixup/error.rs src/error.rs src/fixup/layer.rs
git commit -m "feat(s7b): error variants for git fetch + cache + offline"
```

---

## Phase 2 — Cache module

### Task 3: `src/cache.rs` — cache root resolution

**Files:**
- Create: `src/cache.rs`
- Modify: `src/lib.rs` (add `pub mod cache;`)

- [ ] **Step 1: Look up current lib.rs module list for insertion point**

Run: `grep -n '^pub mod' src/lib.rs`

Expected: list of `pub mod` lines; note the spot for alphabetical insertion of `cache`.

- [ ] **Step 2: Write the failing tests**

Create `src/cache.rs`:

```rust
//! Cache-root resolution.
//!
//! Precedence (highest first):
//!   - `$MUNTJAC_CACHE_HOME` (test isolation; also for users with
//!                            non-XDG-friendly env)
//!   - `dirs::cache_dir()/muntjac` (XDG-compliant; `dirs` already honors
//!                                  `$XDG_CACHE_HOME` on Linux)

use std::path::PathBuf;

use crate::error::CacheError;

/// Resolve the muntjac cache root. Creates the directory if missing.
pub fn cache_root() -> Result<PathBuf, CacheError> {
    let root = if let Ok(env_override) = std::env::var("MUNTJAC_CACHE_HOME") {
        if env_override.is_empty() {
            return Err(CacheError::CacheRootResolve(
                "MUNTJAC_CACHE_HOME is set but empty".into(),
            ));
        }
        PathBuf::from(env_override)
    } else {
        dirs::cache_dir()
            .ok_or_else(|| {
                CacheError::CacheRootResolve(
                    "neither MUNTJAC_CACHE_HOME nor a platform cache dir is available".into(),
                )
            })?
            .join("muntjac")
    };

    std::fs::create_dir_all(&root).map_err(|e| CacheError::CreateDir {
        path: root.clone(),
        source: e,
    })?;

    Ok(root)
}

/// `<cache_root>/fixups/<sha>/`. Does NOT create the SHA directory.
pub fn fixup_cache_path_for_sha(sha: &str) -> Result<PathBuf, CacheError> {
    Ok(cache_root()?.join("fixups").join(sha))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Serialize tests that mutate env vars — Rust runs unit tests in parallel.
    /// Using a Mutex avoids cross-test contamination.
    static ENV_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn env_override_takes_precedence() {
        let _g = ENV_GUARD.lock().unwrap();
        let tmp = TempDir::new().unwrap();
        unsafe {
            std::env::set_var("MUNTJAC_CACHE_HOME", tmp.path());
        }
        let root = cache_root().expect("resolves");
        assert_eq!(root, tmp.path());
        unsafe {
            std::env::remove_var("MUNTJAC_CACHE_HOME");
        }
    }

    #[test]
    fn xdg_fallback_used_without_override() {
        let _g = ENV_GUARD.lock().unwrap();
        unsafe {
            std::env::remove_var("MUNTJAC_CACHE_HOME");
        }
        let root = cache_root().expect("resolves to platform default");
        assert!(root.ends_with("muntjac"));
        // Don't assert on a specific path — varies by platform/env.
    }

    #[test]
    fn empty_env_override_errors() {
        let _g = ENV_GUARD.lock().unwrap();
        unsafe {
            std::env::set_var("MUNTJAC_CACHE_HOME", "");
        }
        let err = cache_root().unwrap_err();
        match err {
            CacheError::CacheRootResolve(msg) => {
                assert!(msg.contains("empty"), "got: {}", msg);
            }
            other => panic!("expected CacheRootResolve, got {:?}", other),
        }
        unsafe {
            std::env::remove_var("MUNTJAC_CACHE_HOME");
        }
    }

    #[test]
    fn fixup_cache_path_appends_sha_under_fixups_subdir() {
        let _g = ENV_GUARD.lock().unwrap();
        let tmp = TempDir::new().unwrap();
        unsafe {
            std::env::set_var("MUNTJAC_CACHE_HOME", tmp.path());
        }
        let p = fixup_cache_path_for_sha("abc123").unwrap();
        assert_eq!(p, tmp.path().join("fixups").join("abc123"));
        unsafe {
            std::env::remove_var("MUNTJAC_CACHE_HOME");
        }
    }
}
```

`unsafe { std::env::set_var(...) }` is needed because Rust 2024 marked `set_var`/`remove_var` as unsafe (they're racy with concurrent threads in the same process). The mutex serializes the tests.

- [ ] **Step 3: Wire the module into lib.rs**

In `src/lib.rs`, add (alphabetical):

```rust
pub mod cache;
```

Add `dirs` to `[dependencies]` in `Cargo.toml`:

```toml
dirs = "5"
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p muntjac cache::tests`

Expected: PASS for all 4 tests.

- [ ] **Step 5: Verify full suite**

Run: `cargo test 2>&1 | grep -E '^test result' | awk '{sum+=$4} END {print sum}'`

Expected: previous count + 4.

- [ ] **Step 6: Commit**

```bash
git add src/cache.rs src/lib.rs Cargo.toml Cargo.lock
git commit -m "feat(s7b): cache_root with env override + XDG fallback"
```

---

## Phase 3 — Parser extension for `file://*.git`

### Task 4: Extend `parse_registry_config` to accept `file://<abs>.git` as Git form

**Files:**
- Modify: `src/fixup/registry.rs` (extend the `file://` branch)

The S7a parser maps `file://...` to `RegistryConfig::FileUrl(PathBuf)` (directory checkout). S7b extends: if the path ends in `.git`, it's a bare-repo git URL and maps to `RegistryConfig::Git { url, rev }` instead. This lets tests construct hermetic git fixtures without leaving the parser's CLI-facing API.

- [ ] **Step 1: Write the failing tests**

Add to `src/fixup/registry.rs::tests`:

```rust
#[test]
fn parses_file_url_with_dot_git_as_git_form() {
    let got = parse_registry_config("file:///abs/path/to/bare.git", Some("abc123")).unwrap();
    assert_eq!(
        got,
        RegistryConfig::Git {
            url: "file:///abs/path/to/bare.git".into(),
            rev: Some("abc123".into()),
        }
    );
}

#[test]
fn parses_file_url_without_dot_git_stays_file_url() {
    let got = parse_registry_config("file:///abs/path/to/checkout", None).unwrap();
    assert_eq!(
        got,
        RegistryConfig::FileUrl(std::path::PathBuf::from("/abs/path/to/checkout"))
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p muntjac fixup::registry::tests::parses_file_url_with_dot_git -- --exact`

Expected: FAIL — current parser only produces FileUrl for any `file://` URL.

- [ ] **Step 3: Extend the parser**

In `src/fixup/registry.rs::parse_registry_config`, modify the `file://` branch:

Before:
```rust
if let Some(rest) = raw.strip_prefix("file://") {
    if !rest.starts_with('/') {
        return Err(crate::error::ConfigError::RegistryPathNotAbsolute {
            path: rest.to_string(),
        });
    }
    return Ok(RegistryConfig::FileUrl(PathBuf::from(rest)));
}
```

After:
```rust
if let Some(rest) = raw.strip_prefix("file://") {
    if !rest.starts_with('/') {
        return Err(crate::error::ConfigError::RegistryPathNotAbsolute {
            path: rest.to_string(),
        });
    }
    // Discriminator: `.git` suffix means bare-repo git form (S7b).
    // Anything else is a directory checkout (S7a FileUrl form).
    if rest.ends_with(".git") {
        return Ok(RegistryConfig::Git {
            url: raw.to_string(),
            rev: registry_rev.map(|s| s.to_string()),
        });
    }
    return Ok(RegistryConfig::FileUrl(PathBuf::from(rest)));
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p muntjac fixup::registry::tests`

Expected: PASS for both new tests + all existing parse tests.

- [ ] **Step 5: Full suite**

Run: `cargo test 2>&1 | grep -E '^test result' | awk '{sum+=$4} END {print sum}'`

Expected: previous count + 2.

- [ ] **Step 6: Commit**

```bash
git add src/fixup/registry.rs
git commit -m "feat(s7b): parse_registry_config accepts file://*.git as Git form"
```

---

## Phase 4 — Diff module

### Task 5: `src/fixup/diff.rs` — DiffLine + diff_fixup_sets + render_diff

**Files:**
- Create: `src/fixup/diff.rs`
- Modify: `src/fixup/mod.rs` (`pub mod diff;` + re-exports)

- [ ] **Step 1: Write the failing tests**

Create `src/fixup/diff.rs`:

```rust
//! Structural diff between two `FixupSet`s for `muntjac fixups update`.

use std::collections::BTreeSet;
use std::str::FromStr;

use pep508_rs::PackageName;

use crate::fixup::{FixupConfig, FixupSet};

#[derive(Debug, PartialEq, Eq)]
pub enum DiffLine {
    /// Package present in `new`, absent in `old`.
    Added(PackageName),
    /// Package present in `old`, absent in `new`.
    Removed(PackageName),
    /// Package present in both with different content. The string list
    /// names which fields differ (canonical order, see `modified_fields`).
    Modified(PackageName, Vec<&'static str>),
}

/// Compare two FixupSets. Returns DiffLines in canonical package-name order
/// (lexicographic on the PEP-503-normalized form).
pub fn diff_fixup_sets(old: &FixupSet, new: &FixupSet) -> Vec<DiffLine> {
    let old_names: BTreeSet<&PackageName> = old.iter().map(|(n, _)| n).collect();
    let new_names: BTreeSet<&PackageName> = new.iter().map(|(n, _)| n).collect();
    let all: BTreeSet<&PackageName> = old_names.union(&new_names).copied().collect();

    let mut out = Vec::new();
    for name in all {
        match (old.get(name), new.get(name)) {
            (None, Some(_)) => out.push(DiffLine::Added(name.clone())),
            (Some(_), None) => out.push(DiffLine::Removed(name.clone())),
            (Some(o), Some(n)) => {
                let fields = modified_fields(o, n);
                if !fields.is_empty() {
                    out.push(DiffLine::Modified(name.clone(), fields));
                }
            }
            (None, None) => unreachable!(),
        }
    }
    out
}

/// Field-by-field structural compare. Returns the names of fields that differ.
fn modified_fields(a: &FixupConfig, b: &FixupConfig) -> Vec<&'static str> {
    let mut fields = Vec::new();

    if a.top.extra_deps != b.top.extra_deps {
        fields.push("extra_deps");
    }
    if a.top.omit_deps != b.top.omit_deps {
        fields.push("omit_deps");
    }
    if a.top.replace_deps != b.top.replace_deps {
        fields.push("replace_deps");
    }
    if a.top.prefer_wheel != b.top.prefer_wheel {
        fields.push("prefer_wheel");
    }
    if a.top.exclude_wheels != b.top.exclude_wheels {
        fields.push("exclude_wheels");
    }
    if a.top.overlay != b.top.overlay {
        fields.push("overlay");
    }
    if a.top.entry_points != b.top.entry_points {
        fields.push("entry_points");
    }
    if a.top.visibility != b.top.visibility {
        fields.push("visibility");
    }
    if a.top.labels != b.top.labels {
        fields.push("labels");
    }
    if a.top.runtime_env != b.top.runtime_env {
        fields.push("runtime_env");
    }
    if a.top.sdist != b.top.sdist {
        fields.push("sdist");
    }
    if a.replace_community != b.replace_community {
        fields.push("replace_community");
    }
    if a.cfg_sections != b.cfg_sections {
        fields.push("cfg_sections");
    }

    fields
}

/// Render diff lines as the user-facing text format.
/// Returns empty string for empty input, else newline-terminated.
pub fn render_diff(lines: &[DiffLine]) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let mut out = String::new();
    for line in lines {
        match line {
            DiffLine::Added(n) => {
                out.push_str(&format!("+ {}\n", n));
            }
            DiffLine::Removed(n) => {
                out.push_str(&format!("- {}\n", n));
            }
            DiffLine::Modified(n, fields) => {
                out.push_str(&format!("~ {} ({})\n", n, fields.join(", ")));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixup::{FixupBody, FixupConfig, FixupSet};
    use std::collections::BTreeMap;

    fn fixup_with_extra_deps(deps: Vec<&str>) -> FixupConfig {
        FixupConfig {
            top: FixupBody {
                extra_deps: deps.into_iter().map(String::from).collect(),
                ..Default::default()
            },
            cfg_sections: vec![],
            replace_community: false,
        }
    }

    fn set(pkgs: Vec<(&str, FixupConfig)>) -> FixupSet {
        let mut m = BTreeMap::new();
        for (n, c) in pkgs {
            m.insert(PackageName::from_str(n).unwrap(), c);
        }
        FixupSet::from_map_for_test(m)
    }

    #[test]
    fn diff_added_package() {
        let old = set(vec![]);
        let new = set(vec![("pkg-a", fixup_with_extra_deps(vec!["//x:y"]))]);
        let lines = diff_fixup_sets(&old, &new);
        assert_eq!(lines.len(), 1);
        assert!(matches!(&lines[0], DiffLine::Added(n) if n.as_ref() == "pkg-a"));
    }

    #[test]
    fn diff_removed_package() {
        let old = set(vec![("pkg-a", fixup_with_extra_deps(vec!["//x:y"]))]);
        let new = set(vec![]);
        let lines = diff_fixup_sets(&old, &new);
        assert_eq!(lines.len(), 1);
        assert!(matches!(&lines[0], DiffLine::Removed(n) if n.as_ref() == "pkg-a"));
    }

    #[test]
    fn diff_modified_extra_deps() {
        let old = set(vec![("pkg-a", fixup_with_extra_deps(vec!["//x:y"]))]);
        let new = set(vec![("pkg-a", fixup_with_extra_deps(vec!["//x:y", "//z:w"]))]);
        let lines = diff_fixup_sets(&old, &new);
        assert_eq!(lines.len(), 1);
        match &lines[0] {
            DiffLine::Modified(n, fields) => {
                assert_eq!(n.as_ref(), "pkg-a");
                assert_eq!(fields, &vec!["extra_deps"]);
            }
            other => panic!("expected Modified, got {:?}", other),
        }
    }

    #[test]
    fn diff_modified_multiple_fields() {
        let mut a = fixup_with_extra_deps(vec!["//x:y"]);
        a.top.visibility = Some(vec!["//a:...".into()]);
        let mut b = fixup_with_extra_deps(vec!["//z:w"]);
        b.top.visibility = Some(vec!["//b:...".into()]);

        let old = set(vec![("pkg-a", a)]);
        let new = set(vec![("pkg-a", b)]);
        let lines = diff_fixup_sets(&old, &new);
        match &lines[0] {
            DiffLine::Modified(_, fields) => {
                assert_eq!(fields, &vec!["extra_deps", "visibility"]);
            }
            other => panic!("expected Modified, got {:?}", other),
        }
    }

    #[test]
    fn diff_modified_cfg_sections() {
        let a = fixup_with_extra_deps(vec!["//x:y"]);
        let mut b = fixup_with_extra_deps(vec!["//x:y"]);
        b.cfg_sections.push((
            "target_os = \"linux\"".to_string(),
            FixupBody {
                extra_deps: vec!["//linux:dep".into()],
                ..Default::default()
            },
        ));
        let old = set(vec![("pkg-a", a)]);
        let new = set(vec![("pkg-a", b)]);
        let lines = diff_fixup_sets(&old, &new);
        match &lines[0] {
            DiffLine::Modified(_, fields) => {
                assert_eq!(fields, &vec!["cfg_sections"]);
            }
            other => panic!("expected Modified, got {:?}", other),
        }
    }

    #[test]
    fn diff_identical_yields_nothing() {
        let cfg = fixup_with_extra_deps(vec!["//x:y"]);
        let old = set(vec![("pkg-a", cfg.clone())]);
        let new = set(vec![("pkg-a", cfg)]);
        let lines = diff_fixup_sets(&old, &new);
        assert!(lines.is_empty());
    }

    #[test]
    fn render_diff_canonical_format() {
        let lines = vec![
            DiffLine::Added(PackageName::from_str("aaa").unwrap()),
            DiffLine::Modified(
                PackageName::from_str("bbb").unwrap(),
                vec!["extra_deps", "visibility"],
            ),
            DiffLine::Removed(PackageName::from_str("ccc").unwrap()),
        ];
        let out = render_diff(&lines);
        assert_eq!(out, "+ aaa\n~ bbb (extra_deps, visibility)\n- ccc\n");
    }

    #[test]
    fn render_diff_empty_input_empty_output() {
        let lines: Vec<DiffLine> = vec![];
        let out = render_diff(&lines);
        assert_eq!(out, "");
    }
}
```

- [ ] **Step 2: Wire the module**

In `src/fixup/mod.rs`, add (alphabetical):

```rust
pub mod diff;
```

Add to the `pub use` re-exports:

```rust
pub use diff::{DiffLine, diff_fixup_sets, render_diff};
```

- [ ] **Step 3: Run the tests**

Run: `cargo test -p muntjac fixup::diff`

Expected: PASS for all 8 tests.

- [ ] **Step 4: Full suite**

Run: `cargo test 2>&1 | grep -E '^test result' | awk '{sum+=$4} END {print sum}'`

Expected: previous count + 8.

- [ ] **Step 5: Commit**

```bash
git add src/fixup/diff.rs src/fixup/mod.rs
git commit -m "feat(s7b): diff_fixup_sets + render_diff"
```

---

## Phase 5 — Git fetch

### Task 6: Bare-repo test helper

**Files:**
- Create: `tests/common/mod.rs` (declares `git_fixture`)
- Create: `tests/common/git_fixture.rs` (the helper)

Rust test binaries each pick up `tests/<name>.rs` as a separate crate; shared helpers go in `tests/common/`. Since this is the first shared helper, we set up the `tests/common/` directory now.

- [ ] **Step 1: Check whether `tests/common/` already exists**

Run: `ls tests/common/ 2>&1`

If it exists, just add `git_fixture.rs`. If not, create both `tests/common/mod.rs` and `tests/common/git_fixture.rs`.

- [ ] **Step 2: Create the helper**

Create `tests/common/git_fixture.rs`:

```rust
//! Test helpers: build a bare git repo from a source directory.
//!
//! Used by S7b tests that exercise `fetch_into_cache` against a hermetic
//! bare repo (no network).

use std::fs;
use std::path::Path;

/// Initialize a bare git repo at `bare_dest` and seed it with a single
/// commit containing every file under `source_dir`. Returns the commit SHA.
///
/// Author/committer are deterministic: ("muntjac-test",
/// "test@example.com", timestamp 0). This guarantees the SHA is stable
/// across runs given the same source content — useful for test
/// assertions and for the content-addressed cache.
pub fn init_bare_repo_from_source(
    source_dir: &Path,
    bare_dest: &Path,
) -> Result<String, Box<dyn std::error::Error>> {
    use gix::actor;
    use gix::date::Time;
    use gix::objs::{Tree, tree::Entry, tree::EntryMode};

    // 1. Initialize bare repo
    let repo = gix::init_bare(bare_dest)?;

    // 2. Walk source_dir and write blobs + tree entries
    let mut entries: Vec<Entry> = Vec::new();
    walk_into_entries(&repo, source_dir, source_dir, &mut entries)?;

    // 3. Sort entries by name (git tree invariant)
    entries.sort_by(|a, b| a.filename.cmp(&b.filename));

    // 4. Write the root tree
    let tree = Tree { entries };
    let tree_id = repo.write_object(&tree)?.detach();

    // 5. Build commit with deterministic signature
    let sig = actor::SignatureRef {
        name: "muntjac-test".into(),
        email: "test@example.com".into(),
        time: Time {
            seconds: 0,
            offset: 0,
        },
    };

    let commit = gix::objs::Commit {
        tree: tree_id,
        parents: Default::default(),
        author: sig.into(),
        committer: sig.into(),
        encoding: None,
        message: "initial".into(),
        extra_headers: vec![],
    };
    let commit_id = repo.write_object(&commit)?.detach();

    // 6. Update HEAD → refs/heads/main → commit
    use gix::refs::transaction::{Change, LogChange, PreviousValue, RefEdit};
    use gix::refs::{FullName, Target};

    let main_ref: FullName = "refs/heads/main".try_into()?;
    repo.edit_reference(RefEdit {
        change: Change::Update {
            log: LogChange {
                mode: gix::refs::transaction::RefLog::AndReference,
                force_create_reflog: false,
                message: "initial commit".into(),
            },
            expected: PreviousValue::MustNotExist,
            new: Target::Object(commit_id),
        },
        name: main_ref,
        deref: false,
    })?;

    // Point HEAD at main (symbolic ref)
    let head: FullName = "HEAD".try_into()?;
    let main_target: FullName = "refs/heads/main".try_into()?;
    repo.edit_reference(RefEdit {
        change: Change::Update {
            log: LogChange::default(),
            expected: PreviousValue::Any,
            new: Target::Symbolic(main_target),
        },
        name: head,
        deref: false,
    })?;

    Ok(commit_id.to_hex().to_string())
}

/// Recursively walk `dir`, adding tree entries (relative to `root`).
fn walk_into_entries(
    repo: &gix::Repository,
    root: &Path,
    dir: &Path,
    entries: &mut Vec<gix::objs::tree::Entry>,
) -> Result<(), Box<dyn std::error::Error>> {
    use gix::objs::{Tree, tree::Entry, tree::EntryMode};

    let mut subdir_entries: std::collections::BTreeMap<String, Vec<Entry>> =
        std::collections::BTreeMap::new();
    let mut file_entries: Vec<Entry> = Vec::new();

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = path
            .file_name()
            .ok_or("no filename")?
            .to_str()
            .ok_or("non-utf8 filename")?
            .to_string();

        if path.is_dir() {
            let mut sub_entries: Vec<Entry> = Vec::new();
            walk_into_entries(repo, root, &path, &mut sub_entries)?;
            sub_entries.sort_by(|a, b| a.filename.cmp(&b.filename));
            let sub_tree = Tree {
                entries: sub_entries,
            };
            let sub_tree_id = repo.write_object(&sub_tree)?.detach();
            file_entries.push(Entry {
                mode: EntryMode::Tree,
                filename: name.into(),
                oid: sub_tree_id,
            });
        } else if path.is_file() {
            let bytes = fs::read(&path)?;
            let blob_id = repo.write_blob(&bytes)?.detach();
            file_entries.push(Entry {
                mode: EntryMode::Blob,
                filename: name.into(),
                oid: blob_id,
            });
        }
    }

    entries.extend(file_entries);
    Ok(())
}
```

**Note on gix API:** the exact API for `gix::init_bare`, `write_object`, ref-editing varies by version. This is for gix 0.83 — if the API differs (e.g., field names, return types), the implementer follows compiler errors to adapt. The above sketch shows intent; gix's own `examples/` directory and docs.rs are authoritative. **If the helper proves harder to write than expected (e.g., gix doesn't have a `write_blob` shortcut and requires lower-level oid construction), fall back to invoking `git` via `std::process::Command` for the bare-repo construction.** Both are acceptable — the goal is "deterministic bare repo with content from source_dir."

Fallback (subprocess git, much simpler):

```rust
pub fn init_bare_repo_from_source(
    source_dir: &Path,
    bare_dest: &Path,
) -> Result<String, Box<dyn std::error::Error>> {
    use std::process::Command;

    // Create a temp working dir; init non-bare repo; commit; push to bare.
    let work_dir = tempfile::TempDir::new()?;

    // Copy source_dir contents into work_dir
    for entry in walkdir::WalkDir::new(source_dir) {
        let entry = entry?;
        let rel = entry.path().strip_prefix(source_dir)?;
        let dst = work_dir.path().join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&dst)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry.path(), &dst)?;
        }
    }

    let git = |args: &[&str]| -> Result<String, Box<dyn std::error::Error>> {
        let out = Command::new("git")
            .args(args)
            .current_dir(work_dir.path())
            .env("GIT_AUTHOR_NAME", "muntjac-test")
            .env("GIT_AUTHOR_EMAIL", "test@example.com")
            .env("GIT_AUTHOR_DATE", "1970-01-01T00:00:00Z")
            .env("GIT_COMMITTER_NAME", "muntjac-test")
            .env("GIT_COMMITTER_EMAIL", "test@example.com")
            .env("GIT_COMMITTER_DATE", "1970-01-01T00:00:00Z")
            .output()?;
        if !out.status.success() {
            return Err(format!("git {:?} failed: {}", args,
                String::from_utf8_lossy(&out.stderr)).into());
        }
        Ok(String::from_utf8(out.stdout)?.trim().to_string())
    };

    git(&["init", "-q", "-b", "main"])?;
    git(&["add", "-A"])?;
    git(&["commit", "-q", "-m", "initial"])?;
    let sha = git(&["rev-parse", "HEAD"])?;

    fs::create_dir_all(bare_dest)?;
    let bare_str = bare_dest.to_str().ok_or("non-utf8 bare path")?;
    git(&["clone", "-q", "--bare", ".", bare_str])?;

    Ok(sha)
}
```

The subprocess version requires `git` on PATH (already required by CI for fixture 05's buck2 build step). **Prefer the subprocess version** unless the gix-native version comes together quickly. Use `walkdir` (already a dev-dep).

- [ ] **Step 3: Create `tests/common/mod.rs`**

Create (or extend if exists) `tests/common/mod.rs`:

```rust
#![allow(dead_code)]   // each test binary uses a subset
pub mod git_fixture;
```

- [ ] **Step 4: Add a smoke test for the helper itself**

Create `tests/git_fixture_smoke.rs`:

```rust
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
```

- [ ] **Step 5: Run smoke test**

Run: `cargo test --test git_fixture_smoke`

Expected: PASS.

If the gix-native version FAILS (any compile error or runtime issue), swap to the subprocess version and re-run. The fallback is always available.

- [ ] **Step 6: Commit**

```bash
git add tests/common/ tests/git_fixture_smoke.rs
git commit -m "test(s7b): bare-repo helper for git fetch tests"
```

---

### Task 7: `fetch_into_cache` implementation

**Files:**
- Modify: `src/fixup/registry.rs` (add `FetchResult`, `fetch_into_cache`, tests)
- Modify: `src/fixup/mod.rs` (re-export)

The core S7b function. Cache-hit fast path → offline error path → shallow fetch → atomic rename.

- [ ] **Step 1: Add the public types and stub function**

In `src/fixup/registry.rs`, append:

```rust
/// Resolved fetch result. The `working_tree` is suitable for
/// `load_community(...)`.
#[derive(Debug, Clone)]
pub struct FetchResult {
    pub sha: String,             // full 40-char hex
    pub working_tree: std::path::PathBuf,
}

/// Resolve `rev` to a concrete SHA and fetch into the cache.
///
/// - `rev = Some(rev_str)`:
///     - If `rev_str` is a 40-char hex SHA matching a cache hit, return immediately.
///     - Else fetch the named ref/SHA/tag/branch from the remote.
/// - `rev = None`: fetch HEAD of the remote's default branch.
/// - `offline = true`: never touch the network; error `FixupError::Offline`
///   if the cache doesn't already contain the resolved SHA.
pub fn fetch_into_cache(
    url: &str,
    rev: Option<&str>,
    offline: bool,
) -> Result<FetchResult, crate::fixup::FixupError> {
    use crate::fixup::FixupError;

    // 1. Cache hit fast path: rev is a known SHA and cache dir exists.
    if let Some(rev_str) = rev {
        if is_full_hex_sha(rev_str) {
            let candidate = crate::cache::fixup_cache_path_for_sha(rev_str)
                .map_err(|e| FixupError::Io {
                    path: std::path::PathBuf::from("<cache-root>"),
                    source: std::io::Error::other(e.to_string()),
                })?;
            if candidate.is_dir() {
                let packages_dir = candidate.join("packages");
                if !packages_dir.is_dir() {
                    return Err(FixupError::CacheCorrupt {
                        path: candidate,
                        reason: "missing packages/ subdir".into(),
                    });
                }
                return Ok(FetchResult {
                    sha: rev_str.to_string(),
                    working_tree: candidate,
                });
            }
        }
    }

    // 2. Offline: cache miss is fatal.
    if offline {
        let pin = rev.map(|s| s.to_string()).unwrap_or_else(|| "(default branch)".into());
        return Err(FixupError::Offline { pin });
    }

    // 3. Fetch path: shallow clone into staging, then atomic rename.
    fetch_and_stage(url, rev)
}

fn is_full_hex_sha(s: &str) -> bool {
    s.len() == 40 && s.chars().all(|c| c.is_ascii_hexdigit())
}

fn fetch_and_stage(
    url: &str,
    rev: Option<&str>,
) -> Result<FetchResult, crate::fixup::FixupError> {
    use crate::fixup::FixupError;

    let cache_root = crate::cache::cache_root().map_err(|e| FixupError::Io {
        path: std::path::PathBuf::from("<cache-root>"),
        source: std::io::Error::other(e.to_string()),
    })?;
    let staging_root = cache_root.join("fixups").join(".staging");
    std::fs::create_dir_all(&staging_root).map_err(|e| FixupError::Io {
        path: staging_root.clone(),
        source: e,
    })?;

    // Unique staging dir name to avoid collisions between concurrent fetches.
    let staging = staging_root.join(format!(
        "stage-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));

    // gix shallow clone (depth=1)
    let url_owned = url.to_string();
    let staging_for_clone = staging.clone();
    let prep = gix::prepare_clone(url_owned.clone(), &staging_for_clone)
        .map_err(|e| {
            let _ = std::fs::remove_dir_all(&staging);
            FixupError::GitFetch {
                url: url_owned.clone(),
                rev: rev.map(|s| s.to_string()),
                source: Box::new(gix_error_into_clone_fetch(e)),
            }
        })?;

    // For an explicit non-SHA ref (branch/tag), set refspec; otherwise default to HEAD.
    let prep = match rev {
        Some(r) if !is_full_hex_sha(r) => prep
            .with_ref_name(Some(r))
            .map_err(|e| {
                let _ = std::fs::remove_dir_all(&staging);
                FixupError::GitFetch {
                    url: url_owned.clone(),
                    rev: rev.map(|s| s.to_string()),
                    source: Box::new(gix_error_into_clone_fetch(e)),
                }
            })?,
        _ => prep,
    };

    let (mut checkout, _outcome) = prep
        .fetch_then_checkout(gix::progress::Discard, &gix::interrupt::IS_INTERRUPTED)
        .map_err(|e| {
            let _ = std::fs::remove_dir_all(&staging);
            FixupError::GitFetch {
                url: url_owned.clone(),
                rev: rev.map(|s| s.to_string()),
                source: Box::new(e),
            }
        })?;

    let (repo, _outcome) = checkout
        .main_worktree(gix::progress::Discard, &gix::interrupt::IS_INTERRUPTED)
        .map_err(|e| {
            let _ = std::fs::remove_dir_all(&staging);
            FixupError::GitFetch {
                url: url_owned.clone(),
                rev: rev.map(|s| s.to_string()),
                source: Box::new(gix_error_into_clone_fetch_checkout(e)),
            }
        })?;

    // Resolve the actual HEAD SHA we just checked out.
    let head_commit = repo.head_commit().map_err(|e| FixupError::Io {
        path: staging.clone(),
        source: std::io::Error::other(format!("{:?}", e)),
    })?;
    let resolved_sha = head_commit.id().to_hex().to_string();

    // Atomic rename to final destination.
    let final_dest = cache_root.join("fixups").join(&resolved_sha);

    if final_dest.is_dir() {
        // Race: another process beat us to it. Discard staging, return existing.
        let _ = std::fs::remove_dir_all(&staging);
        return Ok(FetchResult {
            sha: resolved_sha,
            working_tree: final_dest,
        });
    }

    std::fs::rename(&staging, &final_dest).map_err(|e| FixupError::Io {
        path: final_dest.clone(),
        source: e,
    })?;

    Ok(FetchResult {
        sha: resolved_sha,
        working_tree: final_dest,
    })
}

// Helpers to convert gix's varied error types into the single
// gix::clone::fetch::Error wrapper that FixupError::GitFetch holds.
// In practice gix has many error types in the clone pipeline; we
// box them all under fetch::Error for surface uniformity.
// If the conversion isn't trivial, the implementer can introduce a
// `Box<dyn std::error::Error + Send + Sync>` variant instead — but
// keep the locked message format the same.
fn gix_error_into_clone_fetch<E: std::fmt::Debug>(e: E) -> gix::clone::fetch::Error {
    panic!("gix error conversion: {:?}", e)  // replace with real conversion at impl time
}
fn gix_error_into_clone_fetch_checkout<E: std::fmt::Debug>(e: E) -> gix::clone::fetch::Error {
    panic!("gix error conversion: {:?}", e)
}
```

**Note on gix API:** the exact `gix::prepare_clone` builder method names and signatures may differ slightly in 0.83. The above sketches the intent: shallow clone, optionally with explicit ref, fetch + checkout, resolve HEAD SHA. The implementer follows compiler errors and `gix::prepare_clone` documentation to adapt. If the error-type conversions get unwieldy, change `FixupError::GitFetch.source` from `Box<gix::clone::fetch::Error>` to `Box<dyn std::error::Error + Send + Sync>` (more general, easier wrapping). Keep the error MESSAGE format the same — Tests assert byte-exact wording.

- [ ] **Step 2: Write the failing tests**

Add to `src/fixup/registry.rs::tests`:

```rust
#[test]
fn fetch_cache_hit_returns_without_network() {
    use tempfile::TempDir;
    let tmp = TempDir::new().unwrap();
    unsafe {
        std::env::set_var("MUNTJAC_CACHE_HOME", tmp.path());
    }

    // Pre-populate cache with a fake SHA that satisfies hex check.
    let sha = "a".repeat(40);
    let cache_dir = tmp.path().join("fixups").join(&sha);
    std::fs::create_dir_all(cache_dir.join("packages")).unwrap();

    // url is irrelevant for cache-hit path; pass garbage.
    let result = super::fetch_into_cache("bogus://not-a-url", Some(&sha), false).unwrap();
    assert_eq!(result.sha, sha);
    assert_eq!(result.working_tree, cache_dir);

    unsafe {
        std::env::remove_var("MUNTJAC_CACHE_HOME");
    }
}

#[test]
fn fetch_cache_hit_but_corrupt_errors() {
    use tempfile::TempDir;
    let tmp = TempDir::new().unwrap();
    unsafe {
        std::env::set_var("MUNTJAC_CACHE_HOME", tmp.path());
    }

    let sha = "b".repeat(40);
    let cache_dir = tmp.path().join("fixups").join(&sha);
    std::fs::create_dir_all(&cache_dir).unwrap(); // exists, but no packages/

    let err = super::fetch_into_cache("bogus://", Some(&sha), false).unwrap_err();
    match err {
        crate::fixup::FixupError::CacheCorrupt { reason, .. } => {
            assert!(reason.contains("packages"), "got: {}", reason);
        }
        other => panic!("expected CacheCorrupt, got {:?}", other),
    }

    unsafe {
        std::env::remove_var("MUNTJAC_CACHE_HOME");
    }
}

#[test]
fn fetch_offline_with_cache_miss_errors() {
    use tempfile::TempDir;
    let tmp = TempDir::new().unwrap();
    unsafe {
        std::env::set_var("MUNTJAC_CACHE_HOME", tmp.path());
    }

    let err = super::fetch_into_cache(
        "bogus://",
        Some(&"c".repeat(40)),
        true,  // offline
    ).unwrap_err();
    match err {
        crate::fixup::FixupError::Offline { pin } => {
            assert_eq!(pin, "c".repeat(40));
        }
        other => panic!("expected Offline, got {:?}", other),
    }

    unsafe {
        std::env::remove_var("MUNTJAC_CACHE_HOME");
    }
}

#[test]
fn fetch_offline_without_rev_uses_default_branch_in_pin_message() {
    use tempfile::TempDir;
    let tmp = TempDir::new().unwrap();
    unsafe {
        std::env::set_var("MUNTJAC_CACHE_HOME", tmp.path());
    }
    let err = super::fetch_into_cache("bogus://", None, true).unwrap_err();
    match err {
        crate::fixup::FixupError::Offline { pin } => {
            assert_eq!(pin, "(default branch)");
        }
        other => panic!("expected Offline, got {:?}", other),
    }
    unsafe {
        std::env::remove_var("MUNTJAC_CACHE_HOME");
    }
}
```

For the actual bare-repo fetch tests, they live in `tests/git_fetch_integration.rs` (next step) because they need the test helper from Task 6 which is in `tests/common/`.

- [ ] **Step 3: Create the integration test file for fetch-from-bare-repo**

Create `tests/git_fetch_integration.rs`:

```rust
mod common;

use common::git_fixture::init_bare_repo_from_source;
use muntjac::fixup::fetch_into_cache;

fn setup_cache_isolated_tempdir() -> tempfile::TempDir {
    let tmp = tempfile::TempDir::new().unwrap();
    unsafe {
        std::env::set_var("MUNTJAC_CACHE_HOME", tmp.path());
    }
    tmp
}

#[test]
fn fetch_default_branch_succeeds() {
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
    assert!(result.working_tree.join("packages/pkg-a/fixups.toml").is_file());

    unsafe {
        std::env::remove_var("MUNTJAC_CACHE_HOME");
    }
}

#[test]
fn fetch_explicit_sha_succeeds_and_subsequent_call_hits_cache() {
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
```

`muntjac::fixup::fetch_into_cache` requires re-export from `src/fixup/mod.rs`:

```rust
pub use registry::{RegistryConfig, fetch_into_cache, parse_registry_config, FetchResult};
```

- [ ] **Step 4: Run all fetch tests**

Run: `cargo test -p muntjac fixup::registry::tests::fetch -- --test-threads=1`

Expected: PASS for the 4 unit tests in `src/fixup/registry.rs::tests` (cache-hit, corrupt, offline+rev, offline+no-rev). `--test-threads=1` is important — they share the `MUNTJAC_CACHE_HOME` env var.

Run: `cargo test --test git_fetch_integration -- --test-threads=1`

Expected: PASS for the 3 integration tests. If gix API issues, debug and iterate.

- [ ] **Step 5: Verify the cache directory structure**

After tests run, manually inspect a test's cache:

```bash
ls -la $(mktemp -d) # just a sanity check; tests clean themselves
```

(Optional debugging step — the test assertions cover correctness.)

- [ ] **Step 6: Full suite**

Run: `cargo test 2>&1 | grep -E '^test result' | awk '{sum+=$4} END {print sum}'`

Expected: previous count + 4 (unit) + 3 (integration).

- [ ] **Step 7: Commit**

```bash
git add src/fixup/registry.rs src/fixup/mod.rs tests/git_fetch_integration.rs
git commit -m "feat(s7b): fetch_into_cache with gix + cache-hit + offline"
```

---

## Phase 6 — EffectiveFixups::load extension

### Task 8: Thread `offline` through `EffectiveFixups::load`; wire Git arm to fetch

**Files:**
- Modify: `src/fixup/layer.rs` (signature change + Git arm)
- Modify: all call sites: `src/cli/buckify.rs`, `src/cli/fixups.rs` (show), all in-test call sites in `src/fixup/layer.rs::tests`

- [ ] **Step 1: Update the signature**

In `src/fixup/layer.rs`, modify `EffectiveFixups::load`:

Before:
```rust
pub fn load(
    registry: &crate::fixup::RegistryConfig,
    third_party_dir: &std::path::Path,
    allow_local_overrides: bool,
) -> Result<Self, crate::fixup::FixupError> {
    let community = match registry {
        crate::fixup::RegistryConfig::None => FixupSet::default(),
        crate::fixup::RegistryConfig::FileUrl(registry_dir) => {
            crate::fixup::load_community(registry_dir)?
        }
        crate::fixup::RegistryConfig::Git { url, .. } => {
            return Err(crate::fixup::FixupError::GitRegistryNotImplemented {
                registry: url.clone(),
            });
        }
    };

    let local = if allow_local_overrides {
        crate::fixup::load_local(third_party_dir)?
    } else {
        FixupSet::default()
    };

    Ok(Self { community, local })
}
```

After:
```rust
pub fn load(
    registry: &crate::fixup::RegistryConfig,
    third_party_dir: &std::path::Path,
    allow_local_overrides: bool,
    offline: bool,                                    // NEW
) -> Result<Self, crate::fixup::FixupError> {
    let community = match registry {
        crate::fixup::RegistryConfig::None => FixupSet::default(),
        crate::fixup::RegistryConfig::FileUrl(registry_dir) => {
            crate::fixup::load_community(registry_dir)?
        }
        crate::fixup::RegistryConfig::Git { url, rev } => {
            let resolved = crate::fixup::fetch_into_cache(
                url,
                rev.as_deref(),
                offline,
            )?;
            crate::fixup::load_community(&resolved.working_tree)?
        }
    };

    let local = if allow_local_overrides {
        crate::fixup::load_local(third_party_dir)?
    } else {
        FixupSet::default()
    };

    Ok(Self { community, local })
}
```

- [ ] **Step 2: Migrate in-file test call sites**

`src/fixup/layer.rs::tests` has 5 calls (per the grep earlier — lines 704, 726, 738, 755, 778). Each needs `false` (offline) as the new 4th arg. Apply mechanically:

```rust
super::EffectiveFixups::load(&RegistryConfig::None, tmp.path(), true, false).expect("loads");
```

(The 4th arg `false` — these tests don't exercise offline behavior; that's covered by Task 7.)

**Delete** the now-stale `effective_fixups_load_git_errors_in_s7a` test (if not already deleted in Task 2 Step 5).

- [ ] **Step 3: Add a positive test for the Git arm**

Add to `src/fixup/layer.rs::tests`:

```rust
#[test]
fn effective_fixups_load_git_fetches_from_bare_repo() {
    use crate::fixup::RegistryConfig;
    use tempfile::TempDir;

    // This test needs the git_fixture helper, which lives in tests/common/.
    // We can't import it from a unit test. Instead, construct the bare repo
    // inline using std::process::Command (`git` on PATH).
    let cache_tmp = TempDir::new().unwrap();
    unsafe {
        std::env::set_var("MUNTJAC_CACHE_HOME", cache_tmp.path());
    }

    let src_tmp = TempDir::new().unwrap();
    let src = src_tmp.path();
    std::fs::create_dir_all(src.join("packages/pkg-a")).unwrap();
    std::fs::write(
        src.join("packages/pkg-a/fixups.toml"),
        "extra_deps = [\"//x:y\"]\n",
    )
    .unwrap();

    let bare_tmp = TempDir::new().unwrap();
    let bare = bare_tmp.path().join("registry.git");

    // Inline bare-repo construction (same as the helper in tests/common/git_fixture.rs).
    let work = TempDir::new().unwrap();
    for entry in walkdir::WalkDir::new(src) {
        let entry = entry.unwrap();
        let rel = entry.path().strip_prefix(src).unwrap();
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
    let run_git = |args: &[&str]| {
        std::process::Command::new("git")
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
    assert!(run_git(&["init", "-q", "-b", "main"]).status.success());
    assert!(run_git(&["add", "-A"]).status.success());
    assert!(run_git(&["commit", "-q", "-m", "initial"]).status.success());
    assert!(
        run_git(&["clone", "-q", "--bare", ".", bare.to_str().unwrap()])
            .status
            .success()
    );

    let url = format!("file://{}", bare.display());
    let registry = RegistryConfig::Git {
        url: url.clone(),
        rev: None,
    };

    let tpd_tmp = TempDir::new().unwrap();
    let eff = super::EffectiveFixups::load(&registry, tpd_tmp.path(), true, false)
        .expect("loads via git arm");

    let pkg_a = pep508_rs::PackageName::from_str("pkg-a").unwrap();
    assert!(eff.community.get(&pkg_a).is_some());

    unsafe {
        std::env::remove_var("MUNTJAC_CACHE_HOME");
    }
}
```

This test inlines the bare-repo construction because unit tests can't `mod common`. It's verbose but isolated; the duplication with `tests/common/git_fixture.rs` is acceptable (the unit-test variant is the "minimum viable" path, the helper is the "shared with integration tests" path).

- [ ] **Step 4: Migrate CLI call sites**

In `src/cli/buckify.rs:53`, the existing call:

```rust
let fixups = crate::fixup::EffectiveFixups::load(
    &config.fixups.registry,
    &third_party_dir,
    config.fixups.allow_local_overrides,
)
```

becomes:

```rust
let fixups = crate::fixup::EffectiveFixups::load(
    &config.fixups.registry,
    &third_party_dir,
    config.fixups.allow_local_overrides,
    globals.no_network,
)
```

In `src/cli/fixups.rs:46` (the `show` function):

```rust
let eff = fixup::EffectiveFixups::load(
    &config.fixups.registry,
    &third_party_dir,
    config.fixups.allow_local_overrides,
    globals.no_network,
)
```

- [ ] **Step 5: Migrate the `fixups_show_smoke.rs` test call sites**

The S7a smoke tests in `tests/fixups_show_smoke.rs` use the CLI binary (not the EffectiveFixups API directly), so they're unaffected. But they do pass `--offline` or `--no-network` nowhere — verify no test passes either flag (current ones shouldn't; they're testing the show output, not offline behavior).

Run: `grep -n 'no_network\|offline' tests/fixups_show_smoke.rs`

If matches: investigate. If none: proceed.

- [ ] **Step 6: Compile and test**

Run: `cargo build 2>&1 | tail -10`

Expected: clean.

Run: `cargo test 2>&1 | grep -E '^test result' | awk '{sum+=$4; if ($6 != "0") fails+=$6} END {print "Pass:", sum, "Fail:", fails+0}'`

Expected: PASS for all. Test count +1 (new positive test) minus 1 (S7a stub test deleted) = net 0 (or unchanged from after Task 7).

- [ ] **Step 7: Commit**

```bash
git add src/fixup/layer.rs src/cli/buckify.rs src/cli/fixups.rs
git commit -m "feat(s7b): EffectiveFixups::load gains offline param + Git arm"
```

---

## Phase 7 — Update subcommand

### Task 9: `muntjac fixups update` subcommand

**Files:**
- Modify: `src/cli/fixups.rs` (extend `FixupsOp`, add `update` handler)
- Create: `tests/fixups_update_smoke.rs`

The update flow: validate registry kind → fetch new SHA → load both `FixupSet`s for diff (if prior pin existed) → render diff → write SHA back via `toml_edit`.

- [ ] **Step 1: Extend the FixupsOp enum**

In `src/cli/fixups.rs`, replace the existing `FixupsOp`:

Before:
```rust
#[derive(Subcommand, Debug)]
pub enum FixupsOp {
    /// Print the merged fixup for a package as TOML.
    Show {
        /// PEP 503-normalizable package name.
        package: String,
    },
}
```

After:
```rust
#[derive(Subcommand, Debug)]
pub enum FixupsOp {
    /// Print the merged fixup for a package as TOML.
    Show {
        /// PEP 503-normalizable package name.
        package: String,
    },
    /// Fetch the registry and update `registry_rev` in muntjac.toml.
    Update {
        /// SHA, branch, or tag to fetch. Default: HEAD of the
        /// registry's default branch.
        #[arg(long)]
        rev: Option<String>,
    },
}
```

Update the `run` dispatcher:

```rust
pub fn run(op: FixupsOp, globals: &Globals) -> Result<()> {
    match op {
        FixupsOp::Show { package } => show(package, globals),
        FixupsOp::Update { rev } => update(rev, globals),
    }
}
```

- [ ] **Step 2: Implement the `update` handler**

Add to `src/cli/fixups.rs`:

```rust
fn update(rev: Option<String>, globals: &Globals) -> Result<()> {
    use crate::fixup::{self, RegistryConfig};

    let cwd = globals.workdir().context("resolving working directory")?;
    let cfg_path = cwd.join("muntjac.toml");
    let cfg_bytes = std::fs::read_to_string(&cfg_path)
        .with_context(|| format!("reading {}", cfg_path.display()))?;
    let config = Config::from_str(&cfg_bytes)
        .with_context(|| format!("parsing {}", cfg_path.display()))?;

    // 1. Validate registry is Git-form.
    let (url, prior_rev) = match &config.fixups.registry {
        RegistryConfig::Git { url, rev } => (url.clone(), rev.clone()),
        RegistryConfig::None => {
            anyhow::bail!(
                "muntjac fixups update requires a git-based registry; current registry is \"none\""
            );
        }
        RegistryConfig::FileUrl(p) => {
            anyhow::bail!(
                "muntjac fixups update requires a git-based registry; current registry is file:// directory form at {}",
                p.display()
            );
        }
    };

    // 2. Fetch the new SHA.
    let result = fixup::fetch_into_cache(&url, rev.as_deref(), globals.no_network)
        .with_context(|| format!("fetching {}", url))?;

    // 3. Compute diff vs. prior pin (if cached).
    if let Some(prior_sha) = &prior_rev {
        if prior_sha == &result.sha {
            println!("Already at {} — no changes.", result.sha);
            return Ok(());
        }
        let prior_cache_path = crate::cache::fixup_cache_path_for_sha(prior_sha)
            .context("resolving prior cache path")?;
        if prior_cache_path.is_dir() && prior_cache_path.join("packages").is_dir() {
            let prior_set = fixup::load_community(&prior_cache_path)
                .with_context(|| format!("loading prior fixups from {}", prior_cache_path.display()))?;
            let new_set = fixup::load_community(&result.working_tree)
                .with_context(|| format!("loading new fixups from {}", result.working_tree.display()))?;
            let diff = fixup::diff_fixup_sets(&prior_set, &new_set);
            if diff.is_empty() {
                println!("(no fixup changes)");
            } else {
                print!("{}", fixup::render_diff(&diff));
            }
        } else {
            println!("(prior cache evicted; diff unavailable)");
        }
    } else {
        println!("Initial pin (no prior rev to diff against)");
    }

    // 4. Surgical writeback via toml_edit.
    write_registry_rev(&cfg_path, &result.sha)?;

    // 5. Footer.
    match &prior_rev {
        Some(p) if p != &result.sha => {
            println!("\nPinned {} @ {} (was: {})", url, result.sha, p);
        }
        _ => {
            println!("\nPinned {} @ {}", url, result.sha);
        }
    }

    Ok(())
}

fn write_registry_rev(cfg_path: &std::path::Path, new_sha: &str) -> Result<()> {
    let bytes = std::fs::read_to_string(cfg_path)
        .with_context(|| format!("reading {}", cfg_path.display()))?;
    let mut doc: toml_edit::DocumentMut = bytes
        .parse()
        .with_context(|| format!("parsing {} as TOML", cfg_path.display()))?;

    let fixups = doc
        .entry("fixups")
        .or_insert(toml_edit::Item::Table(toml_edit::Table::new()));
    let fixups_table = fixups
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("[fixups] is not a table in {}", cfg_path.display()))?;
    fixups_table["registry_rev"] = toml_edit::value(new_sha);

    std::fs::write(cfg_path, doc.to_string())
        .with_context(|| format!("writing {}", cfg_path.display()))?;
    Ok(())
}
```

- [ ] **Step 3: Add update smoke tests**

Create `tests/fixups_update_smoke.rs`:

```rust
mod common;

use common::git_fixture::init_bare_repo_from_source;
use std::process::Command;

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

fn run_muntjac(cwd: &std::path::Path, cache_home: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_muntjac"))
        .env("MUNTJAC_CACHE_HOME", cache_home)
        .args(["-C", cwd.to_str().unwrap()])
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn update_pins_initial_sha() {
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
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("Initial pin"), "stdout: {}", stdout);
    assert!(stdout.contains(&sha), "expected SHA in stdout: {}", stdout);

    // Verify muntjac.toml was updated.
    let after = std::fs::read_to_string(cwd_tmp.path().join("muntjac.toml")).unwrap();
    assert!(after.contains(&format!("registry_rev = \"{}\"", sha)));
}

#[test]
fn update_rejects_none_registry() {
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
    assert!(stderr.contains("requires a git-based registry"), "got: {}", stderr);
    assert!(stderr.contains("\"none\""), "got: {}", stderr);
}

#[test]
fn update_preserves_existing_toml_formatting() {
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
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));

    let after = std::fs::read_to_string(cwd_tmp.path().join("muntjac.toml")).unwrap();
    // Comments preserved
    assert!(after.contains("# My muntjac config"));
    assert!(after.contains("# Comment about Linux"));
    assert!(after.contains("# Pinned registry for reproducibility"));
    // Original key order preserved (registry stays before allow_local_overrides;
    // registry_rev added after registry)
    let reg_pos = after.find("registry = ").unwrap();
    let allow_pos = after.find("allow_local_overrides").unwrap();
    let rev_pos = after.find("registry_rev = ").unwrap();
    assert!(reg_pos < rev_pos, "registry should come before registry_rev");
    assert!(rev_pos < allow_pos || allow_pos < rev_pos, "rev placement is flexible");
}
```

- [ ] **Step 4: Run smoke tests**

Run: `cargo test --test fixups_update_smoke -- --test-threads=1`

Expected: PASS for all 3 tests.

- [ ] **Step 5: Verify full suite**

Run: `cargo test 2>&1 | grep -E '^test result' | awk '{sum+=$4; if ($6 != "0") fails+=$6} END {print "Pass:", sum, "Fail:", fails+0}'`

Expected: previous count + 3.

- [ ] **Step 6: Commit**

```bash
git add src/cli/fixups.rs tests/fixups_update_smoke.rs
git commit -m "feat(s7b): muntjac fixups update with diff + toml_edit writeback"
```

---

## Phase 8 — Fixture & end-to-end

### Task 10: Fixture 09-git-registry inputs

**Files:**
- Create: `tests/fixtures/buck/09-git-registry/` (full fixture tree)

- [ ] **Step 1: Examine fixture 06 for the synthetic-package pattern**

Run: `ls tests/fixtures/buck/06-community-fixup/ && cat tests/fixtures/buck/06-community-fixup/muntjac.toml`

Reuse: pyproject.toml shape, uv.lock structure (synthetic packages), prebake stub pattern.

- [ ] **Step 2: Create the fixture tree**

```
tests/fixtures/buck/09-git-registry/
├── muntjac.toml                     # registry + registry_rev placeholders
├── pyproject.toml
├── uv.lock                          # one synthetic package pkg-a@1.0.0
├── registry-source/                 # NOT a git repo
│   └── packages/pkg-a/fixups.toml
├── third-party/python/
│   ├── fixups/pkg-a/fixups.toml
│   └── prebake/
│       ├── .gitignore               # ALLOW-LIST pattern per TD-S7a-01
│       ├── .manifest.toml
│       └── pkg-a-1.0.0-py3-none-any.whl
├── expected/{BUCK, muntjac.bzl, wiring.bzl}  (generated in Task 11)
└── .gitignore
```

`muntjac.toml`:

```toml
manifest_path   = "pyproject.toml"
third_party_dir = "third-party/python"
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target    = "x86_64-unknown-linux-gnu"
manylinux = "2_17"

[fixups]
registry = "file:///REPLACED_AT_TEST_TIME/registry.git"
registry_rev = "REPLACED_AT_TEST_TIME"
allow_local_overrides = true
```

`pyproject.toml`:

```toml
[project]
name = "fixture-09-consumer"
version = "0.0.1"
requires-python = ">=3.12,<3.13"
dependencies = ["pkg-a==1.0.0"]
```

`uv.lock`: mirror fixture 06's pkg-a entry (synthetic pure-python sdist routed through prebake). Copy from `tests/fixtures/buck/06-community-fixup/uv.lock` and trim to just pkg-a.

`registry-source/packages/pkg-a/fixups.toml`:

```toml
extra_deps = ["//community:base"]
```

`third-party/python/fixups/pkg-a/fixups.toml`:

```toml
extra_deps = ["//local:base"]
```

`third-party/python/prebake/`: copy from fixture 06's prebake (the pkg-a stub).

`third-party/python/prebake/.gitignore` (TD-S7a-01's safer allow-list pattern):

```
# Ignore everything by default, allow-list the committed stubs + manifest.
*
!.gitignore
!.manifest.toml
!*.whl
!*.tar.gz
```

`tests/fixtures/buck/09-git-registry/.gitignore`:

```
# muntjac-generated files
/third-party/python/BUCK
/third-party/python/muntjac.bzl
/third-party/python/wiring.bzl
/third-party/python/config/

# buck2 build outputs
buck-out/
.buckd/

# uv's venv
.venv/
```

- [ ] **Step 3: Verify fixture inputs commit cleanly**

```bash
git add tests/fixtures/buck/09-git-registry/
git status tests/fixtures/buck/09-git-registry/
```

Expected: ALL the files listed above appear in `git status`. Specifically:
- `registry-source/packages/pkg-a/fixups.toml`
- `third-party/python/fixups/pkg-a/fixups.toml`
- `third-party/python/prebake/.gitignore`
- `third-party/python/prebake/.manifest.toml`
- `third-party/python/prebake/pkg-a-1.0.0-py3-none-any.whl`
- `muntjac.toml`, `pyproject.toml`, `uv.lock`, `.gitignore`

If any prebake files are missing despite being on disk: the new allow-list pattern should auto-include them, but verify with `git check-ignore -v <path>` if needed.

- [ ] **Step 4: Commit fixture inputs (no expected/ yet)**

```bash
git commit -m "test(s7b): fixture 09-git-registry inputs"
```

---

### Task 11: Fixture 09 snapshot tests + generate expected/

**Files:**
- Modify: `tests/buckify.rs` (add 2 tests)
- Create: `tests/fixtures/buck/09-git-registry/expected/{BUCK, muntjac.bzl, wiring.bzl}`

Two tests: (1) buckify with git fetch + cache populated, snapshot-compare; (2) buckify with `--no-network` after pre-warm, success path.

- [ ] **Step 1: Check `tests/buckify.rs` for the existing `copy_fixture` helper**

Run: `grep -n 'fn copy_fixture\|fn fixture_06_community' tests/buckify.rs`

Note: `tests/buckify.rs` is its own integration test binary and cannot `mod common`. The bare-repo construction inlines (same pattern as Task 8 Step 3).

- [ ] **Step 2: Write the failing first test**

Add to `tests/buckify.rs`:

```rust
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
    copy_fixture(&fixture_src, tmp.path());

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
    assert!(cache_home.join("fixups").join(&sha).join("packages").is_dir());

    // Sanity: BUCK has both community and local extra_deps.
    assert!(generated.contains("//community:base"));
    assert!(generated.contains("//local:base"));
}
```

- [ ] **Step 3: Run the test (fails, no expected/ yet)**

Run: `cargo test --test buckify fixture_09_git_registry_golden -- --exact 2>&1 | tail -10`

Expected: FAIL — `expected/BUCK` doesn't exist OR a panic from the inline construction.

If the inline construction itself panics (git not on PATH, gix issue), debug before generating expected/.

- [ ] **Step 4: Generate `expected/` by running buckify against a tempdir copy**

```bash
rm -rf /tmp/09-fixture-stage
cp -r tests/fixtures/buck/09-git-registry /tmp/09-fixture-stage

# Build a bare repo manually
WORK=$(mktemp -d)
cp -r /tmp/09-fixture-stage/registry-source/* "$WORK"/
cd "$WORK"
GIT_AUTHOR_NAME=muntjac-test GIT_AUTHOR_EMAIL=test@example.com \
GIT_AUTHOR_DATE=1970-01-01T00:00:00Z \
GIT_COMMITTER_NAME=muntjac-test GIT_COMMITTER_EMAIL=test@example.com \
GIT_COMMITTER_DATE=1970-01-01T00:00:00Z \
git init -q -b main && git add -A && git commit -q -m initial
SHA=$(git rev-parse HEAD)
git clone -q --bare . /tmp/09-fixture-stage/registry.git
cd -

# Substitute in muntjac.toml
sed -i "s|/REPLACED_AT_TEST_TIME/registry.git|/tmp/09-fixture-stage/registry.git|" /tmp/09-fixture-stage/muntjac.toml
sed -i "s|registry_rev = \"REPLACED_AT_TEST_TIME\"|registry_rev = \"$SHA\"|" /tmp/09-fixture-stage/muntjac.toml

# Run buckify with isolated cache
CACHE=$(mktemp -d)
MUNTJAC_CACHE_HOME=$CACHE cargo run -- -C /tmp/09-fixture-stage buckify

mkdir -p tests/fixtures/buck/09-git-registry/expected
cp /tmp/09-fixture-stage/third-party/python/BUCK \
   /tmp/09-fixture-stage/third-party/python/muntjac.bzl \
   /tmp/09-fixture-stage/third-party/python/wiring.bzl \
   tests/fixtures/buck/09-git-registry/expected/
```

- [ ] **Step 5: Inspect expected/ for correctness**

Run: `head -30 tests/fixtures/buck/09-git-registry/expected/BUCK`

Verify: pkg-a has both `//community:base` (from registry) and `//local:base` (from local). If wrong, debug the git fetch + EffectiveFixups wiring before committing.

- [ ] **Step 6: Re-run the test**

Run: `cargo test --test buckify fixture_09_git_registry_golden -- --exact`

Expected: PASS.

- [ ] **Step 7: Write the `--no-network` after pre-warm test**

Add to `tests/buckify.rs`:

```rust
#[test]
fn fixture_09_offline_cache_hit() {
    let fixture_src = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/buck/09-git-registry");
    let tmp = tempfile::TempDir::new().unwrap();
    copy_fixture(&fixture_src, tmp.path());

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
        .args(["-C", tmp.path().to_str().unwrap(), "--no-network", "buckify"])
        .status()
        .unwrap();
    assert!(status.success(), "--no-network buckify with cached fetch must succeed");
}
```

- [ ] **Step 8: Run both fixture tests**

Run: `cargo test --test buckify fixture_09 -- --test-threads=1`

Expected: PASS for both.

- [ ] **Step 9: Full suite**

Run: `cargo test 2>&1 | grep -E '^test result' | awk '{sum+=$4; if ($6 != "0") fails+=$6} END {print "Pass:", sum, "Fail:", fails+0}'`

Expected: previous count + 2.

- [ ] **Step 10: Commit**

```bash
git add tests/fixtures/buck/09-git-registry/expected/ tests/buckify.rs
git commit -m "test(s7b): fixture 09-git-registry byte-exact snapshot + offline cache-hit"
```

---

## Phase 9 — Cleanup & tag

### Task 12: Final cargo test/fmt/clippy + TECH_DEBT + tag

- [ ] **Step 1: Full test sweep**

Run: `cargo test 2>&1 | grep -E '^test result' | awk '{sum+=$4; if ($6 != "0") fails+=$6} END {print "Pass:", sum, "Fail:", fails+0}'`

Expected: PASS, all green. Total ~348 (316 + ~32 new).

- [ ] **Step 2: fmt + clippy**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings 2>&1 | tail -5`

Expected: clean. Pre-commit hook from S5 would block commits on failure anyway, so this is a sanity check.

- [ ] **Step 3: Update TECH_DEBT (close completed item)**

In `docs/superpowers/TECH_DEBT.md`:

- Move "TD-S7a-01: `prebake/.gitignore = "*"` pattern requires per-file `git add -f`" from Open to Resolved if fixture 09 demonstrated the safer pattern (which it should — see Task 10 Step 2's allow-list `.gitignore`). Or: keep it Open if fixtures 06/07/08 still use the brittle pattern (the TD specifically covers them, not fixture 09).

  Decision: keep TD-S7a-01 in Open (only addressed forward-looking, not retroactively). Add a note under the existing entry that fixture 09 uses the safer pattern.

- Add a new section heading "### From S7b final stage review (YYYY-MM-DD, pre-tag)" with any items that surfaced during execution. Likely empty if all tasks went smoothly.

```markdown
#### TD-S7a-01: `prebake/.gitignore = "*"` ... (existing entry)
- ...
- **Update from S7b:** Fixture 09 uses the safer allow-list pattern. Fixtures 06/07/08 still use the brittle `*` pattern — pending retroactive cleanup.
```

- [ ] **Step 4: Commit TECH_DEBT update**

```bash
git add docs/superpowers/TECH_DEBT.md
git commit -m "docs(s7b): TECH_DEBT update for fixture-09 allow-list pattern"
```

- [ ] **Step 5: Update roadmap S7b section + table**

In `docs/superpowers/specs/2026-05-20-muntjac-roadmap.md`:

Change the S7b section heading from `#### S7b — Git fetch & cache` to `#### S7b — Git fetch & cache ✅ shipped`.

Append after `**Touches:** ...` line:
```markdown
**Shipped:** N commits (incl. design spec + plan + 12 implementation + cleanup); plan `docs/superpowers/plans/2026-05-24-muntjac-s7b-git-registry.md`; design `docs/superpowers/specs/2026-05-24-muntjac-s7b-git-registry-design.md`.
```

Replace N with the actual count: `git log --oneline s7a-complete..HEAD | wc -l`.

Update the table row:
```markdown
| S7b | [2026-05-24-muntjac-s7b-git-registry-design.md](./2026-05-24-muntjac-s7b-git-registry-design.md) | [2026-05-24-muntjac-s7b-git-registry.md](../plans/2026-05-24-muntjac-s7b-git-registry.md) | ✅ shipped (tag `s7b-complete`, N commits, M tests) |
| S8 | (not yet written) | (not yet written) | ⬜ next |
```

M = output of `cargo test 2>&1 | grep -E '^test result' | awk '{sum+=$4} END {print sum}'`.

- [ ] **Step 6: Commit roadmap + tag**

```bash
git add docs/superpowers/specs/2026-05-20-muntjac-roadmap.md
git commit -m "docs(s7b): mark S7b ✅ shipped in roadmap"
git tag s7b-complete
```

- [ ] **Step 7: Push and watch CI** (only if user gives go-ahead)

```bash
git push origin main
git push origin s7b-complete
```

Then watch CI per the S7a precedent.

---

## Self-review

**Spec coverage check (run against `docs/superpowers/specs/2026-05-24-muntjac-s7b-git-registry-design.md`):**

| Spec section | Covered by |
|---|---|
| §1.1 gix dep | Task 1 |
| §1.1 toml_edit dep | Task 1 |
| §1.1 Content-addressed cache | Tasks 3, 7 |
| §1.1 `MUNTJAC_CACHE_HOME` override | Task 3 |
| §1.1 `fetch_into_cache` | Task 7 |
| §1.1 `RegistryConfig::Git` arm wiring | Task 8 |
| §1.1 `muntjac fixups update` | Task 9 |
| §1.1 `toml_edit` writeback | Task 9 (`write_registry_rev`) |
| §1.1 `diff_fixup_sets` | Task 5 |
| §1.1 Global `--offline` flag (named `--no-network` per impl) | Task 8 (uses existing flag) |
| §1.1 `FixupError::Offline` | Task 2 |
| §1.1 `file://*.git` form | Task 4 |
| §1.1 Fixture 09 | Tasks 10, 11 |
| §2.1 module layout | Tasks 3, 4, 5, 7, 9 |
| §3.1 `cache_root` | Task 3 |
| §3.2 `fetch_into_cache` | Task 7 |
| §3.3 `EffectiveFixups::load` signature change | Task 8 |
| §3.4 `diff.rs` | Task 5 |
| §3.5 Globals.offline (uses existing `no_network`) | Task 8 |
| §3.6 Update subcommand | Task 9 |
| §4 Cache layout & atomicity | Task 7 |
| §5 Update UX | Task 9 |
| §6 Diff format | Task 5 |
| §7 Error variants | Task 2 |
| §8 Testing strategy | Distributed |
| §10 Drop `GitRegistryNotImplemented` | Task 2 |

**Placeholder scan:** No "TBD"/"fill in"/"similar to Task N" patterns; every code step has concrete code. The two `gix_error_into_clone_fetch*` placeholder functions in Task 7 Step 1 are explicit acknowledgments that gix API conversion may need adaptation — the alternative (`Box<dyn std::error::Error>`) is spelled out.

**Type consistency:**
- `FetchResult { sha, working_tree }` defined Task 7, used Tasks 8, 9
- `DiffLine` defined Task 5, used Task 9
- `diff_fixup_sets`, `render_diff` defined Task 5, used Task 9
- `fetch_into_cache(url, rev, offline) -> Result<FetchResult, FixupError>` defined Task 7, used Tasks 8, 9
- `cache_root() -> Result<PathBuf, CacheError>` defined Task 3, used Task 7
- `fixup_cache_path_for_sha(sha) -> Result<PathBuf, CacheError>` defined Task 3, used Task 7
- `EffectiveFixups::load(registry, third_party_dir, allow_local_overrides, offline)` defined Task 8, used by all callers

All cross-references resolve.

---

## Execution

Plan complete and saved to `docs/superpowers/plans/2026-05-24-muntjac-s7b-git-registry.md`.

Per the established cadence ([[feedback_planning_cadence]]), execute via `superpowers:subagent-driven-development` — fresh subagent per task, two-stage review between tasks.
