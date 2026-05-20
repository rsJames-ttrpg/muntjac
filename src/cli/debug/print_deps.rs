use anyhow::Result;

use crate::cli::Globals;
use crate::cli::debug::PrintDepsArgs;

pub fn run(_args: PrintDepsArgs, _globals: &Globals) -> Result<()> {
    anyhow::bail!("print-deps not yet implemented (filled in by Task 14)")
}
