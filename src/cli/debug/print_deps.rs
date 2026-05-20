use std::fs;
use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Result};

use crate::cli::Globals;
use crate::cli::debug::PrintDepsArgs;
use crate::config::Config;
use crate::lock;

pub fn run(args: PrintDepsArgs, globals: &Globals) -> Result<()> {
    let cwd = globals.workdir().context("resolving working directory")?;
    let cfg_path = cwd.join("muntjac.toml");
    let cfg_bytes =
        fs::read_to_string(&cfg_path).with_context(|| format!("reading {}", cfg_path.display()))?;
    let config =
        Config::from_str(&cfg_bytes).with_context(|| format!("parsing {}", cfg_path.display()))?;
    config.validate()?;

    let tree = pick_tree(&config, args.tree.as_deref())?;

    // Resolve the lockfile path: relative to the directory containing
    // muntjac.toml, joined to (manifest_path.parent or ".").
    let cfg_dir = cfg_path.parent().unwrap_or(Path::new("."));
    let manifest_dir = cfg_dir.join(tree.manifest_path.parent().unwrap_or(Path::new("")));
    let lockfile_path = manifest_dir.join("uv.lock");
    let lock_bytes = fs::read_to_string(&lockfile_path)
        .with_context(|| format!("reading {}", lockfile_path.display()))?;
    let lockfile = lock::parser::parse(&lock_bytes)
        .with_context(|| format!("parsing {}", lockfile_path.display()))?;

    let graph = lock::graph::build(&lockfile)?;
    lock::graph::detect_cycles(&graph)?;

    let view = lock::resolved::project(&graph, &config, tree);

    let json = if args.pretty {
        serde_json::to_string_pretty(&view)?
    } else {
        serde_json::to_string(&view)?
    };
    println!("{json}");
    Ok(())
}

fn pick_tree<'a>(config: &'a Config, requested: Option<&str>) -> Result<&'a crate::config::Tree> {
    match requested {
        Some(name) => config
            .trees
            .iter()
            .find(|t| t.name == name)
            .ok_or_else(|| anyhow::anyhow!("no tree named `{name}` in muntjac.toml")),
        None => {
            if config.trees.len() == 1 {
                Ok(&config.trees[0])
            } else {
                anyhow::bail!(
                    "multi-tree config: pass --tree <name> (available: {})",
                    config
                        .trees
                        .iter()
                        .map(|t| t.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        }
    }
}
