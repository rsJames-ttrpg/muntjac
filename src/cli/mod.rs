use crate::config::{Config, Tree};
use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

pub mod buckify;
pub mod config_check;
pub mod debug;
pub mod fixups;
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

/// Per-invocation override for `[buck] vendor`. Affects this invocation only;
/// never writes back to the config. `Committed` ⇔ `vendor = true`,
/// `PrebakeOnly` ⇔ `vendor = false`.
#[derive(clap::ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeFlag {
    Committed,
    PrebakeOnly,
}

#[derive(Args, Debug, Clone, Default)]
pub struct VendorArgs {
    /// Override `[buck] vendor` for this invocation.
    #[arg(long, value_enum, value_name = "MODE")]
    pub mode: Option<ModeFlag>,

    /// Skip the sync-prune step in committed mode (keeps stale wheels).
    #[arg(long)]
    pub no_prune: bool,
}

#[derive(Args, Debug, Clone, Default)]
pub struct BuckifyArgs {
    /// Override `[buck] vendor` for this invocation.
    #[arg(long, value_enum, value_name = "MODE")]
    pub mode: Option<ModeFlag>,
}

/// Resolve the effective vendor mode: CLI override wins; else read `[buck] vendor`.
pub fn resolve_vendor_mode(config: &Config, mode_override: Option<ModeFlag>) -> bool {
    match mode_override {
        Some(ModeFlag::Committed) => true,
        Some(ModeFlag::PrebakeOnly) => false,
        None => config.buck.vendor,
    }
}

/// Resolve which trees a command operates on. `None` → all trees;
/// `Some(name)` → just that tree, or an error naming available trees.
pub fn resolve_trees<'a>(config: &'a Config, tree_filter: Option<&str>) -> Result<Vec<&'a Tree>> {
    match tree_filter {
        None => Ok(config.trees.iter().collect()),
        Some(name) => match config.trees.iter().find(|t| t.name == name) {
            Some(t) => Ok(vec![t]),
            None => {
                let available: Vec<&str> = config.trees.iter().map(|t| t.name.as_str()).collect();
                anyhow::bail!(
                    "tree `{}` not found in muntjac.toml; available: {}",
                    name,
                    available.join(", ")
                )
            }
        },
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

    /// Vendor wheels for the project. In prebake-only mode (default), prebakes
    /// pure-python sdists into `<third_party_dir>/prebake/`. In committed mode
    /// (`[buck] vendor = true` or `--mode=committed`), additionally downloads
    /// all registry wheels into `<third_party_dir>/vendor/`.
    Vendor(VendorArgs),
    /// Read uv.lock + fixups and emit BUCK, muntjac.bzl, config/BUCK, and wiring.bzl.
    Buckify(BuckifyArgs),
    /// Cross-check uv.lock against pypa/advisory-database — UNIMPLEMENTED (S10).
    Audit,
    /// Manage fixups (show).
    Fixups {
        #[command(subcommand)]
        op: fixups::FixupsOp,
    },
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
        Command::Vendor(args) => vendor::run(&cli.globals, args),
        Command::Buckify(args) => buckify::run(&cli.globals, args),
        Command::Audit => stub::run("audit", "S10"),
        Command::Fixups { op } => fixups::run(op, &cli.globals),
        Command::Unused => stub::run("unused", "S10"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn cfg() -> Config {
        Config::from_str(
            r#"
[platforms]
macos-arm64 = { target = "aarch64-apple-darwin", macos_min = "11.0" }
[tree.modern]
manifest_path = "m/pyproject.toml"
third_party_dir = "tp/modern"
python_versions = ["3.12"]
[tree.legacy]
manifest_path = "l/pyproject.toml"
third_party_dir = "tp/legacy"
python_versions = ["3.12"]
"#,
        )
        .unwrap()
    }

    #[test]
    fn resolve_trees_none_returns_all() {
        let c = cfg();
        assert_eq!(resolve_trees(&c, None).unwrap().len(), 2);
    }

    #[test]
    fn resolve_trees_filters_by_name() {
        let c = cfg();
        let got = resolve_trees(&c, Some("modern")).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].name, "modern");
    }

    #[test]
    fn resolve_trees_unknown_errors() {
        let c = cfg();
        let err = resolve_trees(&c, Some("ghost")).unwrap_err().to_string();
        assert!(err.contains("ghost"));
        assert!(err.contains("modern"));
        assert!(err.contains("legacy"));
    }

    #[test]
    fn resolve_vendor_mode_uses_config_when_no_override() {
        let toml = r#"
manifest_path = "pyproject.toml"
third_party_dir = "third-party/python"
python_versions = ["3.12"]
[platforms]
linux-x86_64-gnu = { target = "x86_64-unknown-linux-gnu", manylinux = "2_17" }
[buck]
vendor = true
"#;
        let config: Config = toml.parse().unwrap();
        assert!(resolve_vendor_mode(&config, None));
    }

    #[test]
    fn resolve_vendor_mode_override_committed_beats_config_false() {
        let toml = r#"
manifest_path = "pyproject.toml"
third_party_dir = "third-party/python"
python_versions = ["3.12"]
[platforms]
linux-x86_64-gnu = { target = "x86_64-unknown-linux-gnu", manylinux = "2_17" }
"#;
        let config: Config = toml.parse().unwrap();
        assert!(resolve_vendor_mode(&config, Some(ModeFlag::Committed)));
    }

    #[test]
    fn resolve_vendor_mode_override_prebake_only_beats_config_true() {
        let toml = r#"
manifest_path = "pyproject.toml"
third_party_dir = "third-party/python"
python_versions = ["3.12"]
[platforms]
linux-x86_64-gnu = { target = "x86_64-unknown-linux-gnu", manylinux = "2_17" }
[buck]
vendor = true
"#;
        let config: Config = toml.parse().unwrap();
        assert!(!resolve_vendor_mode(&config, Some(ModeFlag::PrebakeOnly)));
    }

    #[test]
    fn resolve_vendor_mode_defaults_to_false_when_no_override_and_no_buck_section() {
        let toml = r#"
manifest_path = "pyproject.toml"
third_party_dir = "third-party/python"
python_versions = ["3.12"]
[platforms]
linux-x86_64-gnu = { target = "x86_64-unknown-linux-gnu", manylinux = "2_17" }
"#;
        let config: Config = toml.parse().unwrap();
        assert!(!resolve_vendor_mode(&config, None));
    }

    #[test]
    fn cli_parses_vendor_mode_and_no_prune() {
        let cli = Cli::try_parse_from(["muntjac", "vendor", "--mode", "committed", "--no-prune"])
            .unwrap();
        match cli.command {
            Command::Vendor(args) => {
                assert!(matches!(args.mode, Some(ModeFlag::Committed)));
                assert!(args.no_prune);
            }
            _ => panic!("expected Vendor"),
        }
    }

    #[test]
    fn cli_parses_buckify_mode() {
        let cli = Cli::try_parse_from(["muntjac", "buckify", "--mode", "prebake-only"]).unwrap();
        match cli.command {
            Command::Buckify(args) => assert!(matches!(args.mode, Some(ModeFlag::PrebakeOnly))),
            _ => panic!("expected Buckify"),
        }
    }
}
