# Muntjac S6 — Local Fixups Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Spec:** `docs/superpowers/specs/2026-05-23-muntjac-s6-local-fixups-design.md`

**Goal:** Ship local per-package fixups — per-repo TOML files at `<third_party_dir>/fixups/<pkg>/fixups.toml` that override or augment what `muntjac buckify` emits — including `extra_deps`/`omit_deps`/`replace_deps`, `prefer_wheel`/`exclude_wheels`, wheel `overlay`, explicit `entry_points` list, `visibility`/`labels`/`runtime_env`, and the `cfg(...)` predicate grammar.

**Architecture:** New `src/fixup/` module (schema, cfg parser/evaluator, loader, merge, errors). Loader runs in `cli/buckify.rs` after config + lockfile parse and threads `Option<&FixupSet>` into `build_emit_input`, mirroring S5's manifest pattern. Resolution evaluates `cfg(...)` predicates per `(package, cell)` and feeds `EmitPackage` augmentations (overlay file list, entry-point names, visibility, labels, runtime_env). `pypi_package` macro grows two new branches: an `unzip → cp → zip` genrule for overlay, and `python_binary` rule generation for entry points.

**Tech Stack:** Rust 2024 edition, `serde`/`toml` (schema), `pep440_rs` (version specifiers in cfg predicates), `pep508_rs` (PEP 503 package-name normalization), `glob` (wheel exclude patterns — NEW dep), `walkdir` (overlay tree walk), `thiserror`/`anyhow` (errors), `insta` (snapshot tests), `httpmock` (integration smokes).

---

## File Structure

**New files:**

| Path | Responsibility |
|---|---|
| `src/fixup/mod.rs` | Module facade — re-exports `FixupSet`, `FixupConfig`, `FixupBody`, `ResolvedFixup`, `CfgContext`, `FixupError` |
| `src/fixup/error.rs` | `FixupError` typed enum (thiserror) |
| `src/fixup/schema.rs` | `FixupConfig`, `FixupBody`, `EntryPoints`, `SdistFixup` — serde-derived, `deny_unknown_fields`, cfg sections as `BTreeMap<String, FixupBody>` raw, then post-processed into `Vec<(CfgPredicate, FixupBody)>` |
| `src/fixup/cfg.rs` | `CfgPredicate` AST, hand-rolled parser, evaluator, `CfgContext` |
| `src/fixup/layer.rs` | `merge_into`, `ResolvedFixup`, `resolve_for_cell` |
| `src/fixup/loader.rs` | `load_local(third_party_dir) -> Result<FixupSet, FixupError>` |
| `src/cli/fixups.rs` | `muntjac fixups show <pkg>` |
| `tests/fixtures/buck/05-local-fixup/` | fixture: synthetic `fake-pillow` with hand-rolled wheels, fixups.toml, overlay/, expected snapshots |
| `tests/fixups_smoke.rs` | End-to-end smoke (buckify + assert snapshot match + fixups show round-trip) |

**Modified files:**

| Path | Change |
|---|---|
| `src/lib.rs:1` | add `pub mod fixup;` |
| `src/cli/mod.rs` | grow `Fixups` variant from no-arg stub to `Fixups { op: FixupsOp }` with `Show { package: String }` subcommand |
| `src/cli/buckify.rs` | load `FixupSet` before `build_emit_input`; pass as 5th arg |
| `src/buck/emit.rs` | `build_emit_input` signature gains `fixups: Option<&FixupSet>`; `EmitPackage` gains `overlay`, `entry_points`, `visibility`, `labels`, `runtime_env` |
| `src/buck/string_writer.rs` | `pypi_package` macro: overlay genrule branch, python_binary branch, per-package visibility/labels threading |
| `Cargo.toml` | add `glob = "0.3"` to `[dependencies]` |
| `.github/workflows/ci.yml` | add ubuntu-latest step running `buck2 build` against the 05-local-fixup fixture |
| `docs/superpowers/TECH_DEBT.md` | log 3 follow-up entries (PEP 427 RECORD regen, entry_points=true, non-__main__ entries) |
| `docs/superpowers/specs/2026-05-20-muntjac-roadmap.md` | mark S6 ✅ shipped |

---

## Phase 1 — Foundation

### Task 1: Scaffold `src/fixup/` module + register in lib

**Files:**
- Create: `src/fixup/mod.rs`
- Modify: `src/lib.rs:1`

- [ ] **Step 1: Write the failing test**

Add to `src/fixup/mod.rs` (this file doesn't exist yet — `cargo test` will fail at compile because `fixup` isn't a module).

```rust
//! Local & community fixup configuration parsing, layering, and resolution.

pub mod cfg;
pub mod error;
pub mod layer;
pub mod loader;
pub mod schema;

pub use cfg::{CfgContext, CfgPredicate};
pub use error::FixupError;
pub use layer::{ResolvedFixup, resolve_for_cell};
pub use loader::{FixupSet, load_local};
pub use schema::{EntryPoints, FixupBody, FixupConfig, SdistFixup};

#[cfg(test)]
mod smoke_tests {
    #[test]
    fn module_compiles() {
        // Pure compile-time gate: ensures every re-exported item resolves.
    }
}
```

- [ ] **Step 2: Add `pub mod fixup;` to `src/lib.rs`**

```rust
pub mod buck;
pub mod cli;
pub mod config;
pub mod error;
pub mod fixup;        // <-- ADD this line, alphabetically placed
pub mod lock;
pub mod platform;
pub mod sdist;
pub mod uv;
pub mod wheel;
```

- [ ] **Step 3: Create empty submodule stubs so the `pub mod ...` directives in `fixup/mod.rs` compile**

Create `src/fixup/cfg.rs`, `src/fixup/error.rs`, `src/fixup/layer.rs`, `src/fixup/loader.rs`, `src/fixup/schema.rs` — each containing only:

```rust
//! placeholder for S6 Task <N>; replaced in subsequent task
```

For each file, also declare the items re-exported by `mod.rs` as empty/uninhabited stubs so the re-exports resolve:

In `src/fixup/error.rs`:
```rust
#[derive(Debug, thiserror::Error)]
pub enum FixupError {}
```

In `src/fixup/schema.rs`:
```rust
#[derive(Debug, Default)]
pub struct FixupBody;
#[derive(Debug, Default)]
pub struct FixupConfig;
#[derive(Debug)]
pub enum EntryPoints { Auto(bool), Named(Vec<String>) }
#[derive(Debug)]
pub struct SdistFixup;
```

In `src/fixup/cfg.rs`:
```rust
#[derive(Debug)]
pub struct CfgPredicate;
pub struct CfgContext<'a> { _marker: std::marker::PhantomData<&'a ()> }
```

In `src/fixup/layer.rs`:
```rust
#[derive(Debug, Default)]
pub struct ResolvedFixup;
pub fn resolve_for_cell() {}
```

In `src/fixup/loader.rs`:
```rust
#[derive(Debug, Default)]
pub struct FixupSet;
pub fn load_local() {}
```

- [ ] **Step 4: Run the smoke test to verify the module compiles**

```bash
cargo test --lib fixup::smoke_tests::module_compiles
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/lib.rs src/fixup/
git commit -m "feat(s6): scaffold src/fixup/ module"
```

---

### Task 2: `FixupError` typed enum

**Files:**
- Modify: `src/fixup/error.rs`

- [ ] **Step 1: Write the failing test in `src/fixup/error.rs`**

Replace the placeholder enum with the full type + a test that asserts the canonical messages from spec §6 byte-for-byte (a subset of variants; the rest are covered indirectly via integration tests).

```rust
use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum FixupError {
    #[error("failed to parse fixup at {file}:\n  {source}")]
    ParseError {
        file: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("unknown field `{field}` in fixup at {file}\n  v1 schema: see docs/superpowers/specs/2026-05-20-muntjac-design.md §7")]
    UnknownField { file: PathBuf, field: String },

    #[error("failed to parse cfg() in {file} section `{section}`:\n  {source}")]
    CfgParse {
        file: PathBuf,
        section: String,
        #[source]
        source: CfgParseError,
    },

    #[error("entry_points = true is not supported in v1; list the binaries explicitly (e.g. entry_points = [\"ruff\"]).\n  package: {pkg}")]
    EntryPointsAuto { pkg: String },

    #[error("prefer_wheel sha256:{sha} not found for {pkg} on cell {cell}.\n  available wheel shas: {available}")]
    PreferWheelNotFound {
        pkg: String,
        sha: String,
        cell: String,
        available: String,
    },

    #[error("exclude_wheels eliminates every wheel for {pkg} on cell {cell}.\n  loosen the patterns or remove the fixup")]
    ExcludeWheelsLeavesNone { pkg: String, cell: String },

    #[error("overlay/ contains a symlink to outside the fixup directory: {file}\n  refusing for safety")]
    OverlayPathOutsideTree { file: PathBuf },

    #[error("overlay = \"{path}\" is set for {pkg} but the directory is empty or missing")]
    OverlayEmpty { pkg: String, path: String },

    #[error("replace_deps target for {pkg} is not a valid Buck target: `{target}`\n  expected //path:name or :name form")]
    ReplaceDepInvalid { pkg: String, target: String },

    #[error("extra_deps target for {pkg} is not a valid Buck target: `{target}`\n  expected //path:name or :name form")]
    ExtraDepInvalid { pkg: String, target: String },

    #[error("unknown cfg atom `{atom}`; expected one of: version, python, target_os, target_arch, target_env")]
    BadCfgAtom { atom: String },

    #[error("I/O error reading fixups directory {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Errors from `cfg.rs` parser; wrapped by `FixupError::CfgParse`.
#[derive(Debug, thiserror::Error)]
pub enum CfgParseError {
    #[error("at byte {offset}: {message}")]
    AtOffset { offset: usize, message: String },
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn unknown_field_message_is_exact() {
        let e = FixupError::UnknownField {
            file: PathBuf::from("fixups/pillow/fixups.toml"),
            field: "extra_things".into(),
        };
        assert_eq!(
            e.to_string(),
            "unknown field `extra_things` in fixup at fixups/pillow/fixups.toml\n  v1 schema: see docs/superpowers/specs/2026-05-20-muntjac-design.md §7"
        );
    }

    #[test]
    fn entry_points_auto_message_is_exact() {
        let e = FixupError::EntryPointsAuto { pkg: "pillow".into() };
        assert_eq!(
            e.to_string(),
            "entry_points = true is not supported in v1; list the binaries explicitly (e.g. entry_points = [\"ruff\"]).\n  package: pillow"
        );
    }

    #[test]
    fn overlay_empty_message_is_exact() {
        let e = FixupError::OverlayEmpty {
            pkg: "pillow".into(),
            path: "overlay/".into(),
        };
        assert_eq!(
            e.to_string(),
            "overlay = \"overlay/\" is set for pillow but the directory is empty or missing"
        );
    }

    #[test]
    fn bad_cfg_atom_message_is_exact() {
        let e = FixupError::BadCfgAtom { atom: "target_family".into() };
        assert_eq!(
            e.to_string(),
            "unknown cfg atom `target_family`; expected one of: version, python, target_os, target_arch, target_env"
        );
    }
}
```

- [ ] **Step 2: Update `src/fixup/mod.rs` to also re-export `CfgParseError`**

```rust
pub use error::{CfgParseError, FixupError};
```

- [ ] **Step 3: Run the error tests**

```bash
cargo test --lib fixup::error
```

Expected: PASS (4 tests).

- [ ] **Step 4: Commit**

```bash
git add src/fixup/error.rs src/fixup/mod.rs
git commit -m "feat(s6): FixupError + CfgParseError typed enums"
```

---

## Phase 2 — Schema

### Task 3: `FixupBody` + `FixupConfig` schema (serde)

**Files:**
- Modify: `src/fixup/schema.rs`

- [ ] **Step 1: Write the failing test**

Replace the placeholder in `src/fixup/schema.rs`:

```rust
//! Fixup TOML schema. `deny_unknown_fields` everywhere — community
//! fixup authors must use the v1 schema exactly; typos are errors.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// One package's fixup, as loaded from `fixups/<pkg>/fixups.toml`.
///
/// `top` is the body of the top-level fields; `cfg_sections` is the
/// ordered list of `['cfg(<expr>)']` sections, each paired with its
/// raw predicate string (parsed lazily by `cfg.rs`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FixupConfig {
    pub top: FixupBody,
    /// Raw `(predicate_string, body)` pairs in source order.
    pub cfg_sections: Vec<(String, FixupBody)>,
}

/// The body of either a top-level fixup or a single `cfg(...)` section.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct FixupBody {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub extra_deps: Vec<String>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub omit_deps: Vec<String>,

    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub replace_deps: BTreeMap<String, String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefer_wheel: Option<String>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub exclude_wheels: Vec<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub overlay: Option<PathBuf>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry_points: Option<EntryPoints>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,

    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub runtime_env: BTreeMap<String, String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub sdist: Option<SdistFixup>,

    #[serde(skip_serializing_if = "is_false")]
    pub replace_community: bool,
}

fn is_false(b: &bool) -> bool { !b }

/// `entry_points = true | ["name1", "name2"]`. v1: only the list form
/// can be applied; the `true` shorthand parses but errors at apply.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum EntryPoints {
    Auto(bool),
    Named(Vec<String>),
}

/// v2 sdist-build surface. Schema-only in S6 — parsed but not applied.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields, default)]
pub struct SdistFixup {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub build_env: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub build_deps: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub extra_native_libs: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub data_files: Vec<String>,
}

impl FixupConfig {
    /// Parse a fixup TOML document into top-level body + cfg sections.
    ///
    /// `[cfg(...)]` sections are read by collecting top-level table
    /// keys that begin with `cfg(` (the TOML section header IS the key
    /// at the top level). The remaining keys flatten into `FixupBody`.
    pub fn from_toml_str(s: &str) -> Result<Self, toml::de::Error> {
        // toml -> typed shim that captures every key.
        let raw: toml::Table = toml::from_str(s)?;
        let mut top_table = toml::Table::new();
        let mut cfg_sections: Vec<(String, FixupBody)> = Vec::new();

        for (key, value) in raw {
            if let Some(predicate) = key.strip_prefix("cfg(").and_then(|t| t.strip_suffix(')')) {
                let body: FixupBody = value.try_into()?;
                cfg_sections.push((predicate.to_string(), body));
            } else {
                top_table.insert(key, value);
            }
        }

        let top: FixupBody = toml::Value::Table(top_table).try_into()?;
        Ok(FixupConfig { top, cfg_sections })
    }

    /// Re-emit as canonical TOML. Round-trippable up to formatting
    /// (key ordering, default omission).
    pub fn to_toml_string(&self) -> Result<String, toml::ser::Error> {
        // Use serde's flattening: emit top fields, then each cfg section.
        let mut out = toml::to_string_pretty(&self.top)?;
        for (predicate, body) in &self.cfg_sections {
            // Skip empty bodies (would emit just a header).
            let body_str = toml::to_string_pretty(body)?;
            if body_str.trim().is_empty() {
                continue;
            }
            out.push_str(&format!("\n[\"cfg({})\"]\n", predicate));
            out.push_str(&body_str);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_fixup() {
        let toml = r#"
            extra_deps = ["//third-party/c:libjpeg"]
            omit_deps = ["useless"]
        "#;
        let cfg = FixupConfig::from_toml_str(toml).unwrap();
        assert_eq!(cfg.top.extra_deps, vec!["//third-party/c:libjpeg"]);
        assert_eq!(cfg.top.omit_deps, vec!["useless"]);
        assert!(cfg.cfg_sections.is_empty());
    }

    #[test]
    fn parses_with_cfg_section() {
        let toml = r#"
            extra_deps = ["//a:b"]

            ["cfg(target_os = \"linux\")"]
            extra_deps = ["//c:d"]
        "#;
        let cfg = FixupConfig::from_toml_str(toml).unwrap();
        assert_eq!(cfg.cfg_sections.len(), 1);
        assert_eq!(cfg.cfg_sections[0].0, "target_os = \"linux\"");
        assert_eq!(cfg.cfg_sections[0].1.extra_deps, vec!["//c:d"]);
    }

    #[test]
    fn rejects_unknown_top_level_field() {
        let toml = r#"
            extra_things = ["x"]
        "#;
        let err = FixupConfig::from_toml_str(toml).unwrap_err();
        assert!(err.to_string().contains("extra_things"), "got: {}", err);
    }

    #[test]
    fn parses_entry_points_list() {
        let toml = r#"entry_points = ["ruff", "ruff-lsp"]"#;
        let cfg = FixupConfig::from_toml_str(toml).unwrap();
        match cfg.top.entry_points {
            Some(EntryPoints::Named(v)) => assert_eq!(v, vec!["ruff", "ruff-lsp"]),
            other => panic!("expected Named, got {:?}", other),
        }
    }

    #[test]
    fn parses_entry_points_true() {
        let toml = r#"entry_points = true"#;
        let cfg = FixupConfig::from_toml_str(toml).unwrap();
        match cfg.top.entry_points {
            Some(EntryPoints::Auto(true)) => {}
            other => panic!("expected Auto(true), got {:?}", other),
        }
    }

    #[test]
    fn parses_replace_deps() {
        let toml = r#"replace_deps = { numpy = "//company/numpy:numpy" }"#;
        let cfg = FixupConfig::from_toml_str(toml).unwrap();
        assert_eq!(
            cfg.top.replace_deps.get("numpy").map(|s| s.as_str()),
            Some("//company/numpy:numpy")
        );
    }

    #[test]
    fn parses_replace_community_default_false() {
        let toml = r#"extra_deps = ["//x:y"]"#;
        let cfg = FixupConfig::from_toml_str(toml).unwrap();
        assert!(!cfg.top.replace_community);
    }

    #[test]
    fn parses_replace_community_true() {
        let toml = r#"replace_community = true"#;
        let cfg = FixupConfig::from_toml_str(toml).unwrap();
        assert!(cfg.top.replace_community);
    }

    #[test]
    fn parses_sdist_block() {
        let toml = r#"
            [sdist]
            backend = "maturin"
            build_env = { FOO = "bar" }
        "#;
        let cfg = FixupConfig::from_toml_str(toml).unwrap();
        let sdist = cfg.top.sdist.unwrap();
        assert_eq!(sdist.backend.as_deref(), Some("maturin"));
        assert_eq!(sdist.build_env.get("FOO").map(|s| s.as_str()), Some("bar"));
    }

    #[test]
    fn round_trip_preserves_user_set_values() {
        let toml = r#"
            extra_deps = ["//a:b"]
            labels = ["x"]
            entry_points = ["bin"]
        "#;
        let cfg = FixupConfig::from_toml_str(toml).unwrap();
        let out = cfg.to_toml_string().unwrap();
        let cfg2 = FixupConfig::from_toml_str(&out).unwrap();
        assert_eq!(cfg, cfg2);
    }
}
```

- [ ] **Step 2: Run the schema tests**

```bash
cargo test --lib fixup::schema
```

Expected: 9 PASS.

- [ ] **Step 3: Commit**

```bash
git add src/fixup/schema.rs
git commit -m "feat(s6): FixupConfig schema with deny_unknown_fields"
```

---

## Phase 3 — cfg grammar

### Task 4: `cfg()` predicate parser

**Files:**
- Modify: `src/fixup/cfg.rs`

- [ ] **Step 1: Write the failing test**

Replace the placeholder in `src/fixup/cfg.rs` with the parser implementation and tests:

```rust
//! `cfg(<expr>)` predicate parser + evaluator.
//!
//! Grammar (informal):
//!   expr       := atom | combinator
//!   atom       := IDENT '=' STRING
//!   combinator := ('all'|'any'|'not') '(' expr (',' expr)* ')'
//!   IDENT      := version | python | target_os | target_arch | target_env

use crate::fixup::error::CfgParseError;

#[derive(Debug, Clone, PartialEq)]
pub enum CfgPredicate {
    Version(String),       // PEP 440 specifier string, e.g. ">=10.0"
    Python(String),
    TargetOs(String),
    TargetArch(String),
    TargetEnv(String),
    All(Vec<CfgPredicate>),
    Any(Vec<CfgPredicate>),
    Not(Box<CfgPredicate>),
}

#[derive(Debug, Clone)]
pub struct CfgContext<'a> {
    pub package_version: &'a pep440_rs::Version,
    pub python_version: pep440_rs::Version,
    pub target_os: &'a str,
    pub target_arch: &'a str,
    pub target_env: &'a str,
}

impl CfgPredicate {
    /// Parse a predicate string (the inside of `cfg(...)` — i.e. the
    /// stripped section-header content).
    pub fn parse(input: &str) -> Result<Self, CfgParseError> {
        let mut p = Parser::new(input);
        let expr = p.parse_expr()?;
        p.skip_ws();
        if p.pos < p.input.len() {
            return Err(CfgParseError::AtOffset {
                offset: p.pos,
                message: format!("unexpected trailing input: `{}`", &p.input[p.pos..]),
            });
        }
        Ok(expr)
    }
}

struct Parser<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self { Self { input, pos: 0 } }

    fn skip_ws(&mut self) {
        while self.pos < self.input.len()
            && self.input.as_bytes()[self.pos].is_ascii_whitespace()
        {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.input.as_bytes().get(self.pos).copied()
    }

    fn parse_expr(&mut self) -> Result<CfgPredicate, CfgParseError> {
        self.skip_ws();
        let ident = self.parse_ident()?;
        match ident.as_str() {
            "all" | "any" | "not" => self.parse_combinator(&ident),
            atom => self.parse_atom_body(atom),
        }
    }

    fn parse_ident(&mut self) -> Result<String, CfgParseError> {
        self.skip_ws();
        let start = self.pos;
        while self.pos < self.input.len() {
            let b = self.input.as_bytes()[self.pos];
            if b.is_ascii_alphanumeric() || b == b'_' { self.pos += 1; } else { break; }
        }
        if start == self.pos {
            return Err(CfgParseError::AtOffset {
                offset: self.pos,
                message: "expected identifier".into(),
            });
        }
        Ok(self.input[start..self.pos].to_string())
    }

    fn expect_byte(&mut self, b: u8) -> Result<(), CfgParseError> {
        self.skip_ws();
        if self.peek() == Some(b) { self.pos += 1; Ok(()) }
        else {
            Err(CfgParseError::AtOffset {
                offset: self.pos,
                message: format!("expected `{}`", b as char),
            })
        }
    }

    fn parse_combinator(&mut self, name: &str) -> Result<CfgPredicate, CfgParseError> {
        self.expect_byte(b'(')?;
        let mut args = Vec::new();
        loop {
            args.push(self.parse_expr()?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => { self.pos += 1; }
                Some(b')') => { self.pos += 1; break; }
                _ => return Err(CfgParseError::AtOffset {
                    offset: self.pos,
                    message: "expected `,` or `)`".into(),
                }),
            }
        }
        match name {
            "all" => Ok(CfgPredicate::All(args)),
            "any" => Ok(CfgPredicate::Any(args)),
            "not" => {
                if args.len() != 1 {
                    return Err(CfgParseError::AtOffset {
                        offset: self.pos,
                        message: format!("`not` takes exactly 1 argument, got {}", args.len()),
                    });
                }
                Ok(CfgPredicate::Not(Box::new(args.into_iter().next().unwrap())))
            }
            _ => unreachable!("matched above"),
        }
    }

    fn parse_atom_body(&mut self, ident: &str) -> Result<CfgPredicate, CfgParseError> {
        self.expect_byte(b'=')?;
        let value = self.parse_string()?;
        match ident {
            "version" => Ok(CfgPredicate::Version(value)),
            "python" => Ok(CfgPredicate::Python(value)),
            "target_os" => Ok(CfgPredicate::TargetOs(value)),
            "target_arch" => Ok(CfgPredicate::TargetArch(value)),
            "target_env" => Ok(CfgPredicate::TargetEnv(value)),
            other => Err(CfgParseError::AtOffset {
                offset: self.pos,
                message: format!("unknown atom `{}`", other),
            }),
        }
    }

    fn parse_string(&mut self) -> Result<String, CfgParseError> {
        self.skip_ws();
        if self.peek() != Some(b'"') {
            return Err(CfgParseError::AtOffset {
                offset: self.pos,
                message: "expected `\"`".into(),
            });
        }
        self.pos += 1;
        let start = self.pos;
        while self.pos < self.input.len() && self.input.as_bytes()[self.pos] != b'"' {
            self.pos += 1;
        }
        if self.pos >= self.input.len() {
            return Err(CfgParseError::AtOffset {
                offset: start,
                message: "unterminated string".into(),
            });
        }
        let value = self.input[start..self.pos].to_string();
        self.pos += 1; // consume closing quote
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_target_os_atom() {
        let p = CfgPredicate::parse("target_os = \"linux\"").unwrap();
        assert_eq!(p, CfgPredicate::TargetOs("linux".into()));
    }

    #[test]
    fn parses_version_atom() {
        let p = CfgPredicate::parse("version = \">=10.0\"").unwrap();
        assert_eq!(p, CfgPredicate::Version(">=10.0".into()));
    }

    #[test]
    fn parses_all_combinator() {
        let p = CfgPredicate::parse("all(target_os = \"linux\", target_env = \"musl\")").unwrap();
        match p {
            CfgPredicate::All(args) => {
                assert_eq!(args.len(), 2);
                assert_eq!(args[0], CfgPredicate::TargetOs("linux".into()));
                assert_eq!(args[1], CfgPredicate::TargetEnv("musl".into()));
            }
            other => panic!("expected All, got {:?}", other),
        }
    }

    #[test]
    fn parses_any_with_nested() {
        let p = CfgPredicate::parse(
            "any(target_os = \"macos\", all(target_os = \"linux\", target_arch = \"aarch64\"))",
        )
        .unwrap();
        match p {
            CfgPredicate::Any(args) => assert_eq!(args.len(), 2),
            other => panic!("expected Any, got {:?}", other),
        }
    }

    #[test]
    fn parses_not_unary() {
        let p = CfgPredicate::parse("not(target_os = \"windows\")").unwrap();
        match p {
            CfgPredicate::Not(inner) => {
                assert_eq!(*inner, CfgPredicate::TargetOs("windows".into()));
            }
            other => panic!("expected Not, got {:?}", other),
        }
    }

    #[test]
    fn rejects_not_with_two_args() {
        let err = CfgPredicate::parse("not(target_os = \"linux\", target_os = \"macos\")")
            .unwrap_err();
        assert!(err.to_string().contains("not"), "got: {}", err);
    }

    #[test]
    fn rejects_unknown_atom() {
        let err = CfgPredicate::parse("target_family = \"unix\"").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("target_family"), "got: {}", msg);
    }

    #[test]
    fn rejects_trailing_input() {
        let err = CfgPredicate::parse("target_os = \"linux\" extra").unwrap_err();
        assert!(err.to_string().contains("trailing"), "got: {}", err);
    }

    #[test]
    fn parses_with_whitespace_insensitivity() {
        let p = CfgPredicate::parse("  all (  target_os = \"linux\" , python = \">=3.12\"  )  ")
            .unwrap();
        match p {
            CfgPredicate::All(args) => assert_eq!(args.len(), 2),
            other => panic!("expected All, got {:?}", other),
        }
    }

    #[test]
    fn parses_python_atom() {
        let p = CfgPredicate::parse("python = \">=3.12\"").unwrap();
        assert_eq!(p, CfgPredicate::Python(">=3.12".into()));
    }
}
```

- [ ] **Step 2: Run cfg parser tests**

```bash
cargo test --lib fixup::cfg
```

Expected: 10 PASS.

- [ ] **Step 3: Commit**

```bash
git add src/fixup/cfg.rs
git commit -m "feat(s6): cfg() predicate parser"
```

---

### Task 5: `cfg()` predicate evaluator

**Files:**
- Modify: `src/fixup/cfg.rs`

- [ ] **Step 1: Write the failing test (append to existing `mod tests`)**

Add to `src/fixup/cfg.rs`, BELOW the `impl Parser` block (still inside the file, above the `#[cfg(test)] mod tests` block):

```rust
impl CfgPredicate {
    /// Evaluate against a cell context. Returns true iff the predicate
    /// holds for `(package_version, python_version, target_os, target_arch, target_env)`.
    ///
    /// PEP 440 specifier mismatches (malformed `version`/`python` strings) cause
    /// the predicate to return false rather than error — by construction these
    /// strings come from user-written fixups, and validation happens at parse
    /// time (separate path; not implemented here for v1 simplicity).
    pub fn evaluate(&self, ctx: &CfgContext<'_>) -> bool {
        use pep440_rs::VersionSpecifiers;
        use std::str::FromStr;
        match self {
            Self::Version(spec) => VersionSpecifiers::from_str(spec)
                .map(|s| s.contains(ctx.package_version))
                .unwrap_or(false),
            Self::Python(spec) => VersionSpecifiers::from_str(spec)
                .map(|s| s.contains(&ctx.python_version))
                .unwrap_or(false),
            Self::TargetOs(want) => ctx.target_os == want,
            Self::TargetArch(want) => ctx.target_arch == want,
            Self::TargetEnv(want) => ctx.target_env == want,
            Self::All(args) => args.iter().all(|a| a.evaluate(ctx)),
            Self::Any(args) => args.iter().any(|a| a.evaluate(ctx)),
            Self::Not(arg) => !arg.evaluate(ctx),
        }
    }
}

/// Decompose a rustc-style target triple (e.g. "x86_64-unknown-linux-musl")
/// into `(arch, os, env)`. Convention:
///   arch = first segment
///   os   = "linux" | "macos" | "windows"  (extracted from the triple)
///   env  = trailing "gnu"/"musl" for linux; "" for macos/windows
pub fn split_target_triple(triple: &str) -> (String, String, String) {
    let parts: Vec<&str> = triple.split('-').collect();
    let arch = parts.first().copied().unwrap_or("").to_string();
    let (os, env) = if triple.contains("-linux-") || triple.ends_with("-linux") {
        let env = if triple.ends_with("-musl") { "musl" }
                  else if triple.ends_with("-gnu") { "gnu" }
                  else { "" };
        ("linux".to_string(), env.to_string())
    } else if triple.contains("apple-darwin") || triple.contains("-macos") {
        ("macos".to_string(), "".to_string())
    } else if triple.contains("-windows-") || triple.contains("-windows") {
        ("windows".to_string(), "".to_string())
    } else {
        ("".to_string(), "".to_string())
    };
    (arch, os, env)
}
```

Append these tests to the existing `#[cfg(test)] mod tests` block in the same file:

```rust
    use std::str::FromStr;

    fn ctx_for(version: &str, py: &str, arch: &str, os: &str, env: &str)
        -> (pep440_rs::Version, pep440_rs::Version)
    {
        (
            pep440_rs::Version::from_str(version).unwrap(),
            pep440_rs::Version::from_str(py).unwrap(),
        )
    }

    #[test]
    fn evaluates_target_os_match() {
        let (v, py) = ctx_for("1.0", "3.12", "x86_64", "linux", "gnu");
        let ctx = CfgContext {
            package_version: &v, python_version: py,
            target_os: "linux", target_arch: "x86_64", target_env: "gnu",
        };
        assert!(CfgPredicate::parse("target_os = \"linux\"").unwrap().evaluate(&ctx));
        assert!(!CfgPredicate::parse("target_os = \"macos\"").unwrap().evaluate(&ctx));
    }

    #[test]
    fn evaluates_version_specifier() {
        let (v, py) = ctx_for("10.5", "3.12", "x86_64", "linux", "gnu");
        let ctx = CfgContext {
            package_version: &v, python_version: py,
            target_os: "linux", target_arch: "x86_64", target_env: "gnu",
        };
        assert!(CfgPredicate::parse("version = \">=10.0\"").unwrap().evaluate(&ctx));
        assert!(!CfgPredicate::parse("version = \">=11.0\"").unwrap().evaluate(&ctx));
        assert!(CfgPredicate::parse("version = \">=10.0,<11\"").unwrap().evaluate(&ctx));
    }

    #[test]
    fn evaluates_python_specifier() {
        let (v, py) = ctx_for("1.0", "3.12.0", "x86_64", "linux", "gnu");
        let ctx = CfgContext {
            package_version: &v, python_version: py,
            target_os: "linux", target_arch: "x86_64", target_env: "gnu",
        };
        assert!(CfgPredicate::parse("python = \">=3.12\"").unwrap().evaluate(&ctx));
        assert!(!CfgPredicate::parse("python = \">=3.13\"").unwrap().evaluate(&ctx));
    }

    #[test]
    fn evaluates_combinators() {
        let (v, py) = ctx_for("10.5", "3.12", "x86_64", "linux", "musl");
        let ctx = CfgContext {
            package_version: &v, python_version: py,
            target_os: "linux", target_arch: "x86_64", target_env: "musl",
        };
        assert!(CfgPredicate::parse("all(target_os = \"linux\", target_env = \"musl\")").unwrap().evaluate(&ctx));
        assert!(!CfgPredicate::parse("all(target_os = \"linux\", target_env = \"gnu\")").unwrap().evaluate(&ctx));
        assert!(CfgPredicate::parse("any(target_env = \"musl\", target_env = \"gnu\")").unwrap().evaluate(&ctx));
        assert!(CfgPredicate::parse("not(target_os = \"macos\")").unwrap().evaluate(&ctx));
    }

    #[test]
    fn evaluates_target_env_empty_for_macos() {
        let (v, py) = ctx_for("1.0", "3.12", "aarch64", "macos", "");
        let ctx = CfgContext {
            package_version: &v, python_version: py,
            target_os: "macos", target_arch: "aarch64", target_env: "",
        };
        assert!(CfgPredicate::parse("target_env = \"\"").unwrap().evaluate(&ctx));
    }

    #[test]
    fn split_triple_x86_64_linux_gnu() {
        let (arch, os, env) = split_target_triple("x86_64-unknown-linux-gnu");
        assert_eq!(arch, "x86_64");
        assert_eq!(os, "linux");
        assert_eq!(env, "gnu");
    }

    #[test]
    fn split_triple_aarch64_apple_darwin() {
        let (arch, os, env) = split_target_triple("aarch64-apple-darwin");
        assert_eq!(arch, "aarch64");
        assert_eq!(os, "macos");
        assert_eq!(env, "");
    }

    #[test]
    fn split_triple_musl() {
        let (arch, os, env) = split_target_triple("x86_64-unknown-linux-musl");
        assert_eq!(env, "musl");
    }
```

- [ ] **Step 2: Run all cfg tests**

```bash
cargo test --lib fixup::cfg
```

Expected: 17 PASS (10 parser + 7 evaluator).

- [ ] **Step 3: Update `src/fixup/mod.rs` re-exports**

```rust
pub use cfg::{CfgContext, CfgPredicate, split_target_triple};
```

- [ ] **Step 4: Commit**

```bash
git add src/fixup/cfg.rs src/fixup/mod.rs
git commit -m "feat(s6): cfg() predicate evaluator + target-triple split"
```

---

## Phase 4 — Resolution

### Task 6: `merge_into` + `ResolvedFixup` + `resolve_for_cell`

**Files:**
- Modify: `src/fixup/layer.rs`

- [ ] **Step 1: Write the failing test**

Replace placeholder in `src/fixup/layer.rs`:

```rust
//! Merge multiple `FixupBody` instances into a single `ResolvedFixup`.
//!
//! S6 is local-only: `resolve_for_cell` runs `merge_into` over the
//! top-level body, then over each cfg section whose predicate matches.
//! S7 will add the community layer as an outer wrapper.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::fixup::cfg::{CfgContext, CfgPredicate};
use crate::fixup::schema::{EntryPoints, FixupBody, FixupConfig};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ResolvedFixup {
    pub extra_deps: Vec<String>,
    pub omit_deps: Vec<String>,
    pub replace_deps: BTreeMap<String, String>,
    pub prefer_wheel: Option<String>,
    pub exclude_wheels: Vec<String>,
    pub overlay: Option<PathBuf>,
    pub entry_points: Option<EntryPoints>,
    pub visibility: Option<Vec<String>>,
    pub labels: Vec<String>,
    pub runtime_env: BTreeMap<String, String>,
}

/// Merge `rhs` into `lhs`. List-valued fields accumulate (no dedupe;
/// emitter dedupes after applying omit_deps). Scalar fields replace
/// when rhs is `Some`. Map fields extend (later keys win).
pub fn merge_into(lhs: &mut ResolvedFixup, rhs: &FixupBody) {
    lhs.extra_deps.extend(rhs.extra_deps.iter().cloned());
    lhs.omit_deps.extend(rhs.omit_deps.iter().cloned());
    lhs.replace_deps.extend(
        rhs.replace_deps.iter().map(|(k, v)| (k.clone(), v.clone())),
    );
    if rhs.prefer_wheel.is_some() {
        lhs.prefer_wheel = rhs.prefer_wheel.clone();
    }
    lhs.exclude_wheels.extend(rhs.exclude_wheels.iter().cloned());
    if rhs.overlay.is_some() {
        lhs.overlay = rhs.overlay.clone();
    }
    if rhs.entry_points.is_some() {
        lhs.entry_points = rhs.entry_points.clone();
    }
    if rhs.visibility.is_some() {
        lhs.visibility = rhs.visibility.clone();
    }
    lhs.labels.extend(rhs.labels.iter().cloned());
    lhs.runtime_env.extend(
        rhs.runtime_env.iter().map(|(k, v)| (k.clone(), v.clone())),
    );
}

/// Resolve a package's fixup for one cell. Returns `None` if no fixup
/// exists for this package. Predicate parse failures become `None`
/// for that one section (logged at load time in T7's caller).
pub fn resolve_for_cell(
    config: &FixupConfig,
    ctx: &CfgContext<'_>,
) -> ResolvedFixup {
    let mut out = ResolvedFixup::default();
    merge_into(&mut out, &config.top);
    for (predicate_str, body) in &config.cfg_sections {
        let Ok(predicate) = CfgPredicate::parse(predicate_str) else {
            continue;  // parse error reported at load time, not here
        };
        if predicate.evaluate(ctx) {
            merge_into(&mut out, body);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixup::cfg::CfgContext;
    use pep440_rs::Version;
    use std::str::FromStr;

    fn empty_ctx() -> (Version, Version) {
        (
            Version::from_str("1.0").unwrap(),
            Version::from_str("3.12").unwrap(),
        )
    }

    #[test]
    fn merge_lists_accumulate() {
        let mut acc = ResolvedFixup::default();
        let a = FixupBody { extra_deps: vec!["//a:b".into()], ..Default::default() };
        let b = FixupBody { extra_deps: vec!["//c:d".into()], ..Default::default() };
        merge_into(&mut acc, &a);
        merge_into(&mut acc, &b);
        assert_eq!(acc.extra_deps, vec!["//a:b", "//c:d"]);
    }

    #[test]
    fn merge_scalars_replace_when_some() {
        let mut acc = ResolvedFixup::default();
        let a = FixupBody { overlay: Some("first".into()), ..Default::default() };
        let b = FixupBody { overlay: Some("second".into()), ..Default::default() };
        let c = FixupBody { overlay: None, ..Default::default() };
        merge_into(&mut acc, &a);
        merge_into(&mut acc, &b);
        merge_into(&mut acc, &c);
        assert_eq!(acc.overlay.as_deref(), Some(std::path::Path::new("second")));
    }

    #[test]
    fn merge_maps_extend() {
        let mut acc = ResolvedFixup::default();
        let mut a_rd = BTreeMap::new();
        a_rd.insert("numpy".into(), "//company/numpy:numpy".into());
        let mut b_rd = BTreeMap::new();
        b_rd.insert("numpy".into(), "//new/numpy:numpy".into());
        b_rd.insert("scipy".into(), "//company/scipy:scipy".into());

        merge_into(&mut acc, &FixupBody { replace_deps: a_rd, ..Default::default() });
        merge_into(&mut acc, &FixupBody { replace_deps: b_rd, ..Default::default() });

        assert_eq!(acc.replace_deps.get("numpy").map(|s| s.as_str()), Some("//new/numpy:numpy"));
        assert_eq!(acc.replace_deps.get("scipy").map(|s| s.as_str()), Some("//company/scipy:scipy"));
    }

    #[test]
    fn resolve_picks_matching_cfg_sections() {
        let (v, py) = empty_ctx();
        let ctx = CfgContext {
            package_version: &v, python_version: py,
            target_os: "linux", target_arch: "x86_64", target_env: "gnu",
        };
        let config = FixupConfig {
            top: FixupBody { extra_deps: vec!["//base:dep".into()], ..Default::default() },
            cfg_sections: vec![
                (
                    "target_os = \"linux\"".into(),
                    FixupBody { extra_deps: vec!["//linux:dep".into()], ..Default::default() },
                ),
                (
                    "target_os = \"macos\"".into(),
                    FixupBody { extra_deps: vec!["//macos:dep".into()], ..Default::default() },
                ),
            ],
        };
        let resolved = resolve_for_cell(&config, &ctx);
        assert_eq!(resolved.extra_deps, vec!["//base:dep", "//linux:dep"]);
    }

    #[test]
    fn resolve_with_no_matches_yields_top_only() {
        let (v, py) = empty_ctx();
        let ctx = CfgContext {
            package_version: &v, python_version: py,
            target_os: "macos", target_arch: "aarch64", target_env: "",
        };
        let config = FixupConfig {
            top: FixupBody { extra_deps: vec!["//base:dep".into()], ..Default::default() },
            cfg_sections: vec![(
                "target_os = \"linux\"".into(),
                FixupBody { extra_deps: vec!["//linux:dep".into()], ..Default::default() },
            )],
        };
        let resolved = resolve_for_cell(&config, &ctx);
        assert_eq!(resolved.extra_deps, vec!["//base:dep"]);
    }

    #[test]
    fn resolve_section_order_preserved() {
        // Later cfg sections override earlier scalar values.
        let (v, py) = empty_ctx();
        let ctx = CfgContext {
            package_version: &v, python_version: py,
            target_os: "linux", target_arch: "x86_64", target_env: "gnu",
        };
        let config = FixupConfig {
            top: FixupBody { overlay: Some("top".into()), ..Default::default() },
            cfg_sections: vec![
                (
                    "target_os = \"linux\"".into(),
                    FixupBody { overlay: Some("first".into()), ..Default::default() },
                ),
                (
                    "target_arch = \"x86_64\"".into(),
                    FixupBody { overlay: Some("second".into()), ..Default::default() },
                ),
            ],
        };
        let resolved = resolve_for_cell(&config, &ctx);
        assert_eq!(resolved.overlay.as_deref(), Some(std::path::Path::new("second")));
    }
}
```

- [ ] **Step 2: Run layer tests**

```bash
cargo test --lib fixup::layer
```

Expected: 6 PASS.

- [ ] **Step 3: Commit**

```bash
git add src/fixup/layer.rs
git commit -m "feat(s6): merge_into + resolve_for_cell"
```

---

### Task 7: Disk loader (`load_local`)

**Files:**
- Modify: `src/fixup/loader.rs`

- [ ] **Step 1: Write the failing test**

Replace placeholder in `src/fixup/loader.rs`:

```rust
//! Disk walk: load every `fixups/<pkg>/fixups.toml` under `third_party_dir`.

use std::collections::BTreeMap;
use std::path::Path;
use std::str::FromStr;

use pep508_rs::PackageName;

use crate::fixup::error::FixupError;
use crate::fixup::schema::FixupConfig;

#[derive(Debug, Default, Clone)]
pub struct FixupSet {
    fixups: BTreeMap<PackageName, FixupConfig>,
}

impl FixupSet {
    pub fn get(&self, name: &PackageName) -> Option<&FixupConfig> {
        self.fixups.get(name)
    }
    pub fn iter(&self) -> impl Iterator<Item = (&PackageName, &FixupConfig)> {
        self.fixups.iter()
    }
    pub fn len(&self) -> usize { self.fixups.len() }
    pub fn is_empty(&self) -> bool { self.fixups.is_empty() }
}

/// Load every `<third_party_dir>/fixups/<pkg>/fixups.toml` into a
/// `FixupSet`. Package names are PEP 503-normalized.
///
/// Returns an empty `FixupSet` if `<third_party_dir>/fixups/` doesn't
/// exist — that's the no-fixups case, not an error.
pub fn load_local(third_party_dir: &Path) -> Result<FixupSet, FixupError> {
    let fixups_dir = third_party_dir.join("fixups");
    if !fixups_dir.is_dir() {
        return Ok(FixupSet::default());
    }

    let mut fixups = BTreeMap::new();
    let entries = std::fs::read_dir(&fixups_dir).map_err(|e| FixupError::Io {
        path: fixups_dir.clone(),
        source: e,
    })?;

    for entry in entries {
        let entry = entry.map_err(|e| FixupError::Io {
            path: fixups_dir.clone(),
            source: e,
        })?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let toml_path = path.join("fixups.toml");
        if !toml_path.is_file() {
            continue;
        }

        let pkg_dir_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| FixupError::Io {
                path: path.clone(),
                source: std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "non-utf8 directory name",
                ),
            })?;

        // PEP 503: normalize via PackageName.
        let pkg_name = PackageName::from_str(pkg_dir_name).map_err(|e| FixupError::Io {
            path: path.clone(),
            source: std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()),
        })?;

        let body = std::fs::read_to_string(&toml_path).map_err(|e| FixupError::Io {
            path: toml_path.clone(),
            source: e,
        })?;

        let config = FixupConfig::from_toml_str(&body).map_err(|source| {
            // Distinguish unknown-field vs other parse errors.
            let msg = source.to_string();
            if let Some(field) = extract_unknown_field(&msg) {
                FixupError::UnknownField { file: toml_path.clone(), field }
            } else {
                FixupError::ParseError { file: toml_path.clone(), source }
            }
        })?;

        fixups.insert(pkg_name, config);
    }

    Ok(FixupSet { fixups })
}

/// `toml::de::Error` for unknown fields reads like:
///   `unknown field `foo`, expected one of `extra_deps`, ...`
/// Extract the field name if present.
fn extract_unknown_field(msg: &str) -> Option<String> {
    let needle = "unknown field `";
    let start = msg.find(needle)? + needle.len();
    let rest = &msg[start..];
    let end = rest.find('`')?;
    Some(rest[..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write(dir: &Path, rel: &str, contents: &str) {
        let path = dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn missing_fixups_dir_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let set = load_local(tmp.path()).unwrap();
        assert!(set.is_empty());
    }

    #[test]
    fn loads_single_fixup() {
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "fixups/pillow/fixups.toml",
            r#"extra_deps = ["//third-party/c:libjpeg"]"#,
        );
        let set = load_local(tmp.path()).unwrap();
        assert_eq!(set.len(), 1);
        let name = PackageName::from_str("pillow").unwrap();
        let cfg = set.get(&name).unwrap();
        assert_eq!(cfg.top.extra_deps, vec!["//third-party/c:libjpeg"]);
    }

    #[test]
    fn normalizes_pep503_name() {
        // PEP 503: "Pillow" and "pillow" and "PIL_LOW" all normalize.
        let tmp = TempDir::new().unwrap();
        write(
            tmp.path(),
            "fixups/Pillow/fixups.toml",
            r#"extra_deps = ["//x:y"]"#,
        );
        let set = load_local(tmp.path()).unwrap();
        let name = PackageName::from_str("pillow").unwrap();
        assert!(set.get(&name).is_some());
    }

    #[test]
    fn skips_dirs_without_fixups_toml() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "fixups/pillow/fixups.toml", r#"extra_deps = []"#);
        std::fs::create_dir_all(tmp.path().join("fixups/orphan-dir")).unwrap();
        let set = load_local(tmp.path()).unwrap();
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn unknown_field_error_is_typed() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "fixups/pillow/fixups.toml", r#"extras = []"#);
        let err = load_local(tmp.path()).unwrap_err();
        match err {
            FixupError::UnknownField { field, .. } => assert_eq!(field, "extras"),
            other => panic!("expected UnknownField, got {:?}", other),
        }
    }

    #[test]
    fn multiple_fixups_load_deterministically() {
        let tmp = TempDir::new().unwrap();
        write(tmp.path(), "fixups/aaa/fixups.toml", r#"extra_deps = []"#);
        write(tmp.path(), "fixups/zzz/fixups.toml", r#"extra_deps = []"#);
        write(tmp.path(), "fixups/mmm/fixups.toml", r#"extra_deps = []"#);

        let set = load_local(tmp.path()).unwrap();
        let names: Vec<String> = set.iter().map(|(n, _)| n.to_string()).collect();
        assert_eq!(names, vec!["aaa", "mmm", "zzz"]); // BTreeMap sorted
    }
}
```

- [ ] **Step 2: Run loader tests**

```bash
cargo test --lib fixup::loader
```

Expected: 6 PASS.

- [ ] **Step 3: Update `src/fixup/mod.rs` re-exports**

(Already includes `pub use loader::{FixupSet, load_local};` from T1.)

- [ ] **Step 4: Commit**

```bash
git add src/fixup/loader.rs
git commit -m "feat(s6): FixupSet disk loader with PEP 503 normalization"
```

---

## Phase 5 — Emitter integration

### Task 8: Add `glob` dep + Buck target validator

**Files:**
- Modify: `Cargo.toml`
- Create: `src/fixup/validate.rs`
- Modify: `src/fixup/mod.rs`

- [ ] **Step 1: Add `glob` to Cargo.toml `[dependencies]`**

Locate the `[dependencies]` block (where `walkdir`, `thiserror`, etc. live) and append:

```toml
glob = "0.3"
```

- [ ] **Step 2: Write the failing test**

Create `src/fixup/validate.rs`:

```rust
//! Buck target validation. Cheap pre-flight to catch typos in fixup
//! `extra_deps` / `replace_deps` before they hit Buck's parser.

/// Valid Buck target: optional `//path` followed by `:name`.
/// Examples:
///   //third-party/c:libjpeg
///   //a/b/c:my_target
///   :local
///   //foo/...        (NOT supported — wildcards aren't fixup-substitutable)
pub fn is_valid_buck_target(s: &str) -> bool {
    // Empty or no colon → invalid.
    let Some(colon_pos) = s.rfind(':') else { return false; };
    let (pkg, name) = s.split_at(colon_pos);
    let name = &name[1..]; // strip the colon

    // Name must be non-empty and consist of [A-Za-z0-9_./\-].
    if name.is_empty() || !name.bytes().all(is_target_char) {
        return false;
    }

    // Package portion: either empty (`:local` form) or //...
    if pkg.is_empty() {
        return true;
    }
    if !pkg.starts_with("//") {
        return false;
    }
    // Allow [A-Za-z0-9_./\-] in path portion. Reject `..` segments.
    let path = &pkg[2..];
    if path.is_empty() {
        return false;
    }
    for seg in path.split('/') {
        if seg.is_empty() || seg == ".." || seg == "." {
            return false;
        }
        if !seg.bytes().all(is_target_char) {
            return false;
        }
    }
    true
}

fn is_target_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'/' | b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_canonical_forms() {
        assert!(is_valid_buck_target("//third-party/c:libjpeg"));
        assert!(is_valid_buck_target(":local"));
        assert!(is_valid_buck_target("//a/b/c:target"));
        assert!(is_valid_buck_target("//x:y-z_w.0"));
    }

    #[test]
    fn rejects_no_colon() {
        assert!(!is_valid_buck_target("//third-party/c/libjpeg"));
        assert!(!is_valid_buck_target("libjpeg"));
    }

    #[test]
    fn rejects_empty_name() {
        assert!(!is_valid_buck_target("//foo:"));
        assert!(!is_valid_buck_target(":"));
    }

    #[test]
    fn rejects_wildcards() {
        assert!(!is_valid_buck_target("//foo/...:bar"));
        assert!(!is_valid_buck_target("//foo:bar*"));
    }

    #[test]
    fn rejects_traversal() {
        assert!(!is_valid_buck_target("//../foo:bar"));
        assert!(!is_valid_buck_target("//foo/..:bar"));
    }

    #[test]
    fn rejects_missing_leading_slashes() {
        assert!(!is_valid_buck_target("foo:bar"));
        assert!(!is_valid_buck_target("/foo:bar"));
    }
}
```

- [ ] **Step 3: Re-export from `src/fixup/mod.rs`**

Add at the bottom of the existing pub-mod block:

```rust
pub mod validate;
pub use validate::is_valid_buck_target;
```

- [ ] **Step 4: Run tests**

```bash
cargo test --lib fixup::validate
```

Expected: 6 PASS.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock src/fixup/validate.rs src/fixup/mod.rs
git commit -m "feat(s6): Buck target validator + glob dep"
```

---

### Task 9: Overlay tree walk + symlink rejection

**Files:**
- Create: `src/fixup/overlay.rs`
- Modify: `src/fixup/mod.rs`

- [ ] **Step 1: Write the failing test**

Create `src/fixup/overlay.rs`:

```rust
//! Discover overlay files: walk `<fixup_dir>/<overlay>/` recursively,
//! return `(path_in_wheel, path_relative_to_third_party_dir)` pairs.
//!
//! Rejects symlinks that escape the fixup directory. Returns
//! `OverlayEmpty` if the overlay directory is missing or empty.

use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::fixup::error::FixupError;

/// Walk `<fixup_dir>/<overlay_rel>/` and return `(path_in_wheel,
/// src_path_relative_to_third_party_dir)` pairs.
///
/// `pkg_name`: only used for error messages.
/// `third_party_dir`: the tree's third-party dir (e.g. `third-party/python`).
/// `fixup_dir`: the package's fixup directory (e.g. `third-party/python/fixups/pillow`).
pub fn walk_overlay(
    pkg_name: &str,
    third_party_dir: &Path,
    fixup_dir: &Path,
    overlay_rel: &Path,
) -> Result<Vec<(String, String)>, FixupError> {
    let root = fixup_dir.join(overlay_rel);
    if !root.is_dir() {
        return Err(FixupError::OverlayEmpty {
            pkg: pkg_name.to_string(),
            path: overlay_rel.display().to_string(),
        });
    }

    let canonical_root = std::fs::canonicalize(&root).map_err(|e| FixupError::Io {
        path: root.clone(),
        source: e,
    })?;

    let mut files = Vec::new();
    for entry in WalkDir::new(&root).follow_links(false) {
        let entry = entry.map_err(|e| FixupError::Io {
            path: root.clone(),
            source: std::io::Error::other(e.to_string()),
        })?;
        if !entry.file_type().is_file() {
            continue;
        }

        // Symlink escape: canonicalize the file path and confirm it's
        // still under canonical_root.
        let canon = std::fs::canonicalize(entry.path()).map_err(|e| FixupError::Io {
            path: entry.path().to_path_buf(),
            source: e,
        })?;
        if !canon.starts_with(&canonical_root) {
            return Err(FixupError::OverlayPathOutsideTree {
                file: entry.path().to_path_buf(),
            });
        }

        // Path inside the wheel: relative path from overlay root.
        let in_wheel = entry
            .path()
            .strip_prefix(&root)
            .map_err(|e| FixupError::Io {
                path: entry.path().to_path_buf(),
                source: std::io::Error::other(e.to_string()),
            })?
            .to_string_lossy()
            .replace('\\', "/")
            .to_string();

        // Path used as a Buck `srcs` label — relative to third_party_dir
        // (Buck file paths are relative to the BUCK file, which lives in
        // third_party_dir).
        let src_rel = entry
            .path()
            .strip_prefix(third_party_dir)
            .map_err(|e| FixupError::Io {
                path: entry.path().to_path_buf(),
                source: std::io::Error::other(e.to_string()),
            })?
            .to_string_lossy()
            .replace('\\', "/")
            .to_string();

        files.push((in_wheel, src_rel));
    }

    if files.is_empty() {
        return Err(FixupError::OverlayEmpty {
            pkg: pkg_name.to_string(),
            path: overlay_rel.display().to_string(),
        });
    }

    // Sort for determinism.
    files.sort();
    Ok(files)
}

/// Wrapper that returns the discovered files plus the path-in-wheel set
/// as a single Vec<(String, String)>. Provided as the public API used
/// by `build_emit_input`.
pub fn discover_overlay_files(
    pkg_name: &str,
    third_party_dir: &Path,
    fixup_dir: &Path,
    overlay_rel: &Path,
) -> Result<Vec<(String, String)>, FixupError> {
    walk_overlay(pkg_name, third_party_dir, fixup_dir, overlay_rel)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_file(p: &Path, contents: &str) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, contents).unwrap();
    }

    #[test]
    fn discovers_overlay_files() {
        let tmp = TempDir::new().unwrap();
        let tpd = tmp.path();
        let fixup_dir = tpd.join("fixups/pillow");
        let overlay = fixup_dir.join("overlay");
        write_file(&overlay.join("PIL/_imaging.py"), "print('hi')");
        write_file(&overlay.join("PIL/extra.py"), "x = 1");

        let files = walk_overlay(
            "pillow",
            tpd,
            &fixup_dir,
            Path::new("overlay"),
        ).unwrap();
        let in_wheel: Vec<&str> = files.iter().map(|(w, _)| w.as_str()).collect();
        assert_eq!(in_wheel, vec!["PIL/_imaging.py", "PIL/extra.py"]);
        let src_paths: Vec<&str> = files.iter().map(|(_, s)| s.as_str()).collect();
        assert_eq!(src_paths, vec![
            "fixups/pillow/overlay/PIL/_imaging.py",
            "fixups/pillow/overlay/PIL/extra.py",
        ]);
    }

    #[test]
    fn empty_overlay_errors() {
        let tmp = TempDir::new().unwrap();
        let tpd = tmp.path();
        let fixup_dir = tpd.join("fixups/pillow");
        fs::create_dir_all(fixup_dir.join("overlay")).unwrap();

        let err = walk_overlay(
            "pillow",
            tpd,
            &fixup_dir,
            Path::new("overlay"),
        ).unwrap_err();
        match err {
            FixupError::OverlayEmpty { pkg, .. } => assert_eq!(pkg, "pillow"),
            other => panic!("expected OverlayEmpty, got {:?}", other),
        }
    }

    #[test]
    fn missing_overlay_errors() {
        let tmp = TempDir::new().unwrap();
        let tpd = tmp.path();
        let fixup_dir = tpd.join("fixups/pillow");
        fs::create_dir_all(&fixup_dir).unwrap();

        let err = walk_overlay(
            "pillow",
            tpd,
            &fixup_dir,
            Path::new("overlay"),
        ).unwrap_err();
        match err {
            FixupError::OverlayEmpty { pkg, .. } => assert_eq!(pkg, "pillow"),
            other => panic!("expected OverlayEmpty, got {:?}", other),
        }
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_escape() {
        let tmp = TempDir::new().unwrap();
        let tpd = tmp.path();
        let fixup_dir = tpd.join("fixups/pillow");
        let overlay = fixup_dir.join("overlay");
        fs::create_dir_all(&overlay).unwrap();

        // Create a file outside the fixup dir and symlink to it from overlay.
        let outside = tpd.join("outside_target.py");
        fs::write(&outside, "import os").unwrap();
        std::os::unix::fs::symlink(&outside, overlay.join("escape.py")).unwrap();

        let err = walk_overlay(
            "pillow",
            tpd,
            &fixup_dir,
            Path::new("overlay"),
        ).unwrap_err();
        match err {
            FixupError::OverlayPathOutsideTree { .. } => {}
            other => panic!("expected OverlayPathOutsideTree, got {:?}", other),
        }
    }
}
```

- [ ] **Step 2: Re-export from `src/fixup/mod.rs`**

Add to the existing module declarations:

```rust
pub mod overlay;
pub use overlay::discover_overlay_files;
```

- [ ] **Step 3: Run tests**

```bash
cargo test --lib fixup::overlay
```

Expected: 4 PASS (3 always-on + 1 unix-only).

- [ ] **Step 4: Commit**

```bash
git add src/fixup/overlay.rs src/fixup/mod.rs
git commit -m "feat(s6): overlay tree walk with symlink-escape rejection"
```

---

### Task 10: Extend `EmitPackage` with fixup-derived fields

**Files:**
- Modify: `src/buck/emit.rs:29-35`

- [ ] **Step 1: Write the failing test**

Add a new test at the end of `src/buck/emit.rs`'s `mod tests` block. This test exercises the new field constructors so any signature drift triggers a compile error:

```rust
    #[test]
    fn emit_package_with_fixup_fields_constructs() {
        use std::collections::BTreeMap;

        let mut rt_env = BTreeMap::new();
        rt_env.insert("LIBJPEG_PATH".into(), "/opt/libjpeg/lib".into());

        let pkg = EmitPackage {
            name: "pillow".into(),
            version: "10.0.0".into(),
            deps: EmitDeps::Uniform(vec![]),
            wheels: BTreeMap::new(),
            overlay: Some(EmitOverlay {
                files: vec![("PIL/_imaging.py".into(), "fixups/pillow/overlay/PIL/_imaging.py".into())],
            }),
            entry_points: vec!["pillow-cli".into()],
            visibility: Some(vec!["//apps/imaging/...".into()]),
            labels: vec!["security-sensitive".into()],
            runtime_env: rt_env,
        };
        assert_eq!(pkg.entry_points, vec!["pillow-cli"]);
        assert_eq!(pkg.overlay.unwrap().files.len(), 1);
    }
```

- [ ] **Step 2: Add the new types + fields**

Modify `src/buck/emit.rs:29-35`. Replace the existing `EmitPackage` struct with:

```rust
#[derive(Debug, Clone)]
pub struct EmitPackage {
    pub name: String,
    pub version: String,
    pub deps: EmitDeps,
    pub wheels: BTreeMap<ConfigName, EmitWheel>,
    /// Overlay file list (NEW in S6). `None` = no overlay.
    pub overlay: Option<EmitOverlay>,
    /// Entry-point names to emit `python_binary` rules for (NEW in S6).
    /// Empty vec = no binaries.
    pub entry_points: Vec<String>,
    /// Top-level alias visibility (NEW in S6). `None` = `["PUBLIC"]`.
    pub visibility: Option<Vec<String>>,
    /// Top-level alias labels (NEW in S6).
    pub labels: Vec<String>,
    /// Env applied to emitted `python_binary` rules (NEW in S6).
    pub runtime_env: std::collections::BTreeMap<String, String>,
}

/// Overlay file list. Each entry is `(path_in_wheel, src_path_rel_to_tpd)`.
#[derive(Debug, Clone)]
pub struct EmitOverlay {
    pub files: Vec<(String, String)>,
}
```

- [ ] **Step 3: Fix every existing `EmitPackage { ... }` literal in the file (tests + other call sites)**

Every test in `src/buck/emit.rs` that constructs an `EmitPackage` needs the 5 new fields. Use this default to add wherever an existing literal exists:

```rust
overlay: None,
entry_points: vec![],
visibility: None,
labels: vec![],
runtime_env: std::collections::BTreeMap::new(),
```

In `src/buck/emit.rs`, find each `EmitPackage {` literal in test functions and append these fields before the closing `}`. The list of affected functions (per `grep -n 'EmitPackage {' src/buck/emit.rs`):
- `emit_input_constructs` (~line 335)
- `build_emit_input_from_synthetic_resolved` (the `EmitPackage` shouldn't appear here — it's a `build_emit_input` test; only the new construct test will exercise the fields). If a literal exists, patch it.

Also patch `src/buck/string_writer.rs` — every test that constructs `EmitPackage` literals. Run `grep -n 'EmitPackage {' src/buck/string_writer.rs` to find them.

- [ ] **Step 4: Fix `build_emit_input` to populate the new fields with defaults (no fixup application yet)**

In `src/buck/emit.rs`, find the `packages.push(EmitPackage { ... })` line (~line 306). Update to:

```rust
packages.push(EmitPackage {
    name: key.0,
    version: key.1,
    deps,
    wheels: wheel_map,
    overlay: None,
    entry_points: vec![],
    visibility: None,
    labels: vec![],
    runtime_env: BTreeMap::new(),
});
```

- [ ] **Step 5: Run all buck tests to verify nothing regressed**

```bash
cargo test --lib buck::
```

Expected: all PASS.

- [ ] **Step 6: Commit**

```bash
git add src/buck/emit.rs src/buck/string_writer.rs
git commit -m "feat(s6): extend EmitPackage with overlay/entry_points/visibility/labels/runtime_env"
```

---

### Task 11: Thread `Option<&FixupSet>` through `build_emit_input` (no application yet)

**Files:**
- Modify: `src/buck/emit.rs:105-110` (the `build_emit_input` signature)
- Modify: `src/cli/buckify.rs`

- [ ] **Step 1: Write the failing test**

Add to `src/buck/emit.rs` tests:

```rust
    #[test]
    fn build_emit_input_accepts_none_fixups() {
        // Re-use the existing build_emit_input_from_synthetic_resolved harness
        // pattern but pass None for fixups. Goal: signature compiles + behavior
        // is unchanged from the no-fixup case.
        use crate::config::{Config, Platform, PythonVersion, Tree};
        use crate::lock::types::{DepEdge, FirstPartyKind, Lockfile, Package, Source, Wheel};
        use pep440_rs::Version;
        use pep508_rs::PackageName;
        use url::Url;

        let tree = Tree {
            name: "default".into(),
            manifest_path: "pyproject.toml".into(),
            third_party_dir: "third-party/python".into(),
            python_versions: vec![PythonVersion(3, 12)],
        };
        let mut platforms = std::collections::BTreeMap::new();
        platforms.insert(
            "linux-x86_64-gnu".into(),
            Platform {
                target: "x86_64-unknown-linux-gnu".into(),
                manylinux: Some("2_17".into()),
                musllinux: None,
                macos_min: None,
            },
        );
        let config = Config {
            trees: vec![tree.clone()],
            platforms,
            fixups: Default::default(),
            buck: Default::default(),
            lockfile: Default::default(),
        };
        let lockfile = Lockfile {
            version: 1,
            revision: 3,
            requires_python: ">=3.12".into(),
            packages: vec![
                Package {
                    name: PackageName::from_str("app").unwrap(),
                    version: Version::from_str("0.1").unwrap(),
                    source: Source::FirstParty {
                        kind: FirstPartyKind::Virtual,
                        path: ".".into(),
                    },
                    dependencies: vec![DepEdge {
                        name: PackageName::from_str("certifi").unwrap(),
                        extra: vec![],
                        marker: None,
                    }],
                    sdist: None, wheels: vec![], metadata: None,
                },
                Package {
                    name: PackageName::from_str("certifi").unwrap(),
                    version: Version::from_str("2025.4.26").unwrap(),
                    source: Source::Registry {
                        url: Url::parse("https://pypi.org/simple").unwrap(),
                    },
                    dependencies: vec![],
                    sdist: None,
                    wheels: vec![Wheel {
                        url: Url::parse("https://files.pythonhosted.org/p/certifi.whl").unwrap(),
                        hash: "sha256:abc".into(),
                        size: None,
                        filename: "certifi-2025.4.26-py3-none-any.whl".into(),
                    }],
                    metadata: None,
                },
            ],
        };

        let input = build_emit_input(&config, &tree, &lockfile, None, None)
            .expect("build_emit_input with None fixups");
        assert_eq!(input.packages.len(), 1);
    }
```

- [ ] **Step 2: Update `build_emit_input` signature**

In `src/buck/emit.rs`, locate `pub fn build_emit_input(...)` (~line 105). Change:

```rust
pub fn build_emit_input(
    config: &Config,
    tree: &Tree,
    lockfile: &Lockfile,
    manifest: Option<&crate::sdist::Manifest>,
    fixups: Option<&crate::fixup::FixupSet>,   // NEW
) -> anyhow::Result<EmitInput> {
```

The body need not consume `fixups` yet — just accept it. Add `let _ = fixups;` at the top to suppress unused warning (will be wired in T12).

- [ ] **Step 3: Update every existing call site**

Search:
```bash
grep -rn 'build_emit_input(' src/ tests/
```

Update each call to pass `None` for the new arg:

- `src/cli/buckify.rs` — add `None` (will become real in T16).
- `src/buck/emit.rs` test cases — add `None`.
- Any others.

Specifically in `src/cli/buckify.rs`, change:
```rust
let input = build_emit_input(&config, tree, &lockfile, manifest.as_ref())?;
```
to:
```rust
let input = build_emit_input(&config, tree, &lockfile, manifest.as_ref(), None)?;
```

- [ ] **Step 4: Run tests**

```bash
cargo test --lib buck::
cargo test --test fixtures_emit_pure_python 2>/dev/null || true  # fixture tests, may not be named exactly that
cargo test
```

Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add src/buck/emit.rs src/cli/buckify.rs
git commit -m "feat(s6): thread Option<&FixupSet> through build_emit_input"
```

---

### Task 12: Apply `extra_deps` / `omit_deps` / `replace_deps` in `build_emit_input`

**Files:**
- Modify: `src/buck/emit.rs` (in `build_emit_input` body; also `format_cell_deps`)

- [ ] **Step 1: Write the failing test**

Add this test to `src/buck/emit.rs` tests:

```rust
    #[test]
    fn fixup_extra_deps_appends() {
        use crate::config::{Config, Platform, PythonVersion, Tree};
        use crate::fixup::schema::{FixupBody, FixupConfig};
        use crate::fixup::FixupSet;
        use crate::lock::types::{DepEdge, FirstPartyKind, Lockfile, Package, Source, Wheel};
        use pep440_rs::Version;
        use pep508_rs::PackageName;
        use url::Url;

        let tree = Tree {
            name: "default".into(),
            manifest_path: "pyproject.toml".into(),
            third_party_dir: "third-party/python".into(),
            python_versions: vec![PythonVersion(3, 12)],
        };
        let mut platforms = std::collections::BTreeMap::new();
        platforms.insert(
            "linux-x86_64-gnu".into(),
            Platform {
                target: "x86_64-unknown-linux-gnu".into(),
                manylinux: Some("2_17".into()),
                musllinux: None, macos_min: None,
            },
        );
        let config = Config {
            trees: vec![tree.clone()],
            platforms, fixups: Default::default(),
            buck: Default::default(), lockfile: Default::default(),
        };
        let lockfile = Lockfile {
            version: 1, revision: 3, requires_python: ">=3.12".into(),
            packages: vec![
                Package {
                    name: PackageName::from_str("app").unwrap(),
                    version: Version::from_str("0.1").unwrap(),
                    source: Source::FirstParty {
                        kind: FirstPartyKind::Virtual, path: ".".into(),
                    },
                    dependencies: vec![DepEdge {
                        name: PackageName::from_str("certifi").unwrap(),
                        extra: vec![], marker: None,
                    }],
                    sdist: None, wheels: vec![], metadata: None,
                },
                Package {
                    name: PackageName::from_str("certifi").unwrap(),
                    version: Version::from_str("2025.4.26").unwrap(),
                    source: Source::Registry {
                        url: Url::parse("https://pypi.org/simple").unwrap(),
                    },
                    dependencies: vec![], sdist: None,
                    wheels: vec![Wheel {
                        url: Url::parse("https://files.pythonhosted.org/p/certifi.whl").unwrap(),
                        hash: "sha256:abc".into(), size: None,
                        filename: "certifi-2025.4.26-py3-none-any.whl".into(),
                    }],
                    metadata: None,
                },
            ],
        };

        // Build a FixupSet with certifi getting an extra_dep.
        let mut fixups_map = std::collections::BTreeMap::new();
        let cfg = FixupConfig {
            top: FixupBody {
                extra_deps: vec!["//third-party/c:openssl".into()],
                ..Default::default()
            },
            cfg_sections: vec![],
        };
        fixups_map.insert(PackageName::from_str("certifi").unwrap(), cfg);
        // FixupSet is private-field; expose a test-only constructor.
        let fixups = crate::fixup::FixupSet::from_map_for_test(fixups_map);

        let input = build_emit_input(&config, &tree, &lockfile, None, Some(&fixups))
            .expect("build_emit_input with fixups");
        let pkg = &input.packages[0];
        assert_eq!(pkg.name, "certifi");
        match &pkg.deps {
            EmitDeps::Uniform(deps) => {
                assert!(deps.contains(&"//third-party/c:openssl".to_string()),
                    "deps did not contain the extra_dep: {:?}", deps);
            }
            other => panic!("expected Uniform, got {:?}", other),
        }
    }
```

Note: this test uses `FixupSet::from_map_for_test`. Add this helper to `src/fixup/loader.rs`:

```rust
impl FixupSet {
    #[cfg(test)]
    pub fn from_map_for_test(
        fixups: BTreeMap<PackageName, FixupConfig>,
    ) -> Self {
        Self { fixups }
    }
}
```

Also add a non-test helper for production wiring (avoid `#[cfg(test)]` for emit's test code that compiles in lib tests too). Actually `#[cfg(test)]` in loader.rs works because `src/buck/emit.rs` test code is compiled under the same test cfg. Verify by running the test.

- [ ] **Step 2: Implement fixup-aware dep formatting in `build_emit_input`**

Locate the `format_cell_deps` closure in `src/buck/emit.rs` (~line 273). Replace it with a fixup-aware version. The full diff:

**Step 2a.** At the top of `build_emit_input`, near where `wheel_index` is built, add the cfg context derivation helper:

```rust
    use crate::fixup::cfg::{CfgContext, split_target_triple};
    use std::str::FromStr;
    use pep440_rs::Version;
```

**Step 2b.** Inside the cfg loop (where `for resolved_cfg in &view.configs` starts), after `let cfg_name = ConfigName::new(...)`, compute the `(arch, os, env)` triple split — this gets used when resolving fixups for that cell:

```rust
        let (arch, os, env) = split_target_triple(&plat.target);
```

**Step 2c.** Inside the `for pkg in &resolved_cfg.packages` loop, before the wheel-handling code, resolve the fixup for this `(pkg, cell)`:

```rust
            // Resolve fixup for (pkg, cell). None if no fixup configured.
            let resolved_fixup: Option<crate::fixup::ResolvedFixup> = fixups.and_then(|fs| {
                let name = match pep508_rs::PackageName::from_str(&pkg.name) {
                    Ok(n) => n,
                    Err(_) => return None,
                };
                let cfg = fs.get(&name)?;
                let pkg_ver = Version::from_str(&pkg.version).ok()?;
                let py_ver = Version::from_str(&resolved_cfg.python_version).ok()?;
                let ctx = CfgContext {
                    package_version: &pkg_ver,
                    python_version: py_ver,
                    target_os: &os,
                    target_arch: &arch,
                    target_env: &env,
                };
                Some(crate::fixup::resolve_for_cell(cfg, &ctx))
            });
```

**Step 2d.** In the deps storage area where we currently do `pkg_deps_per_cell.entry(key).or_default().insert(cfg_name.clone(), pkg.deps.clone())`, apply the fixup transformations BEFORE inserting. Locate the existing line (appears in both the sdist-only branch and wheels-present branch). Replace each occurrence with:

```rust
                        let mut cell_deps = pkg.deps.clone();
                        if let Some(rf) = &resolved_fixup {
                            apply_dep_ops(&mut cell_deps, rf);
                        }
                        pkg_deps_per_cell
                            .entry(key.clone())
                            .or_default()
                            .insert(cfg_name.clone(), cell_deps);
```

The two existing occurrences are around lines 210 and 248. Apply to BOTH.

**Step 2e.** Add the `apply_dep_ops` private helper at the bottom of the file (before `#[cfg(test)] mod tests`):

```rust
/// Apply a `ResolvedFixup`'s dep-side ops to a package's raw dep list.
/// Raw deps are formatted as "name@version" by the lock-resolver.
///
/// Order:
///   1. `replace_deps`: substitute package names. The "name" portion of "name@version" is matched.
///   2. `omit_deps`: drop entries matching by name.
///   3. `extra_deps`: append (verbatim Buck targets — no transformation).
fn apply_dep_ops(deps: &mut Vec<String>, fixup: &crate::fixup::ResolvedFixup) {
    // Pre-compute the dep-name → index map for cheap matching.
    let extract_name = |s: &str| s.split('@').next().unwrap_or(s).to_string();

    // 1. replace: rewrite each entry whose name is in replace_deps.
    for dep in deps.iter_mut() {
        let name = extract_name(dep);
        if let Some(target) = fixup.replace_deps.get(&name) {
            // The replacement is a full Buck target; mark it with a sentinel
            // so format_cell_deps knows NOT to add the ":" prefix later.
            *dep = format!("__BUCK_TARGET__{}", target);
        }
    }

    // 2. omit: drop by name (does not affect already-replaced sentinels).
    deps.retain(|d| {
        if d.starts_with("__BUCK_TARGET__") { return true; }
        let name = extract_name(d);
        !fixup.omit_deps.iter().any(|o| o == &name)
    });

    // 3. extra: append as Buck-target sentinels.
    for ed in &fixup.extra_deps {
        deps.push(format!("__BUCK_TARGET__{}", ed));
    }
}
```

**Step 2f.** Update `format_cell_deps` (~line 273) to handle the new sentinel:

```rust
    let format_cell_deps = |raw: &Vec<String>| -> Vec<String> {
        let mut v: Vec<String> = raw
            .iter()
            .map(|d| {
                if let Some(target) = d.strip_prefix("__BUCK_TARGET__") {
                    target.to_string()
                } else {
                    format!(":{}", d.split('@').next().unwrap_or(d))
                }
            })
            .collect();
        v.sort();
        v.dedup();
        v
    };
```

- [ ] **Step 3: Run the new test + regression tests**

```bash
cargo test --lib buck::emit::tests::fixup_extra_deps_appends
cargo test --lib buck::
```

Expected: new test PASS; existing tests still PASS.

- [ ] **Step 4: Commit**

```bash
git add src/buck/emit.rs src/fixup/loader.rs
git commit -m "feat(s6): apply extra_deps/omit_deps/replace_deps in build_emit_input"
```

---

### Task 13: Apply `exclude_wheels` + `prefer_wheel` in wheel selection

**Files:**
- Modify: `src/buck/emit.rs` (wheel selection branches)

- [ ] **Step 1: Write the failing test**

Add to `src/buck/emit.rs` tests:

```rust
    #[test]
    fn fixup_prefer_wheel_overrides_picker() {
        use crate::config::{Config, Platform, PythonVersion, Tree};
        use crate::fixup::schema::{FixupBody, FixupConfig};
        use crate::lock::types::{DepEdge, FirstPartyKind, Lockfile, Package, Source, Wheel};
        use pep440_rs::Version;
        use pep508_rs::PackageName;
        use url::Url;

        let tree = Tree {
            name: "default".into(),
            manifest_path: "pyproject.toml".into(),
            third_party_dir: "third-party/python".into(),
            python_versions: vec![PythonVersion(3, 12)],
        };
        let mut platforms = std::collections::BTreeMap::new();
        platforms.insert(
            "linux-x86_64-gnu".into(),
            Platform {
                target: "x86_64-unknown-linux-gnu".into(),
                manylinux: Some("2_17".into()),
                musllinux: None, macos_min: None,
            },
        );
        let config = Config {
            trees: vec![tree.clone()],
            platforms, fixups: Default::default(),
            buck: Default::default(), lockfile: Default::default(),
        };
        // Two wheels; one would be the picker's natural choice.
        // prefer_wheel = sha256:xyz selects the OTHER one.
        let lockfile = Lockfile {
            version: 1, revision: 3, requires_python: ">=3.12".into(),
            packages: vec![
                Package {
                    name: PackageName::from_str("app").unwrap(),
                    version: Version::from_str("0.1").unwrap(),
                    source: Source::FirstParty {
                        kind: FirstPartyKind::Virtual, path: ".".into(),
                    },
                    dependencies: vec![DepEdge {
                        name: PackageName::from_str("certifi").unwrap(),
                        extra: vec![], marker: None,
                    }],
                    sdist: None, wheels: vec![], metadata: None,
                },
                Package {
                    name: PackageName::from_str("certifi").unwrap(),
                    version: Version::from_str("2025.4.26").unwrap(),
                    source: Source::Registry {
                        url: Url::parse("https://pypi.org/simple").unwrap(),
                    },
                    dependencies: vec![], sdist: None,
                    wheels: vec![
                        Wheel {
                            url: Url::parse("https://files.pythonhosted.org/p/certifi-v1.whl").unwrap(),
                            hash: "sha256:abc".into(), size: None,
                            filename: "certifi-2025.4.26-py3-none-any.whl".into(),
                        },
                        Wheel {
                            url: Url::parse("https://files.pythonhosted.org/p/certifi-v2.whl").unwrap(),
                            hash: "sha256:xyz".into(), size: None,
                            filename: "certifi-2025.4.26-py3-none-any.whl".into(),
                        },
                    ],
                    metadata: None,
                },
            ],
        };

        let mut fixups_map = std::collections::BTreeMap::new();
        fixups_map.insert(
            PackageName::from_str("certifi").unwrap(),
            FixupConfig {
                top: FixupBody {
                    prefer_wheel: Some("sha256:xyz".into()),
                    ..Default::default()
                },
                cfg_sections: vec![],
            },
        );
        let fixups = crate::fixup::FixupSet::from_map_for_test(fixups_map);

        let input = build_emit_input(&config, &tree, &lockfile, None, Some(&fixups))
            .expect("build_emit_input with prefer_wheel");
        let pkg = &input.packages[0];
        let wheel = pkg.wheels.values().next().unwrap();
        assert_eq!(wheel.hash, "sha256:xyz", "prefer_wheel should override picker");
    }
```

- [ ] **Step 2: Implement wheel-side application**

In `src/buck/emit.rs`, find the wheel-handling code (~line 162 where `let wheels = wheel_index.get(&key).copied().unwrap_or(&[])` lives). Insert filter/prefer logic right after `let wheels = ...`:

```rust
            // Apply fixup wheel-side ops.
            let (wheels, prefer_overrides) = if let Some(rf) = &resolved_fixup {
                let filtered = apply_exclude_wheels(wheels, &rf.exclude_wheels);
                let prefer = rf.prefer_wheel.clone();
                (filtered, prefer)
            } else {
                (wheels.to_vec(), None)
            };
            let wheels: &[Wheel] = &wheels;
```

And add the helper, before `#[cfg(test)] mod tests`:

```rust
fn apply_exclude_wheels(wheels: &[Wheel], patterns: &[String]) -> Vec<Wheel> {
    if patterns.is_empty() {
        return wheels.to_vec();
    }
    let patterns: Vec<glob::Pattern> = patterns
        .iter()
        .filter_map(|p| glob::Pattern::new(p).ok())
        .collect();
    wheels
        .iter()
        .filter(|w| !patterns.iter().any(|p| p.matches(&w.filename)))
        .cloned()
        .collect()
}
```

Then in the `pick_wheel` branch, override the picker when `prefer_overrides` is set:

```rust
            // ---- Wheels-present path: pick the best wheel. ----
            let picked_wheel: Option<&Wheel> = if let Some(sha) = &prefer_overrides {
                let want = sha.trim_start_matches("sha256:");
                wheels.iter().find(|w| {
                    w.hash.trim_start_matches("sha256:") == want
                })
            } else {
                None
            };

            if let Some(wheel) = picked_wheel {
                pkg_wheels.entry(key.clone()).or_default().insert(
                    cfg_name.clone(),
                    EmitWheel {
                        url: wheel.url.to_string(),
                        hash: wheel.hash.clone(),
                    },
                );
                let mut cell_deps = pkg.deps.clone();
                if let Some(rf) = &resolved_fixup {
                    apply_dep_ops(&mut cell_deps, rf);
                }
                pkg_deps_per_cell.entry(key.clone()).or_default().insert(cfg_name.clone(), cell_deps);
                continue;
            }

            if prefer_overrides.is_some() {
                let available: Vec<String> = wheels.iter()
                    .map(|w| w.hash.trim_start_matches("sha256:").to_string())
                    .collect();
                anyhow::bail!(
                    "prefer_wheel sha256:{} not found for {} on cell {}.\n  available wheel shas: {}",
                    prefer_overrides.unwrap().trim_start_matches("sha256:"),
                    pkg.name,
                    cfg_name,
                    available.join(", "),
                );
            }

            // No prefer_wheel: fall through to existing pick_wheel logic.
            match pick_wheel(wheels, &compat) {
                PickResult::Picked { wheel, .. } => {
                    /* existing block */
                }
                PickResult::NoWheel => {
                    if !patterns_were_empty_for_this_pkg(&resolved_fixup) {
                        anyhow::bail!(
                            "exclude_wheels eliminates every wheel for {} on cell {}.\n  loosen the patterns or remove the fixup",
                            pkg.name, cfg_name,
                        );
                    }
                    /* existing NoWheel error */
                }
            }
```

Adjust the existing `match pick_wheel(...)` to use the local helper that distinguishes "no wheels in lockfile" from "exclude_wheels filtered them all out". A simple check: if `resolved_fixup.exclude_wheels.is_empty() == false` and `wheels.is_empty()` (post-filter), emit `ExcludeWheelsLeavesNone`-style error:

```rust
fn patterns_were_empty_for_this_pkg(
    rf: &Option<crate::fixup::ResolvedFixup>,
) -> bool {
    rf.as_ref().map(|r| r.exclude_wheels.is_empty()).unwrap_or(true)
}
```

Note: this is structural — when the wheels Vec is empty AFTER filtering BUT was non-empty before, that's the exclude_wheels error path. The current wheel-handling code checks `if wheels.is_empty()` for the sdist-only branch; that needs distinguishing. Easier: track the pre-filter count:

```rust
            let pre_filter_count = wheels.len();
            // [filter/prefer logic above produces `wheels: &[Wheel]`]
            let post_filter_count = wheels.len();
            let excluded_all = pre_filter_count > 0 && post_filter_count == 0;

            if excluded_all {
                anyhow::bail!(
                    "exclude_wheels eliminates every wheel for {} on cell {}.\n  loosen the patterns or remove the fixup",
                    pkg.name, cfg_name,
                );
            }
```

Put that check right after the filter block, BEFORE the existing `if wheels.is_empty() { /* sdist path */ }`. The integration of the two branches needs care:

```
wheels = wheel_index[key]
let pre_filter_count = wheels.len();
(wheels_filtered, prefer) = apply_fixup_filters(...);
let excluded_all = pre_filter_count > 0 && wheels_filtered.is_empty();
if excluded_all { bail!(ExcludeWheelsLeavesNone) }

if wheels_filtered.is_empty() {
    // sdist path (unchanged) — uses original wheel_index test
}
else if let Some(wheel) = prefer match in wheels_filtered { use it }
else if prefer.is_some() { bail!(PreferWheelNotFound) }
else { existing pick_wheel(wheels_filtered, compat) }
```

The implementer should adjust to this control flow.

- [ ] **Step 3: Run all buck::emit tests**

```bash
cargo test --lib buck::emit
```

Expected: PASS, including new `fixup_prefer_wheel_overrides_picker`.

- [ ] **Step 4: Commit**

```bash
git add src/buck/emit.rs
git commit -m "feat(s6): apply exclude_wheels + prefer_wheel in build_emit_input"
```

---

### Task 14: Apply overlay/visibility/labels/runtime_env/entry_points to EmitPackage

**Files:**
- Modify: `src/buck/emit.rs`

- [ ] **Step 1: Write the failing test**

Add to `src/buck/emit.rs` tests:

```rust
    #[test]
    fn fixup_visibility_and_entry_points_thread_to_emit_package() {
        // Construct a fixture: certifi has a fixup with visibility +
        // entry_points. Verify EmitPackage's fields are populated.
        use crate::config::{Config, Platform, PythonVersion, Tree};
        use crate::fixup::schema::{EntryPoints, FixupBody, FixupConfig};
        use crate::lock::types::{DepEdge, FirstPartyKind, Lockfile, Package, Source, Wheel};
        use pep440_rs::Version;
        use pep508_rs::PackageName;
        use url::Url;

        let tree = Tree {
            name: "default".into(),
            manifest_path: "pyproject.toml".into(),
            third_party_dir: "third-party/python".into(),
            python_versions: vec![PythonVersion(3, 12)],
        };
        let mut platforms = std::collections::BTreeMap::new();
        platforms.insert(
            "linux-x86_64-gnu".into(),
            Platform {
                target: "x86_64-unknown-linux-gnu".into(),
                manylinux: Some("2_17".into()),
                musllinux: None, macos_min: None,
            },
        );
        let config = Config {
            trees: vec![tree.clone()],
            platforms, fixups: Default::default(),
            buck: Default::default(), lockfile: Default::default(),
        };
        let lockfile = Lockfile {
            version: 1, revision: 3, requires_python: ">=3.12".into(),
            packages: vec![
                Package {
                    name: PackageName::from_str("app").unwrap(),
                    version: Version::from_str("0.1").unwrap(),
                    source: Source::FirstParty {
                        kind: FirstPartyKind::Virtual, path: ".".into(),
                    },
                    dependencies: vec![DepEdge {
                        name: PackageName::from_str("certifi").unwrap(),
                        extra: vec![], marker: None,
                    }],
                    sdist: None, wheels: vec![], metadata: None,
                },
                Package {
                    name: PackageName::from_str("certifi").unwrap(),
                    version: Version::from_str("2025.4.26").unwrap(),
                    source: Source::Registry {
                        url: Url::parse("https://pypi.org/simple").unwrap(),
                    },
                    dependencies: vec![], sdist: None,
                    wheels: vec![Wheel {
                        url: Url::parse("https://files.pythonhosted.org/p/certifi.whl").unwrap(),
                        hash: "sha256:abc".into(), size: None,
                        filename: "certifi-2025.4.26-py3-none-any.whl".into(),
                    }],
                    metadata: None,
                },
            ],
        };
        let mut fixups_map = std::collections::BTreeMap::new();
        let mut runtime_env = std::collections::BTreeMap::new();
        runtime_env.insert("FOO".into(), "bar".into());
        fixups_map.insert(
            PackageName::from_str("certifi").unwrap(),
            FixupConfig {
                top: FixupBody {
                    visibility: Some(vec!["//apps/imaging/...".into()]),
                    labels: vec!["security-sensitive".into()],
                    entry_points: Some(EntryPoints::Named(vec!["certifi-cli".into()])),
                    runtime_env,
                    ..Default::default()
                },
                cfg_sections: vec![],
            },
        );
        let fixups = crate::fixup::FixupSet::from_map_for_test(fixups_map);

        let input = build_emit_input(&config, &tree, &lockfile, None, Some(&fixups))
            .expect("build_emit_input with visibility");
        let pkg = &input.packages[0];
        assert_eq!(pkg.visibility.as_deref(), Some(&vec!["//apps/imaging/...".to_string()][..]));
        assert_eq!(pkg.labels, vec!["security-sensitive"]);
        assert_eq!(pkg.entry_points, vec!["certifi-cli"]);
        assert_eq!(pkg.runtime_env.get("FOO").map(|s| s.as_str()), Some("bar"));
    }

    /// Test fixture builder: returns the (tree, config, lockfile) tuple used
    /// by multiple fixup tests. Avoids per-test 60-line setups while keeping
    /// the harness in one place readers can audit.
    #[cfg(test)]
    fn build_certifi_harness()
        -> (crate::config::Tree, crate::config::Config, crate::lock::types::Lockfile)
    {
        use crate::config::{Config, Platform, PythonVersion, Tree};
        use crate::lock::types::{DepEdge, FirstPartyKind, Lockfile, Package, Source, Wheel};
        use pep440_rs::Version;
        use pep508_rs::PackageName;
        use url::Url;

        let tree = Tree {
            name: "default".into(),
            manifest_path: "pyproject.toml".into(),
            third_party_dir: "third-party/python".into(),
            python_versions: vec![PythonVersion(3, 12)],
        };
        let mut platforms = std::collections::BTreeMap::new();
        platforms.insert(
            "linux-x86_64-gnu".into(),
            Platform {
                target: "x86_64-unknown-linux-gnu".into(),
                manylinux: Some("2_17".into()),
                musllinux: None, macos_min: None,
            },
        );
        let config = Config {
            trees: vec![tree.clone()],
            platforms, fixups: Default::default(),
            buck: Default::default(), lockfile: Default::default(),
        };
        let lockfile = Lockfile {
            version: 1, revision: 3, requires_python: ">=3.12".into(),
            packages: vec![
                Package {
                    name: PackageName::from_str("app").unwrap(),
                    version: Version::from_str("0.1").unwrap(),
                    source: Source::FirstParty {
                        kind: FirstPartyKind::Virtual, path: ".".into(),
                    },
                    dependencies: vec![DepEdge {
                        name: PackageName::from_str("certifi").unwrap(),
                        extra: vec![], marker: None,
                    }],
                    sdist: None, wheels: vec![], metadata: None,
                },
                Package {
                    name: PackageName::from_str("certifi").unwrap(),
                    version: Version::from_str("2025.4.26").unwrap(),
                    source: Source::Registry {
                        url: Url::parse("https://pypi.org/simple").unwrap(),
                    },
                    dependencies: vec![], sdist: None,
                    wheels: vec![Wheel {
                        url: Url::parse("https://files.pythonhosted.org/p/certifi.whl").unwrap(),
                        hash: "sha256:abc".into(), size: None,
                        filename: "certifi-2025.4.26-py3-none-any.whl".into(),
                    }],
                    metadata: None,
                },
            ],
        };
        (tree, config, lockfile)
    }

    #[test]
    fn fixup_entry_points_auto_errors() {
        use crate::fixup::schema::{EntryPoints, FixupBody, FixupConfig};
        use pep508_rs::PackageName;

        let (tree, config, lockfile) = build_certifi_harness();
        let mut fixups_map = std::collections::BTreeMap::new();
        fixups_map.insert(
            PackageName::from_str("certifi").unwrap(),
            FixupConfig {
                top: FixupBody {
                    entry_points: Some(EntryPoints::Auto(true)),
                    ..Default::default()
                },
                cfg_sections: vec![],
            },
        );
        let fixups = crate::fixup::FixupSet::from_map_for_test(fixups_map);

        let err = build_emit_input(&config, &tree, &lockfile, None, Some(&fixups)).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("entry_points = true is not supported"), "got: {}", msg);
    }
```

The previous test (`fixup_visibility_and_entry_points_thread_to_emit_package`) should be refactored to use `build_certifi_harness()` for consistency — replace its tree/config/lockfile setup block with a single `let (tree, config, lockfile) = build_certifi_harness();` and keep only the fixup-building lines + assertions.

- [ ] **Step 2: Wire fixup → EmitPackage population**

In `src/buck/emit.rs`, find the `packages.push(EmitPackage { ... })` block (~line 306). Replace it to consume a per-package resolved fixup. Since fixups vary per cell, we need to either:
  (a) Pick the fixup from one canonical cell (the first one), OR
  (b) Merge top-level + every section-that-fires-for-any-cell.

S6 uses (a) — the top-level body is cell-independent, so we resolve from the FIRST cell only and use its scalar fields (visibility, labels, entry_points, runtime_env, overlay). This matches what users mean when they write a top-level fixup.

Add right before `packages.push(...)`:

```rust
        // For per-package scalar fields (visibility/labels/entry_points/overlay/runtime_env),
        // use the resolved fixup from the FIRST cell (cells differ in section-firing but
        // top-level body is cell-independent). For lists/maps, top-level survives.
        let first_cell_rf: Option<crate::fixup::ResolvedFixup> = if let Some(fs) = fixups {
            let name_str = &key.0;
            let pkg_name = pep508_rs::PackageName::from_str(name_str).ok();
            pkg_name.and_then(|n| fs.get(&n)).map(|fc| {
                // Resolve against the first cell of the configs vec.
                let first_cfg = &configs[0];
                let s_cfg = first_cfg.as_str();
                let (py_str, plat_name) = match s_cfg.split_once('-') {
                    Some((p, r)) => (p.trim_start_matches("py"), r),
                    None => ("", ""),
                };
                let py_str_dotted = format!("{}.{}", &py_str[0..1], &py_str[1..]);
                let plat = config.platforms.get(plat_name).expect("platform present");
                let (arch, os, env) = crate::fixup::cfg::split_target_triple(&plat.target);
                let pkg_ver = Version::from_str(&key.1).expect("valid version");
                let py_ver = Version::from_str(&py_str_dotted).expect("valid python version");
                let ctx = crate::fixup::CfgContext {
                    package_version: &pkg_ver,
                    python_version: py_ver,
                    target_os: &os,
                    target_arch: &arch,
                    target_env: &env,
                };
                crate::fixup::resolve_for_cell(fc, &ctx)
            })
        } else {
            None
        };

        // Validate entry_points form + Buck targets.
        let entry_points_vec: Vec<String> = match &first_cell_rf {
            Some(rf) => match &rf.entry_points {
                Some(crate::fixup::EntryPoints::Auto(true)) => {
                    anyhow::bail!(
                        "entry_points = true is not supported in v1; list the binaries explicitly (e.g. entry_points = [\"ruff\"]).\n  package: {}",
                        key.0
                    );
                }
                Some(crate::fixup::EntryPoints::Auto(false)) | None => vec![],
                Some(crate::fixup::EntryPoints::Named(names)) => names.clone(),
            },
            None => vec![],
        };

        // Validate extra/replace targets.
        if let Some(rf) = &first_cell_rf {
            for ed in &rf.extra_deps {
                if !crate::fixup::is_valid_buck_target(ed) {
                    anyhow::bail!(
                        "extra_deps target for {} is not a valid Buck target: `{}`\n  expected //path:name or :name form",
                        key.0, ed
                    );
                }
            }
            for (_, target) in &rf.replace_deps {
                if !crate::fixup::is_valid_buck_target(target) {
                    anyhow::bail!(
                        "replace_deps target for {} is not a valid Buck target: `{}`\n  expected //path:name or :name form",
                        key.0, target
                    );
                }
            }
        }

        // Overlay file discovery.
        let overlay_emit: Option<EmitOverlay> = if let Some(rf) = &first_cell_rf {
            if let Some(overlay_rel) = &rf.overlay {
                let fixup_dir = std::path::Path::new(&tree.third_party_dir)
                    .join("fixups").join(&key.0);
                let third_party_dir = std::path::Path::new(&tree.third_party_dir);
                let files = crate::fixup::discover_overlay_files(
                    &key.0,
                    third_party_dir,
                    &fixup_dir,
                    overlay_rel,
                )?;
                Some(EmitOverlay { files })
            } else { None }
        } else { None };

        packages.push(EmitPackage {
            name: key.0.clone(),
            version: key.1.clone(),
            deps,
            wheels: wheel_map,
            overlay: overlay_emit,
            entry_points: entry_points_vec,
            visibility: first_cell_rf.as_ref().and_then(|r| r.visibility.clone()),
            labels: first_cell_rf.as_ref().map(|r| r.labels.clone()).unwrap_or_default(),
            runtime_env: first_cell_rf.as_ref().map(|r| r.runtime_env.clone()).unwrap_or_default(),
        });
```

Note: `key.0` is moved by the existing code on lines 307. Change the existing line `name: key.0,` to `name: key.0.clone(),` (and same for version) so the value is reusable in the overlay-discover block above. The above code already uses `.clone()` patterns.

- [ ] **Step 3: Run new tests + regression**

```bash
cargo test --lib buck::emit::tests::fixup_visibility_and_entry_points_thread_to_emit_package
cargo test --lib buck::emit::tests::fixup_entry_points_auto_errors
cargo test --lib buck::
```

Expected: all PASS.

- [ ] **Step 4: Commit**

```bash
git add src/buck/emit.rs
git commit -m "feat(s6): thread overlay/visibility/labels/runtime_env/entry_points to EmitPackage"
```

---

### Task 15: `pypi_package` macro — overlay genrule + python_binary branches

**Files:**
- Modify: `src/buck/string_writer.rs`

- [ ] **Step 1: Write the failing test**

Add to `src/buck/string_writer.rs` `mod tests`:

```rust
    #[test]
    fn macro_emits_overlay_genrule_when_present() {
        use crate::buck::emit::{EmitDeps, EmitInput, EmitOverlay, EmitPackage, EmitWheel, ConfigName};
        use std::collections::BTreeMap;

        let mut wheels = BTreeMap::new();
        wheels.insert(
            ConfigName::new("3.12", "linux-x86_64-gnu"),
            EmitWheel {
                url: "https://example.com/pillow-10.whl".into(),
                hash: "sha256:abc".into(),
            },
        );
        let input = EmitInput {
            tree: "default".into(),
            third_party_dir: "third-party/python".into(),
            configs: vec![ConfigName::new("3.12", "linux-x86_64-gnu")],
            packages: vec![EmitPackage {
                name: "fake-pillow".into(),
                version: "10.0.0".into(),
                deps: EmitDeps::Uniform(vec![]),
                wheels,
                overlay: Some(EmitOverlay {
                    files: vec![("PIL/_imaging.py".into(), "fixups/fake-pillow/overlay/PIL/_imaging.py".into())],
                }),
                entry_points: vec![],
                visibility: None,
                labels: vec![],
                runtime_env: BTreeMap::new(),
            }],
        };
        let emitter = StringTemplateEmitter;
        let out = emitter.emit(&input);
        assert!(out.muntjac_bzl.contains("native.genrule"),
            "expected genrule for overlay; got:\n{}", out.muntjac_bzl);
        assert!(out.muntjac_bzl.contains("unzip"),
            "expected unzip command in genrule");
        assert!(out.muntjac_bzl.contains("zip -qrX"),
            "expected zip -qrX in genrule");
    }

    #[test]
    fn macro_emits_python_binary_when_entry_points_set() {
        use crate::buck::emit::{EmitDeps, EmitInput, EmitPackage, EmitWheel, ConfigName};
        use std::collections::BTreeMap;

        let mut wheels = BTreeMap::new();
        wheels.insert(
            ConfigName::new("3.12", "linux-x86_64-gnu"),
            EmitWheel {
                url: "https://example.com/ruff.whl".into(),
                hash: "sha256:abc".into(),
            },
        );
        let input = EmitInput {
            tree: "default".into(),
            third_party_dir: "third-party/python".into(),
            configs: vec![ConfigName::new("3.12", "linux-x86_64-gnu")],
            packages: vec![EmitPackage {
                name: "ruff".into(),
                version: "0.4.1".into(),
                deps: EmitDeps::Uniform(vec![]),
                wheels,
                overlay: None,
                entry_points: vec!["ruff".into()],
                visibility: None,
                labels: vec![],
                runtime_env: BTreeMap::new(),
            }],
        };
        let emitter = StringTemplateEmitter;
        let out = emitter.emit(&input);
        assert!(out.muntjac_bzl.contains("native.python_binary"),
            "expected python_binary rule; got:\n{}", out.muntjac_bzl);
        assert!(out.muntjac_bzl.contains("ruff.__main__"),
            "expected ruff.__main__ as main_module");
    }
```

- [ ] **Step 2: Modify the `pypi_package` macro signature in `string_writer.rs`**

Locate the `def pypi_package(name, version, wheels, deps = [], visibility = None, **kwargs):` line (~line 144). Replace with:

```rust
    writeln!(
        s,
        "def pypi_package(name, version, wheels, deps = [], visibility = None, labels = [], overlay_files = [], entry_points = [], runtime_env = {{}}, **kwargs):"
    )
    .unwrap();
```

- [ ] **Step 3: Add the overlay genrule branch**

Inside the macro body — after the `seen_sources`/`src_targets` loop emits each `http_file`/`export_file` rule (~line 206, the blank line after `native.export_file(...)`'s closing block), insert the overlay genrule branch. The conceptual placement: AFTER source-target rules, BEFORE the per-cell `prebuilt_python_library` loop.

```rust
    writeln!(s, "    # Overlay (S6): if overlay_files non-empty, build an `__overlaid` rule").unwrap();
    writeln!(s, "    # that unzips the wheel, copies overlay sources in, and re-zips.").unwrap();
    writeln!(s, "    overlay_label = None").unwrap();
    writeln!(s, "    if overlay_files:").unwrap();
    writeln!(s, "        # Each src target has its own overlaid output").unwrap();
    writeln!(s, "        for cfg, src_target in sorted(src_targets.items()):").unwrap();
    writeln!(s, "            if cfg in src_targets and src_targets[cfg] == seen_sources.get((wheels[cfg][0], wheels[cfg][1].removeprefix(\"sha256:\")), \"\"):").unwrap();
    writeln!(s, "                pass").unwrap();
    writeln!(s, "        # Single overlaid output (overlay is package-scoped, not cfg-scoped).").unwrap();
    writeln!(s, "        first_src = sorted(src_targets.values())[0]").unwrap();
    writeln!(s, "        cp_lines = []").unwrap();
    writeln!(s, "        for (path_in_wheel, src_label) in overlay_files:").unwrap();
    writeln!(s, "            parent = path_in_wheel.rsplit(\"/\", 1)[0] if \"/\" in path_in_wheel else \".\"").unwrap();
    writeln!(s, "            cp_lines.append(\"mkdir -p _u/\" + parent + \" && cp $(location \" + src_label + \") _u/\" + path_in_wheel)").unwrap();
    writeln!(s, "        cp_cmds = \" && \".join(cp_lines)").unwrap();
    writeln!(s, "        cmd_template = (").unwrap();
    writeln!(s, "            \"set -e && mkdir _u && cd _u && unzip -q $(location :\" + first_src + \") && cd .. && \" + ").unwrap();
    writeln!(s, "            cp_cmds + \" && cd _u && zip -qrX ../$OUT . -x '*/RECORD'\"").unwrap();
    writeln!(s, "        )").unwrap();
    writeln!(s, "        native.genrule(").unwrap();
    writeln!(s, "            name = \"{{}}-{{}}__overlaid\".format(name, version),").unwrap();
    writeln!(s, "            srcs = [\":\" + first_src] + [src for (_, src) in overlay_files],").unwrap();
    writeln!(s, "            out = \"{{}}-{{}}-overlaid.whl\".format(name, version),").unwrap();
    writeln!(s, "            cmd = cmd_template,").unwrap();
    writeln!(s, "            visibility = [],").unwrap();
    writeln!(s, "        )").unwrap();
    writeln!(s, "        overlay_label = \":{{}}-{{}}__overlaid\".format(name, version)").unwrap();
    writeln!(s).unwrap();
```

Then change the per-cell `prebuilt_python_library` binary_src line to:

```rust
    writeln!(s, "            binary_src = overlay_label or \":{{}}\".format(src_targets[cfg]),").unwrap();
```

- [ ] **Step 4: Add the python_binary branch**

After the existing top-level alias (where `name = name, actual = ":{}-{}.format(name, version)"` emits), add an entry_points loop:

```rust
    writeln!(s).unwrap();
    writeln!(s, "    # Entry points (S6): one python_binary + convenience alias per name").unwrap();
    writeln!(s, "    for ep_name in entry_points:").unwrap();
    writeln!(s, "        importable = name.replace(\"-\", \"_\")").unwrap();
    writeln!(s, "        native.python_binary(").unwrap();
    writeln!(s, "            name = \"{{}}-{{}}__bin-{{}}\".format(name, version, ep_name),").unwrap();
    writeln!(s, "            main_module = importable + \".__main__\",").unwrap();
    writeln!(s, "            deps = [\":\" + name],").unwrap();
    writeln!(s, "            env = runtime_env,").unwrap();
    writeln!(s, "            visibility = visibility or [\"PUBLIC\"],").unwrap();
    writeln!(s, "            labels = labels,").unwrap();
    writeln!(s, "        )").unwrap();
    writeln!(s, "        native.alias(").unwrap();
    writeln!(s, "            name = ep_name,").unwrap();
    writeln!(s, "            actual = \":{{}}-{{}}__bin-{{}}\".format(name, version, ep_name),").unwrap();
    writeln!(s, "            visibility = visibility or [\"PUBLIC\"],").unwrap();
    writeln!(s, "        )").unwrap();
```

- [ ] **Step 5: Modify the per-package `pypi_package(...)` call-site in the rendered BUCK output to pass new kwargs**

In `string_writer.rs`, find where individual `pypi_package(...)` calls are emitted (in `emit_buck` or equivalent — the function that writes the package-level BUCK file invoking `pypi_package(...)` per package). Locate the section emitting:

```
pypi_package(
    name = "<pkg>",
    version = "<ver>",
    wheels = { ... },
    deps = [...],
)
```

Add the new kwargs after `deps`:

```rust
    if !pkg.overlay.as_ref().map(|o| o.files.is_empty()).unwrap_or(true) {
        writeln!(s, "    overlay_files = [").unwrap();
        let overlay = pkg.overlay.as_ref().unwrap();
        for (in_wheel, src_rel) in &overlay.files {
            writeln!(s, "        (\"{}\", \"{}\"),", in_wheel, src_rel).unwrap();
        }
        writeln!(s, "    ],").unwrap();
    }
    if !pkg.entry_points.is_empty() {
        writeln!(s, "    entry_points = [").unwrap();
        for ep in &pkg.entry_points {
            writeln!(s, "        \"{}\",", ep).unwrap();
        }
        writeln!(s, "    ],").unwrap();
    }
    if let Some(vis) = &pkg.visibility {
        writeln!(s, "    visibility = [").unwrap();
        for v in vis {
            writeln!(s, "        \"{}\",", v).unwrap();
        }
        writeln!(s, "    ],").unwrap();
    }
    if !pkg.labels.is_empty() {
        writeln!(s, "    labels = [").unwrap();
        for l in &pkg.labels {
            writeln!(s, "        \"{}\",", l).unwrap();
        }
        writeln!(s, "    ],").unwrap();
    }
    if !pkg.runtime_env.is_empty() {
        writeln!(s, "    runtime_env = {{").unwrap();
        for (k, v) in &pkg.runtime_env {
            writeln!(s, "        \"{}\": \"{}\",", k, v).unwrap();
        }
        writeln!(s, "    }},").unwrap();
    }
```

- [ ] **Step 6: Run all writer tests**

```bash
cargo test --lib buck::string_writer
cargo test --lib buck::
```

Expected: all PASS, including the 2 new tests.

- [ ] **Step 7: Commit**

```bash
git add src/buck/string_writer.rs
git commit -m "feat(s6): pypi_package macro grows overlay genrule + python_binary branches"
```

---

### Task 16: Wire `load_local` into `cli/buckify.rs`

**Files:**
- Modify: `src/cli/buckify.rs`

- [ ] **Step 1: Write the failing test**

Skip — this is wiring, covered by the integration test in T19. The smoke test exercises this path.

- [ ] **Step 2: Modify `cli/buckify.rs` to load fixups before `build_emit_input`**

Replace the `let input = build_emit_input(&config, tree, &lockfile, manifest.as_ref(), None)?;` line with:

```rust
        let fixups = crate::fixup::load_local(&third_party_dir)
            .with_context(|| format!("loading fixups under {}", third_party_dir.display()))?;
        let input = build_emit_input(&config, tree, &lockfile, manifest.as_ref(), Some(&fixups))?;
```

(`third_party_dir` is already computed a few lines above.)

- [ ] **Step 3: Run the full test suite**

```bash
cargo test
```

Expected: all PASS. Existing fixture snapshots stay green because no fixups means `load_local` returns an empty `FixupSet` and behavior is unchanged.

- [ ] **Step 4: Commit**

```bash
git add src/cli/buckify.rs
git commit -m "feat(s6): wire load_local into muntjac buckify"
```

---

## Phase 6 — CLI command

### Task 17: `muntjac fixups show <pkg>` subcommand

**Files:**
- Create: `src/cli/fixups.rs`
- Modify: `src/cli/mod.rs`

- [ ] **Step 1: Write the failing test**

Create `tests/fixups_show_smoke.rs`:

```rust
//! Smoke test for `muntjac fixups show <pkg>`.

use std::fs;
use tempfile::TempDir;

fn muntjac_bin() -> std::path::PathBuf {
    let exe = env!("CARGO_BIN_EXE_muntjac");
    std::path::PathBuf::from(exe)
}

#[test]
fn fixups_show_prints_round_trippable_toml() {
    let tmp = TempDir::new().unwrap();

    // muntjac.toml + third-party tree
    fs::write(
        tmp.path().join("muntjac.toml"),
        r#"
[platforms.linux-x86_64-gnu]
target = "x86_64-unknown-linux-gnu"
manylinux = "2_17"

[[tree]]
name = "default"
manifest_path = "pyproject.toml"
third_party_dir = "third-party/python"
python_versions = ["3.12"]
"#,
    )
    .unwrap();

    fs::create_dir_all(tmp.path().join("third-party/python/fixups/pillow")).unwrap();
    let fixup_body = r#"extra_deps = ["//third-party/c:libjpeg"]
omit_deps = ["useless"]
"#;
    fs::write(
        tmp.path().join("third-party/python/fixups/pillow/fixups.toml"),
        fixup_body,
    )
    .unwrap();

    let out = std::process::Command::new(muntjac_bin())
        .args(["-C", tmp.path().to_str().unwrap(), "fixups", "show", "pillow"])
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr:\n{}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("//third-party/c:libjpeg"), "got:\n{}", stdout);
    assert!(stdout.contains("useless"), "got:\n{}", stdout);
}

#[test]
fn fixups_show_missing_package_exits_nonzero() {
    let tmp = TempDir::new().unwrap();
    fs::write(
        tmp.path().join("muntjac.toml"),
        r#"
[platforms.linux-x86_64-gnu]
target = "x86_64-unknown-linux-gnu"
manylinux = "2_17"

[[tree]]
name = "default"
manifest_path = "pyproject.toml"
third_party_dir = "third-party/python"
python_versions = ["3.12"]
"#,
    )
    .unwrap();
    fs::create_dir_all(tmp.path().join("third-party/python")).unwrap();

    let out = std::process::Command::new(muntjac_bin())
        .args(["-C", tmp.path().to_str().unwrap(), "fixups", "show", "nonexistent"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("no fixup for package"), "got:\n{}", stderr);
}
```

- [ ] **Step 2: Wire the subcommand into the CLI types**

In `src/cli/mod.rs`, replace the `Fixups` variant + its routing:

```rust
    /// Manage fixups (show).
    Fixups {
        #[command(subcommand)]
        op: fixups::FixupsOp,
    },
```

Add `pub mod fixups;` to the existing module declarations at the top of `src/cli/mod.rs`.

In the `run` function's match, replace `Command::Fixups => stub::run(...)` with:

```rust
        Command::Fixups { op } => fixups::run(op, &cli.globals),
```

- [ ] **Step 3: Implement `src/cli/fixups.rs`**

Create the file:

```rust
//! `muntjac fixups show <pkg>` — print the parsed-and-merged-from-disk
//! fixup for a package as canonical TOML.

use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Result, anyhow};
use clap::Subcommand;
use pep508_rs::PackageName;

use crate::cli::Globals;
use crate::config::Config;
use crate::fixup;

#[derive(Subcommand, Debug)]
pub enum FixupsOp {
    /// Print the merged fixup for a package as TOML.
    Show {
        /// PEP 503-normalizable package name.
        package: String,
    },
}

pub fn run(op: FixupsOp, globals: &Globals) -> Result<()> {
    match op {
        FixupsOp::Show { package } => show(package, globals),
    }
}

fn show(package: String, globals: &Globals) -> Result<()> {
    let cwd = globals.workdir().context("resolving working directory")?;
    let cfg_path = cwd.join("muntjac.toml");
    let cfg_bytes = std::fs::read_to_string(&cfg_path)
        .with_context(|| format!("reading {}", cfg_path.display()))?;
    let config = Config::from_str(&cfg_bytes)
        .with_context(|| format!("parsing {}", cfg_path.display()))?;

    // S6 v1: use the first tree's third_party_dir. Multi-tree fixup show is
    // future work (use --tree).
    let tree = config.trees.first().ok_or_else(|| anyhow!("no trees in muntjac.toml"))?;
    let third_party_dir = cwd.join(&tree.third_party_dir);

    let pkg_name = PackageName::from_str(&package)
        .with_context(|| format!("normalizing package name `{}`", package))?;

    let set = fixup::load_local(&third_party_dir)
        .with_context(|| format!("loading fixups under {}", third_party_dir.display()))?;

    let fixup_cfg = set.get(&pkg_name).ok_or_else(|| anyhow!(
        "no fixup for package `{}` at {}/fixups/",
        package, third_party_dir.display()
    ))?;

    let toml_out = fixup_cfg
        .to_toml_string()
        .context("re-emitting fixup as TOML")?;
    print!("{}", toml_out);
    Ok(())
}
```

- [ ] **Step 4: Run the smoke test**

```bash
cargo build
cargo test --test fixups_show_smoke
```

Expected: both PASS.

- [ ] **Step 5: Update `tests/help.rs` snapshot if present**

If `tests/help.rs` exists with a `main_help_surface` snapshot (per S5 history), the new `fixups` subcommand line will change the help output. Run with `INSTA_UPDATE=accept`:

```bash
INSTA_UPDATE=accept cargo test --test help 2>/dev/null || true
```

Inspect the diff (`cargo insta pending-snapshots`) before committing.

- [ ] **Step 6: Commit**

```bash
git add src/cli/fixups.rs src/cli/mod.rs tests/fixups_show_smoke.rs tests/snapshots 2>/dev/null
git commit -m "feat(s6): muntjac fixups show <pkg>"
```

---

## Phase 7 — Fixture & integration tests

### Task 18: `05-local-fixup` fixture

**Files:**
- Create: `tests/fixtures/buck/05-local-fixup/pyproject.toml`
- Create: `tests/fixtures/buck/05-local-fixup/muntjac.toml`
- Create: `tests/fixtures/buck/05-local-fixup/uv.lock`
- Create: `tests/fixtures/buck/05-local-fixup/third-party/python/fixups/fake-pillow/fixups.toml`
- Create: `tests/fixtures/buck/05-local-fixup/third-party/python/fixups/fake-pillow/overlay/fake_pillow/_paths.py`
- Create: `tests/fixtures/buck/05-local-fixup/expected/BUCK`
- Create: `tests/fixtures/buck/05-local-fixup/expected/muntjac.bzl`
- Create: `tests/fixtures/buck/05-local-fixup/expected/config/BUCK`
- Create: `tests/fixtures/buck/05-local-fixup/expected/wiring.bzl`

- [ ] **Step 1: Create fixture inputs**

`tests/fixtures/buck/05-local-fixup/pyproject.toml`:
```toml
[project]
name = "demo-app"
version = "0.1"
requires-python = ">=3.12"
dependencies = ["fake-pillow"]
```

`tests/fixtures/buck/05-local-fixup/muntjac.toml`:
```toml
[platforms.linux-x86_64-gnu]
target = "x86_64-unknown-linux-gnu"
manylinux = "2_17"

[platforms.macos-aarch64]
target = "aarch64-apple-darwin"
macos_min = "11.0"

[[tree]]
name = "default"
manifest_path = "pyproject.toml"
third_party_dir = "third-party/python"
python_versions = ["3.11", "3.12"]
```

`tests/fixtures/buck/05-local-fixup/uv.lock`:

A minimal uv.lock with `fake-pillow 1.0.0` resolved (pure-python wheel, py3-none-any) + dependency `useless-transitive` + dependency `typing-extensions`. Adjust based on lock format used in S4's `02-numpy-pandas` fixture (consult its uv.lock for the canonical schema). Synthesize accordingly:

```toml
version = 1
revision = 3
requires-python = ">=3.12"

[[package]]
name = "demo-app"
version = "0.1"
source = { virtual = "." }
dependencies = [
    { name = "fake-pillow" },
]

[[package]]
name = "fake-pillow"
version = "1.0.0"
source = { registry = "https://pypi.org/simple" }
dependencies = [
    { name = "useless-transitive" },
    { name = "typing-extensions" },
]
wheels = [
    { url = "https://files.pythonhosted.org/packages/fake/fake_pillow-1.0.0-py3-none-any.whl", hash = "sha256:abc123fake", size = 100 },
]

[[package]]
name = "useless-transitive"
version = "0.1.0"
source = { registry = "https://pypi.org/simple" }
wheels = [
    { url = "https://files.pythonhosted.org/packages/u/useless_transitive-0.1.0-py3-none-any.whl", hash = "sha256:deadbeef", size = 50 },
]

[[package]]
name = "typing-extensions"
version = "4.10.0"
source = { registry = "https://pypi.org/simple" }
wheels = [
    { url = "https://files.pythonhosted.org/packages/t/typing_extensions-4.10.0-py3-none-any.whl", hash = "sha256:eecafe", size = 70 },
]
```

`tests/fixtures/buck/05-local-fixup/third-party/python/fixups/fake-pillow/fixups.toml`:

```toml
extra_deps     = ["//third-party/c:libjpeg"]
omit_deps      = ["useless-transitive"]
replace_deps   = { typing-extensions = "//company/typing:te" }
overlay        = "overlay/"
entry_points   = ["fake-pillow-cli"]
visibility     = ["//apps/imaging/..."]
labels         = ["security-sensitive"]
runtime_env    = { LIBJPEG_PATH = "/opt/libjpeg/lib" }
exclude_wheels = ["*-cp311-*-macosx_*_arm64.*"]

["cfg(target_os = \"linux\")"]
extra_deps = ["//third-party/c:libssl"]

["cfg(all(version = \">=1.0\", python = \">=3.12\"))"]
labels = ["needs-mod-3.12-shim"]
```

`tests/fixtures/buck/05-local-fixup/third-party/python/fixups/fake-pillow/overlay/fake_pillow/_paths.py`:

```python
# Overridden by muntjac fixup to point libjpeg lookup at a controlled path.
LIBJPEG_PATH = "/opt/libjpeg/lib"
```

- [ ] **Step 2: Run buckify to generate expected outputs**

```bash
cd tests/fixtures/buck/05-local-fixup
cargo run --quiet -- buckify
```

Inspect the generated `third-party/python/BUCK`, `third-party/python/muntjac.bzl`, `third-party/python/config/BUCK`, `third-party/python/wiring.bzl`. Copy them to `expected/`:

```bash
cp third-party/python/BUCK expected/BUCK
cp third-party/python/muntjac.bzl expected/muntjac.bzl
mkdir -p expected/config
cp third-party/python/config/BUCK expected/config/BUCK
cp third-party/python/wiring.bzl expected/wiring.bzl
```

Manually inspect each file for sanity:
- `BUCK` should contain `pypi_package(name = "fake-pillow", ..., overlay_files = [...], entry_points = ["fake-pillow-cli"], visibility = ["//apps/imaging/..."], labels = ["security-sensitive", ...], runtime_env = {...})`.
- `muntjac.bzl` should contain the overlay genrule branch + entry_points loop.
- The deps list for fake-pillow should NOT contain `:useless-transitive` (omitted), should contain `//company/typing:te` (replaced), and should contain `//third-party/c:libjpeg` (extra).

- [ ] **Step 3: Commit the fixture**

```bash
git add tests/fixtures/buck/05-local-fixup/
git commit -m "test(s6): 05-local-fixup fixture inputs + expected outputs"
```

---

### Task 19: Snapshot test wiring (`tests/fixtures_local_fixup.rs`)

**Files:**
- Create: `tests/fixtures_local_fixup.rs`

- [ ] **Step 1: Write the failing test**

```rust
//! Snapshot test for the 05-local-fixup fixture.

use std::fs;
use std::path::Path;

fn run_buckify_and_compare(fixture_dir: &Path) {
    // Copy fixture inputs into a tempdir so buckify writes are isolated
    // from the committed fixture files.
    let tmp = tempfile::tempdir().unwrap();
    copy_dir_all(fixture_dir, tmp.path());

    let exe = env!("CARGO_BIN_EXE_muntjac");
    let out = std::process::Command::new(exe)
        .args(["-C", tmp.path().to_str().unwrap(), "buckify"])
        .output()
        .unwrap();
    assert!(out.status.success(),
        "buckify failed: {}\n{}",
        String::from_utf8_lossy(&out.stderr),
        String::from_utf8_lossy(&out.stdout),
    );

    let tpd = tmp.path().join("third-party/python");
    let expected = fixture_dir.join("expected");
    compare_file(&tpd.join("BUCK"), &expected.join("BUCK"));
    compare_file(&tpd.join("muntjac.bzl"), &expected.join("muntjac.bzl"));
    compare_file(&tpd.join("config/BUCK"), &expected.join("config/BUCK"));
    compare_file(&tpd.join("wiring.bzl"), &expected.join("wiring.bzl"));
}

fn compare_file(generated: &Path, expected: &Path) {
    let got = fs::read_to_string(generated)
        .unwrap_or_else(|e| panic!("read {}: {}", generated.display(), e));
    let want = fs::read_to_string(expected)
        .unwrap_or_else(|e| panic!("read {}: {}", expected.display(), e));
    assert_eq!(
        got, want,
        "{} differs from expected.\n--- got:\n{}\n--- want:\n{}\n",
        generated.display(), got, want,
    );
}

fn copy_dir_all(src: &Path, dst: &Path) {
    for entry in walkdir::WalkDir::new(src) {
        let entry = entry.unwrap();
        let rel = entry.path().strip_prefix(src).unwrap();
        let target = dst.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target).unwrap();
        } else if entry.file_type().is_file() {
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::copy(entry.path(), &target).unwrap();
        }
    }
}

#[test]
fn local_fixup_fixture_byte_matches() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/buck/05-local-fixup");
    run_buckify_and_compare(&fixture);
}
```

- [ ] **Step 2: Run the snapshot test**

```bash
cargo test --test fixtures_local_fixup
```

Expected: PASS. If anything mismatches, inspect the diff and decide:
- (a) Generated output is wrong → fix the emitter
- (b) Expected was wrong → regenerate via Task 18 step 2 and re-commit

- [ ] **Step 3: Commit**

```bash
git add tests/fixtures_local_fixup.rs
git commit -m "test(s6): byte-snapshot test for 05-local-fixup fixture"
```

---

### Task 20: `tests/fixups_smoke.rs` — end-to-end smoke

**Files:**
- Create: `tests/fixups_smoke.rs`

- [ ] **Step 1: Write the smoke test**

This is a separate, lighter test than fixtures_local_fixup. It checks `muntjac fixups show` round-trip behavior against an inline tempdir.

```rust
//! End-to-end smoke for muntjac fixups show + buckify with fixups.

use std::fs;
use tempfile::TempDir;

fn muntjac_bin() -> std::path::PathBuf {
    env!("CARGO_BIN_EXE_muntjac").into()
}

#[test]
fn buckify_emits_overlay_genrule_for_fixed_up_package() {
    let tmp = TempDir::new().unwrap();
    fs::write(tmp.path().join("muntjac.toml"), r#"
[platforms.linux-x86_64-gnu]
target = "x86_64-unknown-linux-gnu"
manylinux = "2_17"

[[tree]]
name = "default"
manifest_path = "pyproject.toml"
third_party_dir = "third-party/python"
python_versions = ["3.12"]
"#).unwrap();

    fs::write(tmp.path().join("pyproject.toml"), r#"
[project]
name = "demo"
version = "0.1"
requires-python = ">=3.12"
dependencies = ["fake-pillow"]
"#).unwrap();

    fs::write(tmp.path().join("uv.lock"), r#"version = 1
revision = 3
requires-python = ">=3.12"

[[package]]
name = "demo"
version = "0.1"
source = { virtual = "." }
dependencies = [{ name = "fake-pillow" }]

[[package]]
name = "fake-pillow"
version = "1.0.0"
source = { registry = "https://pypi.org/simple" }
wheels = [
    { url = "https://example.com/fake_pillow-1.0.0-py3-none-any.whl", hash = "sha256:abc", size = 100 },
]
"#).unwrap();

    // Overlay + fixup.
    fs::create_dir_all(tmp.path().join("third-party/python/fixups/fake-pillow/overlay/fake_pillow")).unwrap();
    fs::write(
        tmp.path().join("third-party/python/fixups/fake-pillow/fixups.toml"),
        r#"overlay = "overlay/"
extra_deps = ["//third-party/c:libjpeg"]
"#,
    ).unwrap();
    fs::write(
        tmp.path().join("third-party/python/fixups/fake-pillow/overlay/fake_pillow/_paths.py"),
        "LIBJPEG_PATH = \"/opt/libjpeg/lib\"\n",
    ).unwrap();

    let out = std::process::Command::new(muntjac_bin())
        .args(["-C", tmp.path().to_str().unwrap(), "buckify"])
        .output()
        .unwrap();
    assert!(out.status.success(),
        "buckify failed:\n{}",
        String::from_utf8_lossy(&out.stderr));

    let bzl = fs::read_to_string(tmp.path().join("third-party/python/muntjac.bzl")).unwrap();
    assert!(bzl.contains("native.genrule"), "expected overlay genrule in muntjac.bzl");

    let buck = fs::read_to_string(tmp.path().join("third-party/python/BUCK")).unwrap();
    assert!(buck.contains("//third-party/c:libjpeg"), "expected extra_deps in BUCK");
    assert!(buck.contains("overlay_files"), "expected overlay_files kwarg in BUCK");
}
```

- [ ] **Step 2: Run the test**

```bash
cargo test --test fixups_smoke
```

Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add tests/fixups_smoke.rs
git commit -m "test(s6): end-to-end smoke for buckify + overlay"
```

---

## Phase 8 — CI & bookkeeping

### Task 21: CI buck2 smoke step

**Files:**
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Add the buck2 build step**

Locate the existing S5 `buck2 build` step (added in CI run 26335900104, commit 49d308f). Add a sibling step gated on `ubuntu-latest`:

```yaml
      - name: buck2 build 05-local-fixup
        if: matrix.runner == 'ubuntu-latest'
        run: |
          cd tests/fixtures/buck/05-local-fixup
          buck2 build //third-party/python:fake-pillow
```

- [ ] **Step 2: Inspect the workflow file locally for syntax**

```bash
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))"
```

Expected: no errors.

- [ ] **Step 3: Commit & push**

```bash
git add .github/workflows/ci.yml
git commit -m "ci(s6): add buck2 build for 05-local-fixup fixture"
git push
```

Wait for CI to go green on all 3 runners.

---

### Task 22: TECH_DEBT + roadmap update

**Files:**
- Modify: `docs/superpowers/TECH_DEBT.md`
- Modify: `docs/superpowers/specs/2026-05-20-muntjac-roadmap.md`

- [ ] **Step 1: Append TECH_DEBT entries**

Open `docs/superpowers/TECH_DEBT.md` and append (after the last existing S5 entry):

```markdown
---

## S6 review findings

### TD-S6-01: Overlay genrule strips PEP 427 RECORD

- **Source:** S6 spec §5.3
- **Severity:** Minor
- **What:** The overlay genrule emits `zip -qrX ../$OUT . -x '*/RECORD'`, which drops the wheel's RECORD file. The resulting wheel passes Buck's `prebuilt_python_library` because Buck doesn't verify RECORD.
- **Why:** Overlaying invalidates RECORD's sha256 lines. Regenerating RECORD properly requires a small Python script (or built-in tool).
- **Fix:** Add a tool that walks the unpacked wheel post-overlay and rewrites `*.dist-info/RECORD` with fresh sha256/size.
- **Target:** post-launch (when a downstream tool starts caring)

### TD-S6-02: `entry_points = true` auto-discovery

- **Source:** S6 spec §1.2, §5.4
- **Severity:** Minor
- **What:** The shorthand `entry_points = true` parses but errors at apply with a pointer to the explicit-list form.
- **Why:** uv.lock doesn't carry entry-point metadata, and downloading wheels at buckify time cuts against buckify's no-network property.
- **Fix:** Either (a) extend vendor mode to scrape wheel `entry_points.txt`, or (b) emit a Buck genrule that extracts metadata at build time.
- **Target:** post-launch

### TD-S6-03: `python_binary` entry-points that aren't `<pkg>.__main__`

- **Source:** S6 spec §5.4
- **Severity:** Minor
- **What:** v1 emits `main_module = "<importable_pkg>.__main__"`. Entry points that map to a different `module:function` aren't supported.
- **Why:** Mapping name → module:function requires reading wheel metadata (same problem as TD-S6-02). The `__main__` convention covers most well-formed Python tools.
- **Fix:** When entry-points metadata is available, emit a small shim main module per entry point.
- **Target:** post-launch
```

- [ ] **Step 2: Mark S6 ✅ shipped in roadmap**

In `docs/superpowers/specs/2026-05-20-muntjac-roadmap.md`, locate the `### S6 — Local fixups` section. Update the header and add a "Shipped" footer line referencing this plan and the S6 spec:

```markdown
### S6 — Local fixups  ✅ shipped

[existing scope/exit/demo/touches content unchanged]

**Shipped:** [N] commits; plan `docs/superpowers/plans/2026-05-23-muntjac-s6-local-fixups.md`; design `docs/superpowers/specs/2026-05-23-muntjac-s6-local-fixups-design.md`.
```

`[N]` is filled in at commit time — count via `git log --oneline | grep -c '(s6)'`.

- [ ] **Step 3: Commit**

```bash
git add docs/superpowers/TECH_DEBT.md docs/superpowers/specs/2026-05-20-muntjac-roadmap.md
git commit -m "docs(s6): TECH_DEBT entries + mark S6 ✅ shipped"
```

- [ ] **Step 4: Tag**

```bash
git tag s6-complete
git push --tags
```

---

## Summary

**Tasks:** 22.
**Phases:** 1 (foundation) → 2 (schema) → 3 (cfg grammar) → 4 (resolution) → 5 (emitter) → 6 (CLI) → 7 (fixture + tests) → 8 (CI + bookkeeping).

**Test coverage at exit:**
- Unit tests: ~50 across schema, cfg, layer, loader, validate, overlay, error.
- Snapshot test: 05-local-fixup fixture byte-equal to committed expected/.
- Smoke tests: `fixups_show_smoke.rs`, `fixups_smoke.rs`.
- CI buck2 smoke: ubuntu-latest builds the overlay-genrule end-to-end.

**Deferred to follow-ups:** PEP 427 RECORD regeneration after overlay, `entry_points = true` auto-discovery, non-`__main__` entry points.
