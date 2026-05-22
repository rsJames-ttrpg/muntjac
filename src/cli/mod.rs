use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

pub mod buckify;
pub mod config_check;
pub mod debug;
pub mod init;
pub mod stub;
pub mod vendor;

#[derive(Parser, Debug)]
#[command(
    name = "muntjac",
    version,
    about = "Translate uv.lock into Buck2 build rules",
    long_about = None,
)]
pub struct Cli {
    #[command(flatten)]
    pub globals: Globals,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Args, Debug, Clone)]
pub struct Globals {
    /// Run as if muntjac were invoked from this path.
    #[arg(short = 'C', long = "cd", global = true, value_name = "PATH")]
    pub cd: Option<PathBuf>,

    /// Verbose logging. Repeat for more (-v info, -vv debug).
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,

    /// Forbid any network calls.
    #[arg(long, global = true)]
    pub no_network: bool,

    /// Forbid running `uv lock` even if pyproject.toml is newer.
    #[arg(long, global = true)]
    pub frozen: bool,

    /// Operate on a specific tree in a multi-tree config.
    #[arg(long, global = true, value_name = "NAME")]
    pub tree: Option<String>,
}

impl Globals {
    /// Returns the resolved working directory: either the value of `-C` (canonicalized)
    /// or `std::env::current_dir()` if not set.
    pub fn workdir(&self) -> std::io::Result<PathBuf> {
        match &self.cd {
            Some(p) => std::fs::canonicalize(p),
            None => std::env::current_dir(),
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Write a starter muntjac.toml and third-party/python/ skeleton.
    Init(init::InitArgs),

    /// Validate muntjac.toml without performing any side effects.
    Config {
        #[command(subcommand)]
        op: ConfigOp,
    },

    /// Hidden debug subcommands (not stable; for muntjac internals).
    #[command(hide = true)]
    Debug {
        #[command(subcommand)]
        op: Option<debug::DebugOp>,
    },

    /// Prebake pure-python sdists into wheels. Wheel caching → S9.
    Vendor,
    /// Read uv.lock + fixups and emit BUCK, muntjac.bzl, config/BUCK, and wiring.bzl.
    Buckify,
    /// Cross-check uv.lock against pypa/advisory-database — UNIMPLEMENTED (S10).
    Audit,
    /// Manage fixups (update / show) — UNIMPLEMENTED (S6/S7).
    Fixups,
    /// Report vendored wheels not referenced by any tree — UNIMPLEMENTED (S10).
    Unused,
}

#[derive(Subcommand, Debug)]
pub enum ConfigOp {
    /// Validate muntjac.toml.
    Check(config_check::ConfigCheckArgs),
}

pub fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Init(args) => init::run(args, &cli.globals),
        Command::Config {
            op: ConfigOp::Check(args),
        } => config_check::run(args, &cli.globals),
        Command::Debug { op } => debug::run(op, &cli.globals),
        Command::Vendor => vendor::run(&cli.globals),
        Command::Buckify => buckify::run(&cli.globals),
        Command::Audit => stub::run("audit", "S10"),
        Command::Fixups => stub::run("fixups", "S6/S7"),
        Command::Unused => stub::run("unused", "S10"),
    }
}
