use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

pub mod config_check;
pub mod debug;
pub mod init;
pub mod stub;

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
        /// Subcommand name (none defined yet at S0; S1 will add `print-deps`).
        subcommand: Option<String>,
        /// Trailing args forwarded to the subcommand.
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },

    /// Download wheels into ~/.cache/muntjac (or vendor/) — UNIMPLEMENTED (S5/S9).
    Vendor,
    /// Read uv.lock + fixups and emit BUCK — UNIMPLEMENTED (S3+).
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
    if let Some(path) = &cli.globals.cd {
        std::env::set_current_dir(path)
            .map_err(|e| anyhow::anyhow!("failed to cd into {}: {e}", path.display()))?;
    }
    match cli.command {
        Command::Init(args) => init::run(args, &cli.globals),
        Command::Config {
            op: ConfigOp::Check(args),
        } => config_check::run(args, &cli.globals),
        Command::Debug { subcommand, args } => debug::run(subcommand, args, &cli.globals),
        Command::Vendor => stub::run("vendor", "S5/S9"),
        Command::Buckify => stub::run("buckify", "S3+"),
        Command::Audit => stub::run("audit", "S10"),
        Command::Fixups => stub::run("fixups", "S6/S7"),
        Command::Unused => stub::run("unused", "S10"),
    }
}
