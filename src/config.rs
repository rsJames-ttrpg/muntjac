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
    pub lockfile: LockfileConfig,
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

impl Platform {
    /// Returns the parsed manylinux baseline `(major, minor)` if set.
    /// Assumes `Config::validate` has run; panics if the string is malformed.
    pub fn manylinux_baseline(&self) -> Option<(u32, u32)> {
        self.manylinux
            .as_deref()
            .map(|s| parse_underscore_pair(s).expect("validated manylinux string"))
    }

    /// Returns the parsed musllinux baseline `(major, minor)` if set.
    /// Assumes `Config::validate` has run; panics if the string is malformed.
    pub fn musllinux_baseline(&self) -> Option<(u32, u32)> {
        self.musllinux
            .as_deref()
            .map(|s| parse_underscore_pair(s).expect("validated musllinux string"))
    }

    /// Returns the parsed `macos_min` deployment target `(major, minor)` if set.
    /// `macos_min` is the deployment target — the minimum macOS version that
    /// resulting binaries must support. The wheel selector accepts wheels
    /// whose required deployment target is <= this value.
    /// Assumes `Config::validate` has run; panics if the string is malformed.
    pub fn macos_min_parsed(&self) -> Option<(u32, u32)> {
        self.macos_min
            .as_deref()
            .map(|s| parse_dot_pair(s).expect("validated macos_min string"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixupsConfig {
    pub registry: crate::fixup::RegistryConfig,
    pub registry_rev: Option<String>,
    pub allow_local_overrides: bool,
}

// Deserialize wrapper: parse from TOML's raw view (where registry is a
// String) into the typed FixupsConfig.
impl<'de> serde::Deserialize<'de> for FixupsConfig {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct Raw {
            #[serde(default = "default_registry_str")]
            registry: String,
            #[serde(default)]
            registry_rev: Option<String>,
            #[serde(default = "default_true")]
            allow_local_overrides: bool,
        }
        let raw = Raw::deserialize(de)?;
        let registry =
            crate::fixup::parse_registry_config(&raw.registry, raw.registry_rev.as_deref())
                .map_err(serde::de::Error::custom)?;

        // Warn if registry_rev is set but registry is None or FileUrl
        // (only meaningful for Git mode in S7b).
        if raw.registry_rev.is_some() {
            match &registry {
                crate::fixup::RegistryConfig::None | crate::fixup::RegistryConfig::FileUrl(_) => {
                    eprintln!(
                        "[muntjac] warn: registry_rev is ignored when registry is \"none\" or \"file://...\"; effective in S7b for git-based registries"
                    );
                }
                crate::fixup::RegistryConfig::Git { .. } => {}
            }
        }

        Ok(FixupsConfig {
            registry,
            registry_rev: raw.registry_rev,
            allow_local_overrides: raw.allow_local_overrides,
        })
    }
}

fn default_registry_str() -> String {
    "none".into()
}
fn default_true() -> bool {
    true
}

// Serialize back to TOML for fixtures / round-trips. Map RegistryConfig to its
// canonical string form.
impl serde::Serialize for FixupsConfig {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("FixupsConfig", 3)?;
        let reg_str = match &self.registry {
            crate::fixup::RegistryConfig::None => "none".to_string(),
            crate::fixup::RegistryConfig::FileUrl(p) => {
                format!("file://{}", p.display())
            }
            crate::fixup::RegistryConfig::Git { url, .. } => url.clone(),
        };
        st.serialize_field("registry", &reg_str)?;
        st.serialize_field("registry_rev", &self.registry_rev)?;
        st.serialize_field("allow_local_overrides", &self.allow_local_overrides)?;
        st.end()
    }
}

impl Default for FixupsConfig {
    fn default() -> Self {
        Self {
            registry: crate::fixup::RegistryConfig::None,
            registry_rev: None,
            allow_local_overrides: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct BuckConfig {
    #[serde(default = "default_buck_file_name")]
    pub file_name: String,
    #[serde(default)]
    pub vendor: bool,
    /// Where the shared cfg (config/ + wiring.bzl) is written. `None` →
    /// derived as the longest common ancestor of trees' third_party_dirs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cfg_dir: Option<PathBuf>,
}

impl Default for BuckConfig {
    fn default() -> Self {
        Self {
            file_name: default_buck_file_name(),
            vendor: false,
            cfg_dir: None,
        }
    }
}

fn default_buck_file_name() -> String {
    "BUCK".into()
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub struct LockfileConfig {
    #[serde(default)]
    pub include_groups: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
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
        let major: u8 = parts[0]
            .parse()
            .map_err(|_| format!("bad major in `{s}`"))?;
        let minor: u8 = parts[1]
            .parse()
            .map_err(|_| format!("bad minor in `{s}`"))?;
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
    #[serde(default)]
    platforms: BTreeMap<String, Platform>,
    #[serde(default)]
    fixups: FixupsConfig,
    #[serde(default)]
    buck: BuckConfig,
    #[serde(default)]
    lockfile: LockfileConfig,
    #[serde(default)]
    tree: BTreeMap<String, RawTree>,
}

#[derive(Debug, Deserialize)]
struct RawTree {
    manifest_path: PathBuf,
    third_party_dir: PathBuf,
    python_versions: Vec<PythonVersion>,
}

impl FromStr for Config {
    type Err = crate::error::ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let raw: RawConfig = toml::from_str(s).map_err(crate::error::ConfigError::Parse)?;
        let mut config = Self::from_raw(raw)?;
        config.dedupe_include_groups();
        config.validate()?;
        Ok(config)
    }
}

impl Config {
    fn from_raw(raw: RawConfig) -> Result<Self, crate::error::ConfigError> {
        let has_top_level = raw.manifest_path.is_some()
            || raw.third_party_dir.is_some()
            || raw.python_versions.is_some();
        let has_trees = !raw.tree.is_empty();

        if has_top_level && has_trees {
            return Err(crate::error::ConfigError::IncompatibleShape);
        }

        let trees = if has_trees {
            raw.tree
                .into_iter()
                .map(|(name, t)| Tree {
                    name,
                    manifest_path: t.manifest_path,
                    third_party_dir: t.third_party_dir,
                    python_versions: t.python_versions,
                })
                .collect()
        } else {
            let manifest_path = raw
                .manifest_path
                .ok_or(crate::error::ConfigError::MissingField("manifest_path"))?;
            let third_party_dir = raw
                .third_party_dir
                .ok_or(crate::error::ConfigError::MissingField("third_party_dir"))?;
            let python_versions = raw
                .python_versions
                .ok_or(crate::error::ConfigError::MissingField("python_versions"))?;
            vec![Tree {
                name: "default".into(),
                manifest_path,
                third_party_dir,
                python_versions,
            }]
        };

        if raw.platforms.is_empty() {
            return Err(crate::error::ConfigError::MissingField("platforms"));
        }

        Ok(Config {
            trees,
            platforms: raw.platforms,
            fixups: raw.fixups,
            buck: raw.buck,
            lockfile: raw.lockfile,
        })
    }
}

impl Config {
    pub(crate) fn dedupe_include_groups(&mut self) {
        let original_len = self.lockfile.include_groups.len();
        let mut seen = std::collections::HashSet::new();
        self.lockfile
            .include_groups
            .retain(|g| seen.insert(g.clone()));
        if self.lockfile.include_groups.len() < original_len {
            eprintln!(
                "warning: muntjac.toml [lockfile] include_groups contained duplicates ({} → {}); deduplicated",
                original_len,
                self.lockfile.include_groups.len()
            );
        }
    }
}

impl Config {
    /// Resolve where the shared cfg (config/ + wiring.bzl) is written.
    /// Explicit `[buck] cfg_dir` wins; else the longest common path-ancestor
    /// of all trees' third_party_dirs. For a single tree this is that tree's
    /// own third_party_dir, keeping output byte-identical to pre-S11.
    pub fn shared_cfg_dir(&self) -> PathBuf {
        if let Some(explicit) = &self.buck.cfg_dir {
            return explicit.clone();
        }
        let mut dirs = self.trees.iter().map(|t| t.third_party_dir.as_path());
        let first = dirs.next().expect("validated: at least one tree");
        let mut common: Vec<std::path::Component> = first.components().collect();
        for d in dirs {
            let comps: Vec<_> = d.components().collect();
            let keep = common
                .iter()
                .zip(comps.iter())
                .take_while(|(a, b)| a == b)
                .count();
            common.truncate(keep);
        }
        common.iter().collect()
    }

    /// Union of every tree's python_versions, sorted and deduped.
    pub fn python_versions_union(&self) -> Vec<PythonVersion> {
        let mut set: std::collections::BTreeSet<PythonVersion> = std::collections::BTreeSet::new();
        for t in &self.trees {
            for v in &t.python_versions {
                set.insert(v.clone());
            }
        }
        set.into_iter().collect()
    }
}

impl Config {
    pub fn validate(&self) -> Result<(), crate::error::ConfigError> {
        // Reject two trees sharing a third_party_dir (they'd clobber output).
        {
            let mut by_dir: std::collections::BTreeMap<PathBuf, Vec<String>> =
                std::collections::BTreeMap::new();
            for t in &self.trees {
                by_dir
                    .entry(t.third_party_dir.clone())
                    .or_default()
                    .push(t.name.clone());
            }
            if let Some((dir, names)) = by_dir.iter().find(|(_, v)| v.len() > 1) {
                return Err(crate::error::ConfigError::DuplicateTreeDir {
                    dir: dir.display().to_string(),
                    trees: names.clone(),
                });
            }
        }
        // Multi-tree only: the shared cfg_dir must be derivable + not collide
        // with a tree's own dir.
        if self.trees.len() > 1 {
            let cfg_dir = self.shared_cfg_dir();
            if cfg_dir.as_os_str().is_empty() {
                return Err(crate::error::ConfigError::CfgDirNotDerivable);
            }
            if let Some(t) = self.trees.iter().find(|t| t.third_party_dir == cfg_dir) {
                return Err(crate::error::ConfigError::TreeDirIsCfgDir {
                    tree: t.name.clone(),
                    dir: t.third_party_dir.display().to_string(),
                });
            }
        }
        for (name, platform) in &self.platforms {
            validate_target_triple(name, &platform.target)?;
            validate_platform_baseline(name, platform)?;
        }
        for g in &self.lockfile.include_groups {
            validate_group_name(g)?;
        }
        Ok(())
    }
}

fn parse_underscore_pair(s: &str) -> Result<(u32, u32), ()> {
    let (a, b) = s.split_once('_').ok_or(())?;
    Ok((a.parse().map_err(|_| ())?, b.parse().map_err(|_| ())?))
}

fn parse_dot_pair(s: &str) -> Result<(u32, u32), ()> {
    let (a, b) = s.split_once('.').ok_or(())?;
    Ok((a.parse().map_err(|_| ())?, b.parse().map_err(|_| ())?))
}

fn validate_platform_baseline(name: &str, p: &Platform) -> Result<(), crate::error::ConfigError> {
    let is_gnu = p.target.ends_with("linux-gnu");
    let is_musl = p.target.ends_with("linux-musl");
    let is_mac = p.target.ends_with("apple-darwin");

    // Shape checks first.
    if let Some(s) = &p.manylinux {
        parse_underscore_pair(s).map_err(|_| crate::error::ConfigError::BadPlatform {
            name: name.into(),
            reason: format!("manylinux baseline must be N_M (got `{s}`); use `2_17` not `2014`"),
        })?;
    }
    if let Some(s) = &p.musllinux {
        parse_underscore_pair(s).map_err(|_| crate::error::ConfigError::BadPlatform {
            name: name.into(),
            reason: format!("musllinux baseline must be N_M (got `{s}`)"),
        })?;
    }
    if let Some(s) = &p.macos_min {
        parse_dot_pair(s).map_err(|_| crate::error::ConfigError::BadPlatform {
            name: name.into(),
            reason: format!("macos_min must be MAJOR.MINOR (got `{s}`)"),
        })?;
    }

    // Policy: presence + libc coherence.
    if is_gnu || is_musl {
        if p.manylinux.is_none() && p.musllinux.is_none() {
            return Err(crate::error::ConfigError::BadPlatform {
                name: name.into(),
                reason: "linux platform must declare manylinux and/or musllinux baseline".into(),
            });
        }
        if is_gnu && p.musllinux.is_some() {
            return Err(crate::error::ConfigError::BadPlatform {
                name: name.into(),
                reason: "musllinux baseline only valid on *-linux-musl targets, not linux-gnu"
                    .into(),
            });
        }
        if is_musl && p.manylinux.is_some() {
            return Err(crate::error::ConfigError::BadPlatform {
                name: name.into(),
                reason: "manylinux baseline only valid on *-linux-gnu targets, not linux-musl"
                    .into(),
            });
        }
    }
    if is_mac && p.macos_min.is_none() {
        return Err(crate::error::ConfigError::BadPlatform {
            name: name.into(),
            reason: "macOS platform must declare macos_min (deployment target)".into(),
        });
    }
    Ok(())
}

fn validate_target_triple(name: &str, target: &str) -> Result<(), crate::error::ConfigError> {
    let allowed = [
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-gnu",
        "x86_64-unknown-linux-musl",
        "aarch64-unknown-linux-musl",
        "x86_64-apple-darwin",
        "aarch64-apple-darwin",
    ];
    if allowed.contains(&target) {
        Ok(())
    } else {
        Err(crate::error::ConfigError::BadPlatform {
            name: name.into(),
            reason: format!("unknown target triple `{target}`; expected one of {allowed:?}"),
        })
    }
}

fn validate_group_name(name: &str) -> Result<(), crate::error::ConfigError> {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return Err(crate::error::ConfigError::BadGroupName(name.to_string())),
    }
    for c in chars {
        if !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
            return Err(crate::error::ConfigError::BadGroupName(name.to_string()));
        }
    }
    Ok(())
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
        assert_eq!(
            tree.python_versions,
            vec![PythonVersion(3, 11), PythonVersion(3, 12)]
        );
        assert_eq!(config.platforms.len(), 2);
    }

    const MULTI_TREE: &str = r#"
[platforms.linux-x86_64-gnu]
target = "x86_64-unknown-linux-gnu"
manylinux = "2_17"

[tree.modern]
manifest_path   = "pyproject.toml"
third_party_dir = "third-party/python/modern"
python_versions = ["3.12"]

[tree.legacy]
manifest_path   = "legacy/pyproject.toml"
third_party_dir = "third-party/python/legacy"
python_versions = ["3.10"]
"#;

    const MIXED_SHAPE: &str = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target = "x86_64-unknown-linux-gnu"

[tree.extra]
manifest_path   = "other/pyproject.toml"
third_party_dir = "other"
python_versions = ["3.11"]
"#;

    #[test]
    fn parses_multi_tree() {
        let config = Config::from_str(MULTI_TREE).expect("parse");
        assert_eq!(config.trees.len(), 2);
        let names: Vec<&str> = config.trees.iter().map(|t| t.name.as_str()).collect();
        // BTreeMap iteration is sorted, so we expect alphabetical.
        assert_eq!(names, vec!["legacy", "modern"]);
    }

    #[test]
    fn cfg_dir_single_tree_is_third_party_dir() {
        let toml = r#"
manifest_path = "../pyproject.toml"
third_party_dir = "third-party/python"
python_versions = ["3.12"]
[platforms]
macos-arm64 = { target = "aarch64-apple-darwin", macos_min = "11.0" }
"#;
        let config: Config = toml.parse().unwrap();
        assert_eq!(
            config.shared_cfg_dir(),
            std::path::PathBuf::from("third-party/python")
        );
    }

    #[test]
    fn cfg_dir_multi_tree_is_common_ancestor() {
        let toml = r#"
[platforms]
macos-arm64 = { target = "aarch64-apple-darwin", macos_min = "11.0" }
[tree.modern]
manifest_path = "pyproject.toml"
third_party_dir = "third-party/python/modern"
python_versions = ["3.12"]
[tree.legacy]
manifest_path = "legacy/pyproject.toml"
third_party_dir = "third-party/python/legacy"
python_versions = ["3.12"]
"#;
        let config: Config = toml.parse().unwrap();
        assert_eq!(
            config.shared_cfg_dir(),
            std::path::PathBuf::from("third-party/python")
        );
    }

    #[test]
    fn cfg_dir_explicit_override_wins() {
        let toml = r#"
[platforms]
macos-arm64 = { target = "aarch64-apple-darwin", macos_min = "11.0" }
[buck]
cfg_dir = "buck/cfg"
[tree.a]
manifest_path = "a/pyproject.toml"
third_party_dir = "apps/a/tp"
python_versions = ["3.12"]
[tree.b]
manifest_path = "b/pyproject.toml"
third_party_dir = "apps/b/tp"
python_versions = ["3.12"]
"#;
        let config: Config = toml.parse().unwrap();
        assert_eq!(
            config.shared_cfg_dir(),
            std::path::PathBuf::from("buck/cfg")
        );
    }

    #[test]
    fn python_versions_union_dedupes_and_sorts() {
        let toml = r#"
[platforms]
macos-arm64 = { target = "aarch64-apple-darwin", macos_min = "11.0" }
[tree.modern]
manifest_path = "pyproject.toml"
third_party_dir = "tp/modern"
python_versions = ["3.12", "3.11"]
[tree.legacy]
manifest_path = "legacy/pyproject.toml"
third_party_dir = "tp/legacy"
python_versions = ["3.11"]
"#;
        let config: Config = toml.parse().unwrap();
        assert_eq!(
            config.python_versions_union(),
            vec![PythonVersion(3, 11), PythonVersion(3, 12)]
        );
    }

    #[test]
    fn rejects_duplicate_tree_dir() {
        let toml = r#"
[platforms]
macos-arm64 = { target = "aarch64-apple-darwin", macos_min = "11.0" }
[tree.a]
manifest_path = "a/pyproject.toml"
third_party_dir = "tp/shared"
python_versions = ["3.12"]
[tree.b]
manifest_path = "b/pyproject.toml"
third_party_dir = "tp/shared"
python_versions = ["3.12"]
"#;
        let err = toml.parse::<Config>().unwrap_err();
        assert!(matches!(
            err,
            crate::error::ConfigError::DuplicateTreeDir { .. }
        ));
    }

    #[test]
    fn rejects_tree_dir_equal_to_cfg_dir_when_multi_tree() {
        // cfg_dir derives to "tp" (common ancestor of tp + tp/legacy); tree "a"
        // sits exactly at "tp", colliding with the shared cfg location.
        let toml = r#"
[platforms]
macos-arm64 = { target = "aarch64-apple-darwin", macos_min = "11.0" }
[tree.a]
manifest_path = "a/pyproject.toml"
third_party_dir = "tp"
python_versions = ["3.12"]
[tree.legacy]
manifest_path = "legacy/pyproject.toml"
third_party_dir = "tp/legacy"
python_versions = ["3.12"]
"#;
        let err = toml.parse::<Config>().unwrap_err();
        assert!(matches!(
            err,
            crate::error::ConfigError::TreeDirIsCfgDir { .. }
        ));
    }

    #[test]
    fn single_tree_cfg_dir_equals_third_party_dir_is_ok() {
        let toml = r#"
manifest_path = "../pyproject.toml"
third_party_dir = "third-party/python"
python_versions = ["3.12"]
[platforms]
macos-arm64 = { target = "aarch64-apple-darwin", macos_min = "11.0" }
"#;
        assert!(toml.parse::<Config>().is_ok());
    }

    #[test]
    fn rejects_underivable_cfg_dir_no_common_ancestor() {
        // Two trees with no shared path prefix → empty derived cfg_dir → must error
        // (would otherwise emit the shared cfg at the project root).
        let toml = r#"
[platforms]
macos-arm64 = { target = "aarch64-apple-darwin", macos_min = "11.0" }
[tree.a]
manifest_path = "a/pyproject.toml"
third_party_dir = "apps/a/tp"
python_versions = ["3.12"]
[tree.b]
manifest_path = "b/pyproject.toml"
third_party_dir = "services/b/tp"
python_versions = ["3.12"]
"#;
        let err = toml.parse::<Config>().unwrap_err();
        assert!(matches!(err, crate::error::ConfigError::CfgDirNotDerivable));
    }

    #[test]
    fn explicit_cfg_dir_rescues_no_common_ancestor() {
        // Same no-common-ancestor trees, but an explicit [buck] cfg_dir makes it valid.
        let toml = r#"
[platforms]
macos-arm64 = { target = "aarch64-apple-darwin", macos_min = "11.0" }
[buck]
cfg_dir = "buck/cfg"
[tree.a]
manifest_path = "a/pyproject.toml"
third_party_dir = "apps/a/tp"
python_versions = ["3.12"]
[tree.b]
manifest_path = "b/pyproject.toml"
third_party_dir = "services/b/tp"
python_versions = ["3.12"]
"#;
        assert!(toml.parse::<Config>().is_ok());
    }

    #[test]
    fn rejects_mixed_shape() {
        let err = Config::from_str(MIXED_SHAPE).expect_err("should fail");
        assert!(matches!(err, crate::error::ConfigError::IncompatibleShape));
    }

    #[test]
    fn rejects_missing_manifest_path_when_no_trees() {
        let toml_str = r#"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target = "x86_64-unknown-linux-gnu"
"#;
        let err = Config::from_str(toml_str).expect_err("should fail");
        assert!(matches!(
            err,
            crate::error::ConfigError::MissingField("manifest_path")
        ));
    }

    #[test]
    fn rejects_bad_python_version() {
        let toml_str = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3"]

[platforms.linux-x86_64-gnu]
target = "x86_64-unknown-linux-gnu"
"#;
        let err = Config::from_str(toml_str).expect_err("should fail");
        assert!(matches!(err, crate::error::ConfigError::Parse(_)));
    }

    #[test]
    fn validates_platform_target_triple() {
        let toml_str = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.bogus]
target = "not-a-real-triple"
"#;
        let err = Config::from_str(toml_str).expect_err("should fail");
        assert!(
            matches!(err, crate::error::ConfigError::BadPlatform { ref reason, .. }
            if reason.contains("unknown target triple"))
        );
    }

    #[test]
    fn accepts_known_target_triples() {
        let toml_str = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target = "x86_64-unknown-linux-gnu"
manylinux = "2_17"

[platforms.linux-aarch64-gnu]
target = "aarch64-unknown-linux-gnu"
manylinux = "2_17"

[platforms.linux-x86_64-musl]
target = "x86_64-unknown-linux-musl"
musllinux = "1_2"

[platforms.macos-x86_64]
target = "x86_64-apple-darwin"
macos_min = "11.0"

[platforms.macos-arm64]
target = "aarch64-apple-darwin"
macos_min = "11.0"
"#;
        let config = Config::from_str(toml_str).expect("parse");
        config.validate().expect("validate");
    }

    #[test]
    fn validates_registry_form() {
        let bad = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target = "x86_64-unknown-linux-gnu"
manylinux = "2_17"

[fixups]
registry = "https://example.com/whatever"
"#;
        let err = Config::from_str(bad).expect_err("should fail");
        // With typed parse, the bad registry is rejected at deserialize time,
        // which folds into ConfigError::Parse.
        assert!(matches!(err, crate::error::ConfigError::Parse(_)));
    }

    #[test]
    fn accepts_registry_forms() {
        for r in [
            "none",
            "file:///tmp/fixups",
            "github.com/rsJames-ttrpg/muntjac-fixups",
        ] {
            let toml_str = format!(
                r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target    = "x86_64-unknown-linux-gnu"
manylinux = "2_17"

[fixups]
registry = "{r}"
"#
            );
            Config::from_str(&toml_str).unwrap_or_else(|_| panic!("parse+validate `{r}`"));
        }
    }

    #[test]
    fn config_parses_file_url_registry() {
        let toml = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target    = "x86_64-unknown-linux-gnu"
manylinux = "2_17"

[fixups]
registry = "file:///abs/path/to/checkout"
"#;
        let config = Config::from_str(toml).expect("parse");
        use crate::fixup::RegistryConfig;
        assert_eq!(
            config.fixups.registry,
            RegistryConfig::FileUrl(std::path::PathBuf::from("/abs/path/to/checkout"))
        );
    }

    #[test]
    fn config_defaults_registry_to_none() {
        let toml = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target    = "x86_64-unknown-linux-gnu"
manylinux = "2_17"
"#;
        let config = Config::from_str(toml).expect("parse");
        use crate::fixup::RegistryConfig;
        assert_eq!(config.fixups.registry, RegistryConfig::None);
    }

    #[test]
    fn parses_lockfile_config_section() {
        let toml_str = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target    = "x86_64-unknown-linux-gnu"
manylinux = "2_17"

[lockfile]
include_groups = ["test", "docs"]
"#;
        let config = Config::from_str(toml_str).expect("parse");
        assert_eq!(
            config.lockfile.include_groups,
            vec!["test".to_string(), "docs".to_string()]
        );
    }

    #[test]
    fn lockfile_config_defaults_to_empty() {
        let toml_str = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target    = "x86_64-unknown-linux-gnu"
manylinux = "2_17"
"#;
        let config = Config::from_str(toml_str).expect("parse");
        assert!(config.lockfile.include_groups.is_empty());
    }

    #[test]
    fn rejects_invalid_group_identifier() {
        let toml_str = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target    = "x86_64-unknown-linux-gnu"
manylinux = "2_17"

[lockfile]
include_groups = ["bad name with space"]
"#;
        let err = Config::from_str(toml_str).expect_err("should fail");
        assert!(matches!(err, crate::error::ConfigError::BadGroupName(_)));
    }

    #[test]
    fn rejects_linux_platform_without_any_baseline() {
        let toml_str = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.linux-bare]
target = "x86_64-unknown-linux-gnu"
"#;
        let err = Config::from_str(toml_str).expect_err("should fail");
        assert!(
            matches!(err, crate::error::ConfigError::BadPlatform { ref reason, .. }
            if reason.contains("manylinux") && reason.contains("musllinux"))
        );
    }

    #[test]
    fn rejects_macos_platform_without_macos_min() {
        let toml_str = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.macos-bare]
target = "aarch64-apple-darwin"
"#;
        let err = Config::from_str(toml_str).expect_err("should fail");
        assert!(
            matches!(err, crate::error::ConfigError::BadPlatform { ref reason, .. }
            if reason.contains("macos_min"))
        );
    }

    #[test]
    fn rejects_bad_baseline_shape() {
        let toml_str = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.linux]
target = "x86_64-unknown-linux-gnu"
manylinux = "2014"
"#;
        let err = Config::from_str(toml_str).expect_err("should fail");
        assert!(
            matches!(err, crate::error::ConfigError::BadPlatform { ref reason, .. }
            if reason.contains("2014") && reason.contains("2_17"))
        );
    }

    #[test]
    fn rejects_musllinux_on_gnu_target() {
        let toml_str = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.confused]
target = "x86_64-unknown-linux-gnu"
musllinux = "1_2"
"#;
        let err = Config::from_str(toml_str).expect_err("should fail");
        assert!(
            matches!(err, crate::error::ConfigError::BadPlatform { ref reason, .. }
            if reason.contains("musllinux") && reason.contains("linux-gnu"))
        );
    }

    #[test]
    fn include_groups_dedups_with_warning() {
        let toml_str = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target = "x86_64-unknown-linux-gnu"
manylinux = "2_17"

[lockfile]
include_groups = ["test", "test", "docs"]
"#;
        let config = Config::from_str(toml_str).expect("parse");
        assert_eq!(
            config.lockfile.include_groups,
            vec!["test".to_string(), "docs".to_string()]
        );
    }

    #[test]
    fn missing_platforms_table_produces_clear_error() {
        let toml_str = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]
"#;
        let err = Config::from_str(toml_str).expect_err("should fail");
        assert!(matches!(
            err,
            crate::error::ConfigError::MissingField("platforms")
        ));
    }

    #[test]
    fn platform_baseline_accessors() {
        let p_linux = Platform {
            target: "x86_64-unknown-linux-gnu".into(),
            manylinux: Some("2_28".into()),
            musllinux: None,
            macos_min: None,
        };
        assert_eq!(p_linux.manylinux_baseline(), Some((2, 28)));
        assert_eq!(p_linux.musllinux_baseline(), None);
        assert_eq!(p_linux.macos_min_parsed(), None);

        let p_mac = Platform {
            target: "aarch64-apple-darwin".into(),
            manylinux: None,
            musllinux: None,
            macos_min: Some("11.0".into()),
        };
        assert_eq!(p_mac.macos_min_parsed(), Some((11, 0)));
    }

    #[test]
    fn registry_rev_with_none_registry_parses_ok() {
        // This invocation triggers the stderr warning (we can't easily
        // capture it in-process; assert behavior: parse succeeds, value
        // is preserved on the struct).
        let toml = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target    = "x86_64-unknown-linux-gnu"
manylinux = "2_17"

[fixups]
registry = "none"
registry_rev = "abc123"
"#;
        let config = Config::from_str(toml).expect("parse");
        use crate::fixup::RegistryConfig;
        assert_eq!(config.fixups.registry, RegistryConfig::None);
        assert_eq!(config.fixups.registry_rev.as_deref(), Some("abc123"));
    }

    #[test]
    fn registry_rev_with_git_registry_no_warning() {
        let toml = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target    = "x86_64-unknown-linux-gnu"
manylinux = "2_17"

[fixups]
registry = "github.com/o/r"
registry_rev = "abc123"
"#;
        let config = Config::from_str(toml).expect("parse");
        use crate::fixup::RegistryConfig;
        match &config.fixups.registry {
            RegistryConfig::Git { url, rev } => {
                assert_eq!(url, "github.com/o/r");
                assert_eq!(rev.as_deref(), Some("abc123"));
            }
            other => panic!("expected Git, got {:?}", other),
        }
    }
}
