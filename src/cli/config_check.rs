use anyhow::Result;
use clap::Args;

use crate::cli::Globals;

#[derive(Args, Debug)]
pub struct ConfigCheckArgs {
    /// Output format.
    #[arg(long, default_value = "human")]
    pub format: ConfigCheckFormat,
}

#[derive(clap::ValueEnum, Clone, Debug)]
pub enum ConfigCheckFormat {
    Human,
    Json,
}

pub fn run(_args: ConfigCheckArgs, _globals: &Globals) -> Result<()> {
    anyhow::bail!("config check not yet implemented (filled in by Task 7)")
}
