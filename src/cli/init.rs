use anyhow::Result;
use clap::Args;
use std::path::PathBuf;

use crate::cli::Globals;

#[derive(Args, Debug)]
pub struct InitArgs {
    /// Overwrite existing muntjac.toml.
    #[arg(long)]
    pub force: bool,
    /// Target directory (defaults to current dir).
    pub path: Option<PathBuf>,
}

pub fn run(_args: InitArgs, _globals: &Globals) -> Result<()> {
    anyhow::bail!("init not yet implemented (filled in by Task 9)")
}
