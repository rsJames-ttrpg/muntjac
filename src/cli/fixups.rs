//! `muntjac fixups show <pkg>` — print the parsed-and-merged-from-disk
//! fixup for a package as canonical TOML.

use std::str::FromStr;

use anyhow::{Context, Result};
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
    /// Fetch the registry and update `registry_rev` in muntjac.toml.
    Update {
        /// SHA, branch, or tag to fetch. Default: HEAD of the
        /// registry's default branch.
        #[arg(long)]
        rev: Option<String>,
    },
}

pub fn run(op: FixupsOp, globals: &Globals) -> Result<()> {
    match op {
        FixupsOp::Show { package } => show(package, globals),
        FixupsOp::Update { rev } => update(rev, globals),
    }
}

fn show(package: String, globals: &Globals) -> Result<()> {
    let cwd = globals.workdir().context("resolving working directory")?;
    let cfg_path = cwd.join("muntjac.toml");
    let cfg_bytes = std::fs::read_to_string(&cfg_path)
        .with_context(|| format!("reading {}", cfg_path.display()))?;
    let config =
        Config::from_str(&cfg_bytes).with_context(|| format!("parsing {}", cfg_path.display()))?;

    let pkg_name = PackageName::from_str(&package)
        .with_context(|| format!("normalizing package name `{}`", package))?;

    let trees = crate::cli::resolve_trees(&config, globals.tree.as_deref())?;
    let multi = trees.len() > 1;
    for tree in trees {
        if multi {
            println!("# ===== tree: {} =====", tree.name);
        }
        let third_party_dir = cwd.join(&tree.third_party_dir);
        show_one_tree(
            &config,
            &third_party_dir,
            &tree.name,
            &package,
            &pkg_name,
            globals,
            /* bail_if_missing = */ !multi,
        )?;
    }
    Ok(())
}

fn show_one_tree(
    config: &Config,
    third_party_dir: &std::path::Path,
    tree_name: &str,
    package: &str,
    pkg_name: &pep508_rs::PackageName,
    globals: &Globals,
    bail_if_missing: bool,
) -> Result<()> {
    let eff = fixup::EffectiveFixups::load(
        &config.fixups.registry,
        third_party_dir,
        config.fixups.allow_local_overrides,
        globals.no_network,
    )
    .with_context(|| format!("loading layered fixups for tree '{}'", tree_name))?;

    let community_cfg = eff.community.get(pkg_name);
    let local_cfg = eff.local.get(pkg_name);

    if community_cfg.is_none() && local_cfg.is_none() {
        if bail_if_missing {
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
        } else {
            println!("# (no fixup for '{}' in tree '{}')", package, tree_name);
            return Ok(());
        }
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

fn update(rev: Option<String>, globals: &Globals) -> Result<()> {
    use crate::fixup::RegistryConfig;

    let cwd = globals.workdir().context("resolving working directory")?;
    let cfg_path = cwd.join("muntjac.toml");
    let cfg_bytes = std::fs::read_to_string(&cfg_path)
        .with_context(|| format!("reading {}", cfg_path.display()))?;
    let config =
        Config::from_str(&cfg_bytes).with_context(|| format!("parsing {}", cfg_path.display()))?;

    // 1. Validate registry is Git-form.
    let (url, prior_rev) = match &config.fixups.registry {
        RegistryConfig::Git { url, rev } => (url.clone(), rev.clone()),
        RegistryConfig::None => {
            anyhow::bail!(
                "muntjac fixups update requires a git-based registry; current registry is \"none\""
            );
        }
        RegistryConfig::FileUrl(p) => {
            anyhow::bail!(
                "muntjac fixups update requires a git-based registry; current registry is file:// directory form at {}",
                p.display()
            );
        }
    };

    // 2. Fetch the new SHA.
    let result = fixup::fetch_into_cache(&url, rev.as_deref(), globals.no_network)
        .with_context(|| format!("fetching {}", url))?;

    // 3. Compute diff vs. prior pin (if cached).
    if let Some(prior_sha) = &prior_rev {
        if prior_sha == &result.sha {
            println!("Already at {} — no changes.", result.sha);
            return Ok(());
        }
        let prior_cache_path = crate::cache::fixup_cache_path_for_sha(prior_sha)
            .context("resolving prior cache path")?;
        if prior_cache_path.is_dir() && prior_cache_path.join("packages").is_dir() {
            let prior_set = fixup::load_community(&prior_cache_path).with_context(|| {
                format!("loading prior fixups from {}", prior_cache_path.display())
            })?;
            let new_set = fixup::load_community(&result.working_tree).with_context(|| {
                format!("loading new fixups from {}", result.working_tree.display())
            })?;
            let diff = fixup::diff_fixup_sets(&prior_set, &new_set);
            if diff.is_empty() {
                println!("(no fixup changes)");
            } else {
                print!("{}", fixup::render_diff(&diff));
            }
        } else {
            println!("(prior cache evicted; diff unavailable)");
        }
    } else {
        println!("Initial pin (no prior rev to diff against)");
    }

    // 4. Surgical writeback via toml_edit.
    write_registry_rev(&cfg_path, &result.sha)?;

    // 5. Footer.
    match &prior_rev {
        Some(p) if p != &result.sha => {
            println!("\nPinned {} @ {} (was: {})", url, result.sha, p);
        }
        _ => {
            println!("\nPinned {} @ {}", url, result.sha);
        }
    }

    Ok(())
}

fn write_registry_rev(cfg_path: &std::path::Path, new_sha: &str) -> Result<()> {
    let bytes = std::fs::read_to_string(cfg_path)
        .with_context(|| format!("reading {}", cfg_path.display()))?;
    let mut doc: toml_edit::DocumentMut = bytes
        .parse()
        .with_context(|| format!("parsing {} as TOML", cfg_path.display()))?;

    let fixups = doc
        .entry("fixups")
        .or_insert(toml_edit::Item::Table(toml_edit::Table::new()));
    let fixups_table = fixups
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("[fixups] is not a table in {}", cfg_path.display()))?;
    fixups_table["registry_rev"] = toml_edit::value(new_sha);

    std::fs::write(cfg_path, doc.to_string())
        .with_context(|| format!("writing {}", cfg_path.display()))?;
    Ok(())
}
