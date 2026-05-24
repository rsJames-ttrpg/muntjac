//! Disk walk: load every `fixups/<pkg>/fixups.toml` under `third_party_dir`.

use std::collections::BTreeMap;
use std::path::Path;
use std::str::FromStr;

use pep508_rs::PackageName;

use crate::fixup::error::FixupError;
use crate::fixup::schema::FixupConfig;

#[derive(Debug, Default, Clone)]
pub struct FixupSet {
    fixups: BTreeMap<PackageName, FixupConfig>,
}

impl FixupSet {
    pub fn get(&self, name: &PackageName) -> Option<&FixupConfig> {
        self.fixups.get(name)
    }
    pub fn iter(&self) -> impl Iterator<Item = (&PackageName, &FixupConfig)> {
        self.fixups.iter()
    }
    pub fn len(&self) -> usize {
        self.fixups.len()
    }
    pub fn is_empty(&self) -> bool {
        self.fixups.is_empty()
    }

    /// Internal constructor — used by `load_local` and `load_community`.
    pub(crate) fn from_map_internal(fixups: BTreeMap<PackageName, FixupConfig>) -> Self {
        Self { fixups }
    }

    #[cfg(test)]
    pub fn from_map_for_test(fixups: BTreeMap<PackageName, FixupConfig>) -> Self {
        Self { fixups }
    }
}

/// Read and parse a single `fixups.toml` file. Shared internals between
/// `load_local` and `load_community`.
pub(crate) fn load_one_fixup(toml_path: &Path) -> Result<FixupConfig, FixupError> {
    let body = std::fs::read_to_string(toml_path).map_err(|e| FixupError::Io {
        path: toml_path.to_path_buf(),
        source: e,
    })?;

    FixupConfig::from_toml_str(&body).map_err(|source| {
        let msg = source.to_string();
        if let Some(field) = extract_unknown_field(&msg) {
            FixupError::UnknownField {
                file: toml_path.to_path_buf(),
                field,
            }
        } else {
            FixupError::ParseError {
                file: toml_path.to_path_buf(),
                source,
            }
        }
    })
}

/// Load every `<third_party_dir>/fixups/<pkg>/fixups.toml` into a
/// `FixupSet`. Package names are PEP 503-normalized.
///
/// Returns an empty `FixupSet` if `<third_party_dir>/fixups/` doesn't
/// exist — that's the no-fixups case, not an error.
pub fn load_local(third_party_dir: &Path) -> Result<FixupSet, FixupError> {
    let fixups_dir = third_party_dir.join("fixups");
    if !fixups_dir.is_dir() {
        return Ok(FixupSet::default());
    }

    let mut fixups = BTreeMap::new();
    let entries = std::fs::read_dir(&fixups_dir).map_err(|e| FixupError::Io {
        path: fixups_dir.clone(),
        source: e,
    })?;

    for entry in entries {
        let entry = entry.map_err(|e| FixupError::Io {
            path: fixups_dir.clone(),
            source: e,
        })?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let toml_path = path.join("fixups.toml");
        if !toml_path.is_file() {
            continue;
        }

        let pkg_dir_name =
            path.file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| FixupError::Io {
                    path: path.clone(),
                    source: std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "non-utf8 directory name",
                    ),
                })?;

        let pkg_name = PackageName::from_str(pkg_dir_name).map_err(|e| FixupError::Io {
            path: path.clone(),
            source: std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()),
        })?;

        let config = load_one_fixup(&toml_path)?;
        fixups.insert(pkg_name, config);
    }

    Ok(FixupSet::from_map_internal(fixups))
}

/// Load every `<registry_dir>/packages/<pkg>/fixups.toml` into a
/// `FixupSet`. Package directory names are PEP 503-normalized.
///
/// Differs from `load_local` in two ways:
///   1. Walks `<registry_dir>/packages/` (not `<registry_dir>/fixups/`).
///   2. Errors with `RegistryPathNotFound` if `<registry_dir>/packages/`
///      doesn't exist (because the user explicitly pointed registry =
///      "file://<registry_dir>" at this path — silent emptiness would
///      mask a configuration mistake).
///   3. Rejects any fixup with `replace_community = true`; that flag is
///      a local-only opt-out.
pub fn load_community(registry_dir: &Path) -> Result<FixupSet, FixupError> {
    let packages_dir = registry_dir.join("packages");
    if !packages_dir.is_dir() {
        return Err(FixupError::RegistryPathNotFound { path: packages_dir });
    }

    let mut fixups = BTreeMap::new();
    let entries = std::fs::read_dir(&packages_dir).map_err(|e| FixupError::Io {
        path: packages_dir.clone(),
        source: e,
    })?;

    for entry in entries {
        let entry = entry.map_err(|e| FixupError::Io {
            path: packages_dir.clone(),
            source: e,
        })?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let toml_path = path.join("fixups.toml");
        if !toml_path.is_file() {
            continue;
        }

        let pkg_dir_name =
            path.file_name()
                .and_then(|n| n.to_str())
                .ok_or_else(|| FixupError::Io {
                    path: path.clone(),
                    source: std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "non-utf8 directory name",
                    ),
                })?;

        let pkg_name = PackageName::from_str(pkg_dir_name).map_err(|e| FixupError::Io {
            path: path.clone(),
            source: std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()),
        })?;

        let config = load_one_fixup(&toml_path)?;

        if config.replace_community {
            return Err(FixupError::ReplaceCommunityInCommunity { file: toml_path });
        }

        fixups.insert(pkg_name, config);
    }

    Ok(FixupSet::from_map_internal(fixups))
}

/// `toml::de::Error` for unknown fields reads like:
///   `unknown field `foo`, expected one of `extra_deps`, ...`
/// Extract the field name if present.
fn extract_unknown_field(msg: &str) -> Option<String> {
    let needle = "unknown field `";
    let start = msg.find(needle)? + needle.len();
    let rest = &msg[start..];
    let end = rest.find('`')?;
    Some(rest[..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write(dir: &Path, rel: &str, contents: &str) {
        let path = dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn missing_fixups_dir_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let set = load_local(tmp.path()).unwrap();
        assert!(set.is_empty());
    }

    #[test]
    fn loads_single_fixup() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "fixups/pillow/fixups.toml",
            r#"extra_deps = ["//third-party/c:libjpeg"]"#,
        );
        let set = load_local(tmp.path()).unwrap();
        assert_eq!(set.len(), 1);
        let name = PackageName::from_str("pillow").unwrap();
        let cfg = set.get(&name).unwrap();
        assert_eq!(cfg.top.extra_deps, vec!["//third-party/c:libjpeg"]);
    }

    #[test]
    fn normalizes_pep503_name() {
        // PEP 503: "Pillow" and "pillow" and "PIL_LOW" all normalize.
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "fixups/Pillow/fixups.toml",
            r#"extra_deps = ["//x:y"]"#,
        );
        let set = load_local(tmp.path()).unwrap();
        let name = PackageName::from_str("pillow").unwrap();
        assert!(set.get(&name).is_some());
    }

    #[test]
    fn skips_dirs_without_fixups_toml() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "fixups/pillow/fixups.toml",
            r#"extra_deps = []"#,
        );
        std::fs::create_dir_all(tmp.path().join("fixups/orphan-dir")).unwrap();
        let set = load_local(tmp.path()).unwrap();
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn unknown_field_error_is_typed() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "fixups/pillow/fixups.toml", r#"extras = []"#);
        let err = load_local(tmp.path()).unwrap_err();
        match err {
            FixupError::UnknownField { field, .. } => assert_eq!(field, "extras"),
            other => panic!("expected UnknownField, got {:?}", other),
        }
    }

    #[test]
    fn multiple_fixups_load_deterministically() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "fixups/aaa/fixups.toml", r#"extra_deps = []"#);
        write(tmp.path(), "fixups/zzz/fixups.toml", r#"extra_deps = []"#);
        write(tmp.path(), "fixups/mmm/fixups.toml", r#"extra_deps = []"#);

        let set = load_local(tmp.path()).unwrap();
        let names: Vec<String> = set.iter().map(|(n, _)| n.to_string()).collect();
        assert_eq!(names, vec!["aaa", "mmm", "zzz"]); // BTreeMap sorted
    }

    #[test]
    fn load_one_fixup_loads_single_file() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("fixups.toml");
        fs::write(&path, r#"extra_deps = ["//x:y"]"#).unwrap();

        let cfg = super::load_one_fixup(&path).expect("loads");
        assert_eq!(cfg.top.extra_deps, vec!["//x:y"]);
    }

    #[test]
    fn load_one_fixup_unknown_field_is_typed() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("fixups.toml");
        fs::write(&path, r#"unknown_thing = []"#).unwrap();

        let err = super::load_one_fixup(&path).unwrap_err();
        match err {
            FixupError::UnknownField { field, .. } => assert_eq!(field, "unknown_thing"),
            other => panic!("expected UnknownField, got {:?}", other),
        }
    }

    #[test]
    fn load_community_walks_packages_subdir() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "packages/pillow/fixups.toml",
            r#"extra_deps = ["//x:y"]"#,
        );
        write(
            tmp.path(),
            "packages/numpy/fixups.toml",
            r#"extra_deps = ["//a:b"]"#,
        );

        let set = load_community(tmp.path()).expect("loads");
        assert_eq!(set.len(), 2);
        let pillow = PackageName::from_str("pillow").unwrap();
        assert_eq!(set.get(&pillow).unwrap().top.extra_deps, vec!["//x:y"]);
    }

    #[test]
    fn load_community_normalizes_pep503_names() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "packages/Pillow/fixups.toml",
            r#"extra_deps = []"#,
        );
        let set = load_community(tmp.path()).expect("loads");
        assert!(set.get(&PackageName::from_str("pillow").unwrap()).is_some());
    }

    #[test]
    fn load_community_rejects_replace_community_on_community_side() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "packages/evil/fixups.toml",
            "replace_community = true\nextra_deps = []",
        );
        let err = load_community(tmp.path()).unwrap_err();
        match err {
            FixupError::ReplaceCommunityInCommunity { file } => {
                assert!(file.ends_with("packages/evil/fixups.toml"));
            }
            other => panic!("expected ReplaceCommunityInCommunity, got {:?}", other),
        }
    }

    #[test]
    fn load_community_errors_if_packages_dir_missing() {
        let tmp = TempDir::new().unwrap();
        // tmp/packages does not exist.
        let err = load_community(tmp.path()).unwrap_err();
        match err {
            FixupError::RegistryPathNotFound { path } => {
                assert!(path.ends_with("packages"));
            }
            other => panic!("expected RegistryPathNotFound, got {:?}", other),
        }
    }

    #[test]
    fn load_community_loads_zero_packages_ok() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("packages")).unwrap();
        let set = load_community(tmp.path()).expect("loads");
        assert!(set.is_empty());
    }
}
