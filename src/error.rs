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
}
