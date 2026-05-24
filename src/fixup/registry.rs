//! `RegistryConfig` — typed parse of the `[fixups] registry = "…"` field.
//!
//! Three forms:
//!   - `"none"` — no community layer.
//!   - `"file:///abs/path"` — local checkout (S7a).
//!   - `"github.com/<owner>/<repo>"` — git registry (S7b; declared, errors on use).

use std::path::PathBuf;

/// Parsed registry configuration.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum RegistryConfig {
    /// No community registry; community layer is empty.
    #[default]
    None,
    /// Local checkout (path is absolute).
    FileUrl(PathBuf),
    /// Git-hosted registry. `EffectiveFixups::load` wires this up in S7b T8.
    Git { url: String, rev: Option<String> },
}

/// Parse the raw `registry` string (and optional `registry_rev`) from
/// `muntjac.toml` into a typed `RegistryConfig`.
///
/// Returns:
///   - `RegistryConfig::None`            on `"none"`
///   - `RegistryConfig::FileUrl(abs)`    on `"file:///abs/path"`
///   - `RegistryConfig::Git { url, rev}` on `"github.com/<owner>/<repo>"`
///   - `Err(BadRegistry)` for malformed strings.
///   - `Err(RegistryPathNotAbsolute)` for `"file://relative"` /  `"file://./x"`.
pub fn parse_registry_config(
    raw: &str,
    registry_rev: Option<&str>,
) -> Result<RegistryConfig, crate::error::ConfigError> {
    if raw == "none" {
        return Ok(RegistryConfig::None);
    }
    if let Some(rest) = raw.strip_prefix("file://") {
        // RFC 8089: an absolute path begins with `/`.
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
    if let Some(rest) = raw.strip_prefix("github.com/") {
        let parts: Vec<&str> = rest.split('/').collect();
        if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
            return Ok(RegistryConfig::Git {
                url: raw.to_string(),
                rev: registry_rev.map(|s| s.to_string()),
            });
        }
    }
    Err(crate::error::ConfigError::BadRegistry(raw.to_string()))
}

/// Resolved fetch result. The `working_tree` is suitable for
/// `load_community(...)`.
#[derive(Debug, Clone)]
pub struct FetchResult {
    /// Full 40-character hex SHA the fetch resolved to.
    pub sha: String,
    /// Absolute path to the cache directory containing the checkout
    /// (e.g. `<cache>/fixups/<sha>/`).
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
    if let Some(rev_str) = rev
        && is_full_hex_sha(rev_str)
    {
        let candidate =
            crate::cache::fixup_cache_path_for_sha(rev_str).map_err(|e| FixupError::Io {
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

    // 2. Offline: cache miss is fatal.
    if offline {
        let pin = rev
            .map(|s| s.to_string())
            .unwrap_or_else(|| "(default branch)".into());
        return Err(FixupError::Offline { pin });
    }

    // 3. Fetch path: clone into staging, then atomic rename.
    fetch_and_stage(url, rev)
}

fn is_full_hex_sha(s: &str) -> bool {
    s.len() == 40 && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// RAII guard that wipes a staging directory unless `disarm()` was called.
struct StagingGuard {
    path: Option<std::path::PathBuf>,
}

impl StagingGuard {
    fn new(path: std::path::PathBuf) -> Self {
        Self { path: Some(path) }
    }
    fn disarm(mut self) {
        self.path = None;
    }
}

impl Drop for StagingGuard {
    fn drop(&mut self) {
        if let Some(p) = self.path.take() {
            let _ = std::fs::remove_dir_all(&p);
        }
    }
}

fn fetch_and_stage(url: &str, rev: Option<&str>) -> Result<FetchResult, crate::fixup::FixupError> {
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

    // PrepareFetch::new requires the destination to be empty; do NOT pre-create.
    let guard = StagingGuard::new(staging.clone());

    // Build a clone preparation. We always clone with a working tree so we
    // can read packages/ off disk via `load_community`.
    let prepare = gix::prepare_clone(url, &staging).map_err(|e| FixupError::GitFetch {
        url: url.to_string(),
        rev: rev.map(|s| s.to_string()),
        source: Box::new(e),
    })?;

    // Apply explicit-ref if provided. We always pass it as a partial name and
    // let gix figure out branch vs tag vs SHA. If the rev is a full SHA, we
    // hit the cache-fast-path before reaching here on a re-fetch; on a first
    // fetch we still attempt to pass it through, but gix may reject raw SHAs
    // — in that case we just fall through and resolve the SHA from HEAD
    // after the clone.
    let prepare = match rev {
        Some(rev_str) if !is_full_hex_sha(rev_str) => prepare
            .with_ref_name(Some(rev_str))
            .map_err(|e| FixupError::GitFetch {
                url: url.to_string(),
                rev: rev.map(|s| s.to_string()),
                source: Box::new(e),
            })?,
        _ => prepare,
    };

    // fetch_then_checkout returns a PrepareCheckout we must consume.
    let mut prepare = prepare;
    let (mut checkout, _outcome) = prepare
        .fetch_then_checkout(gix::progress::Discard, &gix::interrupt::IS_INTERRUPTED)
        .map_err(|e| FixupError::GitFetch {
            url: url.to_string(),
            rev: rev.map(|s| s.to_string()),
            source: Box::new(e),
        })?;

    let (repo, _checkout_outcome) = checkout
        .main_worktree(gix::progress::Discard, &gix::interrupt::IS_INTERRUPTED)
        .map_err(|e| FixupError::GitFetch {
            url: url.to_string(),
            rev: rev.map(|s| s.to_string()),
            source: Box::new(e),
        })?;

    // Resolve HEAD to its full SHA. After fetch_then_checkout this is the
    // commit we checked out — either the named ref, or the remote's default
    // branch.
    let head_id = repo.head_id().map_err(|e| FixupError::GitFetch {
        url: url.to_string(),
        rev: rev.map(|s| s.to_string()),
        source: Box::new(e),
    })?;
    let resolved_sha = head_id.to_string();

    // Atomic rename to final destination.
    let final_dest = cache_root.join("fixups").join(&resolved_sha);

    if final_dest.is_dir() {
        // Race: another process beat us to it. Discard staging, return existing.
        // (guard's Drop will wipe staging.)
        return Ok(FetchResult {
            sha: resolved_sha,
            working_tree: final_dest,
        });
    }

    // Drop the gix Repository handle first; on Windows/macOS open file handles
    // could otherwise foil the rename. Cheap on Linux but harmless everywhere.
    drop(repo);

    std::fs::rename(&staging, &final_dest).map_err(|e| FixupError::Io {
        path: final_dest.clone(),
        source: e,
    })?;
    // Rename succeeded: don't wipe (the dir no longer exists at `staging`).
    guard.disarm();

    Ok(FetchResult {
        sha: resolved_sha,
        working_tree: final_dest,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialize tests that mutate MUNTJAC_CACHE_HOME — Rust unit tests run in parallel.
    static ENV_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn parses_none() {
        assert_eq!(
            parse_registry_config("none", None).unwrap(),
            RegistryConfig::None
        );
    }

    #[test]
    fn parses_file_url_abs() {
        let got = parse_registry_config("file:///abs/path/to/registry", None).unwrap();
        assert_eq!(
            got,
            RegistryConfig::FileUrl(PathBuf::from("/abs/path/to/registry"))
        );
    }

    #[test]
    fn rejects_file_url_relative() {
        let err = parse_registry_config("file://relative", None).unwrap_err();
        assert!(matches!(
            err,
            crate::error::ConfigError::RegistryPathNotAbsolute { .. }
        ));
    }

    #[test]
    fn rejects_file_url_dot_slash() {
        let err = parse_registry_config("file://./registry", None).unwrap_err();
        assert!(matches!(
            err,
            crate::error::ConfigError::RegistryPathNotAbsolute { .. }
        ));
    }

    #[test]
    fn parses_github_url_with_rev() {
        let got = parse_registry_config("github.com/jackmpcollins/muntjac-fixups", Some("abc123"))
            .unwrap();
        assert_eq!(
            got,
            RegistryConfig::Git {
                url: "github.com/jackmpcollins/muntjac-fixups".into(),
                rev: Some("abc123".into()),
            }
        );
    }

    #[test]
    fn parses_github_url_no_rev() {
        let got = parse_registry_config("github.com/o/r", None).unwrap();
        match got {
            RegistryConfig::Git { url, rev } => {
                assert_eq!(url, "github.com/o/r");
                assert_eq!(rev, None);
            }
            other => panic!("expected Git, got {:?}", other),
        }
    }

    #[test]
    fn rejects_malformed_github_url() {
        let err = parse_registry_config("github.com/onlyname", None).unwrap_err();
        assert!(matches!(err, crate::error::ConfigError::BadRegistry(_)));
    }

    #[test]
    fn rejects_bare_string() {
        let err = parse_registry_config("not-a-url", None).unwrap_err();
        assert!(matches!(err, crate::error::ConfigError::BadRegistry(_)));
    }

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

    #[test]
    fn fetch_cache_hit_returns_without_network() {
        let _g = ENV_GUARD.lock().unwrap();
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
        let _g = ENV_GUARD.lock().unwrap();
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
        let _g = ENV_GUARD.lock().unwrap();
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        unsafe {
            std::env::set_var("MUNTJAC_CACHE_HOME", tmp.path());
        }

        let err = super::fetch_into_cache(
            "bogus://",
            Some(&"c".repeat(40)),
            true, // offline
        )
        .unwrap_err();
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
        let _g = ENV_GUARD.lock().unwrap();
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
}
