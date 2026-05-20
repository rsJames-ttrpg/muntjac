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

    #[error("invalid platform `{name}`: {reason}")]
    BadPlatform { name: String, reason: String },

    #[error("invalid python version `{0}`")]
    BadPythonVersion(String),

    #[error(
        "invalid registry `{0}`: expected \"none\", \"file://<path>\", or \"github.com/<owner>/<repo>\""
    )]
    BadRegistry(String),

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

    #[error("dependency cycle(s) detected: {0:?}")]
    Cycle(Vec<String>),

    #[error("invalid package name `{0}`: {1}")]
    BadPackageName(String, String),
}

#[cfg(test)]
mod tests {
    use super::*;

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
