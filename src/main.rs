use anyhow::Result;
use clap::Parser;
use muntjac::cli::Cli;

fn main() -> Result<()> {
    let cli = Cli::parse();
    muntjac::cli::run(cli)
}
