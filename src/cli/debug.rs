use anyhow::Result;

use crate::cli::Globals;

pub fn run(subcommand: Option<String>, _args: Vec<String>, _globals: &Globals) -> Result<()> {
    match subcommand {
        None => anyhow::bail!("debug requires a subcommand (none defined yet)"),
        Some(s) => anyhow::bail!(
            "unknown debug subcommand `{s}` (none defined yet at S0; \
             S1 will add `print-deps`, S2 will add `pick-wheels`)"
        ),
    }
}
