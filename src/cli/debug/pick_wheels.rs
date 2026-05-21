//! `muntjac debug pick-wheels` — print the wheel selected for each
//! (package, version, platform, python_version) cell as JSON.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Result};
use serde::Serialize;

use crate::cli::Globals;
use crate::cli::debug::PickWheelsArgs;
use crate::config::{Config, PythonVersion, Tree};
use crate::lock;
use crate::lock::types::{Lockfile, Wheel};
use crate::wheel::{
    AbiTag, LinuxArch, MacArch, PickResult, PlatformTag, PythonTag, Tag, build_compatible_tags,
    pick_wheel,
};

// ---- JSON output shape (spec §9) ----

#[derive(Serialize)]
struct Output {
    schema_version: u32,
    tree: String,
    selections: Vec<Selection>,
}

#[derive(Serialize)]
struct Selection {
    package: String,
    version: String,
    platform: String,
    python_version: String,
    #[serde(flatten)]
    outcome: SelectionOutcome,
}

#[derive(Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
enum SelectionOutcome {
    Picked {
        wheel: WheelOutput,
        matched_tag: String,
        rank: usize,
        wheels_considered: usize,
    },
    NoWheel {
        wheels_considered: usize,
        wheels: Vec<String>,
    },
}

#[derive(Serialize)]
struct WheelOutput {
    filename: String,
    url: String,
    hash: String,
}

// ---- Handler ----

pub fn run(args: PickWheelsArgs, globals: &Globals) -> Result<()> {
    let cwd = globals.workdir().context("resolving working directory")?;
    let cfg_path = cwd.join("muntjac.toml");
    let cfg_bytes = fs::read_to_string(&cfg_path)
        .with_context(|| format!("reading {}", cfg_path.display()))?;
    let config = Config::from_str(&cfg_bytes)
        .with_context(|| format!("parsing {}", cfg_path.display()))?;
    // Config::from_str already validates.

    let mut outputs: Vec<Output> = Vec::new();
    for tree in &config.trees {
        if let Some(name) = &args.tree {
            if &tree.name != name {
                continue;
            }
        }
        outputs.push(build_tree_output(&cfg_path, &config, tree, &args)?);
    }

    // Single tree -> object; multi-tree -> array.
    let json = if outputs.len() == 1 {
        serde_json::to_string_pretty(&outputs[0]).context("rendering JSON")?
    } else {
        serde_json::to_string_pretty(&outputs).context("rendering JSON")?
    };
    println!("{json}");
    Ok(())
}

// ---- Per-tree pipeline ----

fn build_tree_output(
    cfg_path: &Path,
    config: &Config,
    tree: &Tree,
    args: &PickWheelsArgs,
) -> Result<Output> {
    // Locate uv.lock the same way print_deps does: relative to muntjac.toml's directory.
    let cfg_dir = cfg_path.parent().unwrap_or(Path::new("."));
    let manifest_dir = cfg_dir.join(tree.manifest_path.parent().unwrap_or(Path::new("")));
    let lockfile_path = manifest_dir.join("uv.lock");
    let lock_bytes = fs::read_to_string(&lockfile_path)
        .with_context(|| format!("reading {}", lockfile_path.display()))?;
    let lockfile = lock::parser::parse(&lock_bytes)
        .with_context(|| format!("parsing {}", lockfile_path.display()))?;

    let graph = lock::graph::build(&lockfile)?;
    lock::graph::detect_cycles(&graph)?;

    let view = lock::resolved::project(&graph, config, tree);

    // (name, version) -> wheels. ResolvedPackage carries no wheel data, so we
    // cross-reference into the Lockfile here.
    let wheels_index = build_wheels_index(&lockfile);

    let mut selections: Vec<Selection> = Vec::new();
    for resolved_cfg in &view.configs {
        if let Some(filter) = &args.platform {
            if &resolved_cfg.platform != filter {
                continue;
            }
        }
        if let Some(filter) = &args.python {
            if &resolved_cfg.python_version != filter {
                continue;
            }
        }

        // Compat tags for this cell.
        let platform = config
            .platforms
            .get(&resolved_cfg.platform)
            .expect("ResolvedConfig.platform was sourced from config.platforms");
        let py = PythonVersion::from_str(&resolved_cfg.python_version)
            .expect("ResolvedConfig.python_version was sourced from tree.python_versions");
        let compat = build_compatible_tags(platform, py);

        for pkg in &resolved_cfg.packages {
            if let Some(filter) = &args.package {
                if &pkg.name != filter {
                    continue;
                }
            }
            // Skip packages with no wheel artifacts: first-party packages always
            // have empty wheels; registry/git packages without wheel entries are
            // silently skipped (sdist-only — out of scope for this command).
            let wheels = match wheels_index.get(&(pkg.name.clone(), pkg.version.clone())) {
                Some(ws) if !ws.is_empty() => *ws,
                _ => continue,
            };

            let outcome = match pick_wheel(wheels, &compat) {
                PickResult::Picked {
                    wheel,
                    matched_tag,
                    rank,
                } => SelectionOutcome::Picked {
                    wheel: WheelOutput {
                        filename: wheel.filename.clone(),
                        url: wheel.url.to_string(),
                        hash: wheel.hash.clone(),
                    },
                    matched_tag: render_tag(&matched_tag),
                    rank,
                    wheels_considered: wheels.len(),
                },
                PickResult::NoWheel => SelectionOutcome::NoWheel {
                    wheels_considered: wheels.len(),
                    wheels: wheels.iter().map(|w| w.filename.clone()).collect(),
                },
            };

            selections.push(Selection {
                package: pkg.name.clone(),
                version: pkg.version.clone(),
                platform: resolved_cfg.platform.clone(),
                python_version: resolved_cfg.python_version.clone(),
                outcome,
            });
        }
    }

    // Deterministic sort: (package, version, platform, python_version).
    selections.sort_by(|a, b| {
        a.package
            .cmp(&b.package)
            .then(a.version.cmp(&b.version))
            .then(a.platform.cmp(&b.platform))
            .then(a.python_version.cmp(&b.python_version))
    });

    Ok(Output {
        schema_version: 1,
        tree: tree.name.clone(),
        selections,
    })
}

fn build_wheels_index(lockfile: &Lockfile) -> BTreeMap<(String, String), &[Wheel]> {
    let mut idx: BTreeMap<(String, String), &[Wheel]> = BTreeMap::new();
    for pkg in &lockfile.packages {
        idx.insert(
            (pkg.name.as_ref().to_string(), pkg.version.to_string()),
            pkg.wheels.as_slice(),
        );
    }
    idx
}

fn render_tag(t: &Tag) -> String {
    let python = match &t.python {
        PythonTag::CPython(maj, min) => format!("cp{maj}{min}"),
        PythonTag::Py(maj, Some(min)) => format!("py{maj}{min}"),
        PythonTag::Py(maj, None) => format!("py{maj}"),
        PythonTag::Other(s) => s.clone(),
    };
    let abi = match &t.abi {
        AbiTag::CPython(maj, min) => format!("cp{maj}{min}"),
        AbiTag::Abi3 => "abi3".into(),
        AbiTag::None => "none".into(),
        AbiTag::Other(s) => s.clone(),
    };
    let plat = match &t.plat {
        PlatformTag::Any => "any".into(),
        PlatformTag::ManyLinux { major, minor, arch } => {
            let a = match arch {
                LinuxArch::X86_64 => "x86_64",
                LinuxArch::Aarch64 => "aarch64",
            };
            format!("manylinux_{major}_{minor}_{a}")
        }
        PlatformTag::MuslLinux { major, minor, arch } => {
            let a = match arch {
                LinuxArch::X86_64 => "x86_64",
                LinuxArch::Aarch64 => "aarch64",
            };
            format!("musllinux_{major}_{minor}_{a}")
        }
        PlatformTag::MacOs { major, minor, arch } => {
            let a = match arch {
                MacArch::X86_64 => "x86_64",
                MacArch::Arm64 => "arm64",
                MacArch::Universal2 => "universal2",
            };
            format!("macosx_{major}_{minor}_{a}")
        }
        PlatformTag::Other(s) => s.clone(),
    };
    format!("{python}-{abi}-{plat}")
}
