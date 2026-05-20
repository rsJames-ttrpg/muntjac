# S1 Lockfile Parser & Dep Graph Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn `uv.lock` into a typed dep graph exposed via a hidden `muntjac debug print-deps` JSON command. Everything downstream (S2 wheel selection, S3 BUCK emission) consumes the data structures this stage produces.

**Architecture:** Two-layer parsing (`RawLockfile` → `Lockfile`), adjacency-map dep graph with Tarjan SCC for cycle detection, `pep508_rs` for marker parsing and evaluation, per-(platform, python_version) projection that produces deterministic sorted JSON. Module boundaries: `lock/` (parser + types + graph + resolved), `platform.rs` (marker env), `cli/debug/` (the user-facing surface).

**Tech Stack:** Rust 2024, `pep508_rs` + `pep440_rs` (Astral's PEP libs, same code uv uses), `url`, plus S0's stack (`clap`, `serde`, `toml`, `thiserror`, `anyhow`, `miette`, `assert_cmd`, `insta`).

**Spec:** `docs/superpowers/specs/2026-05-20-muntjac-s1-lockfile-design.md`

---

## File structure

| Path | Responsibility |
|---|---|
| `Cargo.toml` | Add `pep508_rs`, `pep440_rs`, `url` to `[dependencies]` |
| `src/lock/mod.rs` | Re-exports: `pub use parser::*; pub use types::*; pub use graph::*; pub use resolved::*;` |
| `src/lock/types.rs` | `Package`, `Source`, `FirstPartyKind`, `DepEdge`, `Sdist`, `Wheel`, `Metadata`, `Lockfile` |
| `src/lock/parser.rs` | `RawLockfile`, `parse()`, `Lockfile::from_raw` |
| `src/lock/graph.rs` | `DepGraph`, `GraphNode`, `NodeId`, `build()`, `detect_cycles()`, `reachable_from()` |
| `src/lock/resolved.rs` | `ResolvedView`, `ResolvedConfig`, `ResolvedPackage`, `project()` |
| `src/platform.rs` | `marker_env()`, `marker_matches()`, triple → env-string table |
| `src/cli/debug/mod.rs` | New `Subcommand`-derive `DebugOp` enum |
| `src/cli/debug/print_deps.rs` | `print_deps::run()` |
| `src/config.rs` | Add `LockfileConfig` + `Config.lockfile` field |
| `src/cli/init.rs` | Add commented `[lockfile]` hint to starter template |
| `src/cli/config_check.rs` | Validate `include_groups` identifier shape |
| `src/cli/mod.rs` | Rewrite `Debug` variant from flat-string to `Subcommand` |
| `src/error.rs` | Add `LockfileError` enum |
| `src/lib.rs` | Add `pub mod lock; pub mod platform;` |
| `tests/print_deps.rs` | Integration tests using `assert_cmd` |
| `tests/fixtures/lock/<NN-name>/{pyproject.toml, uv.lock, muntjac.toml, expected-print-deps.json}` | Per-scenario test data |
| `tests/snapshots/help__main_help.snap` | Refresh (help is unchanged but `cargo run -- help debug` text changes; we re-accept the snapshot) |

---

## Task 1: Add deps and create lock/ module scaffold

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/lib.rs`
- Create: `src/lock/mod.rs`
- Create: `src/lock/types.rs` (stub)
- Create: `src/lock/parser.rs` (stub)
- Create: `src/lock/graph.rs` (stub)
- Create: `src/lock/resolved.rs` (stub)
- Create: `src/platform.rs` (stub)

- [ ] **Step 1: Add `pep508_rs`, `pep440_rs`, `url` to `Cargo.toml`**

Edit `Cargo.toml`'s `[dependencies]` section, inserting alphabetically:

```toml
pep440_rs = "0.7"
pep508_rs = "0.9"
```

and (alphabetically after `toml`):

```toml
url = { version = "2", features = ["serde"] }
```

If these exact versions fail to resolve, the implementer should pick the latest compatible major version from crates.io (don't downgrade major). Versions named here are floors known to ship the APIs we need (`MarkerTree::from_str`, `MarkerEnvironmentBuilder`, `Version::from_str`, `PackageName::new`).

- [ ] **Step 2: Create `src/lock/mod.rs`**

```rust
//! Parse `uv.lock` and build a typed dep graph.

pub mod graph;
pub mod parser;
pub mod resolved;
pub mod types;
```

- [ ] **Step 3: Create stub source files**

```rust
// src/lock/types.rs
// (filled in by Task 3)
```

```rust
// src/lock/parser.rs
// (filled in by Tasks 4-6)
```

```rust
// src/lock/graph.rs
// (filled in by Tasks 7-9)
```

```rust
// src/lock/resolved.rs
// (filled in by Task 10)
```

```rust
// src/platform.rs
// (filled in by Task 6)
```

- [ ] **Step 4: Wire new modules into `src/lib.rs`**

Replace `src/lib.rs` contents with:

```rust
pub mod cli;
pub mod config;
pub mod error;
pub mod lock;
pub mod platform;
```

- [ ] **Step 5: Verify cargo build succeeds**

Run: `cargo build`
Expected: builds with warnings about unused empty modules. No compile errors.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock src/lib.rs src/lock/ src/platform.rs
git commit -m "feat(s1): add pep508_rs/pep440_rs/url deps and lock/ module scaffold"
```

---

## Task 2: `LockfileError` enum

**Files:**
- Modify: `src/error.rs`

- [ ] **Step 1: Write the failing test**

Add to the bottom of `src/error.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lockfile_error_messages_include_context() {
        let err = LockfileError::UnsupportedVersion(2);
        assert_eq!(err.to_string(),
            "unsupported uv.lock version `2`; muntjac supports version = 1");

        let err = LockfileError::UnresolvedDep {
            source: "requests-2.34.2".into(),
            dep: "ghost".into(),
        };
        assert!(err.to_string().contains("requests-2.34.2"));
        assert!(err.to_string().contains("ghost"));

        let err = LockfileError::DuplicatePackageName {
            name: "numpy".into(),
            versions: vec!["1.2.0".into(), "2.0.0".into()],
        };
        assert!(err.to_string().contains("numpy"));
    }
}
```

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test --lib error`
Expected: FAIL — `LockfileError` not defined.

- [ ] **Step 3: Add `LockfileError` to `src/error.rs`**

Append after the existing `ConfigError` enum:

```rust
#[derive(Debug, Error)]
pub enum LockfileError {
    #[error("failed to parse uv.lock: {0}")]
    Parse(#[from] toml::de::Error),

    #[error("unsupported uv.lock version `{0}`; muntjac supports version = 1")]
    UnsupportedVersion(u32),

    #[error("package `{package}`: invalid version `{value}`: {reason}")]
    BadVersion { package: String, value: String, reason: String },

    #[error("package `{package}`: source must have exactly one of registry/git/virtual/editable/directory/path; found {found:?}")]
    AmbiguousSource { package: String, found: Vec<&'static str> },

    #[error("package `{package}`: dep `{dep}`: marker `{marker}` failed to parse: {reason}")]
    BadMarker { package: String, dep: String, marker: String, reason: String },

    #[error("package `{source}`: dep `{dep}` has no matching [[package]] entry")]
    UnresolvedDep { source: String, dep: String },

    #[error("duplicate package name `{name}` (found versions {versions:?}); S1 expects one version per name")]
    DuplicatePackageName { name: String, versions: Vec<String> },

    #[error("dependency cycle(s) detected: {0:?}")]
    Cycle(Vec<String>),

    #[error("invalid package name `{0}`: {1}")]
    BadPackageName(String, String),
}
```

- [ ] **Step 4: Run to confirm test passes**

Run: `cargo test --lib error`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/error.rs
git commit -m "feat(s1): LockfileError enum with rich variant context"
```

---

## Task 3: Lockfile type model

**Files:**
- Modify: `src/lock/types.rs`

- [ ] **Step 1: Write the failing test**

Replace `src/lock/types.rs` with the test module first (followed by the impl in step 3):

```rust
use std::collections::BTreeMap;
use std::str::FromStr;

use pep440_rs::Version;
use pep508_rs::{MarkerTree, PackageName, VersionSpecifiers};
use serde::Serialize;
use url::Url;

use crate::config::PythonVersion;

// ---- Types ----
// (filled in by step 3)

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_party_kind_round_trips() {
        let k = FirstPartyKind::Editable;
        assert_eq!(k, FirstPartyKind::Editable);
        assert_ne!(k, FirstPartyKind::Virtual);
    }

    #[test]
    fn package_constructs() {
        let pkg = Package {
            name: PackageName::from_str("numpy").unwrap(),
            version: Version::from_str("2.4.6").unwrap(),
            source: Source::Registry { url: Url::parse("https://pypi.org/simple").unwrap() },
            dependencies: vec![],
            sdist: None,
            wheels: vec![],
            metadata: None,
        };
        assert_eq!(pkg.name.as_ref(), "numpy");
        assert_eq!(pkg.version.to_string(), "2.4.6");
    }
}
```

- [ ] **Step 2: Run to confirm failure (compile error)**

Run: `cargo test --lib lock::types`
Expected: FAIL — types not yet defined.

- [ ] **Step 3: Replace the `// (filled in by step 3)` placeholder with the full type model**

Insert at the marked location (under the imports, before `#[cfg(test)]`):

```rust
#[derive(Debug, Clone)]
pub struct Package {
    pub name: PackageName,
    pub version: Version,
    pub source: Source,
    pub dependencies: Vec<DepEdge>,
    pub sdist: Option<Sdist>,
    pub wheels: Vec<Wheel>,
    pub metadata: Option<Metadata>,
}

#[derive(Debug, Clone)]
pub enum Source {
    Registry { url: Url },
    Git { url: Url, rev: String, subdirectory: Option<String> },
    FirstParty { kind: FirstPartyKind, path: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstPartyKind {
    Virtual,
    Editable,
    Directory,
    Path,
}

#[derive(Debug, Clone)]
pub struct DepEdge {
    pub name: PackageName,
    pub extra: Vec<String>,
    pub marker: Option<MarkerTree>,
}

#[derive(Debug, Clone)]
pub struct Sdist {
    pub url: Url,
    pub hash: String,
    pub size: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct Wheel {
    pub url: Url,
    pub hash: String,
    pub size: Option<u64>,
    pub filename: String,
}

#[derive(Debug, Clone)]
pub struct Metadata {
    pub requires_dist: Vec<DepEdge>,
    pub provides_extras: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Lockfile {
    pub version: u32,
    pub revision: u32,
    pub requires_python: String,  // stored as string; S1 doesn't compare it
    pub packages: Vec<Package>,
}
```

Note: `requires_python` is `String` (not `VersionSpecifiers`) — S1 stores the raw value; nothing in S1 consumes it programmatically. S2+ can promote to a typed value if needed.

Remove the unused `VersionSpecifiers` import (if `cargo build` warns about it).

- [ ] **Step 4: Run tests**

Run: `cargo test --lib lock::types`
Expected: PASS (2 tests).

- [ ] **Step 5: Verify the full build**

Run: `cargo build --tests` and `cargo clippy --all-targets --locked -- -D warnings`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add src/lock/types.rs
git commit -m "feat(s1): lockfile type model (Package, Source, DepEdge, ...)"
```

---

## Task 4: uv.lock parser — basic shape

**Files:**
- Modify: `src/lock/parser.rs`

- [ ] **Step 1: Write the failing test**

Replace `src/lock/parser.rs` with:

```rust
use std::str::FromStr;

use pep440_rs::Version;
use pep508_rs::{MarkerTree, PackageName};
use serde::Deserialize;
use url::Url;

use crate::error::LockfileError;
use crate::lock::types::*;

// ---- Raw wire-format types ----
// (filled in by step 3)

// ---- Public API ----

pub fn parse(toml_src: &str) -> Result<Lockfile, LockfileError> {
    let raw: RawLockfile = toml::from_str(toml_src)?;
    Lockfile::from_raw(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"
version = 1
revision = 3
requires-python = ">=3.12"

[[package]]
name = "certifi"
version = "2026.5.20"
source = { registry = "https://pypi.org/simple" }
"#;

    #[test]
    fn parses_minimal_lockfile() {
        let lock = parse(MINIMAL).expect("parse");
        assert_eq!(lock.version, 1);
        assert_eq!(lock.revision, 3);
        assert_eq!(lock.requires_python, ">=3.12");
        assert_eq!(lock.packages.len(), 1);
        let pkg = &lock.packages[0];
        assert_eq!(pkg.name.as_ref(), "certifi");
        assert_eq!(pkg.version.to_string(), "2026.5.20");
        assert!(matches!(pkg.source, Source::Registry { .. }));
        assert!(pkg.dependencies.is_empty());
        assert!(pkg.sdist.is_none());
        assert!(pkg.wheels.is_empty());
    }

    #[test]
    fn rejects_unsupported_version() {
        let bad = r#"
version = 99
revision = 1
requires-python = ">=3.12"
"#;
        let err = parse(bad).expect_err("should fail");
        assert!(matches!(err, LockfileError::UnsupportedVersion(99)));
    }
}
```

- [ ] **Step 2: Run to confirm failure**

Run: `cargo test --lib lock::parser`
Expected: FAIL — `RawLockfile` / `Lockfile::from_raw` not defined.

- [ ] **Step 3: Implement the raw types and `from_raw`**

Replace the `// (filled in by step 3)` marker with:

```rust
#[derive(Debug, Deserialize)]
struct RawLockfile {
    version: u32,
    #[serde(default = "default_revision")]
    revision: u32,
    #[serde(rename = "requires-python", default)]
    requires_python: String,
    #[serde(default, rename = "package")]
    packages: Vec<RawPackage>,
}

fn default_revision() -> u32 { 0 }

#[derive(Debug, Deserialize)]
struct RawPackage {
    name: String,
    version: String,
    source: RawSource,
    #[serde(default)]
    dependencies: Vec<RawDep>,
    #[serde(default)]
    sdist: Option<RawArtifact>,
    #[serde(default)]
    wheels: Vec<RawArtifact>,
    #[serde(default)]
    metadata: Option<RawMetadata>,
}

#[derive(Debug, Deserialize)]
struct RawSource {
    #[serde(default)]
    registry: Option<String>,
    #[serde(default)]
    git: Option<String>,
    #[serde(default)]
    rev: Option<String>,
    #[serde(default)]
    subdirectory: Option<String>,
    #[serde(default, rename = "virtual")]
    virtual_: Option<String>,
    #[serde(default)]
    editable: Option<String>,
    #[serde(default)]
    directory: Option<String>,
    #[serde(default)]
    path: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawDep {
    name: String,
    #[serde(default)]
    extra: Vec<String>,
    #[serde(default)]
    marker: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawArtifact {
    url: String,
    hash: String,
    #[serde(default)]
    size: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct RawMetadata {
    #[serde(default, rename = "requires-dist")]
    requires_dist: Vec<RawDep>,
    #[serde(default, rename = "provides-extras")]
    provides_extras: Vec<String>,
}

impl Lockfile {
    pub fn from_raw(raw: RawLockfile) -> Result<Self, LockfileError> {
        if raw.version != 1 {
            return Err(LockfileError::UnsupportedVersion(raw.version));
        }
        if raw.revision < 3 {
            eprintln!(
                "warning: uv.lock revision {} is older than the tested floor (3); proceeding",
                raw.revision
            );
        }

        let packages: Result<Vec<Package>, LockfileError> = raw.packages.into_iter()
            .map(package_from_raw)
            .collect();

        Ok(Lockfile {
            version: raw.version,
            revision: raw.revision,
            requires_python: raw.requires_python,
            packages: packages?,
        })
    }
}

fn package_from_raw(rp: RawPackage) -> Result<Package, LockfileError> {
    let name = PackageName::from_str(&rp.name)
        .map_err(|e| LockfileError::BadPackageName(rp.name.clone(), e.to_string()))?;
    let version = Version::from_str(&rp.version)
        .map_err(|e| LockfileError::BadVersion {
            package: rp.name.clone(),
            value: rp.version.clone(),
            reason: e.to_string(),
        })?;
    let source = source_from_raw(&rp.name, rp.source)?;
    Ok(Package {
        name,
        version,
        source,
        dependencies: vec![],   // Task 5 fills this
        sdist: None,            // Task 5
        wheels: vec![],         // Task 5
        metadata: None,         // Task 6
    })
}

fn source_from_raw(pkg_name: &str, rs: RawSource) -> Result<Source, LockfileError> {
    let mut found = Vec::new();
    if rs.registry.is_some()  { found.push("registry"); }
    if rs.git.is_some()       { found.push("git"); }
    if rs.virtual_.is_some()  { found.push("virtual"); }
    if rs.editable.is_some()  { found.push("editable"); }
    if rs.directory.is_some() { found.push("directory"); }
    if rs.path.is_some()      { found.push("path"); }
    if found.len() != 1 {
        return Err(LockfileError::AmbiguousSource {
            package: pkg_name.into(),
            found,
        });
    }
    if let Some(url) = rs.registry {
        let parsed = Url::parse(&url).map_err(|e| LockfileError::BadVersion {
            package: pkg_name.into(),
            value: url.clone(),
            reason: format!("registry URL: {e}"),
        })?;
        return Ok(Source::Registry { url: parsed });
    }
    if let Some(url) = rs.git {
        let parsed = Url::parse(&url).map_err(|e| LockfileError::BadVersion {
            package: pkg_name.into(),
            value: url.clone(),
            reason: format!("git URL: {e}"),
        })?;
        return Ok(Source::Git {
            url: parsed,
            rev: rs.rev.unwrap_or_default(),
            subdirectory: rs.subdirectory,
        });
    }
    let (kind, path) = if let Some(p) = rs.virtual_ {
        (FirstPartyKind::Virtual, p)
    } else if let Some(p) = rs.editable {
        (FirstPartyKind::Editable, p)
    } else if let Some(p) = rs.directory {
        (FirstPartyKind::Directory, p)
    } else if let Some(p) = rs.path {
        (FirstPartyKind::Path, p)
    } else {
        unreachable!("exactly one source field, validated above")
    };
    Ok(Source::FirstParty { kind, path })
}
```

- [ ] **Step 4: Run to confirm passes**

Run: `cargo test --lib lock::parser`
Expected: 2 tests pass.

- [ ] **Step 5: Run clippy + full build**

Run: `cargo build --tests && cargo clippy --all-targets --locked -- -D warnings`
Expected: clean (warnings about unused fields acceptable for now, since Tasks 5-6 will use them).

- [ ] **Step 6: Commit**

```bash
git add src/lock/parser.rs
git commit -m "feat(s1): uv.lock parser basic shape with source disambiguation"
```

---

## Task 5: Parser extension — dependencies, sdist, wheels

**Files:**
- Modify: `src/lock/parser.rs`

- [ ] **Step 1: Write failing tests**

Append to the `tests` module in `src/lock/parser.rs`:

```rust
const WITH_DEPS: &str = r#"
version = 1
revision = 3
requires-python = ">=3.12"

[[package]]
name = "requests"
version = "2.34.2"
source = { registry = "https://pypi.org/simple" }
dependencies = [
    { name = "certifi" },
    { name = "urllib3", extra = ["socks"] },
]
sdist = { url = "https://files.pythonhosted.org/packages/x/requests.tar.gz", hash = "sha256:abc", size = 142856 }
wheels = [
    { url = "https://files.pythonhosted.org/packages/y/requests-2.34.2-py3-none-any.whl", hash = "sha256:def", size = 73075 },
]

[[package]]
name = "certifi"
version = "2026.5.20"
source = { registry = "https://pypi.org/simple" }

[[package]]
name = "urllib3"
version = "2.7.0"
source = { registry = "https://pypi.org/simple" }
"#;

#[test]
fn parses_dependencies_sdist_and_wheels() {
    let lock = parse(WITH_DEPS).expect("parse");
    let requests = lock.packages.iter().find(|p| p.name.as_ref() == "requests").unwrap();
    assert_eq!(requests.dependencies.len(), 2);
    assert_eq!(requests.dependencies[0].name.as_ref(), "certifi");
    assert_eq!(requests.dependencies[1].name.as_ref(), "urllib3");
    assert_eq!(requests.dependencies[1].extra, vec!["socks".to_string()]);
    assert!(requests.sdist.is_some());
    assert_eq!(requests.wheels.len(), 1);
    let wheel = &requests.wheels[0];
    assert_eq!(wheel.hash, "sha256:def");
    assert_eq!(wheel.size, Some(73075));
    assert!(wheel.filename.ends_with(".whl"));
}
```

- [ ] **Step 2: Confirm test fails**

Run: `cargo test --lib lock::parser parses_dependencies`
Expected: FAIL — `requests.dependencies` is empty.

- [ ] **Step 3: Fill in `package_from_raw` to populate deps + sdist + wheels**

In `src/lock/parser.rs`, replace the `package_from_raw` function with:

```rust
fn package_from_raw(rp: RawPackage) -> Result<Package, LockfileError> {
    let name = PackageName::from_str(&rp.name)
        .map_err(|e| LockfileError::BadPackageName(rp.name.clone(), e.to_string()))?;
    let version = Version::from_str(&rp.version)
        .map_err(|e| LockfileError::BadVersion {
            package: rp.name.clone(),
            value: rp.version.clone(),
            reason: e.to_string(),
        })?;
    let source = source_from_raw(&rp.name, rp.source)?;

    let dependencies = rp.dependencies.into_iter()
        .map(|rd| dep_from_raw(&rp.name, rd))
        .collect::<Result<Vec<_>, _>>()?;

    let sdist = rp.sdist.map(|rs| Ok(Sdist {
        url: Url::parse(&rs.url).map_err(|e| LockfileError::BadVersion {
            package: rp.name.clone(),
            value: rs.url.clone(),
            reason: format!("sdist URL: {e}"),
        })?,
        hash: rs.hash,
        size: rs.size,
    })).transpose()?;

    let wheels = rp.wheels.into_iter()
        .map(|rw| Ok(Wheel {
            url: Url::parse(&rw.url).map_err(|e| LockfileError::BadVersion {
                package: rp.name.clone(),
                value: rw.url.clone(),
                reason: format!("wheel URL: {e}"),
            })?,
            filename: rw.url.rsplit('/').next().unwrap_or("").to_string(),
            hash: rw.hash,
            size: rw.size,
        }))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Package {
        name,
        version,
        source,
        dependencies,
        sdist,
        wheels,
        metadata: None,   // Task 6
    })
}

fn dep_from_raw(pkg_name: &str, rd: RawDep) -> Result<DepEdge, LockfileError> {
    let name = PackageName::from_str(&rd.name)
        .map_err(|e| LockfileError::BadPackageName(rd.name.clone(), e.to_string()))?;
    Ok(DepEdge {
        name,
        extra: rd.extra,
        marker: None,   // Task 6
    })
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib lock::parser`
Expected: 3 tests pass.

- [ ] **Step 5: Clippy**

Run: `cargo clippy --all-targets --locked -- -D warnings`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add src/lock/parser.rs
git commit -m "feat(s1): parse dependencies, sdist, and wheels"
```

---

## Task 6: Parser extension — markers and metadata

**Files:**
- Modify: `src/lock/parser.rs`

- [ ] **Step 1: Write failing tests**

Append to the `tests` module:

```rust
const WITH_MARKERS_AND_METADATA: &str = r#"
version = 1
revision = 3
requires-python = ">=3.10"

[[package]]
name = "app"
version = "0.1.0"
source = { virtual = "." }
dependencies = [
    { name = "numpy" },
    { name = "typing-extensions", marker = "python_version < '3.11'" },
]

[package.metadata]
requires-dist = [
    { name = "numpy" },
    { name = "typing-extensions", marker = "python_version < '3.11'" },
]
provides-extras = ["dev"]

[[package]]
name = "numpy"
version = "2.4.6"
source = { registry = "https://pypi.org/simple" }

[[package]]
name = "typing-extensions"
version = "4.12.0"
source = { registry = "https://pypi.org/simple" }
"#;

#[test]
fn parses_markers_and_metadata() {
    let lock = parse(WITH_MARKERS_AND_METADATA).expect("parse");
    let app = lock.packages.iter().find(|p| p.name.as_ref() == "app").unwrap();

    // Marker on the typing-extensions edge
    let te_edge = app.dependencies.iter().find(|d| d.name.as_ref() == "typing-extensions").unwrap();
    assert!(te_edge.marker.is_some());

    let numpy_edge = app.dependencies.iter().find(|d| d.name.as_ref() == "numpy").unwrap();
    assert!(numpy_edge.marker.is_none());

    // Metadata
    let meta = app.metadata.as_ref().expect("metadata");
    assert_eq!(meta.requires_dist.len(), 2);
    assert_eq!(meta.provides_extras, vec!["dev".to_string()]);

    // First-party source
    assert!(matches!(app.source, Source::FirstParty { kind: FirstPartyKind::Virtual, .. }));
}

#[test]
fn rejects_bad_marker() {
    let bad = r#"
version = 1
revision = 3
requires-python = ">=3.10"

[[package]]
name = "app"
version = "0.1.0"
source = { virtual = "." }
dependencies = [
    { name = "x", marker = "this is not a marker expression" },
]
"#;
    let err = parse(bad).expect_err("should fail");
    assert!(matches!(err, LockfileError::BadMarker { .. }));
}
```

- [ ] **Step 2: Confirm tests fail**

Run: `cargo test --lib lock::parser`
Expected: 2 new failures.

- [ ] **Step 3: Fill in marker + metadata parsing**

In `src/lock/parser.rs`, update `dep_from_raw` to parse markers, and `package_from_raw` to populate metadata:

Replace `dep_from_raw` with:

```rust
fn dep_from_raw(pkg_name: &str, rd: RawDep) -> Result<DepEdge, LockfileError> {
    let name = PackageName::from_str(&rd.name)
        .map_err(|e| LockfileError::BadPackageName(rd.name.clone(), e.to_string()))?;
    let marker = rd.marker.as_deref().map(|m| {
        MarkerTree::from_str(m).map_err(|e| LockfileError::BadMarker {
            package: pkg_name.into(),
            dep: rd.name.clone(),
            marker: m.into(),
            reason: e.to_string(),
        })
    }).transpose()?;
    Ok(DepEdge {
        name,
        extra: rd.extra,
        marker,
    })
}
```

In `package_from_raw`, replace `metadata: None` with the populated form:

```rust
let metadata = rp.metadata.map(|rm| Ok::<Metadata, LockfileError>(Metadata {
    requires_dist: rm.requires_dist.into_iter()
        .map(|rd| dep_from_raw(&rp.name, rd))
        .collect::<Result<Vec<_>, _>>()?,
    provides_extras: rm.provides_extras,
})).transpose()?;
```

Then in the final `Ok(Package { ... })`, change `metadata: None` to `metadata`.

- [ ] **Step 4: Run tests**

Run: `cargo test --lib lock::parser`
Expected: 5 tests pass.

- [ ] **Step 5: Clippy**

Run: `cargo clippy --all-targets --locked -- -D warnings`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add src/lock/parser.rs
git commit -m "feat(s1): parse PEP 508 markers and [package.metadata]"
```

---

## Task 7: Marker environment + evaluation

**Files:**
- Modify: `src/platform.rs`

- [ ] **Step 1: Write failing tests**

Replace `src/platform.rs` with:

```rust
//! PEP 508 marker environment construction and evaluation.
//!
//! Maps muntjac's `Platform` + `PythonVersion` to the env strings that
//! `pep508_rs::MarkerEnvironment` expects.

use pep508_rs::{MarkerEnvironment, MarkerEnvironmentBuilder, MarkerTree};

use crate::config::{Platform, PythonVersion};

// ---- Public API ----
// (filled in by step 3)

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn linux_x86_64() -> Platform {
        Platform {
            target: "x86_64-unknown-linux-gnu".into(),
            manylinux: Some("2_17".into()),
            musllinux: None,
            macos_min: None,
        }
    }

    fn macos_arm64() -> Platform {
        Platform {
            target: "aarch64-apple-darwin".into(),
            manylinux: None,
            musllinux: None,
            macos_min: Some("11.0".into()),
        }
    }

    fn marker(s: &str) -> MarkerTree {
        MarkerTree::from_str(s).expect("marker parse")
    }

    #[test]
    fn python_version_marker() {
        let env = marker_env(&linux_x86_64(), PythonVersion(3, 11));
        assert!(marker_matches(Some(&marker("python_version >= '3.10'")), &env));
        assert!(marker_matches(Some(&marker("python_version < '3.13'")), &env));
        assert!(!marker_matches(Some(&marker("python_version < '3.11'")), &env));
    }

    #[test]
    fn sys_platform_marker() {
        let linux = marker_env(&linux_x86_64(), PythonVersion(3, 12));
        let mac = marker_env(&macos_arm64(), PythonVersion(3, 12));
        let m = marker("sys_platform == 'linux'");
        assert!(marker_matches(Some(&m), &linux));
        assert!(!marker_matches(Some(&m), &mac));
    }

    #[test]
    fn platform_machine_marker() {
        let linux_x86 = marker_env(&linux_x86_64(), PythonVersion(3, 12));
        let macos_arm = marker_env(&macos_arm64(), PythonVersion(3, 12));
        assert!(marker_matches(Some(&marker("platform_machine == 'x86_64'")), &linux_x86));
        assert!(marker_matches(Some(&marker("platform_machine == 'arm64'")), &macos_arm));
        assert!(!marker_matches(Some(&marker("platform_machine == 'arm64'")), &linux_x86));
    }

    #[test]
    fn no_marker_always_matches() {
        let env = marker_env(&linux_x86_64(), PythonVersion(3, 11));
        assert!(marker_matches(None, &env));
    }

    #[test]
    fn full_version_lowest_patch() {
        // python_full_version >= '3.12.0' matches PythonVersion(3, 12)
        // python_full_version >= '3.12.4' does NOT match (we use lowest patch)
        let env = marker_env(&linux_x86_64(), PythonVersion(3, 12));
        assert!(marker_matches(Some(&marker("python_full_version >= '3.12.0'")), &env));
        assert!(!marker_matches(Some(&marker("python_full_version >= '3.12.4'")), &env));
    }

    #[test]
    fn excludes_emscripten() {
        // Real numpy marker: platform_system != 'Emscripten'
        let env = marker_env(&linux_x86_64(), PythonVersion(3, 12));
        assert!(marker_matches(Some(&marker("platform_system != 'Emscripten'")), &env));
    }
}
```

- [ ] **Step 2: Confirm tests fail**

Run: `cargo test --lib platform`
Expected: FAIL — `marker_env` / `marker_matches` not defined.

- [ ] **Step 3: Implement `marker_env` + `marker_matches`**

Replace the `// (filled in by step 3)` marker with:

```rust
pub fn marker_env(platform: &Platform, python: PythonVersion) -> MarkerEnvironment {
    let env = derive_env_strings(&platform.target);
    let full = format!("{}.{}.0", python.0, python.1);
    let short = format!("{}.{}", python.0, python.1);

    MarkerEnvironmentBuilder {
        implementation_name: "cpython",
        implementation_version: &full,
        os_name: env.os_name,
        platform_machine: env.platform_machine,
        platform_python_implementation: "CPython",
        platform_release: "",
        platform_system: env.platform_system,
        platform_version: "",
        python_full_version: &full,
        python_version: &short,
        sys_platform: env.sys_platform,
    }
    .into()
}

pub fn marker_matches(marker: Option<&MarkerTree>, env: &MarkerEnvironment) -> bool {
    match marker {
        None => true,
        Some(m) => m.evaluate(env, &mut Vec::new()),
    }
}

struct EnvStrings {
    os_name: &'static str,
    sys_platform: &'static str,
    platform_system: &'static str,
    platform_machine: &'static str,
}

fn derive_env_strings(target: &str) -> EnvStrings {
    match target {
        "x86_64-unknown-linux-gnu" | "x86_64-unknown-linux-musl" => EnvStrings {
            os_name: "posix",
            sys_platform: "linux",
            platform_system: "Linux",
            platform_machine: "x86_64",
        },
        "aarch64-unknown-linux-gnu" | "aarch64-unknown-linux-musl" => EnvStrings {
            os_name: "posix",
            sys_platform: "linux",
            platform_system: "Linux",
            platform_machine: "aarch64",
        },
        "x86_64-apple-darwin" => EnvStrings {
            os_name: "posix",
            sys_platform: "darwin",
            platform_system: "Darwin",
            platform_machine: "x86_64",
        },
        "aarch64-apple-darwin" => EnvStrings {
            os_name: "posix",
            sys_platform: "darwin",
            platform_system: "Darwin",
            platform_machine: "arm64",
        },
        other => {
            // Defensive fallback: leave fields empty. config::validate rejects
            // unknown triples upstream, so we shouldn't reach here in practice.
            eprintln!("warning: unknown target triple `{other}`; marker eval may be inaccurate");
            EnvStrings { os_name: "", sys_platform: "", platform_system: "", platform_machine: "" }
        }
    }
}
```

**Note on the `pep508_rs::MarkerEnvironmentBuilder` API:** if the actual API differs from what's shown above (e.g. the field names are slightly different in the installed crate version), adapt the field names. The intent is to construct a `MarkerEnvironment` with the values shown. Check `cargo doc --open --package pep508_rs` if needed.

- [ ] **Step 4: Run tests**

Run: `cargo test --lib platform`
Expected: 6 tests pass.

- [ ] **Step 5: Clippy**

Run: `cargo clippy --all-targets --locked -- -D warnings`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add src/platform.rs
git commit -m "feat(s1): marker environment construction and evaluation"
```

---

## Task 8: Dep graph construction

**Files:**
- Modify: `src/lock/graph.rs`

- [ ] **Step 1: Write failing tests**

Replace `src/lock/graph.rs` with:

```rust
use std::collections::BTreeMap;
use std::str::FromStr;

use pep440_rs::Version;
use pep508_rs::{MarkerTree, PackageName};
use url::Url;

use crate::error::LockfileError;
use crate::lock::types::*;

pub type NodeId = u32;

#[derive(Debug, Clone)]
pub struct DepGraph {
    pub nodes: Vec<GraphNode>,
    pub by_name: BTreeMap<PackageName, NodeId>,
    pub roots: Vec<NodeId>,
}

#[derive(Debug, Clone)]
pub struct GraphNode {
    pub pkg: Package,
    pub edges_out: Vec<NodeId>,
    pub edge_markers: Vec<Option<MarkerTree>>,
    pub edge_extras: Vec<Vec<String>>,
}

// ---- Public API ----
// (filled in by step 3)

#[cfg(test)]
mod tests {
    use super::*;

    fn pkg(name: &str, version: &str, source: Source, deps: Vec<&str>) -> Package {
        Package {
            name: PackageName::from_str(name).unwrap(),
            version: Version::from_str(version).unwrap(),
            source,
            dependencies: deps.into_iter().map(|n| DepEdge {
                name: PackageName::from_str(n).unwrap(),
                extra: vec![],
                marker: None,
            }).collect(),
            sdist: None,
            wheels: vec![],
            metadata: None,
        }
    }

    fn registry() -> Source {
        Source::Registry { url: Url::parse("https://pypi.org/simple").unwrap() }
    }
    fn first_party() -> Source {
        Source::FirstParty { kind: FirstPartyKind::Virtual, path: ".".into() }
    }

    #[test]
    fn builds_linear_chain() {
        let lock = Lockfile {
            version: 1, revision: 3, requires_python: ">=3.12".into(),
            packages: vec![
                pkg("a", "1.0", first_party(), vec!["b"]),
                pkg("b", "1.0", registry(), vec!["c"]),
                pkg("c", "1.0", registry(), vec![]),
            ],
        };
        let g = build(&lock).expect("build");
        assert_eq!(g.nodes.len(), 3);
        assert_eq!(g.roots.len(), 1);
        // a -> b
        let a_id = g.by_name[&PackageName::from_str("a").unwrap()];
        let b_id = g.by_name[&PackageName::from_str("b").unwrap()];
        assert_eq!(g.nodes[a_id as usize].edges_out, vec![b_id]);
    }

    #[test]
    fn errors_on_duplicate_package_name() {
        let lock = Lockfile {
            version: 1, revision: 3, requires_python: ">=3.12".into(),
            packages: vec![
                pkg("numpy", "1.0", registry(), vec![]),
                pkg("numpy", "2.0", registry(), vec![]),
            ],
        };
        let err = build(&lock).expect_err("should fail");
        assert!(matches!(err, LockfileError::DuplicatePackageName { .. }));
    }

    #[test]
    fn errors_on_unresolved_dep() {
        let lock = Lockfile {
            version: 1, revision: 3, requires_python: ">=3.12".into(),
            packages: vec![
                pkg("a", "1.0", first_party(), vec!["ghost"]),
            ],
        };
        let err = build(&lock).expect_err("should fail");
        match err {
            LockfileError::UnresolvedDep { source, dep } => {
                assert_eq!(source, "a");
                assert_eq!(dep, "ghost");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
```

- [ ] **Step 2: Confirm tests fail**

Run: `cargo test --lib lock::graph`
Expected: FAIL — `build` not defined.

- [ ] **Step 3: Implement `build`**

Replace the `// (filled in by step 3)` marker with:

```rust
pub fn build(lock: &Lockfile) -> Result<DepGraph, LockfileError> {
    // Pass 1: allocate nodes, build by_name index, detect dupes.
    let mut nodes = Vec::with_capacity(lock.packages.len());
    let mut by_name = BTreeMap::<PackageName, NodeId>::new();
    let mut dup_versions = BTreeMap::<PackageName, Vec<String>>::new();

    for pkg in &lock.packages {
        let id = nodes.len() as NodeId;
        if let Some(existing_id) = by_name.get(&pkg.name) {
            let existing = &nodes[*existing_id as usize];
            dup_versions.entry(pkg.name.clone())
                .or_insert_with(|| vec![existing_pkg_version(&existing).to_string()])
                .push(pkg.version.to_string());
        }
        nodes.push(GraphNode {
            pkg: pkg.clone(),
            edges_out: vec![],
            edge_markers: vec![],
            edge_extras: vec![],
        });
        by_name.insert(pkg.name.clone(), id);
    }

    if let Some((name, versions)) = dup_versions.into_iter().next() {
        return Err(LockfileError::DuplicatePackageName {
            name: name.as_ref().to_string(),
            versions,
        });
    }

    // Pass 2: resolve edges.
    for id in 0..(nodes.len() as NodeId) {
        let edges = nodes[id as usize].pkg.dependencies.clone();
        let source_name = nodes[id as usize].pkg.name.clone();
        for edge in edges {
            let target = by_name.get(&edge.name).ok_or_else(|| LockfileError::UnresolvedDep {
                source: source_name.as_ref().to_string(),
                dep: edge.name.as_ref().to_string(),
            })?;
            nodes[id as usize].edges_out.push(*target);
            nodes[id as usize].edge_markers.push(edge.marker);
            nodes[id as usize].edge_extras.push(edge.extra);
        }
    }

    // Roots: first-party packages.
    let roots: Vec<NodeId> = nodes.iter().enumerate()
        .filter(|(_, n)| matches!(n.pkg.source, Source::FirstParty { .. }))
        .map(|(i, _)| i as NodeId)
        .collect();

    Ok(DepGraph { nodes, by_name, roots })
}

fn existing_pkg_version(node: &GraphNode) -> &Version {
    &node.pkg.version
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib lock::graph`
Expected: 3 tests pass.

- [ ] **Step 5: Clippy + build**

Run: `cargo clippy --all-targets --locked -- -D warnings`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add src/lock/graph.rs
git commit -m "feat(s1): build dep graph with name index and root detection"
```

---

## Task 9: Cycle detection (Tarjan SCC)

**Files:**
- Modify: `src/lock/graph.rs`

- [ ] **Step 1: Write failing tests**

Append to the `tests` module in `src/lock/graph.rs`:

```rust
#[test]
fn detects_self_loop() {
    let lock = Lockfile {
        version: 1, revision: 3, requires_python: ">=3.12".into(),
        packages: vec![
            pkg("a", "1.0", first_party(), vec!["a"]),
        ],
    };
    let g = build(&lock).expect("build");
    let err = detect_cycles(&g).expect_err("should fail");
    match err {
        LockfileError::Cycle(cycles) => {
            assert_eq!(cycles.len(), 1);
            assert!(cycles[0].contains("a@1.0"));
        }
        other => panic!("unexpected: {other:?}"),
    }
}

#[test]
fn detects_three_cycle() {
    let lock = Lockfile {
        version: 1, revision: 3, requires_python: ">=3.12".into(),
        packages: vec![
            pkg("a", "1.0", first_party(), vec!["b"]),
            pkg("b", "1.0", registry(), vec!["c"]),
            pkg("c", "1.0", registry(), vec!["a"]),
        ],
    };
    let g = build(&lock).expect("build");
    let err = detect_cycles(&g).expect_err("should fail");
    let s = format!("{err}");
    assert!(s.contains("a@1.0") && s.contains("b@1.0") && s.contains("c@1.0"));
}

#[test]
fn passes_acyclic_diamond() {
    let lock = Lockfile {
        version: 1, revision: 3, requires_python: ">=3.12".into(),
        packages: vec![
            pkg("a", "1.0", first_party(), vec!["b", "c"]),
            pkg("b", "1.0", registry(), vec!["d"]),
            pkg("c", "1.0", registry(), vec!["d"]),
            pkg("d", "1.0", registry(), vec![]),
        ],
    };
    let g = build(&lock).expect("build");
    detect_cycles(&g).expect("acyclic");
}
```

- [ ] **Step 2: Confirm failure**

Run: `cargo test --lib lock::graph`
Expected: 3 new failures.

- [ ] **Step 3: Implement `detect_cycles` using Tarjan's SCC**

Add to `src/lock/graph.rs`:

```rust
pub fn detect_cycles(graph: &DepGraph) -> Result<(), LockfileError> {
    let sccs = tarjan_scc(graph);
    let cycles: Vec<String> = sccs.iter()
        .filter(|scc| scc.len() > 1 || has_self_loop(graph, scc[0]))
        .map(|scc| format_cycle(graph, scc))
        .collect();
    if cycles.is_empty() { return Ok(()); }
    Err(LockfileError::Cycle(cycles))
}

fn has_self_loop(graph: &DepGraph, id: NodeId) -> bool {
    graph.nodes[id as usize].edges_out.contains(&id)
}

fn format_cycle(graph: &DepGraph, scc: &[NodeId]) -> String {
    let mut sorted = scc.to_vec();
    sorted.sort();
    sorted.iter()
        .map(|&id| {
            let n = &graph.nodes[id as usize];
            format!("{}@{}", n.pkg.name, n.pkg.version)
        })
        .collect::<Vec<_>>()
        .join(" -> ")
}

fn tarjan_scc(graph: &DepGraph) -> Vec<Vec<NodeId>> {
    let n = graph.nodes.len();
    let mut index = vec![-1i32; n];
    let mut lowlink = vec![0i32; n];
    let mut on_stack = vec![false; n];
    let mut stack = Vec::new();
    let mut next_index = 0i32;
    let mut sccs = Vec::new();

    fn strongconnect(
        v: usize,
        graph: &DepGraph,
        index: &mut [i32],
        lowlink: &mut [i32],
        on_stack: &mut [bool],
        stack: &mut Vec<NodeId>,
        next_index: &mut i32,
        sccs: &mut Vec<Vec<NodeId>>,
    ) {
        index[v] = *next_index;
        lowlink[v] = *next_index;
        *next_index += 1;
        stack.push(v as NodeId);
        on_stack[v] = true;

        for &w in &graph.nodes[v].edges_out {
            let w = w as usize;
            if index[w] == -1 {
                strongconnect(w, graph, index, lowlink, on_stack, stack, next_index, sccs);
                lowlink[v] = lowlink[v].min(lowlink[w]);
            } else if on_stack[w] {
                lowlink[v] = lowlink[v].min(index[w]);
            }
        }

        if lowlink[v] == index[v] {
            let mut scc = Vec::new();
            loop {
                let w = stack.pop().unwrap();
                on_stack[w as usize] = false;
                scc.push(w);
                if w as usize == v { break; }
            }
            sccs.push(scc);
        }
    }

    for v in 0..n {
        if index[v] == -1 {
            strongconnect(v, graph, &mut index, &mut lowlink, &mut on_stack, &mut stack, &mut next_index, &mut sccs);
        }
    }
    sccs
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib lock::graph`
Expected: 6 tests pass.

- [ ] **Step 5: Clippy**

Run: `cargo clippy --all-targets --locked -- -D warnings`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add src/lock/graph.rs
git commit -m "feat(s1): Tarjan SCC cycle detection with all-cycles reporting"
```

---

## Task 10: Reachability with marker filtering and group gating

**Files:**
- Modify: `src/lock/graph.rs`

- [ ] **Step 1: Write failing tests**

Append to the `tests` module:

```rust
use crate::config::{Platform, PythonVersion};
use crate::platform::marker_env;
use std::collections::BTreeSet;

fn linux_x86() -> Platform {
    Platform {
        target: "x86_64-unknown-linux-gnu".into(),
        manylinux: Some("2_17".into()),
        musllinux: None,
        macos_min: None,
    }
}

#[test]
fn reachable_drops_marker_false_edges() {
    use std::str::FromStr;
    let mut chain = Lockfile {
        version: 1, revision: 3, requires_python: ">=3.10".into(),
        packages: vec![
            pkg("app", "0.1", first_party(), vec![]),
            pkg("typing-extensions", "4.0", registry(), vec![]),
        ],
    };
    // Manually attach a marker-gated edge: app -> typing-extensions when python < 3.11
    chain.packages[0].dependencies.push(DepEdge {
        name: PackageName::from_str("typing-extensions").unwrap(),
        extra: vec![],
        marker: Some(MarkerTree::from_str("python_version < '3.11'").unwrap()),
    });
    let g = build(&chain).expect("build");

    // Under py3.10: typing-extensions IS reachable
    let env_310 = marker_env(&linux_x86(), PythonVersion(3, 10));
    let r = reachable_from(&g, &g.roots, &env_310, &[]);
    assert_eq!(r.len(), 2);

    // Under py3.12: NOT reachable
    let env_312 = marker_env(&linux_x86(), PythonVersion(3, 12));
    let r = reachable_from(&g, &g.roots, &env_312, &[]);
    assert_eq!(r.len(), 1);
}

#[test]
fn reachable_respects_group_gating() {
    use std::str::FromStr;
    let mut lock = Lockfile {
        version: 1, revision: 3, requires_python: ">=3.12".into(),
        packages: vec![
            pkg("app", "0.1", first_party(), vec![]),
            pkg("pytest", "8.0", registry(), vec![]),
        ],
    };
    // Test-group-gated edge: app -> pytest when extra == 'test'
    lock.packages[0].dependencies.push(DepEdge {
        name: PackageName::from_str("pytest").unwrap(),
        extra: vec![],
        marker: Some(MarkerTree::from_str("extra == 'test'").unwrap()),
    });
    let g = build(&lock).expect("build");
    let env = marker_env(&linux_x86(), PythonVersion(3, 12));

    // Without include_groups, pytest NOT reachable
    let r = reachable_from(&g, &g.roots, &env, &[]);
    assert_eq!(r.len(), 1);

    // With include_groups = ["test"], pytest IS reachable
    let r = reachable_from(&g, &g.roots, &env, &["test".to_string()]);
    assert_eq!(r.len(), 2);
}
```

- [ ] **Step 2: Confirm failure**

Run: `cargo test --lib lock::graph reachable`
Expected: 2 failures (`reachable_from` not defined).

- [ ] **Step 3: Implement `reachable_from`**

Add to `src/lock/graph.rs`:

```rust
use pep508_rs::MarkerEnvironment;
use std::collections::BTreeSet;

pub fn reachable_from(
    graph: &DepGraph,
    roots: &[NodeId],
    env: &MarkerEnvironment,
    include_groups: &[String],
) -> BTreeSet<NodeId> {
    let mut reached = BTreeSet::new();
    let mut stack: Vec<NodeId> = roots.to_vec();
    while let Some(node) = stack.pop() {
        if !reached.insert(node) { continue; }
        let n = &graph.nodes[node as usize];
        for (i, &target) in n.edges_out.iter().enumerate() {
            let marker = n.edge_markers[i].as_ref();
            if !edge_applies(marker, env, include_groups) { continue; }
            stack.push(target);
        }
    }
    reached
}

fn edge_applies(
    marker: Option<&MarkerTree>,
    env: &MarkerEnvironment,
    include_groups: &[String],
) -> bool {
    let Some(m) = marker else { return true; };
    // If the marker references `extra == '<name>'`, only apply when <name> is in include_groups.
    // pep508_rs evaluates extras via the `extras` argument to `.evaluate_extras`. We use the
    // simpler approach: evaluate with each included group as an "extra" and take OR.
    let extras: Vec<pep508_rs::ExtraName> = include_groups.iter()
        .filter_map(|s| pep508_rs::ExtraName::new(s.clone()).ok())
        .collect();
    let mut errs = Vec::new();
    m.evaluate_extras(env, &extras, &mut errs)
}
```

**Note on `evaluate_extras`:** if `pep508_rs` exposes this method differently in the installed version, the implementer should look at the docs (`cargo doc --open --package pep508_rs`) and find the equivalent. The intent: evaluate the marker treating `include_groups` items as activated `extra` values. If unavailable, fall back to: (1) when marker is `extra == 'X'`, check `X` is in `include_groups`; (2) otherwise use plain `.evaluate(env, ...)`.

- [ ] **Step 4: Run tests**

Run: `cargo test --lib lock::graph`
Expected: 8 tests pass.

- [ ] **Step 5: Clippy**

Run: `cargo clippy --all-targets --locked -- -D warnings`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add src/lock/graph.rs
git commit -m "feat(s1): reachable_from with marker + group filtering"
```

---

## Task 11: ResolvedView projection

**Files:**
- Modify: `src/lock/resolved.rs`

- [ ] **Step 1: Write failing tests**

Replace `src/lock/resolved.rs` with:

```rust
use serde::Serialize;
use std::collections::BTreeMap;

use crate::config::{Config, PythonVersion, Tree};
use crate::lock::graph::{reachable_from, DepGraph, NodeId};
use crate::lock::types::{FirstPartyKind, Source};
use crate::platform::marker_env;

#[derive(Debug, Clone, Serialize)]
pub struct ResolvedView {
    pub configs: Vec<ResolvedConfig>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolvedConfig {
    pub platform: String,
    pub python_version: String,
    pub packages: Vec<ResolvedPackage>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResolvedPackage {
    pub name: String,
    pub version: String,
    pub kind: ResolvedKind,
    #[serde(flatten)]
    pub source_info: SourceInfo,
    pub deps: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResolvedKind {
    Registry,
    Git,
    FirstParty,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum SourceInfo {
    Registry { registry: String },
    Git { git: String, rev: String, #[serde(skip_serializing_if = "Option::is_none")] subdirectory: Option<String> },
    FirstParty { first_party_kind: String, first_party_path: String },
}

// ---- Public API ----
// (filled in by step 3)

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{FixupRegistry, FixupsConfig, BuckConfig, LockfileConfig, Platform};
    use crate::lock::graph::build;
    use crate::lock::types::*;
    use pep440_rs::Version;
    use pep508_rs::PackageName;
    use std::str::FromStr;
    use url::Url;

    fn fixture_lock() -> Lockfile {
        Lockfile {
            version: 1, revision: 3, requires_python: ">=3.12".into(),
            packages: vec![
                Package {
                    name: PackageName::from_str("app").unwrap(),
                    version: Version::from_str("0.1").unwrap(),
                    source: Source::FirstParty { kind: FirstPartyKind::Virtual, path: ".".into() },
                    dependencies: vec![DepEdge {
                        name: PackageName::from_str("numpy").unwrap(),
                        extra: vec![],
                        marker: None,
                    }],
                    sdist: None, wheels: vec![], metadata: None,
                },
                Package {
                    name: PackageName::from_str("numpy").unwrap(),
                    version: Version::from_str("2.4.6").unwrap(),
                    source: Source::Registry { url: Url::parse("https://pypi.org/simple").unwrap() },
                    dependencies: vec![],
                    sdist: None, wheels: vec![], metadata: None,
                },
            ],
        }
    }

    fn fixture_config() -> (Config, Tree) {
        let mut platforms = BTreeMap::new();
        platforms.insert("linux-x86_64-gnu".into(), Platform {
            target: "x86_64-unknown-linux-gnu".into(),
            manylinux: Some("2_17".into()),
            musllinux: None, macos_min: None,
        });
        let tree = Tree {
            name: "default".into(),
            manifest_path: "pyproject.toml".into(),
            third_party_dir: ".".into(),
            python_versions: vec![PythonVersion(3, 12)],
        };
        let config = Config {
            trees: vec![tree.clone()],
            platforms,
            fixups: FixupsConfig::default(),
            buck: BuckConfig::default(),
            lockfile: LockfileConfig::default(),
        };
        (config, tree)
    }

    #[test]
    fn project_produces_one_config_per_combination() {
        let lock = fixture_lock();
        let g = build(&lock).expect("build");
        let (cfg, tree) = fixture_config();
        let view = project(&g, &cfg, &tree);
        assert_eq!(view.configs.len(), 1);   // 1 platform * 1 python
        let cfg0 = &view.configs[0];
        assert_eq!(cfg0.platform, "linux-x86_64-gnu");
        assert_eq!(cfg0.python_version, "3.12");
        assert_eq!(cfg0.packages.len(), 2);   // app + numpy
        // First-party first, then registry packages alphabetically.
        assert_eq!(cfg0.packages[0].name, "app");
        assert!(matches!(cfg0.packages[0].kind, ResolvedKind::FirstParty));
        assert_eq!(cfg0.packages[1].name, "numpy");
        assert!(matches!(cfg0.packages[1].kind, ResolvedKind::Registry));
    }

    #[test]
    fn project_is_deterministic() {
        let lock = fixture_lock();
        let g = build(&lock).expect("build");
        let (cfg, tree) = fixture_config();
        let v1 = serde_json::to_string(&project(&g, &cfg, &tree)).unwrap();
        let v2 = serde_json::to_string(&project(&g, &cfg, &tree)).unwrap();
        assert_eq!(v1, v2);
    }
}
```

- [ ] **Step 2: Confirm failure**

Run: `cargo test --lib lock::resolved`
Expected: FAIL — `project` not defined, plus `LockfileConfig` not defined (Task 12 adds it).

**If `LockfileConfig` is the only compile error** at this point: skip ahead to Task 12, do that, then come back.

- [ ] **Step 3: Implement `project()`**

Replace the `// (filled in by step 3)` marker with:

```rust
pub fn project(graph: &DepGraph, cfg: &Config, tree: &Tree) -> ResolvedView {
    let mut configs = Vec::new();
    for (platform_name, platform) in &cfg.platforms {
        for &python in &tree.python_versions {
            let env = marker_env(platform, python);
            let reachable = reachable_from(graph, &graph.roots, &env, &cfg.lockfile.include_groups);
            let packages = build_packages(graph, &reachable, &env, &cfg.lockfile.include_groups);
            configs.push(ResolvedConfig {
                platform: platform_name.clone(),
                python_version: format!("{}.{}", python.0, python.1),
                packages,
            });
        }
    }
    // Sort configs by (platform, python_version) lexicographically.
    configs.sort_by(|a, b| (a.platform.as_str(), a.python_version.as_str())
                            .cmp(&(b.platform.as_str(), b.python_version.as_str())));
    ResolvedView { configs }
}

fn build_packages(
    graph: &DepGraph,
    reachable: &std::collections::BTreeSet<NodeId>,
    env: &pep508_rs::MarkerEnvironment,
    include_groups: &[String],
) -> Vec<ResolvedPackage> {
    let mut packages: Vec<ResolvedPackage> = reachable.iter()
        .map(|&id| build_one(graph, id, env, include_groups))
        .collect();
    // Order: first-party first, then alphabetical by name, then version.
    packages.sort_by(|a, b| {
        kind_rank(&a.kind).cmp(&kind_rank(&b.kind))
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.version.cmp(&b.version))
    });
    packages
}

fn kind_rank(k: &ResolvedKind) -> u8 {
    match k {
        ResolvedKind::FirstParty => 0,
        ResolvedKind::Registry => 1,
        ResolvedKind::Git => 2,
    }
}

fn build_one(
    graph: &DepGraph,
    id: NodeId,
    env: &pep508_rs::MarkerEnvironment,
    include_groups: &[String],
) -> ResolvedPackage {
    let n = &graph.nodes[id as usize];
    let kind = match &n.pkg.source {
        Source::Registry { .. } => ResolvedKind::Registry,
        Source::Git { .. } => ResolvedKind::Git,
        Source::FirstParty { .. } => ResolvedKind::FirstParty,
    };
    let source_info = match &n.pkg.source {
        Source::Registry { url } => SourceInfo::Registry { registry: url.to_string() },
        Source::Git { url, rev, subdirectory } => SourceInfo::Git {
            git: url.to_string(),
            rev: rev.clone(),
            subdirectory: subdirectory.clone(),
        },
        Source::FirstParty { kind, path } => SourceInfo::FirstParty {
            first_party_kind: format!("{kind:?}").to_lowercase(),
            first_party_path: path.clone(),
        },
    };
    let mut deps: Vec<String> = n.edges_out.iter().enumerate()
        .filter(|(i, _)| {
            let marker = n.edge_markers[*i].as_ref();
            crate::lock::graph::edge_applies_pub(marker, env, include_groups)
        })
        .map(|(_, &target_id)| {
            let t = &graph.nodes[target_id as usize];
            format!("{}@{}", t.pkg.name, t.pkg.version)
        })
        .collect();
    deps.sort();
    deps.dedup();
    ResolvedPackage {
        name: n.pkg.name.as_ref().to_string(),
        version: n.pkg.version.to_string(),
        kind,
        source_info,
        deps,
    }
}
```

Then expose `edge_applies` from `graph.rs` as a `pub` helper (with renamed name for clarity):

In `src/lock/graph.rs`, change `fn edge_applies` to `pub fn edge_applies_pub`, OR add a small wrapper. The cleanest is to expose it:

```rust
// in src/lock/graph.rs
pub fn edge_applies_pub(
    marker: Option<&MarkerTree>,
    env: &MarkerEnvironment,
    include_groups: &[String],
) -> bool {
    edge_applies(marker, env, include_groups)
}
```

(Leave the private `edge_applies` as-is; the wrapper just exposes it.)

- [ ] **Step 4: Run tests**

Run: `cargo test --lib lock::resolved`
Expected: 2 tests pass (if Task 12 is done) OR fail with `LockfileConfig` undefined (do Task 12 first).

- [ ] **Step 5: Clippy + full suite**

Run: `cargo test --locked && cargo clippy --all-targets --locked -- -D warnings`
Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add src/lock/resolved.rs src/lock/graph.rs
git commit -m "feat(s1): ResolvedView projection with deterministic ordering"
```

---

## Task 12: `LockfileConfig` addition

**Files:**
- Modify: `src/config.rs`

- [ ] **Step 1: Write failing tests**

Add to the `tests` module in `src/config.rs`:

```rust
#[test]
fn parses_lockfile_config_section() {
    let toml_str = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target = "x86_64-unknown-linux-gnu"

[lockfile]
include_groups = ["test", "docs"]
"#;
    let config = Config::from_str(toml_str).expect("parse");
    assert_eq!(config.lockfile.include_groups, vec!["test".to_string(), "docs".to_string()]);
}

#[test]
fn lockfile_config_defaults_to_empty() {
    let toml_str = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target = "x86_64-unknown-linux-gnu"
"#;
    let config = Config::from_str(toml_str).expect("parse");
    assert!(config.lockfile.include_groups.is_empty());
}

#[test]
fn rejects_invalid_group_identifier() {
    let toml_str = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target = "x86_64-unknown-linux-gnu"

[lockfile]
include_groups = ["bad name with space"]
"#;
    let config = Config::from_str(toml_str).expect("parse");
    let err = config.validate().expect_err("should fail");
    assert!(matches!(err, crate::error::ConfigError::BadPlatform { .. }) || err.to_string().contains("group"));
}
```

- [ ] **Step 2: Confirm failures**

Run: `cargo test --lib config`
Expected: 3 new failures.

- [ ] **Step 3: Add `LockfileConfig` to `src/config.rs`**

Add this type definition (alongside `BuckConfig`):

```rust
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Default)]
pub struct LockfileConfig {
    #[serde(default)]
    pub include_groups: Vec<String>,
}
```

Add `lockfile: LockfileConfig` to `Config`:

```rust
pub struct Config {
    pub trees: Vec<Tree>,
    pub platforms: BTreeMap<String, Platform>,
    pub fixups: FixupsConfig,
    pub buck: BuckConfig,
    pub lockfile: LockfileConfig,        // NEW
}
```

Add it to `RawConfig` as well:

```rust
struct RawConfig {
    // ... existing fields ...
    #[serde(default)]
    lockfile: LockfileConfig,
    // ...
}
```

Update `from_raw` to thread it through:

```rust
Ok(Config { trees, platforms: raw.platforms, fixups: raw.fixups, buck: raw.buck, lockfile: raw.lockfile })
```

Add a new error variant and validation to the `Config::validate` method:

In `src/error.rs`, add to `ConfigError`:

```rust
#[error("invalid dependency-group name `{0}`: must match [a-z][a-z0-9-]*")]
BadGroupName(String),
```

In `src/config.rs`'s `Config::validate`, add at the end (before `Ok(())`):

```rust
for g in &self.lockfile.include_groups {
    validate_group_name(g)?;
}
```

Add the helper function:

```rust
fn validate_group_name(name: &str) -> Result<(), crate::error::ConfigError> {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return Err(crate::error::ConfigError::BadGroupName(name.to_string())),
    }
    for c in chars {
        if !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
            return Err(crate::error::ConfigError::BadGroupName(name.to_string()));
        }
    }
    Ok(())
}
```

Update the third test assertion to match the new error variant:

```rust
#[test]
fn rejects_invalid_group_identifier() {
    let toml_str = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target = "x86_64-unknown-linux-gnu"

[lockfile]
include_groups = ["bad name with space"]
"#;
    let config = Config::from_str(toml_str).expect("parse");
    let err = config.validate().expect_err("should fail");
    assert!(matches!(err, crate::error::ConfigError::BadGroupName(_)));
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib config`
Expected: 12 tests pass (9 from S0 + 3 new).

- [ ] **Step 5: Clippy + full suite**

Run: `cargo test --locked && cargo clippy --all-targets --locked -- -D warnings`
Expected: all green. (The integration tests in `tests/config_check.rs` may still pass — they use valid configs.)

- [ ] **Step 6: Commit**

```bash
git add src/config.rs src/error.rs
git commit -m "feat(s1): LockfileConfig with include_groups identifier validation"
```

---

## Task 13: CLI rewrite — `Debug` subcommand + help snapshot refresh

**Files:**
- Modify: `src/cli/mod.rs`
- Delete: `src/cli/debug.rs`
- Create: `src/cli/debug/mod.rs`
- Modify: `tests/snapshots/help__main_help.snap`

- [ ] **Step 1: Move `src/cli/debug.rs` to `src/cli/debug/mod.rs`**

Run:

```bash
mkdir -p src/cli/debug
git mv src/cli/debug.rs src/cli/debug/mod.rs
```

- [ ] **Step 2: Replace `src/cli/debug/mod.rs` with the Subcommand-based dispatcher**

```rust
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
```

- [ ] **Step 3: Create `src/cli/debug/print_deps.rs` stub (filled in by Task 14)**

```rust
use anyhow::Result;

use crate::cli::Globals;
use crate::cli::debug::PrintDepsArgs;

pub fn run(_args: PrintDepsArgs, _globals: &Globals) -> Result<()> {
    anyhow::bail!("print-deps not yet implemented (filled in by Task 14)")
}
```

- [ ] **Step 4: Rewrite the `Debug` variant in `src/cli/mod.rs`**

Find the `Debug { subcommand, args }` variant. Replace its definition AND its match arm.

The variant (in the `Command` enum) becomes:

```rust
/// Hidden debug subcommands (not stable; for muntjac internals).
#[command(hide = true)]
Debug {
    #[command(subcommand)]
    op: Option<debug::DebugOp>,
},
```

The match arm in `run` becomes:

```rust
Command::Debug { op } => debug::run(op, &cli.globals),
```

- [ ] **Step 5: Verify build**

Run: `cargo build`
Expected: builds. If clap complains about the empty enum, that's expected only when `DebugOp` has no variants — but we have `PrintDeps` now, so this should be fine.

- [ ] **Step 6: Refresh the help snapshot**

The main `--help` output doesn't change (`debug` is still hidden), but `cargo run -- help debug` does. The snapshot only locks the main help, so it should still pass. Verify:

Run: `cargo test --test help`
Expected: PASS without snapshot refresh.

If it FAILS due to drift (unlikely, but if Subcommand changes anything cosmetic): run `INSTA_UPDATE=always cargo test --test help`, then inspect `tests/snapshots/help__main_help.snap` and confirm it still does NOT contain `debug` in the Commands list.

- [ ] **Step 7: Re-verify the stub-verb test still passes**

Run: `cargo test --test stubs`
Expected: 6 passed. The `debug_requires_subcommand` test expects the message "debug requires a subcommand"; our new message is "debug requires a subcommand (try `muntjac debug print-deps`)" — `predicates::str::contains` matches substrings, so this still passes.

- [ ] **Step 8: Run full suite + clippy**

Run: `cargo test --locked && cargo clippy --all-targets --locked -- -D warnings`
Expected: all green.

- [ ] **Step 9: Commit**

```bash
git add src/cli/mod.rs src/cli/debug tests/snapshots
git commit -m "feat(s1): CLI Debug becomes a real Subcommand with print-deps slot"
```

---

## Task 14: `muntjac debug print-deps` implementation

**Files:**
- Modify: `src/cli/debug/print_deps.rs`

- [ ] **Step 1: Write the failing integration test (will become Task 15's framework)**

For Task 14, we'll TDD with a small in-process unit test in `print_deps.rs` that exercises the function with a synthetic config + lockfile.

Replace `src/cli/debug/print_deps.rs` with:

```rust
use std::fs;
use std::str::FromStr;

use anyhow::{Context, Result};

use crate::cli::Globals;
use crate::cli::debug::PrintDepsArgs;
use crate::config::Config;
use crate::lock;

pub fn run(args: PrintDepsArgs, _globals: &Globals) -> Result<()> {
    let cwd = std::env::current_dir().context("getting current dir")?;
    let cfg_path = cwd.join("muntjac.toml");
    let cfg_bytes = fs::read_to_string(&cfg_path)
        .with_context(|| format!("reading {}", cfg_path.display()))?;
    let config = Config::from_str(&cfg_bytes)
        .with_context(|| format!("parsing {}", cfg_path.display()))?;
    config.validate()?;

    let tree = pick_tree(&config, args.tree.as_deref())?;
    let manifest_dir = cfg_path
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join(tree.manifest_path.parent().unwrap_or(std::path::Path::new("")));
    let lockfile_path = manifest_dir.join("uv.lock");
    let lock_bytes = fs::read_to_string(&lockfile_path)
        .with_context(|| format!("reading {}", lockfile_path.display()))?;
    let lockfile = lock::parser::parse(&lock_bytes)
        .with_context(|| format!("parsing {}", lockfile_path.display()))?;

    let graph = lock::graph::build(&lockfile)?;
    lock::graph::detect_cycles(&graph)?;
    let view = lock::resolved::project(&graph, &config, tree);

    let json = if args.pretty {
        serde_json::to_string_pretty(&view)?
    } else {
        serde_json::to_string(&view)?
    };
    println!("{json}");
    Ok(())
}

fn pick_tree<'a>(config: &'a Config, requested: Option<&str>) -> Result<&'a crate::config::Tree> {
    match requested {
        Some(name) => config.trees.iter()
            .find(|t| t.name == name)
            .ok_or_else(|| anyhow::anyhow!("no tree named `{name}` in muntjac.toml")),
        None => {
            if config.trees.len() == 1 {
                Ok(&config.trees[0])
            } else {
                anyhow::bail!("multi-tree config: pass --tree <name> (available: {})",
                    config.trees.iter().map(|t| t.name.as_str()).collect::<Vec<_>>().join(", "))
            }
        }
    }
}
```

- [ ] **Step 2: Verify build**

Run: `cargo build`
Expected: builds. (No new tests in this task — Task 15 introduces them as fixture-driven integration tests.)

- [ ] **Step 3: Verify all existing tests pass**

Run: `cargo test --locked`
Expected: all S0 + S1-so-far tests pass.

- [ ] **Step 4: Run clippy**

Run: `cargo clippy --all-targets --locked -- -D warnings`
Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add src/cli/debug/print_deps.rs
git commit -m "feat(s1): muntjac debug print-deps reads uv.lock and projects ResolvedView"
```

---

## Task 15: Fixture 01-pure-python + integration test framework

**Files:**
- Create: `tests/fixtures/lock/01-pure-python/pyproject.toml`
- Create: `tests/fixtures/lock/01-pure-python/uv.lock`
- Create: `tests/fixtures/lock/01-pure-python/muntjac.toml`
- Create: `tests/fixtures/lock/01-pure-python/expected-print-deps.json`
- Create: `tests/print_deps.rs`
- Create: `tests/common/mod.rs` doesn't need changes (already from S0)

- [ ] **Step 1: Create the fixture**

```bash
mkdir -p tests/fixtures/lock/01-pure-python
cd tests/fixtures/lock/01-pure-python
uv init --no-readme --name pure-python-app --no-workspace -q
uv add 'requests>=2.30' --quiet
cd -
```

This generates `pyproject.toml` and `uv.lock`. Verify they look reasonable. Trim `pyproject.toml` to the minimum:

```toml
# tests/fixtures/lock/01-pure-python/pyproject.toml
[project]
name = "pure-python-app"
version = "0.1.0"
requires-python = ">=3.12"
dependencies = ["requests>=2.30"]
```

- [ ] **Step 2: Create `muntjac.toml`**

```toml
# tests/fixtures/lock/01-pure-python/muntjac.toml
manifest_path   = "pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target    = "x86_64-unknown-linux-gnu"
manylinux = "2_17"
```

- [ ] **Step 3: Create the integration test framework**

Create `tests/print_deps.rs`:

```rust
mod common;

use assert_cmd::prelude::*;
use common::muntjac;
use std::fs;
use std::path::Path;

fn run_print_deps(fixture: &str) -> (String, std::process::Output) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/lock")
        .join(fixture);
    let output = muntjac()
        .arg("-C")
        .arg(&dir)
        .args(["debug", "print-deps", "--pretty"])
        .output()
        .expect("run print-deps");
    let stdout = String::from_utf8(output.stdout.clone()).expect("utf-8");
    (stdout, output)
}

fn assert_golden(fixture: &str) {
    let (actual, output) = run_print_deps(fixture);
    assert!(output.status.success(),
        "print-deps failed for fixture {fixture}: stderr=\n{}",
        String::from_utf8_lossy(&output.stderr));
    let golden_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/lock")
        .join(fixture)
        .join("expected-print-deps.json");
    let expected = fs::read_to_string(&golden_path)
        .expect(&format!("read golden {}", golden_path.display()));
    // Tolerate trailing newline differences:
    assert_eq!(actual.trim_end(), expected.trim_end(),
        "mismatch for {fixture}\n--- actual ---\n{actual}\n--- expected ---\n{expected}");
}

#[test]
fn fixture_01_pure_python() {
    assert_golden("01-pure-python");
}
```

- [ ] **Step 4: Generate the golden file**

Run muntjac on the fixture once, capture output, save as golden:

```bash
cargo run -- -C tests/fixtures/lock/01-pure-python debug print-deps --pretty \
    > tests/fixtures/lock/01-pure-python/expected-print-deps.json
```

Inspect the file by hand. Verify:
- `"configs"` array has 1 entry (1 platform × 1 python)
- The entry's `platform` is `"linux-x86_64-gnu"`, `python_version` is `"3.12"`
- `packages` includes `pure-python-app` (kind=first-party) plus `requests` and its transitive deps (certifi, charset-normalizer, idna, urllib3)
- Each non-first-party package has `kind: "registry"` and a `registry: "..."` field

If anything looks wrong, debug before locking it in.

- [ ] **Step 5: Run the integration test**

Run: `cargo test --test print_deps`
Expected: 1 PASS.

- [ ] **Step 6: Clippy + full suite**

Run: `cargo test --locked && cargo clippy --all-targets --locked -- -D warnings`
Expected: all green.

- [ ] **Step 7: Commit**

```bash
git add tests/fixtures/lock/01-pure-python tests/print_deps.rs
git commit -m "test(s1): fixture 01-pure-python and integration test framework"
```

---

## Task 16: Fixtures 02-05, 08 (resolved-output scenarios)

**Files:**
- Create: `tests/fixtures/lock/02-env-markers/{pyproject.toml, uv.lock, muntjac.toml, expected-print-deps.json}`
- Create: `tests/fixtures/lock/03-workspace/...`
- Create: `tests/fixtures/lock/04-extras/...`
- Create: `tests/fixtures/lock/05-dev-deps/...`
- Create: `tests/fixtures/lock/08-multi-platform-marker/...`
- Modify: `tests/print_deps.rs` (add 5 tests)

For each fixture follow the same pattern: `uv init` + `uv add` to create a real uv.lock, write a minimal `muntjac.toml`, run `cargo run -- ... debug print-deps --pretty > expected-print-deps.json`, inspect, commit.

- [ ] **Step 1: Fixture 02-env-markers**

```bash
mkdir -p tests/fixtures/lock/02-env-markers
cd tests/fixtures/lock/02-env-markers
uv init --no-readme --name env-marker-app --no-workspace -q
uv add 'typing-extensions; python_version < "3.13"' --quiet
cd -
```

`muntjac.toml`:

```toml
manifest_path   = "pyproject.toml"
third_party_dir = "."
python_versions = ["3.12", "3.13"]

[platforms.linux-x86_64-gnu]
target = "x86_64-unknown-linux-gnu"
manylinux = "2_17"
```

Note: requires-python in the auto-generated pyproject.toml may default to `>=3.13`. Edit it to `requires-python = ">=3.12"` so the lockfile resolves for 3.12+.

After editing `pyproject.toml`, regenerate uv.lock:
```bash
cd tests/fixtures/lock/02-env-markers && uv lock --quiet && cd -
```

Generate golden, inspect (expect 2 configs; the 3.12 config has typing-extensions, the 3.13 config does not).

- [ ] **Step 2: Fixture 03-workspace**

```bash
mkdir -p tests/fixtures/lock/03-workspace/{member-a,member-b}
cd tests/fixtures/lock/03-workspace
```

Write a root `pyproject.toml`:
```toml
[project]
name = "workspace-root"
version = "0.1.0"
requires-python = ">=3.12"

[tool.uv.workspace]
members = ["member-a", "member-b"]
```

`member-a/pyproject.toml`:
```toml
[project]
name = "member-a"
version = "0.1.0"
requires-python = ">=3.12"
dependencies = ["requests>=2.30"]
```

`member-b/pyproject.toml`:
```toml
[project]
name = "member-b"
version = "0.1.0"
requires-python = ">=3.12"
dependencies = ["packaging>=23"]
```

Then `uv lock --quiet` from the root.

`muntjac.toml`:
```toml
manifest_path   = "pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target = "x86_64-unknown-linux-gnu"
manylinux = "2_17"
```

Generate golden. Expect both `member-a` and `member-b` as first-party, plus their union of third-party deps.

- [ ] **Step 3: Fixture 04-extras**

```bash
mkdir -p tests/fixtures/lock/04-extras
cd tests/fixtures/lock/04-extras
uv init --no-readme --name extras-app --no-workspace -q
# Choose a small package that has extras. `httpx[http2]` pulls in h2.
uv add 'httpx[http2]' --quiet
cd -
```

`muntjac.toml`: same minimal shape.

Generate golden. Expect httpx + h2 + hpack/hyperframe in the resolved set.

- [ ] **Step 4: Fixture 05-dev-deps**

```bash
mkdir -p tests/fixtures/lock/05-dev-deps
cd tests/fixtures/lock/05-dev-deps
uv init --no-readme --name dev-deps-app --no-workspace -q
uv add 'requests' --quiet
uv add --group test 'pytest' --quiet
cd -
```

`muntjac.toml`:
```toml
manifest_path   = "pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target = "x86_64-unknown-linux-gnu"
manylinux = "2_17"

[lockfile]
include_groups = ["test"]
```

Generate golden. Expect both `requests` AND `pytest` (with its transitive deps) in the resolved set. Without `include_groups`, pytest would be excluded.

- [ ] **Step 5: Fixture 08-multi-platform-marker**

```bash
mkdir -p tests/fixtures/lock/08-multi-platform-marker
cd tests/fixtures/lock/08-multi-platform-marker
uv init --no-readme --name macos-only-app --no-workspace -q
uv add 'pyobjc-core; sys_platform == "darwin"' --quiet
cd -
```

`muntjac.toml`: include both linux and macos platforms.

```toml
manifest_path   = "pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target = "x86_64-unknown-linux-gnu"
manylinux = "2_17"

[platforms.macos-arm64]
target    = "aarch64-apple-darwin"
macos_min = "11.0"
```

Generate golden. Expect 2 configs; macos one has pyobjc-core, linux one doesn't.

- [ ] **Step 6: Add tests to `tests/print_deps.rs`**

Append after `fixture_01_pure_python`:

```rust
#[test] fn fixture_02_env_markers()           { assert_golden("02-env-markers"); }
#[test] fn fixture_03_workspace()             { assert_golden("03-workspace"); }
#[test] fn fixture_04_extras()                { assert_golden("04-extras"); }
#[test] fn fixture_05_dev_deps()              { assert_golden("05-dev-deps"); }
#[test] fn fixture_08_multi_platform_marker() { assert_golden("08-multi-platform-marker"); }
```

- [ ] **Step 7: Run integration tests**

Run: `cargo test --test print_deps`
Expected: 6 PASS.

If any fails: inspect the actual vs expected diff. If the actual is correct (the golden was wrong), regenerate the golden. If the actual is wrong, debug in the relevant module.

- [ ] **Step 8: Clippy + full suite**

Run: `cargo test --locked && cargo clippy --all-targets --locked -- -D warnings`
Expected: all green.

- [ ] **Step 9: Commit**

```bash
git add tests/fixtures/lock/02-env-markers tests/fixtures/lock/03-workspace tests/fixtures/lock/04-extras tests/fixtures/lock/05-dev-deps tests/fixtures/lock/08-multi-platform-marker tests/print_deps.rs
git commit -m "test(s1): fixtures 02-05 + 08 covering markers, workspace, extras, dev-deps, multi-platform"
```

---

## Task 17: Fixtures 06 + 07 (error scenarios)

**Files:**
- Create: `tests/fixtures/lock/06-cycle-error/{uv.lock, muntjac.toml, expected-error.txt}`
- Create: `tests/fixtures/lock/07-unresolved-dep-error/{uv.lock, muntjac.toml, expected-error.txt}`
- Modify: `tests/print_deps.rs`

The error fixtures use hand-written `uv.lock` files (uv won't produce these).

- [ ] **Step 1: Fixture 06-cycle-error**

Create `tests/fixtures/lock/06-cycle-error/uv.lock`:

```toml
version = 1
revision = 3
requires-python = ">=3.12"

[[package]]
name = "app"
version = "0.1.0"
source = { virtual = "." }
dependencies = [
    { name = "alpha" },
]

[[package]]
name = "alpha"
version = "1.0"
source = { registry = "https://pypi.org/simple" }
dependencies = [
    { name = "beta" },
]

[[package]]
name = "beta"
version = "1.0"
source = { registry = "https://pypi.org/simple" }
dependencies = [
    { name = "alpha" },
]
```

`tests/fixtures/lock/06-cycle-error/muntjac.toml`:

```toml
manifest_path   = "pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target = "x86_64-unknown-linux-gnu"
manylinux = "2_17"
```

Also create an empty `pyproject.toml` so `config check`'s path-existence check passes:

```toml
[project]
name = "cycle-test"
version = "0.1.0"
requires-python = ">=3.12"
```

`tests/fixtures/lock/06-cycle-error/expected-error.txt`:

```
dependency cycle(s) detected
```

(Substring; the integration test will use `contains`.)

- [ ] **Step 2: Fixture 07-unresolved-dep-error**

`tests/fixtures/lock/07-unresolved-dep-error/uv.lock`:

```toml
version = 1
revision = 3
requires-python = ">=3.12"

[[package]]
name = "app"
version = "0.1.0"
source = { virtual = "." }
dependencies = [
    { name = "ghost" },
]
```

`muntjac.toml` (same as above) and `pyproject.toml` (same).

`expected-error.txt`:

```
dep `ghost` has no matching [[package]] entry
```

- [ ] **Step 3: Add error-checking tests to `tests/print_deps.rs`**

Append after the other fixture tests:

```rust
fn assert_error(fixture: &str) {
    let (stdout, output) = run_print_deps(fixture);
    assert!(!output.status.success(),
        "fixture {fixture}: expected failure but command succeeded; stdout=\n{stdout}");
    let stderr = String::from_utf8(output.stderr).expect("utf-8");
    let expected_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/lock")
        .join(fixture)
        .join("expected-error.txt");
    let expected = fs::read_to_string(&expected_path).expect("read expected-error.txt");
    let expected = expected.trim();
    assert!(stderr.contains(expected),
        "fixture {fixture}: stderr does not contain expected substring\n--- stderr ---\n{stderr}\n--- expected substring ---\n{expected}");
}

#[test] fn fixture_06_cycle_error()          { assert_error("06-cycle-error"); }
#[test] fn fixture_07_unresolved_dep_error() { assert_error("07-unresolved-dep-error"); }
```

- [ ] **Step 4: Run tests**

Run: `cargo test --test print_deps`
Expected: 8 PASS (6 from Tasks 15-16 + 2 new).

- [ ] **Step 5: Clippy + full suite**

Run: `cargo test --locked && cargo clippy --all-targets --locked -- -D warnings`
Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add tests/fixtures/lock/06-cycle-error tests/fixtures/lock/07-unresolved-dep-error tests/print_deps.rs
git commit -m "test(s1): fixtures 06-cycle-error and 07-unresolved-dep-error"
```

---

## Task 18: Fixture 09 (determinism)

**Files:**
- Modify: `tests/print_deps.rs`

The 09 fixture doesn't need its own data — it reuses 01-pure-python and asserts byte-equality across two invocations.

- [ ] **Step 1: Add determinism test**

Append to `tests/print_deps.rs`:

```rust
#[test]
fn fixture_09_runs_twice_identically() {
    let (first, _) = run_print_deps("01-pure-python");
    let (second, _) = run_print_deps("01-pure-python");
    assert_eq!(first, second, "print-deps is not deterministic across invocations");
}
```

- [ ] **Step 2: Run**

Run: `cargo test --test print_deps fixture_09_runs_twice_identically`
Expected: PASS.

- [ ] **Step 3: Run full suite**

Run: `cargo test --locked && cargo clippy --all-targets --locked -- -D warnings`
Expected: all green.

- [ ] **Step 4: Commit**

```bash
git add tests/print_deps.rs
git commit -m "test(s1): fixture 09 asserts print-deps is deterministic across invocations"
```

---

## Task 19: `muntjac init` template hint + `muntjac config check` already done in Task 12

The `config check` validation for `include_groups` shipped in Task 12. This task only updates the `init` template to mention `[lockfile]` as a discoverable optional section.

**Files:**
- Modify: `src/cli/init.rs`
- Modify: `tests/init.rs` (extend the existing test to assert the new comment is present)

- [ ] **Step 1: Update the test expectation**

In `tests/init.rs`, modify `init_creates_starter_in_empty_dir`:

Find the assertion block (after `let cfg = fs::read_to_string(...).unwrap();`) and add:

```rust
assert!(cfg.contains("# Uncomment to include PEP 735 dependency groups"));
assert!(cfg.contains("# [lockfile]"));
```

- [ ] **Step 2: Confirm test fails**

Run: `cargo test --test init init_creates_starter_in_empty_dir`
Expected: FAIL — the new strings aren't in the template yet.

- [ ] **Step 3: Update `render_starter_config` in `src/cli/init.rs`**

Find the `format!(...)` call inside `render_starter_config`. Locate the `[fixups]` block and add the `[lockfile]` hint just before it.

Replace this slice:

```rust
         [fixups]\n\
         # Community fixup registry. Leave as \"none\" until a v0.1.0+ release exists.\n\
```

with:

```rust
         # Uncomment to include PEP 735 dependency groups in the resolved graph.\n\
         # [lockfile]\n\
         # include_groups = [\"test\"]\n\n\
         [fixups]\n\
         # Community fixup registry. Leave as \"none\" until a v0.1.0+ release exists.\n\
```

- [ ] **Step 4: Run the test**

Run: `cargo test --test init`
Expected: 4 passed.

- [ ] **Step 5: Run full suite + clippy + fmt**

Run: `cargo fmt --all && cargo test --locked && cargo clippy --all-targets --locked -- -D warnings`
Expected: all green.

- [ ] **Step 6: Commit**

```bash
git add src/cli/init.rs tests/init.rs
git commit -m "feat(s1): muntjac init template hints at [lockfile] include_groups"
```

---

## Task 20: Final verification + s1-complete tag

**Files:**
- No new files. Verification + tag only.

- [ ] **Step 1: Full local CI equivalent**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo build --locked
cargo test --locked
```

All four must be green. If `cargo fmt --check` flags drift, run `cargo fmt --all` and recommit as a `style(s1):` commit before tagging.

- [ ] **Step 2: Verify test count**

`cargo test --locked` should now show, in total:
- 15+ lib unit tests (S0's 15 + S1's ~12 new in config/error/lock::types/lock::parser/lock::graph/lock::resolved/platform)
- 3 config_check integration
- 4 init integration
- 6 stubs integration
- 1 help snapshot
- 9 print_deps integration (01-08 + the determinism one)

≈ 50 tests total. Exact count may vary; the key is **no failures**.

- [ ] **Step 3: Smoke check the workflow manually**

```bash
TMP=$(mktemp -d)
cd "$TMP"
uv init --no-readme --name smoke-app --no-workspace -q
uv add 'requests' --quiet
cargo run --manifest-path /home/jackm/repos/muntjac/Cargo.toml -- init
cargo run --manifest-path /home/jackm/repos/muntjac/Cargo.toml -- debug print-deps --pretty | head -40
cd - && rm -rf "$TMP"
```

Expected: pretty-printed JSON showing the resolved graph for the smoke-app + transitive requests deps.

- [ ] **Step 4: Tag the stage**

```bash
git tag s1-complete
git log --oneline s0-complete..s1-complete | head -25
```

- [ ] **Step 5: Update the roadmap**

Edit `docs/superpowers/specs/2026-05-20-muntjac-roadmap.md`. Change the S1 row from:

```
| S1 | [2026-05-20-muntjac-s1-lockfile-design.md](./2026-05-20-muntjac-s1-lockfile-design.md) | (not yet written) | 🟡 spec drafted |
```

to:

```
| S1 | [2026-05-20-muntjac-s1-lockfile-design.md](./2026-05-20-muntjac-s1-lockfile-design.md) | [2026-05-20-muntjac-s1-lockfile.md](../plans/2026-05-20-muntjac-s1-lockfile.md) | ✅ shipped (tag `s1-complete`) |
```

Also bump S2's status from `⬜ ready (parallelizable with S1)` to `⬜ next`.

- [ ] **Step 6: Commit roadmap update**

```bash
git add docs/superpowers/specs/2026-05-20-muntjac-roadmap.md
git commit -m "docs: mark S1 shipped in roadmap"
```

---

## Exit criteria checklist (per spec §12)

Run through these manually after Task 20. Each must be true:

- [ ] `muntjac debug print-deps` reads `uv.lock` + `muntjac.toml`, prints sorted deterministic JSON. `--pretty` controls formatting.
- [ ] All 9 fixture tests pass (`01-pure-python` through `08-multi-platform-marker` plus `09` determinism, `06` and `07` error scenarios).
- [ ] Unit tests cover parser shape validation, graph construction, marker evaluation, projection determinism.
- [ ] `Config` gained `lockfile: LockfileConfig`; S0-shaped `muntjac.toml` still parses.
- [ ] `muntjac config check` rejects invalid group names.
- [ ] All S0 tests still pass.
- [ ] `--help` snapshot unchanged (debug is hidden).
- [ ] `cargo build/clippy/fmt/test --locked` all green.

If all pass: S1 done.

---

## Self-review notes

- **Spec coverage**:
  - §2 crate picks → Task 1
  - §3 type model → Tasks 3
  - §4 parser → Tasks 4-6
  - §5 marker env → Task 7
  - §6 graph + cycle → Tasks 8-9
  - §6 reachability → Task 10
  - §7 projection → Task 11
  - §8 print-deps CLI → Tasks 13-14
  - §9 LockfileConfig + init hint + config check group validation → Tasks 12 + 19
  - §10 module layout → all tasks
  - §11 testing (unit + 9 fixtures + determinism) → Tasks 8-11 (unit), 15-18 (fixtures)
  - §12 exit criteria → Task 20 checklist

- **Type consistency**: `Package`, `Source`, `DepEdge`, `Wheel`, `Sdist`, `Metadata`, `Lockfile`, `NodeId`, `DepGraph`, `GraphNode`, `ResolvedView`, `ResolvedConfig`, `ResolvedPackage`, `ResolvedKind`, `SourceInfo`, `LockfileConfig`, `LockfileError`, `DebugOp`, `PrintDepsArgs` — all defined exactly once, with consistent names across tasks.

- **No placeholders**: every step has runnable code or commands.

- **Known judgment calls** (not placeholders; deliberate spec deviations or runtime adaptations):
  - `pep508_rs::MarkerEnvironmentBuilder` exact field names may differ slightly from what's shown in Task 7; implementer adapts via `cargo doc`.
  - `evaluate_extras` API in `pep508_rs` may have a different signature in the installed version; Task 10 includes a fallback note.
  - `requires_python` stored as `String` (not `pep508_rs::VersionSpecifiers`) — spec said `VersionSpecifiers` but S1 doesn't consume it, so YAGNI. Promoted in a later stage when first consumer appears.
