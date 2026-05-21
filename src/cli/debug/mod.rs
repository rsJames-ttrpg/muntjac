use anyhow::Result;
use clap::{Args, Subcommand};

use crate::cli::Globals;

pub mod pick_wheels;
pub mod print_deps;

#[derive(Subcommand, Debug)]
pub enum DebugOp {
    /// Parse uv.lock and print the resolved dep graph as JSON.
    PrintDeps(PrintDepsArgs),
    /// Print the wheel selected per (package, version, platform, python) cell.
    PickWheels(PickWheelsArgs),
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

#[derive(Args, Debug)]
pub struct PickWheelsArgs {
    /// Limit to a single tree (default: all).
    #[arg(long)]
    pub tree: Option<String>,
    /// Limit to a single platform (default: all).
    #[arg(long)]
    pub platform: Option<String>,
    /// Limit to a single python version (default: all).
    #[arg(long)]
    pub python: Option<String>,
    /// Limit to a single package (default: all).
    #[arg(long)]
    pub package: Option<String>,
}

pub fn run(op: Option<DebugOp>, globals: &Globals) -> Result<()> {
    match op {
        None => anyhow::bail!("debug requires a subcommand (try `muntjac debug print-deps`)"),
        Some(DebugOp::PrintDeps(args)) => print_deps::run(args, globals),
        Some(DebugOp::PickWheels(args)) => pick_wheels::run(args, globals),
    }
}
