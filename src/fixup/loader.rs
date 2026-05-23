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

    #[cfg(test)]
    pub fn from_map_for_test(fixups: BTreeMap<PackageName, FixupConfig>) -> Self {
        Self { fixups }
    }
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

        // PEP 503: normalize via PackageName.
        let pkg_name = PackageName::from_str(pkg_dir_name).map_err(|e| FixupError::Io {
            path: path.clone(),
            source: std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()),
        })?;

        let body = std::fs::read_to_string(&toml_path).map_err(|e| FixupError::Io {
            path: toml_path.clone(),
            source: e,
        })?;

        let config = FixupConfig::from_toml_str(&body).map_err(|source| {
            // Distinguish unknown-field vs other parse errors.
            let msg = source.to_string();
            if let Some(field) = extract_unknown_field(&msg) {
                FixupError::UnknownField {
                    file: toml_path.clone(),
                    field,
                }
            } else {
                FixupError::ParseError {
                    file: toml_path.clone(),
                    source,
                }
            }
        })?;

        fixups.insert(pkg_name, config);
    }

    Ok(FixupSet { fixups })
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
}
