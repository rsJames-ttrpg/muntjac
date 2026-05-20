//! Parse `uv.lock` into `Lockfile`.

use std::collections::BTreeMap;
use std::str::FromStr;

use pep440_rs::Version;
use pep508_rs::{MarkerTree, PackageName};
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
    #[serde(default)]
    dependencies: Vec<RawDep>,
    #[serde(default, rename = "optional-dependencies")]
    optional_dependencies: BTreeMap<String, Vec<RawDep>>,
    #[serde(default, rename = "dev-dependencies")]
    dev_dependencies: BTreeMap<String, Vec<RawDep>>,
    #[serde(default)]
    sdist: Option<RawArtifact>,
    #[serde(default)]
    wheels: Vec<RawArtifact>,
    #[serde(default)]
    metadata: Option<RawMetadata>,
}

#[derive(Debug, Deserialize)]
struct RawDep {
    name: String,
    #[serde(default)]
    extra: Vec<String>,
    #[serde(default)]
    marker: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawMetadata {
    #[serde(default, rename = "requires-dist")]
    requires_dist: Vec<RawDep>,
    #[serde(default, rename = "provides-extras")]
    provides_extras: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct RawArtifact {
    url: String,
    hash: String,
    #[serde(default)]
    size: Option<u64>,
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

    let mut dependencies: Vec<DepEdge> = rp
        .dependencies
        .into_iter()
        .map(|rd| dep_from_raw(&rp.name, rd))
        .collect::<Result<Vec<_>, _>>()?;

    // Synthesize edges for [package.optional-dependencies] and [package.dev-dependencies].
    // Each group/extra entry becomes an edge gated by `extra == 'GROUP_NAME'`. This unifies
    // PEP 735 dependency-groups and PEP 508 extras under the same marker-based mechanism
    // already used elsewhere in the graph.
    for (group, deps) in rp
        .optional_dependencies
        .into_iter()
        .chain(rp.dev_dependencies)
    {
        let marker_str = format!("extra == '{group}'");
        let synth_marker =
            MarkerTree::from_str(&marker_str).map_err(|e| LockfileError::BadMarker {
                package: rp.name.clone(),
                dep: format!("<{group}>"),
                marker: marker_str.clone(),
                reason: e.to_string(),
            })?;
        for rd in deps {
            let mut edge = dep_from_raw(&rp.name, rd)?;
            edge.marker = Some(combine_markers(edge.marker, synth_marker.clone()));
            dependencies.push(edge);
        }
    }

    let sdist = rp
        .sdist
        .map(|rs| -> Result<Sdist, LockfileError> {
            let url = Url::parse(&rs.url).map_err(|e| LockfileError::BadVersion {
                package: rp.name.clone(),
                value: rs.url.clone(),
                reason: format!("sdist URL: {e}"),
            })?;
            Ok(Sdist {
                url,
                hash: rs.hash,
                size: rs.size,
            })
        })
        .transpose()?;

    let wheels = rp
        .wheels
        .into_iter()
        .map(|rw| -> Result<Wheel, LockfileError> {
            let url = Url::parse(&rw.url).map_err(|e| LockfileError::BadVersion {
                package: rp.name.clone(),
                value: rw.url.clone(),
                reason: format!("wheel URL: {e}"),
            })?;
            let filename = rw.url.rsplit('/').next().unwrap_or("").to_string();
            Ok(Wheel {
                url,
                hash: rw.hash,
                size: rw.size,
                filename,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    let metadata = rp
        .metadata
        .map(|rm| -> Result<Metadata, LockfileError> {
            let requires_dist = rm
                .requires_dist
                .into_iter()
                .map(|rd| dep_from_raw(&rp.name, rd))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Metadata {
                requires_dist,
                provides_extras: rm.provides_extras,
            })
        })
        .transpose()?;

    Ok(Package {
        name,
        version,
        source,
        dependencies,
        sdist,
        wheels,
        metadata,
    })
}

/// Combine two markers with AND. Used to gate synthesized optional/dev-dep edges
/// by their group/extra activation marker while preserving any existing per-dep marker.
fn combine_markers(existing: Option<MarkerTree>, extra: MarkerTree) -> MarkerTree {
    match existing {
        None => extra,
        Some(mut m) => {
            m.and(extra);
            m
        }
    }
}

fn dep_from_raw(pkg_name: &str, rd: RawDep) -> Result<DepEdge, LockfileError> {
    let name = PackageName::from_str(&rd.name)
        .map_err(|e| LockfileError::BadPackageName(rd.name.clone(), e.to_string()))?;
    let marker = rd
        .marker
        .as_deref()
        .map(|m| {
            MarkerTree::from_str(m).map_err(|e| LockfileError::BadMarker {
                package: pkg_name.into(),
                dep: rd.name.clone(),
                marker: m.into(),
                reason: e.to_string(),
            })
        })
        .transpose()?;
    Ok(DepEdge {
        name,
        extra: rd.extra,
        marker,
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

    const WITH_DEPS: &str = r#"
version = 1
revision = 3
requires-python = ">=3.12"

[[package]]
name = "requests"
version = "2.34.2"
source = { registry = "https://pypi.org/simple" }
dependencies = [
    { name = "certifi" },
    { name = "urllib3", extra = ["socks"] },
]
sdist = { url = "https://files.pythonhosted.org/packages/x/requests.tar.gz", hash = "sha256:abc", size = 142856 }
wheels = [
    { url = "https://files.pythonhosted.org/packages/y/requests-2.34.2-py3-none-any.whl", hash = "sha256:def", size = 73075 },
]

[[package]]
name = "certifi"
version = "2026.5.20"
source = { registry = "https://pypi.org/simple" }

[[package]]
name = "urllib3"
version = "2.7.0"
source = { registry = "https://pypi.org/simple" }
"#;

    #[test]
    fn parses_dependencies_sdist_and_wheels() {
        let lock = parse(WITH_DEPS).expect("parse");
        let requests = lock
            .packages
            .iter()
            .find(|p| p.name.as_ref() == "requests")
            .unwrap();
        assert_eq!(requests.dependencies.len(), 2);
        assert_eq!(requests.dependencies[0].name.as_ref(), "certifi");
        assert_eq!(requests.dependencies[1].name.as_ref(), "urllib3");
        assert_eq!(requests.dependencies[1].extra, vec!["socks".to_string()]);
        assert!(requests.sdist.is_some());
        assert_eq!(requests.wheels.len(), 1);
        let wheel = &requests.wheels[0];
        assert_eq!(wheel.hash, "sha256:def");
        assert_eq!(wheel.size, Some(73075));
        assert!(wheel.filename.ends_with(".whl"));
    }

    const WITH_MARKERS_AND_METADATA: &str = r#"
version = 1
revision = 3
requires-python = ">=3.10"

[[package]]
name = "app"
version = "0.1.0"
source = { virtual = "." }
dependencies = [
    { name = "numpy" },
    { name = "typing-extensions", marker = "python_version < '3.11'" },
]

[package.metadata]
requires-dist = [
    { name = "numpy" },
    { name = "typing-extensions", marker = "python_version < '3.11'" },
]
provides-extras = ["dev"]

[[package]]
name = "numpy"
version = "2.4.6"
source = { registry = "https://pypi.org/simple" }

[[package]]
name = "typing-extensions"
version = "4.12.0"
source = { registry = "https://pypi.org/simple" }
"#;

    #[test]
    fn parses_markers_and_metadata() {
        let lock = parse(WITH_MARKERS_AND_METADATA).expect("parse");
        let app = lock
            .packages
            .iter()
            .find(|p| p.name.as_ref() == "app")
            .unwrap();

        // Marker on the typing-extensions edge
        let te_edge = app
            .dependencies
            .iter()
            .find(|d| d.name.as_ref() == "typing-extensions")
            .unwrap();
        assert!(te_edge.marker.is_some());

        let numpy_edge = app
            .dependencies
            .iter()
            .find(|d| d.name.as_ref() == "numpy")
            .unwrap();
        assert!(numpy_edge.marker.is_none());

        // Metadata
        let meta = app.metadata.as_ref().expect("metadata");
        assert_eq!(meta.requires_dist.len(), 2);
        assert_eq!(meta.provides_extras, vec!["dev".to_string()]);

        // First-party source
        assert!(matches!(
            app.source,
            Source::FirstParty {
                kind: FirstPartyKind::Virtual,
                ..
            }
        ));
    }

    #[test]
    fn rejects_bad_marker() {
        let bad = r#"
version = 1
revision = 3
requires-python = ">=3.10"

[[package]]
name = "app"
version = "0.1.0"
source = { virtual = "." }
dependencies = [
    { name = "x", marker = "this is not a marker expression" },
]
"#;
        let err = parse(bad).expect_err("should fail");
        assert!(matches!(err, LockfileError::BadMarker { .. }));
    }
}
