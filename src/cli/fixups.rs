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

    let tree = config
        .trees
        .first()
        .ok_or_else(|| anyhow!("no trees in muntjac.toml"))?;
    let third_party_dir = cwd.join(&tree.third_party_dir);

    let pkg_name = PackageName::from_str(&package)
        .with_context(|| format!("normalizing package name `{}`", package))?;

    let eff = fixup::EffectiveFixups::load(
        &config.fixups.registry,
        &third_party_dir,
        config.fixups.allow_local_overrides,
    )
    .with_context(|| format!("loading layered fixups for tree '{}'", tree.name))?;

    let community_cfg = eff.community.get(&pkg_name);
    let local_cfg = eff.local.get(&pkg_name);

    if community_cfg.is_none() && local_cfg.is_none() {
        let community_path = match &config.fixups.registry {
            fixup::RegistryConfig::None => "(none)".to_string(),
            fixup::RegistryConfig::FileUrl(p) => p.join("packages").display().to_string(),
            fixup::RegistryConfig::Git { url, .. } => format!("git: {}", url),
        };
        let local_path = third_party_dir.join("fixups").display().to_string();
        anyhow::bail!(
            "no fixup for package '{}' (checked community at {}, local at {})",
            package,
            community_path,
            local_path,
        );
    }

    let both_present = community_cfg.is_some() && local_cfg.is_some();

    if let Some(c) = community_cfg {
        if both_present {
            if let fixup::RegistryConfig::FileUrl(p) = &config.fixups.registry {
                let community_file = p
                    .join("packages")
                    .join(package.to_lowercase())
                    .join("fixups.toml");
                println!("# community: {}", community_file.display());
            } else {
                println!("# community:");
            }
        }
        print!(
            "{}",
            c.to_toml_string()
                .context("re-emitting community fixup as TOML")?
        );
        if both_present {
            println!();
            if local_cfg.is_some_and(|l| l.replace_community) {
                println!("# (community fixup above is disabled by replace_community = true)");
            }
        }
    }

    if let Some(l) = local_cfg {
        if both_present {
            let local_file = third_party_dir
                .join("fixups")
                .join(package.to_lowercase())
                .join("fixups.toml");
            println!("# local: {}", local_file.display());
        }
        print!(
            "{}",
            l.to_toml_string()
                .context("re-emitting local fixup as TOML")?
        );
    }

    Ok(())
}
