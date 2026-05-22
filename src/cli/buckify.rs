//! `muntjac buckify` — read uv.lock + muntjac.toml, emit BUCK + muntjac.bzl + config/BUCK + wiring.bzl.

use std::fs;
use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Result};

use crate::buck::{BuckEmitter, StringTemplateEmitter, build_emit_input, write_outputs};
use crate::cli::Globals;
use crate::config::Config;
use crate::lock;

pub fn run(globals: &Globals) -> Result<()> {
    let cwd = globals.workdir().context("resolving working directory")?;
    let cfg_path = cwd.join("muntjac.toml");
    let cfg_bytes =
        fs::read_to_string(&cfg_path).with_context(|| format!("reading {}", cfg_path.display()))?;
    let config =
        Config::from_str(&cfg_bytes).with_context(|| format!("parsing {}", cfg_path.display()))?;

    let emitter = StringTemplateEmitter;
    let cfg_dir = cfg_path.parent().unwrap_or(Path::new("."));

    for tree in &config.trees {
        if let Some(filter) = &globals.tree {
            if &tree.name != filter {
                continue;
            }
        }

        // Resolve uv.lock relative to the tree's manifest directory.
        let manifest_dir = cfg_dir.join(tree.manifest_path.parent().unwrap_or(Path::new("")));
        let lockfile_path = manifest_dir.join("uv.lock");
        let lock_bytes = fs::read_to_string(&lockfile_path)
            .with_context(|| format!("reading {}", lockfile_path.display()))?;
        let lockfile = lock::parser::parse(&lock_bytes)
            .with_context(|| format!("parsing {}", lockfile_path.display()))?;

        let input = build_emit_input(&config, tree, &lockfile)?;
        let output = emitter.emit(&input);

        let third_party_dir = cwd.join(&tree.third_party_dir);
        write_outputs(&output, &third_party_dir)?;
    }
    Ok(())
}
