//! Parse `uv.lock` into `Lockfile`.

use std::str::FromStr;

use pep440_rs::Version;
use pep508_rs::PackageName;
use serde::Deserialize;
use url::Url;

use crate::error::LockfileError;
use crate::lock::types::*;

// ---- Raw wire-format types ----

#[derive(Debug, Deserialize)]
pub(crate) struct RawLockfile {
    version: u32,
    #[serde(default = "default_revision")]
    revision: u32,
    #[serde(rename = "requires-python", default)]
    requires_python: String,
    #[serde(default, rename = "package")]
    packages: Vec<RawPackage>,
}

fn default_revision() -> u32 {
    0
}

#[derive(Debug, Deserialize)]
struct RawPackage {
    name: String,
    version: String,
    source: RawSource,
    // Tasks 5-6 fill in:
    // #[serde(default)] dependencies: Vec<RawDep>,
    // #[serde(default)] sdist: Option<RawArtifact>,
    // #[serde(default)] wheels: Vec<RawArtifact>,
    // #[serde(default)] metadata: Option<RawMetadata>,
}

#[derive(Debug, Deserialize)]
struct RawSource {
    #[serde(default)]
    registry: Option<String>,
    #[serde(default)]
    git: Option<String>,
    #[serde(default)]
    rev: Option<String>,
    #[serde(default)]
    subdirectory: Option<String>,
    #[serde(default, rename = "virtual")]
    virtual_: Option<String>,
    #[serde(default)]
    editable: Option<String>,
    #[serde(default)]
    directory: Option<String>,
    #[serde(default)]
    path: Option<String>,
}

impl Lockfile {
    pub(crate) fn from_raw(raw: RawLockfile) -> Result<Self, LockfileError> {
        if raw.version != 1 {
            return Err(LockfileError::UnsupportedVersion(raw.version));
        }
        if raw.revision < 3 {
            eprintln!(
                "warning: uv.lock revision {} is older than the tested floor (3); proceeding",
                raw.revision
            );
        }
        let packages: Result<Vec<Package>, LockfileError> =
            raw.packages.into_iter().map(package_from_raw).collect();
        Ok(Lockfile {
            version: raw.version,
            revision: raw.revision,
            requires_python: raw.requires_python,
            packages: packages?,
        })
    }
}

fn package_from_raw(rp: RawPackage) -> Result<Package, LockfileError> {
    let name = PackageName::from_str(&rp.name)
        .map_err(|e| LockfileError::BadPackageName(rp.name.clone(), e.to_string()))?;
    let version = Version::from_str(&rp.version).map_err(|e| LockfileError::BadVersion {
        package: rp.name.clone(),
        value: rp.version.clone(),
        reason: e.to_string(),
    })?;
    let source = source_from_raw(&rp.name, rp.source)?;
    Ok(Package {
        name,
        version,
        source,
        dependencies: vec![], // Task 5
        sdist: None,          // Task 5
        wheels: vec![],       // Task 5
        metadata: None,       // Task 6
    })
}

fn source_from_raw(pkg_name: &str, rs: RawSource) -> Result<Source, LockfileError> {
    let mut found: Vec<&'static str> = Vec::new();
    if rs.registry.is_some() {
        found.push("registry");
    }
    if rs.git.is_some() {
        found.push("git");
    }
    if rs.virtual_.is_some() {
        found.push("virtual");
    }
    if rs.editable.is_some() {
        found.push("editable");
    }
    if rs.directory.is_some() {
        found.push("directory");
    }
    if rs.path.is_some() {
        found.push("path");
    }
    if found.len() != 1 {
        return Err(LockfileError::AmbiguousSource {
            package: pkg_name.into(),
            found,
        });
    }
    if let Some(url) = rs.registry {
        let parsed = Url::parse(&url).map_err(|e| LockfileError::BadVersion {
            package: pkg_name.into(),
            value: url.clone(),
            reason: format!("registry URL: {e}"),
        })?;
        return Ok(Source::Registry { url: parsed });
    }
    if let Some(url) = rs.git {
        let parsed = Url::parse(&url).map_err(|e| LockfileError::BadVersion {
            package: pkg_name.into(),
            value: url.clone(),
            reason: format!("git URL: {e}"),
        })?;
        return Ok(Source::Git {
            url: parsed,
            rev: rs.rev.unwrap_or_default(),
            subdirectory: rs.subdirectory,
        });
    }
    let (kind, path) = if let Some(p) = rs.virtual_ {
        (FirstPartyKind::Virtual, p)
    } else if let Some(p) = rs.editable {
        (FirstPartyKind::Editable, p)
    } else if let Some(p) = rs.directory {
        (FirstPartyKind::Directory, p)
    } else if let Some(p) = rs.path {
        (FirstPartyKind::Path, p)
    } else {
        unreachable!("exactly one source field, validated above")
    };
    Ok(Source::FirstParty { kind, path })
}

// ---- Public API ----

pub fn parse(toml_src: &str) -> Result<Lockfile, LockfileError> {
    let raw: RawLockfile = toml::from_str(toml_src)?;
    Lockfile::from_raw(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"
version = 1
revision = 3
requires-python = ">=3.12"

[[package]]
name = "certifi"
version = "2026.5.20"
source = { registry = "https://pypi.org/simple" }
"#;

    #[test]
    fn parses_minimal_lockfile() {
        let lock = parse(MINIMAL).expect("parse");
        assert_eq!(lock.version, 1);
        assert_eq!(lock.revision, 3);
        assert_eq!(lock.requires_python, ">=3.12");
        assert_eq!(lock.packages.len(), 1);
        let pkg = &lock.packages[0];
        assert_eq!(pkg.name.as_ref(), "certifi");
        assert_eq!(pkg.version.to_string(), "2026.5.20");
        assert!(matches!(pkg.source, Source::Registry { .. }));
        assert!(pkg.dependencies.is_empty());
        assert!(pkg.sdist.is_none());
        assert!(pkg.wheels.is_empty());
    }

    #[test]
    fn rejects_unsupported_version() {
        let bad = r#"
version = 99
revision = 1
requires-python = ">=3.12"
"#;
        let err = parse(bad).expect_err("should fail");
        assert!(matches!(err, LockfileError::UnsupportedVersion(99)));
    }

    #[test]
    fn parses_first_party_virtual_source() {
        let toml_str = r#"
version = 1
revision = 3
requires-python = ">=3.12"

[[package]]
name = "myapp"
version = "0.1.0"
source = { virtual = "." }
"#;
        let lock = parse(toml_str).expect("parse");
        let pkg = &lock.packages[0];
        assert!(matches!(
            pkg.source,
            Source::FirstParty {
                kind: FirstPartyKind::Virtual,
                ..
            }
        ));
    }

    #[test]
    fn parses_git_source() {
        let toml_str = r#"
version = 1
revision = 3
requires-python = ">=3.12"

[[package]]
name = "foo"
version = "0.1.0"
source = { git = "https://github.com/foo/bar", rev = "abc123" }
"#;
        let lock = parse(toml_str).expect("parse");
        assert!(matches!(lock.packages[0].source, Source::Git { .. }));
    }

    #[test]
    fn rejects_ambiguous_source() {
        let toml_str = r#"
version = 1
revision = 3
requires-python = ">=3.12"

[[package]]
name = "foo"
version = "0.1.0"
source = { registry = "https://pypi.org/simple", git = "https://github.com/foo/bar" }
"#;
        let err = parse(toml_str).expect_err("should fail");
        assert!(matches!(err, LockfileError::AmbiguousSource { .. }));
    }
}
