//! Cache-root resolution.
//!
//! Precedence (highest first):
//!   - `$MUNTJAC_CACHE_HOME` (test isolation; also for users with
//!     non-XDG-friendly env)
//!   - `dirs::cache_dir()/muntjac` (XDG-compliant; `dirs` already honors
//!     `$XDG_CACHE_HOME` on Linux)

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
