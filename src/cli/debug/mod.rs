use anyhow::Result;
use clap::{Args, Subcommand};

use crate::cli::Globals;

pub mod print_deps;

#[derive(Subcommand, Debug)]
pub enum DebugOp {
    /// Parse uv.lock and print the resolved dep graph as JSON.
    PrintDeps(PrintDepsArgs),
}

#[derive(Args, Debug)]
pub struct PrintDepsArgs {
    /// Operate on this tree (multi-tree configs).
    #[arg(long)]
    pub tree: Option<String>,

    /// Pretty-print the JSON (default: compact).
    #[arg(long)]
    pub pretty: bool,
}

pub fn run(op: Option<DebugOp>, globals: &Globals) -> Result<()> {
    match op {
        None => anyhow::bail!("debug requires a subcommand (try `muntjac debug print-deps`)"),
        Some(DebugOp::PrintDeps(args)) => print_deps::run(args, globals),
    }
}
