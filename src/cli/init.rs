use anyhow::{Context, Result};
use clap::Args;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

use crate::cli::Globals;
use crate::config::PythonVersion;

const LATEST_KNOWN_STABLE_PY: u8 = 13;

#[derive(Args, Debug)]
pub struct InitArgs {
    /// Overwrite existing muntjac.toml.
    #[arg(long)]
    pub force: bool,
    /// Target directory (defaults to current dir).
    pub path: Option<PathBuf>,
}

pub struct Detection {
    pub path: PathBuf,
    pub python_versions: Vec<PythonVersion>,
}

pub fn run(_args: InitArgs, _globals: &Globals) -> Result<()> {
    anyhow::bail!("init file-writing not yet implemented (filled in by Task 9)")
}

pub fn find_pyproject(start: &Path) -> Option<Detection> {
    let mut cur = Some(start);
    while let Some(dir) = cur {
        let candidate = dir.join("pyproject.toml");
        if candidate.is_file() {
            if let Ok(bytes) = fs::read_to_string(&candidate) {
                if has_project_or_uv(&bytes) {
                    let python_versions = extract_python_versions(&bytes);
                    return Some(Detection { path: candidate, python_versions });
                }
            }
        }
        cur = dir.parent();
    }
    None
}

#[derive(Deserialize)]
struct PyprojectProbe {
    project: Option<ProjectTable>,
    tool: Option<ToolTable>,
}

#[derive(Deserialize)]
struct ProjectTable {
    #[serde(rename = "requires-python")]
    requires_python: Option<String>,
}

#[derive(Deserialize)]
struct ToolTable {
    uv: Option<toml::Value>,
}

fn has_project_or_uv(toml_src: &str) -> bool {
    let probe: Result<PyprojectProbe, _> = toml::from_str(toml_src);
    match probe {
        Ok(p) => p.project.is_some() || p.tool.and_then(|t| t.uv).is_some(),
        Err(_) => false,
    }
}

fn extract_python_versions(toml_src: &str) -> Vec<PythonVersion> {
    let probe: Result<PyprojectProbe, _> = toml::from_str(toml_src);
    if let Ok(p) = probe {
        if let Some(rp) = p.project.and_then(|p| p.requires_python) {
            if let Ok(versions) = expand_requires_python(&rp) {
                if !versions.is_empty() {
                    return versions;
                }
            }
        }
    }
    vec![PythonVersion(3, 12)]
}

pub fn expand_requires_python(spec: &str) -> Result<Vec<PythonVersion>> {
    // Very small subset: ">=X.Y", ">=X.Y,<X.Z", "==X.Y.*", "==X.Y".
    let mut min_minor: u8 = 11; // muntjac MVP floor
    let mut max_minor: u8 = LATEST_KNOWN_STABLE_PY;
    for part in spec.split(',') {
        let part = part.trim();
        if let Some(rest) = part.strip_prefix(">=") {
            let (_, minor) = parse_two(rest)?;
            min_minor = min_minor.max(minor);
        } else if let Some(rest) = part.strip_prefix("<") {
            let (_, minor) = parse_two(rest)?;
            max_minor = max_minor.min(minor.saturating_sub(1));
        } else if let Some(rest) = part.strip_prefix("==") {
            let rest = rest.trim_end_matches(".*");
            let parts: Vec<&str> = rest.split('.').collect();
            if parts.len() >= 2 {
                let minor: u8 = parts[1].parse().context("bad minor in == bound")?;
                min_minor = minor;
                max_minor = minor;
            }
        }
    }
    let mut out = Vec::new();
    for m in min_minor..=max_minor {
        out.push(PythonVersion(3, m));
    }
    Ok(out)
}

fn parse_two(s: &str) -> Result<(u8, u8)> {
    let parts: Vec<&str> = s.split('.').collect();
    let major: u8 = parts.first().context("missing major")?.parse().context("major not u8")?;
    let minor: u8 = parts.get(1).copied().unwrap_or("0").parse().context("minor not u8")?;
    Ok((major, minor))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn detects_pyproject_in_cwd() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("pyproject.toml"), "[project]\nname = \"x\"\nrequires-python = \">=3.11\"\n").unwrap();
        let found = find_pyproject(dir.path()).expect("detected");
        assert_eq!(found.path, dir.path().join("pyproject.toml"));
        assert_eq!(found.python_versions, vec![PythonVersion(3, 11), PythonVersion(3, 12), PythonVersion(3, 13)]);
    }

    #[test]
    fn detects_pyproject_in_parent() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("pyproject.toml"), "[project]\nname = \"x\"\n").unwrap();
        let subdir = dir.path().join("sub");
        fs::create_dir(&subdir).unwrap();
        let found = find_pyproject(&subdir).expect("detected");
        assert_eq!(found.path, dir.path().join("pyproject.toml"));
        assert_eq!(found.python_versions, vec![PythonVersion(3, 12)]);
    }

    #[test]
    fn no_detection_in_empty_tree() {
        let dir = tempdir().unwrap();
        assert!(find_pyproject(dir.path()).is_none());
    }

    #[test]
    fn skips_pyproject_without_project_or_tool_uv() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("pyproject.toml"), "[build-system]\nrequires = []\n").unwrap();
        assert!(find_pyproject(dir.path()).is_none());
    }

    #[test]
    fn detects_pyproject_with_tool_uv() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("pyproject.toml"), "[tool.uv]\n").unwrap();
        assert!(find_pyproject(dir.path()).is_some());
    }

    #[test]
    fn expands_requires_python_ranges() {
        assert_eq!(expand_requires_python(">=3.10").unwrap(),
                   vec![PythonVersion(3, 11), PythonVersion(3, 12), PythonVersion(3, 13)]);
        assert_eq!(expand_requires_python(">=3.11,<3.13").unwrap(),
                   vec![PythonVersion(3, 11), PythonVersion(3, 12)]);
        assert_eq!(expand_requires_python("==3.12.*").unwrap(),
                   vec![PythonVersion(3, 12)]);
    }
}
