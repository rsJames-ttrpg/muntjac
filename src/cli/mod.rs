use anyhow::Result;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "muntjac", version, about = "Translate uv.lock into Buck2 build rules")]
pub struct Cli {}

pub fn run(_cli: Cli) -> Result<()> {
    Ok(())
}
