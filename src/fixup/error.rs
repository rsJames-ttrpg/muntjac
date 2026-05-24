use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
#[allow(dead_code)]
pub enum FixupError {
    #[error("failed to parse fixup at {file}:\n  {source}")]
    ParseError {
        file: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error(
        "unknown field `{field}` in fixup at {file}\n  v1 schema: see docs/superpowers/specs/2026-05-20-muntjac-design.md §7"
    )]
    UnknownField { file: PathBuf, field: String },

    #[error("failed to parse cfg() in {file} section `{section}`:\n  {source}")]
    CfgParse {
        file: PathBuf,
        section: String,
        #[source]
        source: CfgParseError,
    },

    #[error(
        "entry_points = true is not supported in v1; list the binaries explicitly (e.g. entry_points = [\"ruff\"]).\n  package: {pkg}"
    )]
    EntryPointsAuto { pkg: String },

    #[error(
        "prefer_wheel sha256:{sha} not found for {pkg} on cell {cell}.\n  available wheel shas: {available}"
    )]
    PreferWheelNotFound {
        pkg: String,
        sha: String,
        cell: String,
        available: String,
    },

    #[error(
        "exclude_wheels eliminates every wheel for {pkg} on cell {cell}.\n  loosen the patterns or remove the fixup"
    )]
    ExcludeWheelsLeavesNone { pkg: String, cell: String },

    #[error(
        "overlay/ contains a symlink to outside the fixup directory: {file}\n  refusing for safety"
    )]
    OverlayPathOutsideTree { file: PathBuf },

    #[error("overlay = \"{path}\" is set for {pkg} but the directory is empty or missing")]
    OverlayEmpty { pkg: String, path: String },

    #[error(
        "replace_deps target for {pkg} is not a valid Buck target: `{target}`\n  expected //path:name or :name form"
    )]
    ReplaceDepInvalid { pkg: String, target: String },

    #[error(
        "extra_deps target for {pkg} is not a valid Buck target: `{target}`\n  expected //path:name or :name form"
    )]
    ExtraDepInvalid { pkg: String, target: String },

    #[error(
        "unknown cfg atom `{atom}`; expected one of: version, python, target_os, target_arch, target_env"
    )]
    BadCfgAtom { atom: String },

    #[error(
        "fixup file {file} sets `replace_community = true`, which is only valid in local fixups, not in the community registry"
    )]
    ReplaceCommunityInCommunity { file: PathBuf },

    #[error(
        "offline mode but cache miss for pinned rev `{pin}`\n  run `muntjac fixups update` (without --offline) first"
    )]
    Offline { pin: String },

    #[error(
        "cache entry at {path} appears corrupt: {reason}\n  delete it and re-run `muntjac fixups update`"
    )]
    CacheCorrupt { path: PathBuf, reason: String },

    #[error(fmt = fmt_git_fetch)]
    GitFetch {
        url: String,
        rev: Option<String>,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },

    #[error("community registry path {path} does not exist")]
    RegistryPathNotFound { path: PathBuf },

    #[error("I/O error reading fixups directory {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

#[allow(clippy::borrowed_box)]
fn fmt_git_fetch(
    url: &str,
    rev: &Option<String>,
    source: &Box<dyn std::error::Error + Send + Sync + 'static>,
    f: &mut std::fmt::Formatter<'_>,
) -> std::fmt::Result {
    write!(f, "git fetch failed for {url}")?;
    if let Some(r) = rev {
        write!(f, " @ {r}")?;
    }
    write!(f, ": {source}")
}

/// Errors from `cfg.rs` parser; wrapped by `FixupError::CfgParse`.
#[derive(Debug, thiserror::Error)]
pub enum CfgParseError {
    #[error("at byte {offset}: {message}")]
    AtOffset { offset: usize, message: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn unknown_field_message_is_exact() {
        let e = FixupError::UnknownField {
            file: PathBuf::from("fixups/pillow/fixups.toml"),
            field: "extra_things".into(),
        };
        assert_eq!(
            e.to_string(),
            "unknown field `extra_things` in fixup at fixups/pillow/fixups.toml\n  v1 schema: see docs/superpowers/specs/2026-05-20-muntjac-design.md §7"
        );
    }

    #[test]
    fn entry_points_auto_message_is_exact() {
        let e = FixupError::EntryPointsAuto {
            pkg: "pillow".into(),
        };
        assert_eq!(
            e.to_string(),
            "entry_points = true is not supported in v1; list the binaries explicitly (e.g. entry_points = [\"ruff\"]).\n  package: pillow"
        );
    }

    #[test]
    fn overlay_empty_message_is_exact() {
        let e = FixupError::OverlayEmpty {
            pkg: "pillow".into(),
            path: "overlay/".into(),
        };
        assert_eq!(
            e.to_string(),
            "overlay = \"overlay/\" is set for pillow but the directory is empty or missing"
        );
    }

    #[test]
    fn bad_cfg_atom_message_is_exact() {
        let e = FixupError::BadCfgAtom {
            atom: "target_family".into(),
        };
        assert_eq!(
            e.to_string(),
            "unknown cfg atom `target_family`; expected one of: version, python, target_os, target_arch, target_env"
        );
    }

    #[test]
    fn replace_community_in_community_message_is_exact() {
        let e = FixupError::ReplaceCommunityInCommunity {
            file: PathBuf::from("registry/packages/pillow/fixups.toml"),
        };
        assert_eq!(
            e.to_string(),
            "fixup file registry/packages/pillow/fixups.toml sets `replace_community = true`, which is only valid in local fixups, not in the community registry"
        );
    }

    #[test]
    fn offline_message_is_exact() {
        let e = FixupError::Offline {
            pin: "abc123def456".into(),
        };
        assert_eq!(
            e.to_string(),
            "offline mode but cache miss for pinned rev `abc123def456`\n  run `muntjac fixups update` (without --offline) first"
        );
    }

    #[test]
    fn offline_default_branch_pin_message_is_exact() {
        let e = FixupError::Offline {
            pin: "(default branch)".into(),
        };
        assert_eq!(
            e.to_string(),
            "offline mode but cache miss for pinned rev `(default branch)`\n  run `muntjac fixups update` (without --offline) first"
        );
    }

    #[test]
    fn cache_corrupt_message_is_exact() {
        let e = FixupError::CacheCorrupt {
            path: PathBuf::from("/tmp/muntjac/fixups/abc"),
            reason: "missing packages/ subdir".into(),
        };
        assert_eq!(
            e.to_string(),
            "cache entry at /tmp/muntjac/fixups/abc appears corrupt: missing packages/ subdir\n  delete it and re-run `muntjac fixups update`"
        );
    }

    #[test]
    fn registry_path_not_found_message_is_exact() {
        let e = FixupError::RegistryPathNotFound {
            path: PathBuf::from("/tmp/no/such/packages"),
        };
        assert_eq!(
            e.to_string(),
            "community registry path /tmp/no/such/packages does not exist"
        );
    }
}
