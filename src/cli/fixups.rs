//! `muntjac fixups show <pkg>` — print the parsed-and-merged-from-disk
//! fixup for a package as canonical TOML.

use std::str::FromStr;

use anyhow::{Context, Result, anyhow};
use clap::Subcommand;
use pep508_rs::PackageName;

use crate::cli::Globals;
use crate::config::Config;
use crate::fixup;

#[derive(Subcommand, Debug)]
pub enum FixupsOp {
    /// Print the merged fixup for a package as TOML.
    Show {
        /// PEP 503-normalizable package name.
        package: String,
    },
}

pub fn run(op: FixupsOp, globals: &Globals) -> Result<()> {
    match op {
        FixupsOp::Show { package } => show(package, globals),
    }
}

fn show(package: String, globals: &Globals) -> Result<()> {
    let cwd = globals.workdir().context("resolving working directory")?;
    let cfg_path = cwd.join("muntjac.toml");
    let cfg_bytes = std::fs::read_to_string(&cfg_path)
        .with_context(|| format!("reading {}", cfg_path.display()))?;
    let config =
        Config::from_str(&cfg_bytes).with_context(|| format!("parsing {}", cfg_path.display()))?;

    // S6 v1: use the first tree's third_party_dir. Multi-tree fixup show is
    // future work (use --tree).
    let tree = config
        .trees
        .first()
        .ok_or_else(|| anyhow!("no trees in muntjac.toml"))?;
    let third_party_dir = cwd.join(&tree.third_party_dir);

    let pkg_name = PackageName::from_str(&package)
        .with_context(|| format!("normalizing package name `{}`", package))?;

    let set = fixup::load_local(&third_party_dir)
        .with_context(|| format!("loading fixups under {}", third_party_dir.display()))?;

    let fixup_cfg = set.get(&pkg_name).ok_or_else(|| {
        anyhow!(
            "no fixup for package `{}` at {}/fixups/",
            package,
            third_party_dir.display()
        )
    })?;

    let toml_out = fixup_cfg
        .to_toml_string()
        .context("re-emitting fixup as TOML")?;
    print!("{}", toml_out);
    Ok(())
}
