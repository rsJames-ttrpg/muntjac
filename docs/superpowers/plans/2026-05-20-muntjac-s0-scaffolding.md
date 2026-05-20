# S0 Scaffolding Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Stand up the muntjac Rust crate with a working CLI (`init`, `config check`, `debug`, stubbed phase-1 verbs), `muntjac.toml` parser, error machinery, and CI on three platforms. No business logic — the deliverable is "the tool runs, validates config, and is ready for S1 to plug into."

**Architecture:** Single Rust crate with `src/lib.rs` + `src/main.rs` split. `clap` for CLI dispatch, `serde` + `toml` for config parsing, `thiserror`-derived module errors bubbling through `anyhow::Error` at the boundary, `miette` for diagnostic display. Integration tests via `assert_cmd`; snapshot tests via `insta`.

**Tech Stack:** Rust 2024 edition, `clap` 4.5, `serde` 1.0, `toml` 0.8, `thiserror` 2.0, `anyhow` 1.0, `miette` 7.x, `tempfile` 3.x (test-only), `assert_cmd` 2.x (test-only), `insta` 1.x (test-only).

**Spec:** `docs/superpowers/specs/2026-05-20-muntjac-s0-scaffolding-design.md`

---

## File structure

| Path | Responsibility |
|---|---|
| `Cargo.toml` | Crate metadata + dependency pins |
| `src/main.rs` | Binary entrypoint — parse CLI, dispatch to subcommand handler, print diagnostics |
| `src/lib.rs` | Internal re-exports (`pub mod cli; pub mod config; pub mod error;`) |
| `src/cli/mod.rs` | clap-derived `Cli` struct + `Command` enum + dispatcher |
| `src/cli/init.rs` | `muntjac init` — pyproject detection + file writing |
| `src/cli/config_check.rs` | `muntjac config check` — config validation reporter |
| `src/cli/debug.rs` | `muntjac debug` — subcommand group (empty at S0) |
| `src/cli/stub.rs` | Stubbed phase-1 verbs (vendor, buckify, audit, fixups, unused) |
| `src/config.rs` | `muntjac.toml` parser + `Config`/`Tree`/`Platform`/`FixupsConfig`/`BuckConfig` types |
| `src/error.rs` | `ConfigError` enum + `Diagnostic` trait + miette printer |
| `tests/init.rs` | Integration tests for `muntjac init` |
| `tests/config_check.rs` | Integration tests for `muntjac config check` |
| `tests/help.rs` | Snapshot test of `muntjac --help` |
| `tests/common/mod.rs` | Shared test helpers (binary path, tempdir setup) |
| `.github/workflows/ci.yml` | CI matrix: build, test, clippy, fmt on 3 platforms |
| `.gitignore` | Add `Cargo.lock`? no — we'll commit the lock. Just `/target` (already present). |

---

## Task 1: Cargo.toml dependencies and crate skeleton

**Files:**
- Modify: `Cargo.toml` (currently has only `[package]` with empty deps)
- Modify: `src/main.rs` (currently `fn main() { println!("Hello, world!"); }`)
- Create: `src/lib.rs`

- [ ] **Step 1: Rewrite `Cargo.toml` with full dependency pins**

```toml
[package]
name = "muntjac"
version = "0.1.0-dev"
edition = "2024"
description = "Translate uv.lock into Buck2 build rules"
license = "MIT"
repository = "https://github.com/jackmpcollins/muntjac"
readme = "README.md"

[[bin]]
name = "muntjac"
path = "src/main.rs"

[lib]
name = "muntjac"
path = "src/lib.rs"

[dependencies]
anyhow = "1.0"
clap = { version = "4.5", features = ["derive"] }
miette = { version = "7", features = ["fancy"] }
serde = { version = "1.0", features = ["derive"] }
thiserror = "2.0"
toml = "0.8"

[dev-dependencies]
assert_cmd = "2"
insta = { version = "1", features = ["yaml"] }
predicates = "3"
tempfile = "3"
```

- [ ] **Step 2: Create `src/lib.rs` with module declarations**

```rust
pub mod cli;
pub mod config;
pub mod error;
```

- [ ] **Step 3: Replace `src/main.rs` with a clap-dispatching stub**

```rust
use anyhow::Result;
use clap::Parser;
use muntjac::cli::Cli;

fn main() -> Result<()> {
    let cli = Cli::parse();
    muntjac::cli::run(cli)
}
```

- [ ] **Step 4: Create `src/cli/mod.rs` placeholder so the build succeeds**

```rust
use anyhow::Result;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "muntjac", version, about = "Translate uv.lock into Buck2 build rules")]
pub struct Cli {}

pub fn run(_cli: Cli) -> Result<()> {
    Ok(())
}
```

- [ ] **Step 5: Create empty `src/config.rs` and `src/error.rs`**

```rust
// src/config.rs
// (filled in by later tasks)
```

```rust
// src/error.rs
// (filled in by later tasks)
```

- [ ] **Step 6: Run cargo build to confirm setup**

Run: `cargo build`
Expected: builds successfully with warnings about unused modules (those will be filled in by later tasks).

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock src/main.rs src/lib.rs src/cli/mod.rs src/config.rs src/error.rs
git commit -m "feat(s0): scaffold crate with deps and module skeleton"
```

---

## Task 2: Config types and single-tree TOML parser

**Files:**
- Modify: `src/config.rs`

- [ ] **Step 1: Write the failing test for parsing a minimal single-tree config**

Add to bottom of `src/config.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL_SINGLE_TREE: &str = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.11", "3.12"]

[platforms.linux-x86_64-gnu]
target    = "x86_64-unknown-linux-gnu"
manylinux = "2_17"

[platforms.macos-arm64]
target    = "aarch64-apple-darwin"
macos_min = "11.0"
"#;

    #[test]
    fn parses_minimal_single_tree() {
        let config = Config::from_str(MINIMAL_SINGLE_TREE).expect("parse");
        assert_eq!(config.trees.len(), 1);
        let tree = &config.trees[0];
        assert_eq!(tree.name, "default");
        assert_eq!(tree.manifest_path.to_str(), Some("../pyproject.toml"));
        assert_eq!(tree.python_versions, vec![PythonVersion(3, 11), PythonVersion(3, 12)]);
        assert_eq!(config.platforms.len(), 2);
    }
}
```

- [ ] **Step 2: Run the test to confirm it fails (compile error)**

Run: `cargo test --lib config`
Expected: FAIL with `unresolved import: super::Config` (or similar — the types don't exist yet).

- [ ] **Step 3: Implement the Config types and a single-tree TOML parser**

Replace `src/config.rs` with:

```rust
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub trees: Vec<Tree>,
    pub platforms: BTreeMap<String, Platform>,
    pub fixups: FixupsConfig,
    pub buck: BuckConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tree {
    pub name: String,
    pub manifest_path: PathBuf,
    pub third_party_dir: PathBuf,
    pub python_versions: Vec<PythonVersion>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Platform {
    pub target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manylinux: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub musllinux: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub macos_min: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub struct FixupsConfig {
    #[serde(default = "default_registry")]
    pub registry: FixupRegistry,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry_rev: Option<String>,
    #[serde(default = "default_true")]
    pub allow_local_overrides: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum FixupRegistry {
    String(String),
}

impl Default for FixupRegistry {
    fn default() -> Self {
        FixupRegistry::String("none".into())
    }
}

fn default_registry() -> FixupRegistry { FixupRegistry::default() }
fn default_true() -> bool { true }

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct BuckConfig {
    #[serde(default = "default_buck_file_name")]
    pub file_name: String,
    #[serde(default)]
    pub vendor: bool,
}

impl Default for BuckConfig {
    fn default() -> Self {
        Self { file_name: default_buck_file_name(), vendor: false }
    }
}

fn default_buck_file_name() -> String { "BUCK".into() }

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PythonVersion(pub u8, pub u8);

impl<'de> Deserialize<'de> for PythonVersion {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let s = String::deserialize(de)?;
        PythonVersion::from_str(&s).map_err(D::Error::custom)
    }
}

impl Serialize for PythonVersion {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(&format!("{}.{}", self.0, self.1))
    }
}

impl FromStr for PythonVersion {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 2 {
            return Err(format!("python version must be MAJOR.MINOR, got `{s}`"));
        }
        let major: u8 = parts[0].parse().map_err(|_| format!("bad major in `{s}`"))?;
        let minor: u8 = parts[1].parse().map_err(|_| format!("bad minor in `{s}`"))?;
        if major != 3 {
            return Err(format!("only Python 3.x supported, got `{s}`"));
        }
        Ok(PythonVersion(major, minor))
    }
}

// Internal raw shape — what serde reads directly from TOML.
#[derive(Debug, Deserialize)]
struct RawConfig {
    manifest_path: Option<PathBuf>,
    third_party_dir: Option<PathBuf>,
    python_versions: Option<Vec<PythonVersion>>,
    platforms: BTreeMap<String, Platform>,
    #[serde(default)]
    fixups: FixupsConfig,
    #[serde(default)]
    buck: BuckConfig,
    #[serde(default)]
    tree: BTreeMap<String, RawTree>,
}

#[derive(Debug, Deserialize)]
struct RawTree {
    manifest_path: PathBuf,
    third_party_dir: PathBuf,
    python_versions: Vec<PythonVersion>,
}

impl Config {
    pub fn from_str(s: &str) -> Result<Self, crate::error::ConfigError> {
        let raw: RawConfig = toml::from_str(s).map_err(crate::error::ConfigError::Parse)?;
        Self::from_raw(raw)
    }

    fn from_raw(raw: RawConfig) -> Result<Self, crate::error::ConfigError> {
        let has_top_level = raw.manifest_path.is_some()
            || raw.third_party_dir.is_some()
            || raw.python_versions.is_some();
        let has_trees = !raw.tree.is_empty();

        if has_top_level && has_trees {
            return Err(crate::error::ConfigError::IncompatibleShape);
        }

        let trees = if has_trees {
            raw.tree.into_iter().map(|(name, t)| Tree {
                name,
                manifest_path: t.manifest_path,
                third_party_dir: t.third_party_dir,
                python_versions: t.python_versions,
            }).collect()
        } else {
            let manifest_path = raw.manifest_path.ok_or(crate::error::ConfigError::MissingField("manifest_path"))?;
            let third_party_dir = raw.third_party_dir.ok_or(crate::error::ConfigError::MissingField("third_party_dir"))?;
            let python_versions = raw.python_versions.ok_or(crate::error::ConfigError::MissingField("python_versions"))?;
            vec![Tree { name: "default".into(), manifest_path, third_party_dir, python_versions }]
        };

        Ok(Config { trees, platforms: raw.platforms, fixups: raw.fixups, buck: raw.buck })
    }
}
```

- [ ] **Step 4: Add the matching error type stub to `src/error.rs`**

Replace `src/error.rs` with:

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to parse muntjac.toml: {0}")]
    Parse(#[from] toml::de::Error),

    #[error("missing required field `{0}`")]
    MissingField(&'static str),

    #[error("config cannot mix top-level manifest_path/third_party_dir/python_versions with [tree.*] sections")]
    IncompatibleShape,

    #[error("invalid platform `{name}`: {reason}")]
    BadPlatform { name: String, reason: String },

    #[error("invalid python version `{0}`")]
    BadPythonVersion(String),

    #[error("invalid registry `{0}`: expected \"none\", \"file://<path>\", or \"github.com/<owner>/<repo>\"")]
    BadRegistry(String),
}
```

- [ ] **Step 5: Run the test to confirm it now passes**

Run: `cargo test --lib config`
Expected: PASS — `parses_minimal_single_tree ... ok`

- [ ] **Step 6: Commit**

```bash
git add src/config.rs src/error.rs
git commit -m "feat(s0): config types and single-tree TOML parser"
```

---

## Task 3: Multi-tree config parsing and shape-mismatch rejection

**Files:**
- Modify: `src/config.rs` (extend tests)

- [ ] **Step 1: Write failing tests for multi-tree shape and shape mismatch**

Add to the `tests` module in `src/config.rs`:

```rust
const MULTI_TREE: &str = r#"
[platforms.linux-x86_64-gnu]
target = "x86_64-unknown-linux-gnu"
manylinux = "2_17"

[tree.modern]
manifest_path   = "pyproject.toml"
third_party_dir = "third-party/python/modern"
python_versions = ["3.12"]

[tree.legacy]
manifest_path   = "legacy/pyproject.toml"
third_party_dir = "third-party/python/legacy"
python_versions = ["3.10"]
"#;

const MIXED_SHAPE: &str = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target = "x86_64-unknown-linux-gnu"

[tree.extra]
manifest_path   = "other/pyproject.toml"
third_party_dir = "other"
python_versions = ["3.11"]
"#;

#[test]
fn parses_multi_tree() {
    let config = Config::from_str(MULTI_TREE).expect("parse");
    assert_eq!(config.trees.len(), 2);
    let names: Vec<&str> = config.trees.iter().map(|t| t.name.as_str()).collect();
    // BTreeMap iteration is sorted, so we expect alphabetical.
    assert_eq!(names, vec!["legacy", "modern"]);
}

#[test]
fn rejects_mixed_shape() {
    let err = Config::from_str(MIXED_SHAPE).expect_err("should fail");
    assert!(matches!(err, crate::error::ConfigError::IncompatibleShape));
}

#[test]
fn rejects_missing_manifest_path_when_no_trees() {
    let toml_str = r#"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target = "x86_64-unknown-linux-gnu"
"#;
    let err = Config::from_str(toml_str).expect_err("should fail");
    assert!(matches!(err, crate::error::ConfigError::MissingField("manifest_path")));
}

#[test]
fn rejects_bad_python_version() {
    let toml_str = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3"]

[platforms.linux-x86_64-gnu]
target = "x86_64-unknown-linux-gnu"
"#;
    let err = Config::from_str(toml_str).expect_err("should fail");
    assert!(matches!(err, crate::error::ConfigError::Parse(_)));
}
```

- [ ] **Step 2: Run the tests — multi-tree should already pass, but mixed-shape and missing-field need the right error type**

Run: `cargo test --lib config`
Expected: PASS for all four — the implementation from Task 2 already handles these cases. If any fails, fix the relevant arm in `Config::from_raw`.

- [ ] **Step 3: Commit**

```bash
git add src/config.rs
git commit -m "test(s0): multi-tree config + shape validation"
```

---

## Task 4: Per-field validators (platforms, python versions, registry)

**Files:**
- Modify: `src/config.rs` (add `validate` method)

- [ ] **Step 1: Write failing tests for field validation**

Add to the `tests` module in `src/config.rs`:

```rust
#[test]
fn validates_platform_target_triple() {
    let toml_str = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.bogus]
target = "not-a-real-triple"
"#;
    let config = Config::from_str(toml_str).expect("parse");
    let err = config.validate().expect_err("should fail");
    assert!(matches!(err, crate::error::ConfigError::BadPlatform { .. }));
}

#[test]
fn accepts_known_target_triples() {
    let toml_str = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target = "x86_64-unknown-linux-gnu"
manylinux = "2_17"

[platforms.linux-aarch64-gnu]
target = "aarch64-unknown-linux-gnu"
manylinux = "2_17"

[platforms.linux-x86_64-musl]
target = "x86_64-unknown-linux-musl"
musllinux = "1_2"

[platforms.macos-x86_64]
target = "x86_64-apple-darwin"
macos_min = "11.0"

[platforms.macos-arm64]
target = "aarch64-apple-darwin"
macos_min = "11.0"
"#;
    let config = Config::from_str(toml_str).expect("parse");
    config.validate().expect("validate");
}

#[test]
fn validates_registry_form() {
    let bad = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target = "x86_64-unknown-linux-gnu"

[fixups]
registry = "https://example.com/whatever"
"#;
    let config = Config::from_str(bad).expect("parse");
    let err = config.validate().expect_err("should fail");
    assert!(matches!(err, crate::error::ConfigError::BadRegistry(_)));
}

#[test]
fn accepts_registry_forms() {
    for r in ["none", "file:///tmp/fixups", "github.com/jackmpcollins/muntjac-fixups"] {
        let toml_str = format!(r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target = "x86_64-unknown-linux-gnu"

[fixups]
registry = "{r}"
"#);
        let config = Config::from_str(&toml_str).expect("parse");
        config.validate().expect(&format!("validate `{r}`"));
    }
}
```

- [ ] **Step 2: Run the tests to confirm they fail (no `validate` method yet)**

Run: `cargo test --lib config`
Expected: FAIL with `no method named validate found for struct Config`.

- [ ] **Step 3: Implement `Config::validate`**

Add to `impl Config` block in `src/config.rs`:

```rust
impl Config {
    // ... existing methods ...

    pub fn validate(&self) -> Result<(), crate::error::ConfigError> {
        for (name, platform) in &self.platforms {
            validate_target_triple(name, &platform.target)?;
        }
        validate_registry(&self.fixups.registry)?;
        Ok(())
    }
}

fn validate_target_triple(name: &str, target: &str) -> Result<(), crate::error::ConfigError> {
    // Accepted shapes:
    //   {x86_64,aarch64}-unknown-linux-{gnu,musl}
    //   {x86_64,aarch64}-apple-darwin
    let allowed = [
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-gnu",
        "x86_64-unknown-linux-musl",
        "aarch64-unknown-linux-musl",
        "x86_64-apple-darwin",
        "aarch64-apple-darwin",
    ];
    if allowed.contains(&target) {
        Ok(())
    } else {
        Err(crate::error::ConfigError::BadPlatform {
            name: name.into(),
            reason: format!("unknown target triple `{target}`; expected one of {allowed:?}"),
        })
    }
}

fn validate_registry(reg: &FixupRegistry) -> Result<(), crate::error::ConfigError> {
    let FixupRegistry::String(s) = reg;
    if s == "none" {
        return Ok(());
    }
    if s.starts_with("file://") {
        return Ok(());
    }
    // github.com/<owner>/<repo>
    if let Some(rest) = s.strip_prefix("github.com/") {
        let parts: Vec<&str> = rest.split('/').collect();
        if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
            return Ok(());
        }
    }
    Err(crate::error::ConfigError::BadRegistry(s.clone()))
}
```

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `cargo test --lib config`
Expected: all four validation tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/config.rs
git commit -m "feat(s0): validate platform triples and registry URLs"
```

---

## Task 5: CLI skeleton — clap derive with subcommands and global flags

**Files:**
- Modify: `src/cli/mod.rs`
- Create: `src/cli/init.rs`, `src/cli/config_check.rs`, `src/cli/debug.rs`, `src/cli/stub.rs`

- [ ] **Step 1: Replace `src/cli/mod.rs` with the full CLI definition**

```rust
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
        Command::Config { op: ConfigOp::Check(args) } => config_check::run(args, &cli.globals),
        Command::Debug { subcommand, args } => debug::run(subcommand, args, &cli.globals),
        Command::Vendor => stub::run("vendor", "S5/S9"),
        Command::Buckify => stub::run("buckify", "S3+"),
        Command::Audit => stub::run("audit", "S10"),
        Command::Fixups => stub::run("fixups", "S6/S7"),
        Command::Unused => stub::run("unused", "S10"),
    }
}
```

- [ ] **Step 2: Create the four submodule stubs that compile but don't yet do their work**

Create `src/cli/init.rs`:

```rust
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
    anyhow::bail!("init not yet implemented (filled in by Task 8)")
}
```

Create `src/cli/config_check.rs`:

```rust
use anyhow::Result;
use clap::Args;

use crate::cli::Globals;

#[derive(Args, Debug)]
pub struct ConfigCheckArgs {
    /// Output format.
    #[arg(long, default_value = "human")]
    pub format: ConfigCheckFormat,
}

#[derive(clap::ValueEnum, Clone, Debug)]
pub enum ConfigCheckFormat {
    Human,
    Json,
}

pub fn run(_args: ConfigCheckArgs, _globals: &Globals) -> Result<()> {
    anyhow::bail!("config check not yet implemented (filled in by Task 7)")
}
```

Create `src/cli/debug.rs`:

```rust
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
```

When S1 adds `print-deps`, this gets refactored into a proper `Subcommand` enum, but at S0 the flat-string form avoids clap's "empty enum can't derive Subcommand" issue.

Create `src/cli/stub.rs`:

```rust
use anyhow::Result;

pub fn run(verb: &str, planned_for: &str) -> Result<()> {
    anyhow::bail!(
        "muntjac {verb} is not implemented yet (planned for {planned_for}); \
         see docs/superpowers/specs/2026-05-20-muntjac-roadmap.md"
    )
}
```

- [ ] **Step 3: Confirm cargo build still passes**

Run: `cargo build`
Expected: builds; warning lines about unused `_args`/`_globals` are acceptable since those modules are stubs.

- [ ] **Step 4: Confirm `muntjac --help` lists the public verbs**

Run: `cargo run -- --help`
Expected output includes the lines (order may vary slightly with clap version):

```
Commands:
  init     Write a starter muntjac.toml and third-party/python/ skeleton
  config   Validate muntjac.toml without performing any side effects
  vendor   Download wheels into ~/.cache/muntjac (or vendor/) — UNIMPLEMENTED (S5/S9)
  buckify  Read uv.lock + fixups and emit BUCK — UNIMPLEMENTED (S3+)
  audit    Cross-check uv.lock against pypa/advisory-database — UNIMPLEMENTED (S10)
  fixups   Manage fixups (update / show) — UNIMPLEMENTED (S6/S7)
  unused   Report vendored wheels not referenced by any tree — UNIMPLEMENTED (S10)
  help     Print this message or the help of the given subcommand(s)
```

The `debug` command is hidden from default `--help` — verify with `cargo run -- --help` does NOT include `debug`, then `cargo run -- help debug` does work.

- [ ] **Step 5: Commit**

```bash
git add src/cli/
git commit -m "feat(s0): CLI skeleton with clap derive subcommands and globals"
```

---

## Task 6: Stub verbs print "not implemented" errors

**Files:**
- (No code changes; this is a behavior verification task. If stub.rs from Task 5 already works, this task only adds the test.)

- [ ] **Step 1: Add an integration test that exercises stub verbs**

Create `tests/common/mod.rs`:

```rust
use assert_cmd::cargo::CommandCargoExt;
use std::process::Command;

pub fn muntjac() -> Command {
    Command::cargo_bin("muntjac").expect("locate muntjac binary")
}
```

Create `tests/stubs.rs`:

```rust
mod common;

use common::muntjac;
use predicates::str::contains;

#[test]
fn vendor_says_unimplemented() {
    muntjac()
        .arg("vendor")
        .assert()
        .failure()
        .stderr(contains("not implemented yet (planned for S5/S9)"));
}

#[test]
fn buckify_says_unimplemented() {
    muntjac()
        .arg("buckify")
        .assert()
        .failure()
        .stderr(contains("not implemented yet (planned for S3+)"));
}

#[test]
fn audit_says_unimplemented() {
    muntjac()
        .arg("audit")
        .assert()
        .failure()
        .stderr(contains("not implemented yet (planned for S10)"));
}

#[test]
fn fixups_says_unimplemented() {
    muntjac()
        .arg("fixups")
        .assert()
        .failure()
        .stderr(contains("not implemented yet (planned for S6/S7)"));
}

#[test]
fn unused_says_unimplemented() {
    muntjac()
        .arg("unused")
        .assert()
        .failure()
        .stderr(contains("not implemented yet (planned for S10)"));
}

#[test]
fn debug_requires_subcommand() {
    muntjac()
        .arg("debug")
        .assert()
        .failure()
        .stderr(contains("debug requires a subcommand"));
}
```

- [ ] **Step 2: Run the tests to confirm they pass**

Run: `cargo test --test stubs`
Expected: 6 passed.

- [ ] **Step 3: Commit**

```bash
git add tests/common/mod.rs tests/stubs.rs
git commit -m "test(s0): verify stub verbs emit planned-for-S<n> errors"
```

---

## Task 7: Implement `muntjac config check`

**Files:**
- Modify: `src/cli/config_check.rs`
- Create: `tests/config_check.rs`

- [ ] **Step 1: Write the failing integration test**

Create `tests/config_check.rs`:

```rust
mod common;

use common::muntjac;
use predicates::str::contains;
use std::fs;
use tempfile::tempdir;

const GOOD_CONFIG: &str = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target    = "x86_64-unknown-linux-gnu"
manylinux = "2_17"
"#;

const BAD_TRIPLE: &str = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.bogus]
target = "not-a-triple"
"#;

#[test]
fn config_check_passes_on_good_config() {
    let dir = tempdir().unwrap();
    let cfg = dir.path().join("muntjac.toml");
    let manifest = dir.path().join("pyproject.toml");
    fs::write(&cfg, GOOD_CONFIG.replace("../pyproject.toml", manifest.to_str().unwrap())).unwrap();
    fs::write(&manifest, "[project]\nname = \"x\"\n").unwrap();

    muntjac()
        .arg("-C")
        .arg(dir.path())
        .args(["config", "check"])
        .assert()
        .success()
        .stdout(contains("muntjac.toml ok"));
}

#[test]
fn config_check_rejects_bad_triple() {
    let dir = tempdir().unwrap();
    let cfg = dir.path().join("muntjac.toml");
    let manifest = dir.path().join("pyproject.toml");
    fs::write(&cfg, BAD_TRIPLE.replace("../pyproject.toml", manifest.to_str().unwrap())).unwrap();
    fs::write(&manifest, "[project]\nname = \"x\"\n").unwrap();

    muntjac()
        .arg("-C")
        .arg(dir.path())
        .args(["config", "check"])
        .assert()
        .failure()
        .stderr(contains("unknown target triple"));
}

#[test]
fn config_check_rejects_missing_file() {
    let dir = tempdir().unwrap();
    muntjac()
        .arg("-C")
        .arg(dir.path())
        .args(["config", "check"])
        .assert()
        .failure()
        .stderr(contains("muntjac.toml"));
}
```

- [ ] **Step 2: Run the test to confirm it fails (config check is stubbed)**

Run: `cargo test --test config_check`
Expected: FAIL — "config check not yet implemented".

- [ ] **Step 3: Implement `config_check::run`**

Replace `src/cli/config_check.rs`:

```rust
use anyhow::{Context, Result};
use clap::Args;
use std::fs;
use std::path::{Path, PathBuf};

use crate::cli::Globals;
use crate::config::Config;

#[derive(Args, Debug)]
pub struct ConfigCheckArgs {
    #[arg(long, default_value = "human")]
    pub format: ConfigCheckFormat,
}

#[derive(clap::ValueEnum, Clone, Debug)]
pub enum ConfigCheckFormat {
    Human,
    Json,
}

pub fn run(args: ConfigCheckArgs, _globals: &Globals) -> Result<()> {
    let cwd = std::env::current_dir().context("getting current dir")?;
    let cfg_path = cwd.join("muntjac.toml");

    let bytes = fs::read_to_string(&cfg_path).with_context(|| {
        format!("reading muntjac.toml at {}", cfg_path.display())
    })?;
    let config = Config::from_str(&bytes).with_context(|| {
        format!("parsing muntjac.toml at {}", cfg_path.display())
    })?;
    config.validate().with_context(|| {
        format!("validating muntjac.toml at {}", cfg_path.display())
    })?;

    check_paths(&cfg_path, &config)?;

    match args.format {
        ConfigCheckFormat::Human => {
            println!(
                "muntjac.toml ok: {} platforms, {} trees",
                config.platforms.len(),
                config.trees.len()
            );
        }
        ConfigCheckFormat::Json => {
            let report = serde_json::json!({
                "ok": true,
                "platforms": config.platforms.len(),
                "trees": config.trees.len(),
            });
            println!("{}", report);
        }
    }
    Ok(())
}

fn check_paths(cfg_path: &Path, config: &Config) -> Result<()> {
    let base = cfg_path.parent().unwrap_or(Path::new("."));
    for tree in &config.trees {
        let manifest = resolve(base, &tree.manifest_path);
        anyhow::ensure!(
            manifest.exists(),
            "manifest_path for tree `{}` does not exist: {}",
            tree.name,
            manifest.display()
        );
        let tpd = resolve(base, &tree.third_party_dir);
        if !tpd.exists() {
            // Try to create it to verify writability.
            fs::create_dir_all(&tpd).with_context(|| {
                format!("creating third_party_dir for tree `{}`: {}", tree.name, tpd.display())
            })?;
        }
    }
    Ok(())
}

fn resolve(base: &Path, p: &Path) -> PathBuf {
    if p.is_absolute() { p.to_path_buf() } else { base.join(p) }
}
```

- [ ] **Step 4: Add `serde_json` to dependencies**

Modify `Cargo.toml` `[dependencies]`:

```toml
serde_json = "1.0"
```

- [ ] **Step 5: Run the tests to confirm they pass**

Run: `cargo test --test config_check`
Expected: 3 passed.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/cli/config_check.rs tests/config_check.rs
git commit -m "feat(s0): implement muntjac config check"
```

---

## Task 8: Implement `muntjac init` — pyproject detection and python-version extraction

**Files:**
- Modify: `src/cli/init.rs`

- [ ] **Step 1: Write the failing unit test for detection**

Add to bottom of `src/cli/init.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn detects_pyproject_in_cwd() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("pyproject.toml"), "[project]\nname = \"x\"\nrequires-python = \">=3.11\"\n").unwrap();
        let found = find_pyproject(dir.path()).expect("detected");
        assert_eq!(found.path, dir.path().join("pyproject.toml"));
        assert_eq!(found.python_versions, vec![PythonVersion(3, 11), PythonVersion(3, 12), PythonVersion(3, 13)]);
    }

    #[test]
    fn detects_pyproject_in_parent() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("pyproject.toml"), "[project]\nname = \"x\"\n").unwrap();
        let subdir = dir.path().join("sub");
        fs::create_dir(&subdir).unwrap();
        let found = find_pyproject(&subdir).expect("detected");
        assert_eq!(found.path, dir.path().join("pyproject.toml"));
        // No requires-python → default ["3.12"].
        assert_eq!(found.python_versions, vec![PythonVersion(3, 12)]);
    }

    #[test]
    fn no_detection_in_empty_tree() {
        let dir = tempdir().unwrap();
        assert!(find_pyproject(dir.path()).is_none());
    }

    #[test]
    fn skips_pyproject_without_project_or_tool_uv() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("pyproject.toml"), "[build-system]\nrequires = []\n").unwrap();
        assert!(find_pyproject(dir.path()).is_none());
    }

    #[test]
    fn detects_pyproject_with_tool_uv() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("pyproject.toml"), "[tool.uv]\n").unwrap();
        assert!(find_pyproject(dir.path()).is_some());
    }

    #[test]
    fn expands_requires_python_ranges() {
        assert_eq!(expand_requires_python(">=3.10").unwrap(),
                   vec![PythonVersion(3, 10), PythonVersion(3, 11), PythonVersion(3, 12), PythonVersion(3, 13)]);
        assert_eq!(expand_requires_python(">=3.11,<3.13").unwrap(),
                   vec![PythonVersion(3, 11), PythonVersion(3, 12)]);
        assert_eq!(expand_requires_python("==3.12.*").unwrap(),
                   vec![PythonVersion(3, 12)]);
    }
}
```

- [ ] **Step 2: Run to confirm failures**

Run: `cargo test --lib cli::init`
Expected: FAIL — `find_pyproject` and `expand_requires_python` not yet defined.

- [ ] **Step 3: Implement the detection helpers**

Replace `src/cli/init.rs` (preserving the test module at the bottom):

```rust
use anyhow::{Context, Result};
use clap::Args;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

use crate::cli::Globals;
use crate::config::PythonVersion;

const LATEST_KNOWN_STABLE_PY: u8 = 13;

#[derive(Args, Debug)]
pub struct InitArgs {
    #[arg(long)]
    pub force: bool,
    pub path: Option<PathBuf>,
}

pub struct Detection {
    pub path: PathBuf,
    pub python_versions: Vec<PythonVersion>,
}

pub fn run(_args: InitArgs, _globals: &Globals) -> Result<()> {
    anyhow::bail!("init file-writing not yet implemented (filled in by Task 9)")
}

pub fn find_pyproject(start: &Path) -> Option<Detection> {
    let mut cur = Some(start);
    while let Some(dir) = cur {
        let candidate = dir.join("pyproject.toml");
        if candidate.is_file() {
            if let Ok(bytes) = fs::read_to_string(&candidate) {
                if has_project_or_uv(&bytes) {
                    let python_versions = extract_python_versions(&bytes);
                    return Some(Detection { path: candidate, python_versions });
                }
            }
        }
        cur = dir.parent();
    }
    None
}

#[derive(Deserialize)]
struct PyprojectProbe {
    project: Option<ProjectTable>,
    tool: Option<ToolTable>,
}

#[derive(Deserialize)]
struct ProjectTable {
    #[serde(rename = "requires-python")]
    requires_python: Option<String>,
}

#[derive(Deserialize)]
struct ToolTable {
    uv: Option<toml::Value>,
}

fn has_project_or_uv(toml_src: &str) -> bool {
    let probe: Result<PyprojectProbe, _> = toml::from_str(toml_src);
    match probe {
        Ok(p) => p.project.is_some() || p.tool.and_then(|t| t.uv).is_some(),
        Err(_) => false,
    }
}

fn extract_python_versions(toml_src: &str) -> Vec<PythonVersion> {
    let probe: Result<PyprojectProbe, _> = toml::from_str(toml_src);
    if let Ok(p) = probe {
        if let Some(rp) = p.project.and_then(|p| p.requires_python) {
            if let Ok(versions) = expand_requires_python(&rp) {
                if !versions.is_empty() {
                    return versions;
                }
            }
        }
    }
    vec![PythonVersion(3, 12)]
}

pub fn expand_requires_python(spec: &str) -> Result<Vec<PythonVersion>> {
    // Very small subset: ">=X.Y", ">=X.Y,<X.Z", "==X.Y.*", "==X.Y".
    let mut min_minor: u8 = 11; // muntjac MVP floor
    let mut max_minor: u8 = LATEST_KNOWN_STABLE_PY; // inclusive
    for part in spec.split(',') {
        let part = part.trim();
        if let Some(rest) = part.strip_prefix(">=") {
            let (_, minor) = parse_two(rest)?;
            min_minor = min_minor.max(minor);
        } else if let Some(rest) = part.strip_prefix("<") {
            let (_, minor) = parse_two(rest)?;
            max_minor = max_minor.min(minor.saturating_sub(1));
        } else if let Some(rest) = part.strip_prefix("==") {
            let rest = rest.trim_end_matches(".*");
            // Allow X.Y or X.Y.Z — take X.Y.
            let parts: Vec<&str> = rest.split('.').collect();
            if parts.len() >= 2 {
                let minor: u8 = parts[1].parse().context("bad minor in == bound")?;
                min_minor = minor;
                max_minor = minor;
            }
        }
    }
    let mut out = Vec::new();
    for m in min_minor..=max_minor {
        out.push(PythonVersion(3, m));
    }
    Ok(out)
}

fn parse_two(s: &str) -> Result<(u8, u8)> {
    let parts: Vec<&str> = s.split('.').collect();
    let major: u8 = parts.first().context("missing major")?.parse().context("major not u8")?;
    let minor: u8 = parts.get(1).copied().unwrap_or("0").parse().context("minor not u8")?;
    Ok((major, minor))
}

// Keep the test module from Step 1 here.
```

- [ ] **Step 4: Run the tests to confirm they pass**

Run: `cargo test --lib cli::init`
Expected: all 6 detection tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/cli/init.rs
git commit -m "feat(s0): pyproject.toml detection and python-version extraction"
```

---

## Task 9: Implement `muntjac init` — file writing

**Files:**
- Modify: `src/cli/init.rs`
- Create: `tests/init.rs`

- [ ] **Step 1: Write the failing integration test**

Create `tests/init.rs`:

```rust
mod common;

use common::muntjac;
use predicates::str::contains;
use std::fs;
use tempfile::tempdir;

#[test]
fn init_creates_starter_in_empty_dir() {
    let dir = tempdir().unwrap();
    muntjac()
        .arg("-C").arg(dir.path())
        .arg("init")
        .assert()
        .success();

    let cfg = fs::read_to_string(dir.path().join("muntjac.toml")).unwrap();
    assert!(cfg.contains("manifest_path"));
    assert!(cfg.contains("TODO: muntjac init could not auto-detect"));
    assert!(cfg.contains("[platforms.linux-x86_64-gnu]"));
    assert!(cfg.contains("[fixups]"));

    assert!(dir.path().join("third-party/python/BUCK").exists());
    assert!(dir.path().join("third-party/python/.gitignore").exists());
    assert!(dir.path().join("third-party/python/fixups/.gitkeep").exists());
}

#[test]
fn init_detects_existing_pyproject() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("pyproject.toml"),
              "[project]\nname = \"app\"\nrequires-python = \">=3.11,<3.13\"\n").unwrap();

    muntjac()
        .arg("-C").arg(dir.path())
        .arg("init")
        .assert()
        .success();

    let cfg = fs::read_to_string(dir.path().join("muntjac.toml")).unwrap();
    assert!(!cfg.contains("TODO: muntjac init could not auto-detect"));
    assert!(cfg.contains("python_versions = [\"3.11\", \"3.12\"]"));
}

#[test]
fn init_refuses_to_overwrite() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("muntjac.toml"), "# existing\n").unwrap();

    muntjac()
        .arg("-C").arg(dir.path())
        .arg("init")
        .assert()
        .failure()
        .stderr(contains("already exists"));
}

#[test]
fn init_with_force_overwrites() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("muntjac.toml"), "# old\n").unwrap();

    muntjac()
        .arg("-C").arg(dir.path())
        .arg("init")
        .arg("--force")
        .assert()
        .success();

    let cfg = fs::read_to_string(dir.path().join("muntjac.toml")).unwrap();
    assert!(!cfg.contains("# old"));
    assert!(cfg.contains("[platforms"));
}
```

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test --test init`
Expected: FAIL — "init file-writing not yet implemented".

- [ ] **Step 3: Implement `init::run`**

Replace the placeholder `run` function in `src/cli/init.rs` with:

```rust
pub fn run(args: InitArgs, _globals: &Globals) -> Result<()> {
    let target = match args.path {
        Some(p) => p,
        None => std::env::current_dir().context("getting current dir")?,
    };
    fs::create_dir_all(&target).with_context(|| {
        format!("creating target directory {}", target.display())
    })?;

    let cfg_path = target.join("muntjac.toml");
    if cfg_path.exists() && !args.force {
        anyhow::bail!("muntjac.toml already exists at {}; pass --force to overwrite", cfg_path.display());
    }

    let detection = find_pyproject(&target);
    let cfg_contents = render_starter_config(&target, detection.as_ref());
    fs::write(&cfg_path, cfg_contents).with_context(|| {
        format!("writing {}", cfg_path.display())
    })?;

    write_third_party_skeleton(&target)?;
    println!("muntjac.toml written to {}", cfg_path.display());
    Ok(())
}

fn render_starter_config(target: &Path, detection: Option<&Detection>) -> String {
    let (manifest_path, python_versions, banner) = match detection {
        Some(d) => {
            let rel = relative_to(target, &d.path);
            let versions: Vec<String> = d.python_versions.iter()
                .map(|v| format!("\"{}.{}\"", v.0, v.1))
                .collect();
            (
                format!("\"{}\"", rel.display()),
                format!("[{}]", versions.join(", ")),
                String::new(),
            )
        }
        None => (
            "\"TODO: path to your pyproject.toml\"".to_string(),
            "[\"3.12\"]".to_string(),
            "# TODO: muntjac init could not auto-detect a uv project.\n\
             # Edit `manifest_path` to point at your pyproject.toml, then run `muntjac config check`.\n\n".to_string(),
        ),
    };

    format!(
        "{banner}# Generated by `muntjac init`. Edit freely.\n\
         # See docs/superpowers/specs/2026-05-20-muntjac-design.md for full schema.\n\n\
         manifest_path   = {manifest_path}\n\
         third_party_dir = \"third-party/python\"\n\
         python_versions = {python_versions}\n\n\
         [platforms.linux-x86_64-gnu]\n\
         target    = \"x86_64-unknown-linux-gnu\"\n\
         manylinux = \"2_17\"\n\n\
         [platforms.linux-aarch64-gnu]\n\
         target    = \"aarch64-unknown-linux-gnu\"\n\
         manylinux = \"2_17\"\n\n\
         [platforms.linux-x86_64-musl]\n\
         target    = \"x86_64-unknown-linux-musl\"\n\
         musllinux = \"1_2\"\n\n\
         [platforms.macos-x86_64]\n\
         target    = \"x86_64-apple-darwin\"\n\
         macos_min = \"11.0\"\n\n\
         [platforms.macos-arm64]\n\
         target    = \"aarch64-apple-darwin\"\n\
         macos_min = \"11.0\"\n\n\
         [fixups]\n\
         # Community fixup registry. Leave as \"none\" until a v0.1.0+ release exists.\n\
         registry              = \"none\"\n\
         allow_local_overrides = true\n\n\
         [buck]\n\
         file_name = \"BUCK\"\n\
         vendor    = false\n"
    )
}

fn relative_to(from: &Path, to: &Path) -> PathBuf {
    pathdiff::diff_paths(to, from).unwrap_or_else(|| to.to_path_buf())
}

fn write_third_party_skeleton(target: &Path) -> Result<()> {
    let tp = target.join("third-party/python");
    fs::create_dir_all(&tp).with_context(|| format!("creating {}", tp.display()))?;
    fs::create_dir_all(tp.join("fixups")).context("creating fixups dir")?;

    let buck = tp.join("BUCK");
    if !buck.exists() {
        fs::write(&buck, "# Generated by muntjac. Run: muntjac buckify\n")
            .with_context(|| format!("writing {}", buck.display()))?;
    }
    let gitignore = tp.join(".gitignore");
    if !gitignore.exists() {
        fs::write(&gitignore, "# muntjac-managed; vendor/ holds downloaded wheels.\nvendor/\n")
            .with_context(|| format!("writing {}", gitignore.display()))?;
    }
    let gitkeep = tp.join("fixups/.gitkeep");
    if !gitkeep.exists() {
        fs::write(&gitkeep, "").with_context(|| format!("writing {}", gitkeep.display()))?;
    }
    Ok(())
}
```

- [ ] **Step 4: Add `pathdiff` to dependencies**

Modify `Cargo.toml` `[dependencies]`:

```toml
pathdiff = "0.2"
```

- [ ] **Step 5: Run the tests to confirm they pass**

Run: `cargo test --test init`
Expected: 4 passed.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/cli/init.rs tests/init.rs
git commit -m "feat(s0): implement muntjac init with detection + skeleton writing"
```

---

## Task 10: Snapshot test for `--help` (locks the verb surface)

**Files:**
- Create: `tests/help.rs`
- Create: `tests/snapshots/help__main_help.snap` (generated by `cargo insta`)

- [ ] **Step 1: Write the snapshot test**

Create `tests/help.rs`:

```rust
mod common;

use common::muntjac;

#[test]
fn main_help_surface() {
    let output = muntjac().arg("--help").output().expect("run --help");
    let stdout = String::from_utf8(output.stdout).expect("utf-8");
    insta::assert_snapshot!("main_help", stdout);
}
```

- [ ] **Step 2: Run insta in "review" mode to accept the initial snapshot**

Run: `cargo test --test help`
Expected: FAIL the first time (no snapshot exists) — insta writes a `.new` file.

Then:

```bash
cargo insta review
```

In the TUI, **inspect the snapshot carefully**: it must include the lines `init`, `vendor`, `buckify`, `audit`, `fixups`, `unused`, `config`, with no extras and no missing verbs. If it looks right, press `a` to accept. If wrong, `r` to reject and revisit Task 5.

Alternatively, accept all without TUI: `INSTA_UPDATE=always cargo test --test help`.

- [ ] **Step 3: Verify snapshot now exists and tests pass**

Run: `cargo test --test help`
Expected: PASS.

Run: `ls tests/snapshots/`
Expected: contains `help__main_help.snap` (or similar — insta's naming).

- [ ] **Step 4: Commit**

```bash
git add tests/help.rs tests/snapshots/
git commit -m "test(s0): snapshot --help surface to lock the verb set"
```

---

## Task 11: CI workflow

**Files:**
- Create: `.github/workflows/ci.yml`

- [ ] **Step 1: Write the CI workflow**

Create `.github/workflows/ci.yml`:

```yaml
name: CI

on:
  push:
    branches: [main]
  pull_request:

jobs:
  test:
    name: ${{ matrix.runner }}
    strategy:
      fail-fast: false
      matrix:
        runner: [ubuntu-latest, ubuntu-24.04-arm, macos-latest]
    runs-on: ${{ matrix.runner }}
    steps:
      - uses: actions/checkout@v4

      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy

      - uses: Swatinem/rust-cache@v2

      - name: cargo fmt --check
        run: cargo fmt --all -- --check

      - name: cargo clippy
        run: cargo clippy --all-targets --locked -- -D warnings

      - name: cargo build
        run: cargo build --locked

      - name: cargo test
        run: cargo test --locked
```

- [ ] **Step 2: Verify the workflow file is valid YAML locally**

Run: `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))"`
Expected: silent success.

- [ ] **Step 3: Run the full local equivalent of CI before pushing**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo build --locked
cargo test --locked
```

Expected: all four green. If `cargo fmt --check` fails, run `cargo fmt --all` and recommit (separate commit, before CI commit).

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci(s0): add multi-platform build/test/clippy/fmt workflow"
```

---

## Task 12: README + final verification

**Files:**
- Create: `README.md`

- [ ] **Step 1: Write a minimal README placeholder**

The full README is an S8 deliverable. For S0, a stub is enough to satisfy `cargo publish --dry-run` later and to give visitors context.

Create `README.md`:

```markdown
# muntjac

Translate `uv.lock` into [Buck2](https://buck2.build/) build rules.

Currently in early development. See:

- [Design spec](docs/superpowers/specs/2026-05-20-muntjac-design.md)
- [Roadmap](docs/superpowers/specs/2026-05-20-muntjac-roadmap.md)

## License

MIT
```

- [ ] **Step 2: Re-run the full local CI equivalent to make sure nothing regressed**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo build --locked
cargo test --locked
```

Expected: all four green.

- [ ] **Step 3: Manually exercise the binary for a smoke check**

```bash
cd /tmp && rm -rf muntjac-s0-smoke && mkdir muntjac-s0-smoke && cd muntjac-s0-smoke
cargo run --manifest-path /home/jackm/repos/muntjac/Cargo.toml -- init
cat muntjac.toml
ls -la third-party/python/
cargo run --manifest-path /home/jackm/repos/muntjac/Cargo.toml -- config check
```

Expected:
- `muntjac.toml` exists, contains TODO comment, has 5 platforms.
- `third-party/python/` contains `BUCK`, `.gitignore`, `fixups/.gitkeep`.
- `muntjac config check` fails because the placeholder `manifest_path` doesn't exist — that's correct behavior; the error message should name the missing path.

- [ ] **Step 4: Commit**

```bash
cd /home/jackm/repos/muntjac
git add README.md
git commit -m "docs(s0): minimal README pointing at design + roadmap"
```

- [ ] **Step 5: Tag the stage**

```bash
git tag s0-complete
```

(No remote push; tagging locally so we can refer back to the stage's HEAD when starting S1.)

---

## Exit criteria checklist (per spec §9)

Run through these manually after Task 12. Each line should be verifiable in <30s.

- [ ] `muntjac init` in an empty dir writes `muntjac.toml` + `third-party/python/{BUCK,.gitignore,fixups/.gitkeep}` (Task 9 integration test, plus Task 12 manual check).
- [ ] `muntjac init` in a dir with `pyproject.toml` detects it (Task 9 integration test `init_detects_existing_pyproject`).
- [ ] `muntjac init` refuses to overwrite without `--force` (Task 9 test `init_refuses_to_overwrite`).
- [ ] `muntjac --help` lists all phase-1 verbs with non-implemented ones marked (Task 5 manual, Task 10 snapshot).
- [ ] `muntjac --version` prints version + build identifier (clap-derived `#[command(version)]` automatic; verify with `cargo run -- --version`).
- [ ] `muntjac config check` validates a good config and rejects each failure class (Task 7 integration tests cover the major classes; the 7 failure classes from spec §4 should each have at least one assertion).
- [ ] `muntjac debug` with no subcommand prints `debug requires a subcommand`, exit 2 (Task 6 test).
- [ ] Every stubbed verb prints `not implemented yet (planned for S<n>)` (Task 6 tests).
- [ ] CI matrix green on `cargo build`, `cargo test`, `cargo clippy -D warnings`, `cargo fmt --check` (Task 11 — confirmed once the PR's CI run completes).
- [ ] Snapshot tests pass; running twice produces no diff (Task 10 plus general determinism).

If all check, S0 is complete and we are ready to start the S1 brainstorming.

---

## Self-review notes

- **Spec coverage:**
  - §2 CLI surface → Tasks 5, 6, 7, 8, 9
  - §3 init behavior → Tasks 8, 9
  - §4 config check → Task 7
  - §5 muntjac.toml parsing → Tasks 2, 3, 4
  - §6 error machinery → covered in Tasks 2 (thiserror enum), with anyhow context wrapping in 7 & 9. Note: full miette source-span integration is deferred to a follow-up (called out in spec §10); for S0 the diagnostic display is `anyhow::Error`'s default chain via the `?` operator, which prints adequately. If you find that error output is unfriendly during Task 7 testing, consider adding `miette::Report` wrapping in `src/main.rs`.
  - §7 module layout → Tasks 1, 5
  - §8 testing → unit tests in Tasks 2, 3, 4, 8; integration tests in Tasks 6, 7, 9, 10; CI in Task 11
  - §9 exit criteria → final checklist above

- **Type consistency:** `PythonVersion(u8, u8)` is used identically in `config.rs` and `init.rs`. `FixupRegistry` is one variant for S0 (parses but doesn't introspect deeply); validation accepts the three string shapes per spec §4.

- **No placeholders:** every step has runnable code or commands. The one judgment call deferred is "full miette integration" — explicitly called out in §10 of the spec as a known follow-up.
