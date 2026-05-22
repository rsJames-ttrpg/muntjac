use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ClassifyError {
    #[error("sdist root `{0}` does not exist or is not a directory")]
    NotADir(PathBuf),

    #[error("failed to read `{path}`: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse `{path}` as TOML: {source}")]
    BadToml {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
}

#[derive(Debug, Error)]
pub enum SdistError {
    #[error("classifier failure: {0}")]
    Classify(#[from] ClassifyError),

    #[error("`uv` binary not found on PATH; install uv (see https://docs.astral.sh/uv)")]
    UvNotFound,

    #[error("failed to spawn `uv`: {source}")]
    UvSpawnFailed {
        #[source]
        source: std::io::Error,
    },

    #[error("`uv build --wheel` failed for {package} {version}:\n--- uv stderr ---\n{stderr}")]
    PrebakeFailed {
        package: String,
        version: String,
        stderr: String,
    },

    #[error(
        "`uv build` for {package} {version} produced {} wheel(s) in {out_dir} (expected 1): {found:?}",
        found.len()
    )]
    PrebakeOutputUnexpected {
        package: String,
        version: String,
        out_dir: PathBuf,
        found: Vec<String>,
    },

    #[error("failed to download `{url}` for {package} {version}: {source}")]
    Download {
        package: String,
        version: String,
        url: String,
        #[source]
        source: reqwest::Error,
    },

    #[error(
        "sha256 mismatch for {package} {version}: lockfile says `{expected}`, got `{actual}`"
    )]
    HashMismatch {
        package: String,
        version: String,
        expected: String,
        actual: String,
    },

    #[error("failed to extract tarball for {package} {version}: {source}")]
    Extract {
        package: String,
        version: String,
        #[source]
        source: std::io::Error,
    },

    #[error("refused to extract `{member}` from {package} {version}: path escapes archive root")]
    PathTraversal {
        package: String,
        version: String,
        member: String,
    },

    #[error("failed to write manifest `{path}`: {source}")]
    ManifestWrite {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse manifest `{path}`: {source}")]
    ManifestParse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("failed to read manifest `{path}`: {source}")]
    ManifestRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}
