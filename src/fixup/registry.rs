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

#[cfg(test)]
mod tests {
    use super::*;

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
}
