use std::collections::BTreeMap;
use std::path::PathBuf;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub trees: Vec<Tree>,
    pub platforms: BTreeMap<String, Platform>,
    pub fixups: FixupsConfig,
    pub buck: BuckConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tree {
    pub name: String,
    pub manifest_path: PathBuf,
    pub third_party_dir: PathBuf,
    pub python_versions: Vec<PythonVersion>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Platform {
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manylinux: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub musllinux: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub macos_min: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub struct FixupsConfig {
    #[serde(default = "default_registry")]
    pub registry: FixupRegistry,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry_rev: Option<String>,
    #[serde(default = "default_true")]
    pub allow_local_overrides: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum FixupRegistry {
    String(String),
}

impl Default for FixupRegistry {
    fn default() -> Self {
        FixupRegistry::String("none".into())
    }
}

fn default_registry() -> FixupRegistry { FixupRegistry::default() }
fn default_true() -> bool { true }

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct BuckConfig {
    #[serde(default = "default_buck_file_name")]
    pub file_name: String,
    #[serde(default)]
    pub vendor: bool,
}

impl Default for BuckConfig {
    fn default() -> Self {
        Self { file_name: default_buck_file_name(), vendor: false }
    }
}

fn default_buck_file_name() -> String { "BUCK".into() }

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PythonVersion(pub u8, pub u8);

impl<'de> Deserialize<'de> for PythonVersion {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let s = String::deserialize(de)?;
        PythonVersion::from_str(&s).map_err(D::Error::custom)
    }
}

impl Serialize for PythonVersion {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(&format!("{}.{}", self.0, self.1))
    }
}

impl FromStr for PythonVersion {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 2 {
            return Err(format!("python version must be MAJOR.MINOR, got `{s}`"));
        }
        let major: u8 = parts[0].parse().map_err(|_| format!("bad major in `{s}`"))?;
        let minor: u8 = parts[1].parse().map_err(|_| format!("bad minor in `{s}`"))?;
        if major != 3 {
            return Err(format!("only Python 3.x supported, got `{s}`"));
        }
        Ok(PythonVersion(major, minor))
    }
}

#[derive(Debug, Deserialize)]
struct RawConfig {
    manifest_path: Option<PathBuf>,
    third_party_dir: Option<PathBuf>,
    python_versions: Option<Vec<PythonVersion>>,
    platforms: BTreeMap<String, Platform>,
    #[serde(default)]
    fixups: FixupsConfig,
    #[serde(default)]
    buck: BuckConfig,
    #[serde(default)]
    tree: BTreeMap<String, RawTree>,
}

#[derive(Debug, Deserialize)]
struct RawTree {
    manifest_path: PathBuf,
    third_party_dir: PathBuf,
    python_versions: Vec<PythonVersion>,
}

impl Config {
    pub fn from_str(s: &str) -> Result<Self, crate::error::ConfigError> {
        let raw: RawConfig = toml::from_str(s).map_err(crate::error::ConfigError::Parse)?;
        Self::from_raw(raw)
    }

    fn from_raw(raw: RawConfig) -> Result<Self, crate::error::ConfigError> {
        let has_top_level = raw.manifest_path.is_some()
            || raw.third_party_dir.is_some()
            || raw.python_versions.is_some();
        let has_trees = !raw.tree.is_empty();

        if has_top_level && has_trees {
            return Err(crate::error::ConfigError::IncompatibleShape);
        }

        let trees = if has_trees {
            raw.tree.into_iter().map(|(name, t)| Tree {
                name,
                manifest_path: t.manifest_path,
                third_party_dir: t.third_party_dir,
                python_versions: t.python_versions,
            }).collect()
        } else {
            let manifest_path = raw.manifest_path.ok_or(crate::error::ConfigError::MissingField("manifest_path"))?;
            let third_party_dir = raw.third_party_dir.ok_or(crate::error::ConfigError::MissingField("third_party_dir"))?;
            let python_versions = raw.python_versions.ok_or(crate::error::ConfigError::MissingField("python_versions"))?;
            vec![Tree { name: "default".into(), manifest_path, third_party_dir, python_versions }]
        };

        Ok(Config { trees, platforms: raw.platforms, fixups: raw.fixups, buck: raw.buck })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL_SINGLE_TREE: &str = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.11", "3.12"]

[platforms.linux-x86_64-gnu]
target    = "x86_64-unknown-linux-gnu"
manylinux = "2_17"

[platforms.macos-arm64]
target    = "aarch64-apple-darwin"
macos_min = "11.0"
"#;

    #[test]
    fn parses_minimal_single_tree() {
        let config = Config::from_str(MINIMAL_SINGLE_TREE).expect("parse");
        assert_eq!(config.trees.len(), 1);
        let tree = &config.trees[0];
        assert_eq!(tree.name, "default");
        assert_eq!(tree.manifest_path.to_str(), Some("../pyproject.toml"));
        assert_eq!(tree.python_versions, vec![PythonVersion(3, 11), PythonVersion(3, 12)]);
        assert_eq!(config.platforms.len(), 2);
    }
}
