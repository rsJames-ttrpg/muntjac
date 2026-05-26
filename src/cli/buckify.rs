//! `muntjac buckify` — read uv.lock + muntjac.toml, emit BUCK + muntjac.bzl + config/BUCK + wiring.bzl.

use std::fs;
use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Result};

use crate::buck::{
    BuckEmitter, BuildEmitContext, StringTemplateEmitter, build_emit_input, build_shared_cfg_input,
    emit_shared_cfg, write_outputs, write_shared_cfg,
};
use crate::cli::Globals;
use crate::config::Config;
use crate::lock;

pub fn run(globals: &Globals, _args: crate::cli::BuckifyArgs) -> Result<()> {
    let cwd = globals.workdir().context("resolving working directory")?;
    let cfg_path = cwd.join("muntjac.toml");
    let cfg_bytes =
        fs::read_to_string(&cfg_path).with_context(|| format!("reading {}", cfg_path.display()))?;
    let config =
        Config::from_str(&cfg_bytes).with_context(|| format!("parsing {}", cfg_path.display()))?;

    let emitter = StringTemplateEmitter;

    // 1. Shared cfg (constraint_settings + config_settings + wiring.bzl),
    //    emitted once at the shared cfg_dir.
    let cfg_dir_rel = config.shared_cfg_dir();
    let shared = emit_shared_cfg(&build_shared_cfg_input(&config));
    write_shared_cfg(&shared, &cwd.join(&cfg_dir_rel))?;
    let cfg_dir_str = cfg_dir_rel.to_string_lossy().into_owned();

    // 2. Per-tree package files (BUCK + muntjac.bzl).
    for tree in crate::cli::resolve_trees(&config, globals.tree.as_deref())? {
        // Resolve uv.lock relative to the tree's manifest directory.
        let manifest_dir = cfg_path
            .parent()
            .unwrap_or(Path::new("."))
            .join(tree.manifest_path.parent().unwrap_or(Path::new("")));
        let lockfile_path = manifest_dir.join("uv.lock");
        let lock_bytes = fs::read_to_string(&lockfile_path)
            .with_context(|| format!("reading {}", lockfile_path.display()))?;
        let lockfile = lock::parser::parse(&lock_bytes)
            .with_context(|| format!("parsing {}", lockfile_path.display()))?;

        let third_party_dir = cwd.join(&tree.third_party_dir);
        let manifest_path = third_party_dir.join("prebake/.manifest.toml");
        let manifest = if manifest_path.is_file() {
            Some(crate::sdist::Manifest::load(&manifest_path)?)
        } else {
            None
        };

        let canonical_third_party_dir =
            std::fs::canonicalize(&third_party_dir).unwrap_or_else(|_| third_party_dir.clone());

        let fixups = crate::fixup::EffectiveFixups::load(
            &config.fixups.registry,
            &third_party_dir,
            config.fixups.allow_local_overrides,
            globals.no_network,
        )
        .with_context(|| format!("loading fixups for tree '{}'", tree.name))?;

        let input = build_emit_input(
            &config,
            tree,
            &lockfile,
            &BuildEmitContext {
                manifest: manifest.as_ref(),
                fixups: Some(&fixups),
                abs_third_party_dir: Some(&canonical_third_party_dir),
                cfg_dir: Some(&cfg_dir_str),
            },
        )?;
        let output = emitter.emit(&input);

        write_outputs(&output, &third_party_dir)?;
    }
    Ok(())
}
