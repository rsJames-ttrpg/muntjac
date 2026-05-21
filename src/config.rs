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
        self.manylinux.as_deref().map(|s| parse_underscore_pair(s)
            .expect("validated manylinux string"))
    }

    /// Returns the parsed musllinux baseline `(major, minor)` if set.
    /// Assumes `Config::validate` has run; panics if the string is malformed.
    pub fn musllinux_baseline(&self) -> Option<(u32, u32)> {
        self.musllinux.as_deref().map(|s| parse_underscore_pair(s)
            .expect("validated musllinux string"))
    }

    /// Returns the parsed `macos_min` deployment target `(major, minor)` if set.
    /// `macos_min` is the deployment target — the minimum macOS version that
    /// resulting binaries must support. The wheel selector accepts wheels
    /// whose required deployment target is <= this value.
    /// Assumes `Config::validate` has run; panics if the string is malformed.
    pub fn macos_min_parsed(&self) -> Option<(u32, u32)> {
        self.macos_min.as_deref().map(|s| parse_dot_pair(s)
            .expect("validated macos_min string"))
    }
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

/// Raw registry URL as read from muntjac.toml. Parsed/dispatched into
/// `"none"` / `"file://…"` / `"github.com/<owner>/<repo>"` forms by
/// `Config::validate` (Task 4) and by S7's registry fetcher.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(transparent)]
pub struct FixupRegistry(pub String);

impl Default for FixupRegistry {
    fn default() -> Self {
        FixupRegistry("none".into())
    }
}

fn default_registry() -> FixupRegistry {
    FixupRegistry::default()
}
fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct BuckConfig {
    #[serde(default = "default_buck_file_name")]
    pub file_name: String,
    #[serde(default)]
    pub vendor: bool,
}

impl Default for BuckConfig {
    fn default() -> Self {
        Self {
            file_name: default_buck_file_name(),
            vendor: false,
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
        self.lockfile.include_groups.retain(|g| seen.insert(g.clone()));
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
    pub fn validate(&self) -> Result<(), crate::error::ConfigError> {
        for (name, platform) in &self.platforms {
            validate_target_triple(name, &platform.target)?;
            validate_platform_baseline(name, platform)?;
        }
        validate_registry(&self.fixups.registry)?;
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

fn validate_registry(reg: &FixupRegistry) -> Result<(), crate::error::ConfigError> {
    let FixupRegistry(s) = reg;
    if s == "none" {
        return Ok(());
    }
    if s.starts_with("file://") {
        return Ok(());
    }
    if let Some(rest) = s.strip_prefix("github.com/") {
        let parts: Vec<&str> = rest.split('/').collect();
        if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
            return Ok(());
        }
    }
    Err(crate::error::ConfigError::BadRegistry(s.clone()))
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
        assert!(matches!(err, crate::error::ConfigError::BadPlatform { ref reason, .. }
            if reason.contains("unknown target triple")));
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
        assert!(matches!(err, crate::error::ConfigError::BadRegistry(_)));
    }

    #[test]
    fn accepts_registry_forms() {
        for r in [
            "none",
            "file:///tmp/fixups",
            "github.com/jackmpcollins/muntjac-fixups",
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
        assert!(matches!(err, crate::error::ConfigError::BadPlatform { ref reason, .. }
            if reason.contains("manylinux") && reason.contains("musllinux")));
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
        assert!(matches!(err, crate::error::ConfigError::BadPlatform { ref reason, .. }
            if reason.contains("macos_min")));
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
        assert!(matches!(err, crate::error::ConfigError::BadPlatform { ref reason, .. }
            if reason.contains("2014") && reason.contains("2_17")));
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
        assert!(matches!(err, crate::error::ConfigError::BadPlatform { ref reason, .. }
            if reason.contains("musllinux") && reason.contains("linux-gnu")));
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
        assert_eq!(config.lockfile.include_groups, vec!["test".to_string(), "docs".to_string()]);
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
}
