use anyhow::{Context, Result};
use clap::Args;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::cli::Globals;
use crate::config::Config;

#[derive(Args, Debug)]
pub struct ConfigCheckArgs {
    #[arg(long, default_value = "human")]
    pub format: ConfigCheckFormat,
}

#[derive(clap::ValueEnum, Clone, Debug)]
pub enum ConfigCheckFormat {
    Human,
    Json,
}

pub fn run(args: ConfigCheckArgs, _globals: &Globals) -> Result<()> {
    let cwd = std::env::current_dir().context("getting current dir")?;
    let cfg_path = cwd.join("muntjac.toml");

    let bytes = fs::read_to_string(&cfg_path).with_context(|| {
        format!("reading muntjac.toml at {}", cfg_path.display())
    })?;
    let config = Config::from_str(&bytes).with_context(|| {
        format!("parsing muntjac.toml at {}", cfg_path.display())
    })?;
    config.validate().with_context(|| {
        format!("validating muntjac.toml at {}", cfg_path.display())
    })?;

    check_paths(&cfg_path, &config)?;

    match args.format {
        ConfigCheckFormat::Human => {
            println!(
                "muntjac.toml ok: {} platforms, {} trees",
                config.platforms.len(),
                config.trees.len()
            );
        }
        ConfigCheckFormat::Json => {
            let report = serde_json::json!({
                "ok": true,
                "platforms": config.platforms.len(),
                "trees": config.trees.len(),
            });
            println!("{}", report);
        }
    }
    Ok(())
}

fn check_paths(cfg_path: &Path, config: &Config) -> Result<()> {
    let base = cfg_path.parent().unwrap_or(Path::new("."));
    for tree in &config.trees {
        let manifest = resolve(base, &tree.manifest_path);
        anyhow::ensure!(
            manifest.exists(),
            "manifest_path for tree `{}` does not exist: {}",
            tree.name,
            manifest.display()
        );
        let tpd = resolve(base, &tree.third_party_dir);
        if !tpd.exists() {
            fs::create_dir_all(&tpd).with_context(|| {
                format!("creating third_party_dir for tree `{}`: {}", tree.name, tpd.display())
            })?;
        }
    }
    Ok(())
}

fn resolve(base: &Path, p: &Path) -> PathBuf {
    if p.is_absolute() { p.to_path_buf() } else { base.join(p) }
}
