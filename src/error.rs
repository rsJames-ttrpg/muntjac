use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to parse muntjac.toml: {0}")]
    Parse(#[from] toml::de::Error),

    #[error("missing required field `{0}`")]
    MissingField(&'static str),

    #[error(
        "config cannot mix top-level manifest_path/third_party_dir/python_versions with [tree.*] sections"
    )]
    IncompatibleShape,

    #[error("trees {trees:?} share third_party_dir `{dir}`; each tree needs a distinct directory")]
    DuplicateTreeDir { dir: String, trees: Vec<String> },

    #[error(
        "tree `{tree}`'s third_party_dir `{dir}` collides with the shared cfg_dir; move the tree under a subdirectory or set [buck] cfg_dir explicitly"
    )]
    TreeDirIsCfgDir { tree: String, dir: String },

    #[error(
        "cannot derive a shared cfg_dir: trees have no common parent directory; set [buck] cfg_dir explicitly"
    )]
    CfgDirNotDerivable,

    #[error("invalid platform `{name}`: {reason}")]
    BadPlatform { name: String, reason: String },

    #[error("invalid python version `{0}`")]
    BadPythonVersion(String),

    #[error(
        "invalid registry `{0}`: expected \"none\", \"file://<path>\", or \"github.com/<owner>/<repo>\""
    )]
    BadRegistry(String),

    #[error(
        "registry path must be absolute (got `{path}`); use a full file:// URL or set registry to a path relative to muntjac.toml"
    )]
    RegistryPathNotAbsolute { path: String },

    #[error("invalid dependency-group name `{0}`: must match [a-z][a-z0-9-]*")]
    BadGroupName(String),
}

#[derive(Debug, Error)]
pub enum LockfileError {
    #[error("failed to parse uv.lock: {0}")]
    Parse(#[from] toml::de::Error),

    #[error("unsupported uv.lock version `{0}`; muntjac supports version = 1")]
    UnsupportedVersion(u32),

    #[error("package `{package}`: invalid version `{value}`: {reason}")]
    BadVersion {
        package: String,
        value: String,
        reason: String,
    },

    #[error("package `{package}`: invalid {field} URL `{url}`: {reason}")]
    BadUrl {
        package: String,
        field: &'static str,
        url: String,
        reason: String,
    },

    #[error(
        "package `{package}`: source must have exactly one of registry/git/virtual/editable/directory/path; found {found:?}"
    )]
    AmbiguousSource {
        package: String,
        found: Vec<&'static str>,
    },

    #[error("package `{package}`: dep `{dep}`: marker `{marker}` failed to parse: {reason}")]
    BadMarker {
        package: String,
        dep: String,
        marker: String,
        reason: String,
    },

    #[error("package `{from_pkg}`: dep `{dep}` has no matching [[package]] entry")]
    UnresolvedDep { from_pkg: String, dep: String },

    #[error(
        "duplicate package name `{name}` (found versions {versions:?}); S1 expects one version per name"
    )]
    DuplicatePackageName { name: String, versions: Vec<String> },

    #[error(fmt = fmt_cycle)]
    Cycle(Vec<Vec<String>>),

    #[error("invalid package name `{0}`: {1}")]
    BadPackageName(String, String),
}

#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("could not resolve cache root: {0}")]
    CacheRootResolve(String),

    #[error("could not create cache directory {path}: {source}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[derive(Debug, Error)]
pub enum VendorError {
    #[error("downloading {package} {version} from {url}: {source}")]
    Download {
        package: String,
        version: String,
        url: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("sha256 mismatch for {package} {version}: expected {expected}, got {actual}")]
    HashMismatch {
        package: String,
        version: String,
        expected: String,
        actual: String,
    },

    #[error(
        "--mode=committed requires network access; remove --no-network, or vendor first then re-run with --frozen"
    )]
    ModeNetworkConflict,
}

#[derive(Debug, Error)]
pub enum BuckifyError {
    #[error(fmt = fmt_missing_vendor_wheels)]
    MissingVendorWheels {
        tree: String,
        /// Each entry is `(package, version, expected wheel filename)`.
        missing: Vec<(String, String, String)>,
    },
}

fn fmt_missing_vendor_wheels(
    tree: &String,
    missing: &Vec<(String, String, String)>,
    f: &mut std::fmt::Formatter<'_>,
) -> std::fmt::Result {
    writeln!(
        f,
        "vendor mode requires committed wheels; tree '{tree}' is missing:"
    )?;
    for (pkg, ver, file) in missing {
        writeln!(f, "  - {pkg} {ver} ({file})")?;
    }
    write!(f, "run `muntjac vendor` to populate the vendor directory.")
}

fn fmt_cycle(cycles: &Vec<Vec<String>>, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    writeln!(f, "dependency cycle(s) detected:")?;
    for cycle in cycles {
        write!(f, "  - ")?;
        for (i, node) in cycle.iter().enumerate() {
            if i > 0 {
                write!(f, " -> ")?;
            }
            write!(f, "{node}")?;
        }
        if let Some(first) = cycle.first() {
            writeln!(f, " -> {first}")?;
        } else {
            writeln!(f)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_root_resolve_error_message() {
        let e = CacheError::CacheRootResolve("no XDG_CACHE_HOME or HOME".into());
        assert_eq!(
            e.to_string(),
            "could not resolve cache root: no XDG_CACHE_HOME or HOME"
        );
    }

    #[test]
    fn registry_path_not_absolute_message_is_exact() {
        let e = ConfigError::RegistryPathNotAbsolute {
            path: "./registry".into(),
        };
        assert_eq!(
            e.to_string(),
            "registry path must be absolute (got `./registry`); use a full file:// URL or set registry to a path relative to muntjac.toml"
        );
    }

    #[test]
    fn duplicate_tree_dir_message_is_exact() {
        let e = ConfigError::DuplicateTreeDir {
            dir: "tp/shared".into(),
            trees: vec!["a".into(), "b".into()],
        };
        assert_eq!(
            e.to_string(),
            "trees [\"a\", \"b\"] share third_party_dir `tp/shared`; each tree needs a distinct directory"
        );
    }

    #[test]
    fn tree_dir_is_cfg_dir_message_is_exact() {
        let e = ConfigError::TreeDirIsCfgDir {
            tree: "a".into(),
            dir: "tp".into(),
        };
        assert_eq!(
            e.to_string(),
            "tree `a`'s third_party_dir `tp` collides with the shared cfg_dir; move the tree under a subdirectory or set [buck] cfg_dir explicitly"
        );
    }

    #[test]
    fn cfg_dir_not_derivable_message_is_exact() {
        let e = ConfigError::CfgDirNotDerivable;
        assert_eq!(
            e.to_string(),
            "cannot derive a shared cfg_dir: trees have no common parent directory; set [buck] cfg_dir explicitly"
        );
    }

    #[test]
    fn vendor_error_hash_mismatch_message_is_exact() {
        let e = VendorError::HashMismatch {
            package: "idna".into(),
            version: "3.10".into(),
            expected: "deadbeef".into(),
            actual: "cafef00d".into(),
        };
        assert_eq!(
            e.to_string(),
            "sha256 mismatch for idna 3.10: expected deadbeef, got cafef00d"
        );
    }

    #[test]
    fn vendor_error_mode_network_conflict_message_is_exact() {
        let e = VendorError::ModeNetworkConflict;
        assert_eq!(
            e.to_string(),
            "--mode=committed requires network access; remove --no-network, or vendor first then re-run with --frozen"
        );
    }

    #[test]
    fn buckify_error_missing_vendor_wheels_message_lists_each() {
        let e = BuckifyError::MissingVendorWheels {
            tree: "default".into(),
            missing: vec![
                (
                    "idna".into(),
                    "3.10".into(),
                    "idna-3.10-py3-none-any.whl".into(),
                ),
                (
                    "urllib3".into(),
                    "2.2.3".into(),
                    "urllib3-2.2.3-py3-none-any.whl".into(),
                ),
            ],
        };
        let s = e.to_string();
        assert!(s.contains("tree 'default'"), "missing tree label: {s}");
        assert!(
            s.contains("idna 3.10 (idna-3.10-py3-none-any.whl)"),
            "missing idna line: {s}"
        );
        assert!(
            s.contains("urllib3 2.2.3 (urllib3-2.2.3-py3-none-any.whl)"),
            "missing urllib3 line: {s}"
        );
        assert!(s.contains("muntjac vendor"), "missing suggestion: {s}");
    }

    #[test]
    fn lockfile_error_messages_include_context() {
        let err = LockfileError::UnsupportedVersion(2);
        assert_eq!(
            err.to_string(),
            "unsupported uv.lock version `2`; muntjac supports version = 1"
        );

        let err = LockfileError::UnresolvedDep {
            from_pkg: "requests-2.34.2".into(),
            dep: "ghost".into(),
        };
        assert!(err.to_string().contains("requests-2.34.2"));
        assert!(err.to_string().contains("ghost"));

        let err = LockfileError::DuplicatePackageName {
            name: "numpy".into(),
            versions: vec!["1.2.0".into(), "2.0.0".into()],
        };
        assert!(err.to_string().contains("numpy"));
    }
}
