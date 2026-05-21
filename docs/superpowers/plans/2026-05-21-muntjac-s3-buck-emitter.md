# Muntjac S3 — First BUCK emitter — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn S2's per-cell wheel picks into a deterministic four-file BUCK output set (`BUCK`, `muntjac.bzl`, `config/BUCK`, `PACKAGE`) for single-platform × N-pythons pure-python projects, behind a swappable `BuckEmitter` trait.

**Architecture:** New `src/buck/` module with three files: `emit.rs` (types + trait + pipeline composer), `string_writer.rs` (v1 hand-rolled `writeln!`-style emitter), `write.rs` (atomic filesystem writer). New `src/cli/buckify.rs` wires the existing `Buckify` CLI command to the new pipeline. Determinism enforced by `BTreeMap` everywhere + explicit `sort` calls + snapshot tests via `insta`.

**Tech Stack:** Rust 2024 edition (pinned in `Cargo.toml`), `anyhow` for handler errors, `serde`/`toml` for config, `insta` for snapshot tests, `assert_cmd` + `tempfile` for integration tests.

---

## File structure

**New files:**

| Path | Responsibility |
|---|---|
| `src/buck/mod.rs` | Re-exports `BuckEmitter`, `StringTemplateEmitter`, `EmitInput`, etc. |
| `src/buck/emit.rs` | `EmitInput`, `EmitPackage`, `EmitWheel`, `ConfigName`, `EmitOutput`, `BuckEmitter` trait. Plus pipeline-composer functions to build `EmitInput` from `Config + Lockfile`. |
| `src/buck/string_writer.rs` | `StringTemplateEmitter` impl of `BuckEmitter`. Hand-rolled `writeln!()` emission of all four files. |
| `src/buck/write.rs` | Atomic file writer: takes `EmitOutput` + target dir; creates dirs; writes via `<file>.tmp` + rename. |
| `src/cli/buckify.rs` | `run(args, globals)` handler: loads config, iterates trees, calls `emit.rs` composer, `string_writer.rs` emitter, `write.rs` writer. |
| `tests/fixtures/buck/README.md` | Frozen-artifact convention (matches S1/S2 fixture READMEs). |
| `tests/fixtures/buck/01-pure-python/` | Fixture: `muntjac.toml`, `pyproject.toml`, `uv.lock`, `expected/{BUCK, muntjac.bzl, PACKAGE, config/BUCK}`. |
| `tests/fixtures/buck/10-determinism/` | Reuses 01's inputs; integration test asserts byte-identical output across two runs. |
| `tests/buckify.rs` | Integration tests for fixtures 01 and 10. |

**Modified files:**

| Path | Change |
|---|---|
| `src/wheel/tag.rs` | `impl Display` for `Tag`/`PythonTag`/`AbiTag`/`PlatformTag`. |
| `src/wheel/compat.rs` | Delete `render_*` test helpers; snapshot tests format via `tag.to_string()`. |
| `src/cli/debug/pick_wheels.rs` | Delete `render_tag` + helpers; consume `tag.to_string()`. |
| `src/cli/init.rs` | Extract `MIN_SUPPORTED_PY_MINOR` constant + doc comment on the Python-version-floor logic. |
| `src/config.rs` | Add `#[serde(default)]` on `RawConfig::platforms` + `MissingField("platforms")` check on empty. |
| `src/cli/mod.rs` | Wire `Command::Buckify` to `buckify::run` (was `stub::run`). |
| `src/lib.rs` | `pub mod buck;` declaration. |
| `docs/superpowers/TECH_DEBT.md` | Move three resolved items to Resolved section. |
| `docs/superpowers/specs/2026-05-20-muntjac-roadmap.md` | Mark S3 ✅ shipped. |

---

## Phase 1 — Tech-debt prerequisites

These land first because later code consumes `Display for Tag` and the friendlier config errors.

### Task 1: `Display` for `Tag` (consolidate `render_tag`)

**Files:**
- Modify: `src/wheel/tag.rs`
- Modify: `src/cli/debug/pick_wheels.rs` (delete `render_tag` + helpers; use `tag.to_string()`)
- Modify: `src/wheel/compat.rs` tests module (delete `render_*` helpers; use `tag.to_string()`)
- May regenerate: `src/wheel/snapshots/*.snap`

- [ ] **Step 1: Write the failing test**

In `src/wheel/tag.rs::tests`:

```rust
#[test]
fn tag_display_canonical_cases() {
    let cp312 = Tag {
        python: PythonTag::CPython(3, 12),
        abi:    AbiTag::CPython(3, 12),
        plat:   PlatformTag::ManyLinux { major: 2, minor: 17, arch: LinuxArch::X86_64 },
    };
    assert_eq!(cp312.to_string(), "cp312-cp312-manylinux_2_17_x86_64");

    let abi3 = Tag {
        python: PythonTag::CPython(3, 12),
        abi:    AbiTag::Abi3,
        plat:   PlatformTag::MacOs { major: 11, minor: 0, arch: MacArch::Universal2 },
    };
    assert_eq!(abi3.to_string(), "cp312-abi3-macosx_11_0_universal2");

    let pure = Tag {
        python: PythonTag::Py(3, None),
        abi:    AbiTag::None,
        plat:   PlatformTag::Any,
    };
    assert_eq!(pure.to_string(), "py3-none-any");

    let py_minor = Tag {
        python: PythonTag::Py(3, Some(7)),
        abi:    AbiTag::None,
        plat:   PlatformTag::Any,
    };
    assert_eq!(py_minor.to_string(), "py37-none-any");

    let other = Tag {
        python: PythonTag::Other("pp310".into()),
        abi:    AbiTag::Other("pypy310_pp73".into()),
        plat:   PlatformTag::MuslLinux { major: 1, minor: 2, arch: LinuxArch::Aarch64 },
    };
    assert_eq!(other.to_string(), "pp310-pypy310_pp73-musllinux_1_2_aarch64");
}
```

- [ ] **Step 2: Run — verify fails**

Run: `cargo test --lib wheel::tag::tests::tag_display_canonical_cases`
Expected: FAIL — `Display` not implemented.

- [ ] **Step 3: Implement `Display` for `Tag` + components**

Add to `src/wheel/tag.rs`:

```rust
use std::fmt::{self, Display, Formatter};

impl Display for Tag {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}-{}", self.python, self.abi, self.plat)
    }
}

impl Display for PythonTag {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            PythonTag::CPython(maj, min) => write!(f, "cp{maj}{min}"),
            PythonTag::Py(maj, Some(min)) => write!(f, "py{maj}{min}"),
            PythonTag::Py(maj, None) => write!(f, "py{maj}"),
            PythonTag::Other(s) => write!(f, "{s}"),
        }
    }
}

impl Display for AbiTag {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            AbiTag::CPython(maj, min) => write!(f, "cp{maj}{min}"),
            AbiTag::Abi3 => write!(f, "abi3"),
            AbiTag::None => write!(f, "none"),
            AbiTag::Other(s) => write!(f, "{s}"),
        }
    }
}

impl Display for PlatformTag {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            PlatformTag::Any => write!(f, "any"),
            PlatformTag::ManyLinux { major, minor, arch } => {
                let a = match arch {
                    LinuxArch::X86_64 => "x86_64",
                    LinuxArch::Aarch64 => "aarch64",
                };
                write!(f, "manylinux_{major}_{minor}_{a}")
            }
            PlatformTag::MuslLinux { major, minor, arch } => {
                let a = match arch {
                    LinuxArch::X86_64 => "x86_64",
                    LinuxArch::Aarch64 => "aarch64",
                };
                write!(f, "musllinux_{major}_{minor}_{a}")
            }
            PlatformTag::MacOs { major, minor, arch } => {
                let a = match arch {
                    MacArch::X86_64 => "x86_64",
                    MacArch::Arm64 => "arm64",
                    MacArch::Universal2 => "universal2",
                };
                write!(f, "macosx_{major}_{minor}_{a}")
            }
            PlatformTag::Other(s) => write!(f, "{s}"),
        }
    }
}
```

- [ ] **Step 4: Verify the new test passes**

Run: `cargo test --lib wheel::tag::tests::tag_display_canonical_cases`
Expected: PASS.

- [ ] **Step 5: Delete `render_tag` in `src/cli/debug/pick_wheels.rs`**

Find the `render_tag(t: &Tag) -> String` function and its `render_python`/`render_abi`/`render_platform` helpers at the bottom of the file. Delete them. Replace each call site (probably `render_tag(&matched_tag)`) with `matched_tag.to_string()`.

If a `use crate::wheel::Tag;` import is now unused, remove it (but `Tag` may still be used in the `pick_wheel` return type — check before deleting).

- [ ] **Step 6: Delete `render_*` helpers in `src/wheel/compat.rs::tests`**

In the `#[cfg(test)] mod tests { ... }` block of `src/wheel/compat.rs`, find `render_tag`, `render_python`, `render_abi`, `render_platform` (used by the snapshot tests). Replace `format_compat_for_snapshot` (or whatever it's named) to call `t.to_string()` directly:

```rust
fn format_compat_for_snapshot(compat: &CompatibleTags) -> String {
    compat.ordered().iter()
        .enumerate()
        .map(|(i, t)| format!("{:3}: {}", i, t))
        .collect::<Vec<_>>()
        .join("\n")
}
```

Delete the four `render_*` helper functions. Delete `platform_for_snapshot` only if no longer needed.

- [ ] **Step 7: Run snapshot tests — verify no drift**

Run: `cargo test --lib wheel::compat::tests::compat_snapshot 2>&1 | tail -15`

If all 5 snapshot tests pass (no diff), proceed.

If there's a diff, the previous `render_*` helpers produced different bytes than the new `Display` impls. Inspect the diff: open one `.snap` file and the matching `.snap.new`. The new bytes should be identical or trivially equivalent (e.g. same content, no whitespace changes). If genuinely identical: accept via `INSTA_UPDATE=always cargo test --lib wheel::compat::tests::compat_snapshot`. If different: STOP and investigate — the `Display` impls have a divergence from the old helpers that needs reconciling.

- [ ] **Step 8: Run full suite**

Run: `cargo test 2>&1 | tail -10`
Expected: 119 tests pass (118 baseline + 1 new). `cargo clippy --all-targets -- -D warnings` clean.

- [ ] **Step 9: Commit**

```bash
git add src/wheel/tag.rs src/cli/debug/pick_wheels.rs src/wheel/compat.rs src/wheel/snapshots/
git commit -m "refactor(s3): Display for Tag, consolidating three render_tag copies

Implements Display for Tag, PythonTag, AbiTag, PlatformTag. Removes
the duplicate render_tag function in pick_wheels.rs and the test
helpers in compat.rs::tests; both now call tag.to_string() directly.

Closes TECH_DEBT item: 'render_tag duplicated between handler and
snapshot tests'."
```

---

### Task 2: `expand_requires_python` floor doc + `MIN_SUPPORTED_PY_MINOR` constant

**Files:**
- Modify: `src/cli/init.rs`

The current `expand_requires_python` (or similarly-named function) clamps Python versions to `>=3.11` inline. Extract the floor as a named constant + doc.

- [ ] **Step 1: Inspect current code**

Read `src/cli/init.rs` to find the function that expands `requires-python`. It contains a hard-coded floor (probably `3.11` or `(3, 11)`). Note the function's exact name and signature.

- [ ] **Step 2: Add the constant + doc comment**

At the top of `src/cli/init.rs` (after `use` imports), add:

```rust
/// The minimum Python minor version muntjac supports for new projects.
/// Requires-python constraints below 3.X for X < MIN_SUPPORTED_PY_MINOR are
/// clamped to this floor in `expand_requires_python`. Muntjac's MVP does not
/// validate against 3.10 or earlier; raise this constant only after CI runs
/// against the new minimum.
pub(crate) const MIN_SUPPORTED_PY_MINOR: u8 = 11;
```

- [ ] **Step 3: Replace the hard-coded 11 (or 3.11) in the expand function**

Find every literal `11` in the floor-clamping logic and replace with `MIN_SUPPORTED_PY_MINOR`. Update the function's doc comment to reference the constant:

```rust
/// Expand a `requires-python` constraint into a concrete list of Python
/// (major, minor) versions. Constraints below MIN_SUPPORTED_PY_MINOR (3.11)
/// are silently clamped — muntjac does not support Python 3.10 or earlier
/// for new projects.
fn expand_requires_python(...) -> Vec<PythonVersion> { ... }
```

(Exact function signature varies — match what's there.)

- [ ] **Step 4: Run tests — confirm nothing broke**

Run: `cargo test 2>&1 | tail -5`
Expected: all tests pass (no behavior change, only constant extraction).

- [ ] **Step 5: Commit**

```bash
git add src/cli/init.rs
git commit -m "refactor(s3): MIN_SUPPORTED_PY_MINOR constant + floor doc

Extracts the hard-coded 3.11 Python floor in expand_requires_python
into a named constant. The doc comment explains the floor is a
deliberate MVP constraint — muntjac does not currently test against
3.10 or earlier.

Closes TECH_DEBT item: 'expand_requires_python floor of 3.11 is
hard-coded'."
```

---

### Task 3: `RawConfig::platforms` `#[serde(default)]` + `MissingField` check

**Files:**
- Modify: `src/config.rs`

A `muntjac.toml` with zero `[platforms.*]` tables currently fails with a generic Parse error from `toml::from_str`. Friendlier: parse successfully (default to empty BTreeMap) and produce a clear `MissingField("platforms")` error in `Config::from_raw`.

- [ ] **Step 1: Write the failing test**

In `src/config.rs::tests`:

```rust
#[test]
fn missing_platforms_table_produces_clear_error() {
    let toml_str = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]
"#;
    let err = Config::from_str(toml_str).expect_err("should fail");
    assert!(matches!(err, crate::error::ConfigError::MissingField("platforms")));
}
```

- [ ] **Step 2: Run — verify fails**

Run: `cargo test --lib config::tests::missing_platforms_table_produces_clear_error`
Expected: FAIL — currently the error is `Parse(_)` (missing field at TOML level), not `MissingField("platforms")`.

- [ ] **Step 3: Add `#[serde(default)]` + emptiness check**

Find `RawConfig` in `src/config.rs`. Add `#[serde(default)]` to the `platforms` field:

```rust
#[derive(Debug, Deserialize)]
struct RawConfig {
    // ... other fields unchanged ...
    #[serde(default)]
    platforms: BTreeMap<String, Platform>,
    // ... other fields unchanged ...
}
```

Then in `Config::from_raw`, add an emptiness check (place it after the trees are computed, before constructing `Config`):

```rust
if raw.platforms.is_empty() {
    return Err(crate::error::ConfigError::MissingField("platforms"));
}
```

- [ ] **Step 4: Run — passes**

Run: `cargo test --lib config::tests::missing_platforms_table_produces_clear_error`
Expected: PASS.

Run full suite: `cargo test 2>&1 | tail -5`. Expected: 120 tests pass (119 baseline + 1 new).

- [ ] **Step 5: Commit**

```bash
git add src/config.rs
git commit -m "fix(s3): missing [platforms.*] tables produce MissingField error

RawConfig::platforms now has #[serde(default)], so a muntjac.toml with
zero platform tables parses successfully and fails with a clear
ConfigError::MissingField('platforms') instead of a generic Parse
error.

Closes TECH_DEBT item: 'RawConfig::platforms has no #[serde(default)]'."
```

---

## Phase 2 — Buck module skeleton

### Task 4: `emit.rs` types + `BuckEmitter` trait

**Files:**
- Create: `src/buck/mod.rs`
- Create: `src/buck/emit.rs`
- Modify: `src/lib.rs` (`pub mod buck;`)

- [ ] **Step 1: Write the failing test for type construction**

Create `src/buck/emit.rs` skeleton with this test:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn emit_input_constructs() {
        let inp = EmitInput {
            tree: "default".into(),
            third_party_dir: "third-party/python".into(),
            configs: vec![
                ConfigName::new("3.12", "linux-x86_64-gnu"),
            ],
            packages: vec![
                EmitPackage {
                    name: "requests".into(),
                    version: "2.32.3".into(),
                    deps: vec![":certifi".into(), ":idna".into()],
                    wheels: {
                        let mut m = BTreeMap::new();
                        m.insert(
                            ConfigName::new("3.12", "linux-x86_64-gnu"),
                            EmitWheel {
                                url: "https://example.com/requests-2.32.3-py3-none-any.whl".into(),
                                hash: "sha256:abc".into(),
                            },
                        );
                        m
                    },
                },
            ],
        };
        assert_eq!(inp.tree, "default");
        assert_eq!(inp.packages.len(), 1);
        assert_eq!(inp.configs[0].as_str(), "py312-linux-x86_64-gnu");
    }

    #[test]
    fn config_name_orders_lexicographically() {
        let a = ConfigName::new("3.11", "linux-x86_64-gnu");
        let b = ConfigName::new("3.12", "linux-x86_64-gnu");
        assert!(a < b);
        assert_eq!(a.as_str(), "py311-linux-x86_64-gnu");
    }
}
```

- [ ] **Step 2: Run — verify fails to compile**

Run: `cargo test --lib buck::emit`
Expected: FAIL — module doesn't exist yet.

- [ ] **Step 3: Implement the types + trait**

Create `src/buck/mod.rs`:

```rust
//! Buck2 BUCK + muntjac.bzl + config/BUCK + PACKAGE emitter.
//!
//! See `docs/superpowers/specs/2026-05-21-muntjac-s3-buck-emitter-design.md`
//! for the design.

pub mod emit;
pub mod string_writer;
pub mod write;

pub use emit::{
    BuckEmitter, ConfigName, EmitInput, EmitOutput, EmitPackage, EmitWheel,
};
pub use string_writer::StringTemplateEmitter;
```

Create `src/buck/emit.rs`:

```rust
//! Types and trait for the BUCK emitter.

use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct EmitInput {
    pub tree: String,
    pub third_party_dir: String,
    pub configs: Vec<ConfigName>,
    pub packages: Vec<EmitPackage>,
}

#[derive(Debug, Clone)]
pub struct EmitPackage {
    pub name: String,
    pub version: String,
    pub deps: Vec<String>,
    pub wheels: BTreeMap<ConfigName, EmitWheel>,
}

#[derive(Debug, Clone)]
pub struct EmitWheel {
    pub url: String,
    pub hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConfigName(String);

impl ConfigName {
    /// Build a config name from a Python version string (e.g. "3.12") and a
    /// platform key (e.g. "linux-x86_64-gnu"). Result: "py312-linux-x86_64-gnu".
    pub fn new(py_version: &str, platform_name: &str) -> Self {
        let mut s = String::with_capacity(8 + platform_name.len());
        s.push_str("py");
        for c in py_version.chars().filter(|c| c.is_ascii_digit()) {
            s.push(c);
        }
        s.push('-');
        s.push_str(platform_name);
        ConfigName(s)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ConfigName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone)]
pub struct EmitOutput {
    pub buck: String,
    pub muntjac_bzl: String,
    pub config_buck: String,
    pub package_file: String,
}

/// Trait for muntjac's BUCK emitter. The v1 implementation is
/// `StringTemplateEmitter` (hand-rolled writeln! formatting). Future
/// implementations (typed-AST or template-engine) can plug in without
/// changing the CLI or pipeline composer.
///
/// Implementations MUST be deterministic: same input -> same byte output.
/// All map iteration must use BTreeMap or pre-sorted Vec.
pub trait BuckEmitter {
    fn emit(&self, input: &EmitInput) -> EmitOutput;
}

#[cfg(test)]
mod tests {
    // (tests from Step 1 above)
}
```

Add `pub mod buck;` to `src/lib.rs` alongside the other modules.

- [ ] **Step 4: Run — passes**

Run: `cargo test --lib buck::emit`
Expected: 2 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/buck/ src/lib.rs
git commit -m "feat(s3): BuckEmitter trait + EmitInput/EmitOutput types

Defines the BuckEmitter trait surface and its input/output data
shapes. ConfigName is a newtype around String with Ord derived so
BTreeMap iteration is byte-stable. The hand-rolled
StringTemplateEmitter implementation lands in the next task."
```

---

### Task 5: `string_writer.rs` skeleton

**Files:**
- Create: `src/buck/string_writer.rs`

Create the `StringTemplateEmitter` struct + `BuckEmitter` impl that returns empty/placeholder strings for all four outputs. Subsequent tasks fill in each emit function.

- [ ] **Step 1: Write the failing test**

Add to `src/buck/string_writer.rs`:

```rust
//! Hand-rolled string-writer implementation of `BuckEmitter`.

use super::emit::{BuckEmitter, EmitInput, EmitOutput};

pub struct StringTemplateEmitter;

impl BuckEmitter for StringTemplateEmitter {
    fn emit(&self, _input: &EmitInput) -> EmitOutput {
        EmitOutput {
            buck: String::new(),
            muntjac_bzl: String::new(),
            config_buck: String::new(),
            package_file: String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buck::emit::{ConfigName, EmitInput};

    fn empty_input() -> EmitInput {
        EmitInput {
            tree: "default".into(),
            third_party_dir: "third-party/python".into(),
            configs: vec![ConfigName::new("3.12", "linux-x86_64-gnu")],
            packages: vec![],
        }
    }

    #[test]
    fn emitter_returns_four_strings() {
        let out = StringTemplateEmitter.emit(&empty_input());
        // For now these are empty; subsequent tasks fill them in.
        // This test just verifies the trait impl wires up.
        let _ = out.buck;
        let _ = out.muntjac_bzl;
        let _ = out.config_buck;
        let _ = out.package_file;
    }
}
```

- [ ] **Step 2: Verify compile + tests pass**

Run: `cargo test --lib buck::string_writer`
Expected: PASS (1 test).

- [ ] **Step 3: Commit**

```bash
git add src/buck/string_writer.rs
git commit -m "feat(s3): StringTemplateEmitter skeleton

Implements BuckEmitter with empty-string placeholders for all four
outputs. Subsequent tasks fill in each emit function."
```

---

## Phase 3 — Emit each output file

### Task 6: Emit `BUCK`

**Files:**
- Modify: `src/buck/string_writer.rs`

The `BUCK` file is the header + `load(...)` of `muntjac.bzl` + one `pypi_package(...)` call per package.

- [ ] **Step 1: Write the failing tests**

Append to `src/buck/string_writer.rs::tests`:

```rust
use std::collections::BTreeMap;
use crate::buck::emit::{EmitPackage, EmitWheel};

fn single_package_input() -> EmitInput {
    let cfg = ConfigName::new("3.12", "linux-x86_64-gnu");
    let mut wheels = BTreeMap::new();
    wheels.insert(cfg.clone(), EmitWheel {
        url: "https://files.pythonhosted.org/p/certifi-2025.4.26-py3-none-any.whl".into(),
        hash: "sha256:0123abcd".into(),
    });
    EmitInput {
        tree: "default".into(),
        third_party_dir: "third-party/python".into(),
        configs: vec![cfg],
        packages: vec![EmitPackage {
            name: "certifi".into(),
            version: "2025.4.26".into(),
            deps: vec![],
            wheels,
        }],
    }
}

#[test]
fn empty_input_buck_has_header_and_load() {
    let out = StringTemplateEmitter.emit(&empty_input());
    assert!(out.buck.starts_with("##\n## @generated by muntjac\n## Do not edit by hand.\n##\n\n"),
        "missing or malformed BUCK header:\n{}", out.buck);
    assert!(out.buck.contains(r#"load("//third-party/python:muntjac.bzl", "pypi_package")"#),
        "missing or wrong load() line:\n{}", out.buck);
}

#[test]
fn single_package_buck_contains_pypi_package_call() {
    let out = StringTemplateEmitter.emit(&single_package_input());
    assert!(out.buck.contains("pypi_package("));
    assert!(out.buck.contains(r#"name = "certifi""#));
    assert!(out.buck.contains(r#"version = "2025.4.26""#));
    assert!(out.buck.contains(r#""py312-linux-x86_64-gnu":"#));
    assert!(out.buck.contains("sha256:0123abcd"));
    assert!(out.buck.contains(r#"visibility = ["PUBLIC"]"#));
}

#[test]
fn two_packages_emitted_alphabetically() {
    let mut input = single_package_input();
    // Add a second package "alpha" that should sort before "certifi".
    let mut wheels = BTreeMap::new();
    wheels.insert(
        input.configs[0].clone(),
        EmitWheel {
            url: "https://example.com/alpha-1.0-py3-none-any.whl".into(),
            hash: "sha256:aaaa".into(),
        },
    );
    input.packages.insert(0, EmitPackage {
        name: "alpha".into(),
        version: "1.0".into(),
        deps: vec![],
        wheels,
    });
    let out = StringTemplateEmitter.emit(&input);
    let alpha_pos = out.buck.find("name = \"alpha\"").expect("alpha not emitted");
    let certifi_pos = out.buck.find("name = \"certifi\"").expect("certifi not emitted");
    assert!(alpha_pos < certifi_pos, "alpha must precede certifi in output:\n{}", out.buck);
}
```

- [ ] **Step 2: Run — verify all three fail**

Run: `cargo test --lib buck::string_writer 2>&1 | tail -15`
Expected: 3 failures (the empty `.buck` doesn't contain the header).

- [ ] **Step 3: Implement `emit_buck`**

Update `src/buck/string_writer.rs`:

```rust
use std::fmt::Write;

use super::emit::{BuckEmitter, EmitInput, EmitOutput, EmitPackage};

pub struct StringTemplateEmitter;

impl BuckEmitter for StringTemplateEmitter {
    fn emit(&self, input: &EmitInput) -> EmitOutput {
        EmitOutput {
            buck: emit_buck(input),
            muntjac_bzl: String::new(),
            config_buck: String::new(),
            package_file: String::new(),
        }
    }
}

fn emit_buck(input: &EmitInput) -> String {
    let mut s = String::new();
    writeln!(s, "##").unwrap();
    writeln!(s, "## @generated by muntjac").unwrap();
    writeln!(s, "## Do not edit by hand.").unwrap();
    writeln!(s, "##").unwrap();
    writeln!(s).unwrap();
    writeln!(
        s,
        "load(\"//{}:muntjac.bzl\", \"pypi_package\")",
        input.third_party_dir
    ).unwrap();

    // Packages are pre-sorted by (name, version) in EmitInput; iterate as given.
    let mut sorted_pkgs: Vec<&EmitPackage> = input.packages.iter().collect();
    sorted_pkgs.sort_by(|a, b| a.name.cmp(&b.name).then(a.version.cmp(&b.version)));

    for pkg in sorted_pkgs {
        writeln!(s).unwrap();
        write_pypi_package(&mut s, pkg);
    }
    s
}

fn write_pypi_package(s: &mut String, pkg: &EmitPackage) {
    writeln!(s, "pypi_package(").unwrap();
    writeln!(s, "    name = \"{}\",", pkg.name).unwrap();
    writeln!(s, "    version = \"{}\",", pkg.version).unwrap();
    if pkg.deps.is_empty() {
        writeln!(s, "    deps = [],").unwrap();
    } else {
        writeln!(s, "    deps = [").unwrap();
        for dep in &pkg.deps {
            writeln!(s, "        \"{}\",", dep).unwrap();
        }
        writeln!(s, "    ],").unwrap();
    }
    writeln!(s, "    wheels = {{").unwrap();
    for (cfg, wheel) in &pkg.wheels {
        writeln!(
            s,
            "        \"{}\": (\"{}\", \"{}\"),",
            cfg, wheel.url, wheel.hash
        ).unwrap();
    }
    writeln!(s, "    }},").unwrap();
    writeln!(s, "    visibility = [\"PUBLIC\"],").unwrap();
    writeln!(s, ")").unwrap();
}
```

The defensive `sort_by` is redundant if `EmitInput.packages` is already sorted by the composer, but it's cheap and makes the emitter robust to caller mistakes.

- [ ] **Step 4: Run — all three pass**

Run: `cargo test --lib buck::string_writer 2>&1 | tail -15`
Expected: 4 tests pass (`emitter_returns_four_strings` + 3 new).

- [ ] **Step 5: Commit**

```bash
git add src/buck/string_writer.rs
git commit -m "feat(s3): emit BUCK file with header, load, and pypi_package calls

emit_buck() writes the @generated header, the muntjac.bzl load(), and
one pypi_package(name, version, deps, wheels, visibility) call per
package. Packages emitted alphabetically by (name, version); wheels
dict iterated by BTreeMap sorted ConfigName key."
```

---

### Task 7: Emit `muntjac.bzl`

**Files:**
- Modify: `src/buck/string_writer.rs`

The `muntjac.bzl` file is mostly constant — only `_CONFIGS` and the `<third_party_dir>` placeholder in `select()` keys vary.

- [ ] **Step 1: Write the failing test**

Append to `src/buck/string_writer.rs::tests`:

```rust
#[test]
fn muntjac_bzl_has_header_and_configs_list() {
    let out = StringTemplateEmitter.emit(&empty_input());
    assert!(out.muntjac_bzl.starts_with("##\n## @generated by muntjac\n##\n\n"),
        "missing muntjac.bzl header:\n{}", out.muntjac_bzl);
    assert!(out.muntjac_bzl.contains("_CONFIGS = ["));
    assert!(out.muntjac_bzl.contains("    \"py312-linux-x86_64-gnu\","));
    assert!(out.muntjac_bzl.contains("def pypi_package("));
    assert!(out.muntjac_bzl.contains("prebuilt_python_library"));
    assert!(out.muntjac_bzl.contains("//third-party/python/config:"));
}

#[test]
fn muntjac_bzl_configs_list_sorted_lex() {
    let mut input = empty_input();
    // Insert configs in non-sorted order; emitter must sort.
    input.configs = vec![
        ConfigName::new("3.12", "linux-x86_64-gnu"),
        ConfigName::new("3.11", "linux-x86_64-gnu"),
    ];
    let out = StringTemplateEmitter.emit(&input);
    let p311 = out.muntjac_bzl.find("\"py311-linux-x86_64-gnu\"").expect("py311 missing");
    let p312 = out.muntjac_bzl.find("\"py312-linux-x86_64-gnu\"").expect("py312 missing");
    assert!(p311 < p312, "_CONFIGS not sorted:\n{}", out.muntjac_bzl);
}
```

- [ ] **Step 2: Run — verify fails**

Run: `cargo test --lib buck::string_writer::tests::muntjac_bzl 2>&1 | tail -10`
Expected: 2 failures.

- [ ] **Step 3: Implement `emit_muntjac_bzl`**

Update `src/buck/string_writer.rs`. First, change the `emit` impl to call `emit_muntjac_bzl`:

```rust
impl BuckEmitter for StringTemplateEmitter {
    fn emit(&self, input: &EmitInput) -> EmitOutput {
        EmitOutput {
            buck: emit_buck(input),
            muntjac_bzl: emit_muntjac_bzl(input),
            config_buck: String::new(),
            package_file: String::new(),
        }
    }
}
```

Then add `emit_muntjac_bzl`:

```rust
fn emit_muntjac_bzl(input: &EmitInput) -> String {
    let mut s = String::new();
    writeln!(s, "##").unwrap();
    writeln!(s, "## @generated by muntjac").unwrap();
    writeln!(s, "##").unwrap();
    writeln!(s).unwrap();
    writeln!(s, "load(\"@prelude//python:python_library.bzl\", \"prebuilt_python_library\")").unwrap();
    writeln!(s, "load(\"@prelude//utils:utils.bzl\", \"expect\")").unwrap();
    writeln!(s).unwrap();
    writeln!(s, "_CONFIGS = [").unwrap();

    let mut sorted_configs: Vec<&ConfigName> = input.configs.iter().collect();
    sorted_configs.sort();
    for cfg in &sorted_configs {
        writeln!(s, "    \"{}\",", cfg).unwrap();
    }

    writeln!(s, "]").unwrap();
    writeln!(s).unwrap();
    writeln!(s, "def pypi_package(name, version, wheels, deps = [], visibility = None, **kwargs):").unwrap();
    writeln!(s, "    expect(set(wheels.keys()).issubset(set(_CONFIGS)),").unwrap();
    writeln!(s, "           \"unknown config in {{}}\".format(name))").unwrap();
    writeln!(s).unwrap();
    writeln!(s, "    for cfg, (url, sha256) in wheels.items():").unwrap();
    writeln!(s, "        http_file(").unwrap();
    writeln!(s, "            name = \"{{}}-{{}}-{{}}-wheel\".format(name, version, cfg),").unwrap();
    writeln!(s, "            sha256 = sha256.removeprefix(\"sha256:\"),").unwrap();
    writeln!(s, "            urls = [url],").unwrap();
    writeln!(s, "            visibility = [],").unwrap();
    writeln!(s, "        )").unwrap();
    writeln!(s, "        prebuilt_python_library(").unwrap();
    writeln!(s, "            name = \"{{}}-{{}}__{{}}\".format(name, version, cfg),").unwrap();
    writeln!(s, "            binary_src = \":{{}}-{{}}-{{}}-wheel\".format(name, version, cfg),").unwrap();
    writeln!(s, "            deps = deps,").unwrap();
    writeln!(s, "            visibility = [],").unwrap();
    writeln!(s, "        )").unwrap();
    writeln!(s).unwrap();
    writeln!(s, "    native.alias(").unwrap();
    writeln!(s, "        name = \"{{}}-{{}}\".format(name, version),").unwrap();
    writeln!(s, "        actual = select({{").unwrap();
    writeln!(
        s,
        "            \"//{}/config:{{}}\".format(cfg): \":{{}}-{{}}__{{}}\".format(name, version, cfg)",
        input.third_party_dir
    ).unwrap();
    writeln!(s, "            for cfg in wheels.keys()").unwrap();
    writeln!(s, "        }}),").unwrap();
    writeln!(s, "        visibility = [],").unwrap();
    writeln!(s, "    )").unwrap();
    writeln!(s, "    native.alias(").unwrap();
    writeln!(s, "        name = name,").unwrap();
    writeln!(s, "        actual = \":{{}}-{{}}\".format(name, version),").unwrap();
    writeln!(s, "        visibility = visibility or [\"PUBLIC\"],").unwrap();
    writeln!(s, "    )").unwrap();
    s
}
```

Note: `ConfigName` needs to be in scope for the sort — add the import to the file's top-level uses if not already there.

- [ ] **Step 4: Run — passes**

Run: `cargo test --lib buck::string_writer 2>&1 | tail -10`
Expected: 6 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/buck/string_writer.rs
git commit -m "feat(s3): emit muntjac.bzl with _CONFIGS list and pypi_package macro

emit_muntjac_bzl() writes the @generated header, load statements, the
_CONFIGS list (sorted lex), and the pypi_package macro definition.
The select() key template embeds the configured third_party_dir."
```

---

### Task 8: Emit `config/BUCK`

**Files:**
- Modify: `src/buck/string_writer.rs`

The `config/BUCK` file declares `config_setting` per cell, plus per-axis `constraint_setting` + `constraint_value`s.

- [ ] **Step 1: Write the failing test**

Append to `src/buck/string_writer.rs::tests`:

```rust
#[test]
fn config_buck_declares_per_cell_settings() {
    let mut input = empty_input();
    input.configs = vec![
        ConfigName::new("3.11", "linux-x86_64-gnu"),
        ConfigName::new("3.12", "linux-x86_64-gnu"),
    ];
    let out = StringTemplateEmitter.emit(&input);
    assert!(out.config_buck.starts_with("##\n## @generated by muntjac\n## Do not edit by hand.\n##\n\n"),
        "missing config/BUCK header:\n{}", out.config_buck);
    assert!(out.config_buck.contains("config_setting("));
    assert!(out.config_buck.contains(r#"name = "py311-linux-x86_64-gnu""#));
    assert!(out.config_buck.contains(r#"name = "py312-linux-x86_64-gnu""#));
    assert!(out.config_buck.contains(r#"constraint_setting(name = "python_version")"#));
    assert!(out.config_buck.contains(r#"constraint_value(name = "py311""#));
    assert!(out.config_buck.contains(r#"constraint_value(name = "py312""#));
    assert!(out.config_buck.contains(r#"constraint_setting(name = "platform")"#));
    assert!(out.config_buck.contains(r#"constraint_value(name = "linux-x86_64-gnu""#));
}

#[test]
fn config_buck_settings_sorted_by_name() {
    let mut input = empty_input();
    input.configs = vec![
        ConfigName::new("3.12", "linux-x86_64-gnu"),
        ConfigName::new("3.11", "linux-x86_64-gnu"),
    ];
    let out = StringTemplateEmitter.emit(&input);
    let p311 = out.config_buck.find(r#"name = "py311-linux-x86_64-gnu""#).expect("py311 missing");
    let p312 = out.config_buck.find(r#"name = "py312-linux-x86_64-gnu""#).expect("py312 missing");
    assert!(p311 < p312, "config_settings must be sorted:\n{}", out.config_buck);
}
```

- [ ] **Step 2: Run — verify fails**

Run: `cargo test --lib buck::string_writer::tests::config_buck 2>&1 | tail -10`
Expected: 2 failures.

- [ ] **Step 3: Implement `emit_config_buck`**

Update the `emit` impl to call it, then add:

```rust
fn emit_config_buck(input: &EmitInput) -> String {
    use std::collections::BTreeSet;

    let mut s = String::new();
    writeln!(s, "##").unwrap();
    writeln!(s, "## @generated by muntjac").unwrap();
    writeln!(s, "## Do not edit by hand.").unwrap();
    writeln!(s, "##").unwrap();

    let mut sorted_configs: Vec<&ConfigName> = input.configs.iter().collect();
    sorted_configs.sort();

    // Per-cell config_setting blocks.
    for cfg in &sorted_configs {
        let (py_part, plat_part) = split_config(cfg.as_str());
        writeln!(s).unwrap();
        writeln!(s, "config_setting(").unwrap();
        writeln!(s, "    name = \"{}\",", cfg).unwrap();
        writeln!(s, "    constraint_values = [").unwrap();
        writeln!(s, "        \"//{}/config:{}\",", input.third_party_dir, py_part).unwrap();
        writeln!(s, "        \"//{}/config:{}\",", input.third_party_dir, plat_part).unwrap();
        writeln!(s, "    ],").unwrap();
        writeln!(s, ")").unwrap();
    }

    // Per-axis constraint_setting + constraint_value blocks.
    let mut pys: BTreeSet<&str> = BTreeSet::new();
    let mut plats: BTreeSet<&str> = BTreeSet::new();
    for cfg in &sorted_configs {
        let (py, plat) = split_config(cfg.as_str());
        pys.insert(py);
        plats.insert(plat);
    }

    writeln!(s).unwrap();
    writeln!(s, "constraint_setting(name = \"python_version\")").unwrap();
    for py in &pys {
        writeln!(s, "constraint_value(name = \"{}\", constraint_setting = \":python_version\")", py).unwrap();
    }

    writeln!(s).unwrap();
    writeln!(s, "constraint_setting(name = \"platform\")").unwrap();
    for plat in &plats {
        writeln!(s, "constraint_value(name = \"{}\", constraint_setting = \":platform\")", plat).unwrap();
    }

    s
}

/// Split a ConfigName like "py312-linux-x86_64-gnu" into ("py312", "linux-x86_64-gnu").
fn split_config(name: &str) -> (&str, &str) {
    name.split_once('-').expect("ConfigName has form pyXY-<platform>")
}
```

- [ ] **Step 4: Run — passes**

Run: `cargo test --lib buck::string_writer 2>&1 | tail -10`
Expected: 8 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/buck/string_writer.rs
git commit -m "feat(s3): emit config/BUCK with config_settings and constraints

emit_config_buck() writes per-cell config_setting blocks plus
per-axis constraint_setting + constraint_value blocks. All sorted
lex (BTreeSet for the axes; sorted Vec for cells)."
```

---

### Task 9: Emit `PACKAGE`

**Files:**
- Modify: `src/buck/string_writer.rs`

The S3 `PACKAGE` file is a placeholder header. Full `set_cfg_modifiers` wiring lands in S4.

- [ ] **Step 1: Write the failing test**

Append to `src/buck/string_writer.rs::tests`:

```rust
#[test]
fn package_file_has_header_and_placeholder_comment() {
    let out = StringTemplateEmitter.emit(&empty_input());
    assert!(out.package_file.starts_with("##\n## @generated by muntjac\n## Do not edit by hand.\n##\n\n"),
        "missing PACKAGE header:\n{}", out.package_file);
    assert!(out.package_file.contains("set_cfg_modifiers"),
        "PACKAGE should reference the future S4 wiring");
    assert!(out.package_file.contains("S4") || out.package_file.contains("placeholder"),
        "PACKAGE should explicitly mark itself as a placeholder");
}
```

- [ ] **Step 2: Run — verify fails**

Run: `cargo test --lib buck::string_writer::tests::package_file 2>&1 | tail -10`
Expected: FAIL.

- [ ] **Step 3: Implement `emit_package_file`**

Update the `emit` impl to call it. Add:

```rust
fn emit_package_file(_input: &EmitInput) -> String {
    let mut s = String::new();
    writeln!(s, "##").unwrap();
    writeln!(s, "## @generated by muntjac").unwrap();
    writeln!(s, "## Do not edit by hand.").unwrap();
    writeln!(s, "##").unwrap();
    writeln!(s).unwrap();
    writeln!(s, "# Wires python_version + platform constraints so consumers' python_binary").unwrap();
    writeln!(s, "# selects the right wheel via the alias-with-select pattern.").unwrap();
    writeln!(s, "#").unwrap();
    writeln!(s, "# S3 emits a minimal PACKAGE; full set_cfg_modifiers wiring lands in S4").unwrap();
    writeln!(s, "# once multi-platform select() has been exercised against a buck2 build.").unwrap();
    writeln!(s, "# (placeholder)").unwrap();
    s
}
```

- [ ] **Step 4: Run — passes**

Run: `cargo test --lib buck::string_writer 2>&1 | tail -10`
Expected: 9 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/buck/string_writer.rs
git commit -m "feat(s3): emit PACKAGE placeholder file

S3's PACKAGE is a header + scope-note placeholder. Full
set_cfg_modifiers wiring lands in S4 alongside multi-platform
select() exercised against a real buck2 build."
```

---

## Phase 4 — Cross-cutting snapshot tests

### Task 10: Snapshot tests for empty / single / multi-package

**Files:**
- Modify: `src/buck/string_writer.rs::tests`
- Add: `src/buck/snapshots/` (insta-generated)

Snapshot the full output for three representative inputs. Catches subtle regressions that the contains-style assertions miss.

- [ ] **Step 1: Verify `insta` is a dev-dep**

Run: `grep insta Cargo.toml`
Expected: `insta = "1.x"` in `[dev-dependencies]` (already present from S2).

- [ ] **Step 2: Append snapshot tests**

Append to `src/buck/string_writer.rs::tests`:

```rust
fn multi_package_input() -> EmitInput {
    let cfg_311 = ConfigName::new("3.11", "linux-x86_64-gnu");
    let cfg_312 = ConfigName::new("3.12", "linux-x86_64-gnu");

    let mut requests_wheels = BTreeMap::new();
    requests_wheels.insert(cfg_311.clone(), EmitWheel {
        url: "https://example.com/requests-2.32.3-py3-none-any.whl".into(),
        hash: "sha256:rrrr".into(),
    });
    requests_wheels.insert(cfg_312.clone(), EmitWheel {
        url: "https://example.com/requests-2.32.3-py3-none-any.whl".into(),
        hash: "sha256:rrrr".into(),
    });

    let mut idna_wheels = BTreeMap::new();
    idna_wheels.insert(cfg_311.clone(), EmitWheel {
        url: "https://example.com/idna-3.7-py3-none-any.whl".into(),
        hash: "sha256:iiii".into(),
    });
    idna_wheels.insert(cfg_312.clone(), EmitWheel {
        url: "https://example.com/idna-3.7-py3-none-any.whl".into(),
        hash: "sha256:iiii".into(),
    });

    EmitInput {
        tree: "default".into(),
        third_party_dir: "third-party/python".into(),
        configs: vec![cfg_311, cfg_312],
        packages: vec![
            EmitPackage {
                name: "idna".into(),
                version: "3.7".into(),
                deps: vec![],
                wheels: idna_wheels,
            },
            EmitPackage {
                name: "requests".into(),
                version: "2.32.3".into(),
                deps: vec![":idna".into()],
                wheels: requests_wheels,
            },
        ],
    }
}

#[test]
fn snapshot_empty_buck() {
    insta::assert_snapshot!(StringTemplateEmitter.emit(&empty_input()).buck);
}

#[test]
fn snapshot_empty_muntjac_bzl() {
    insta::assert_snapshot!(StringTemplateEmitter.emit(&empty_input()).muntjac_bzl);
}

#[test]
fn snapshot_empty_config_buck() {
    insta::assert_snapshot!(StringTemplateEmitter.emit(&empty_input()).config_buck);
}

#[test]
fn snapshot_empty_package_file() {
    insta::assert_snapshot!(StringTemplateEmitter.emit(&empty_input()).package_file);
}

#[test]
fn snapshot_single_package_buck() {
    insta::assert_snapshot!(StringTemplateEmitter.emit(&single_package_input()).buck);
}

#[test]
fn snapshot_multi_package_buck() {
    insta::assert_snapshot!(StringTemplateEmitter.emit(&multi_package_input()).buck);
}

#[test]
fn snapshot_multi_package_muntjac_bzl() {
    insta::assert_snapshot!(StringTemplateEmitter.emit(&multi_package_input()).muntjac_bzl);
}

#[test]
fn snapshot_multi_package_config_buck() {
    insta::assert_snapshot!(StringTemplateEmitter.emit(&multi_package_input()).config_buck);
}
```

- [ ] **Step 3: First run records `.snap.new` files**

Run: `cargo test --lib buck::string_writer::tests::snapshot 2>&1 | tail -10`
Expected: snapshot tests fail with "snapshot file missing"; `.snap.new` files written under `src/buck/snapshots/`.

- [ ] **Step 4: Inspect the generated snapshots**

```bash
ls src/buck/snapshots/
```

Cat each `.snap.new` and verify:
- BUCK has the load() line, packages in alphabetical order (idna before requests), wheels dict sorted by config key.
- muntjac.bzl has the right `_CONFIGS` list with both pythons sorted.
- config/BUCK has the per-cell config_settings and per-axis constraint declarations.
- PACKAGE has the placeholder header.

If anything looks wrong, STOP and investigate the emit functions.

- [ ] **Step 5: Accept snapshots**

Run: `INSTA_UPDATE=always cargo test --lib buck::string_writer::tests::snapshot 2>&1 | tail -10`
Expected: snapshots accepted; `.snap` files created.

- [ ] **Step 6: Re-run — confirm stable**

Run: `cargo test --lib buck::string_writer 2>&1 | tail -10`
Expected: 17 tests pass (9 contains-style + 8 snapshot).

- [ ] **Step 7: Commit**

```bash
git add src/buck/string_writer.rs src/buck/snapshots/
git commit -m "test(s3): insta snapshots for empty/single/multi BUCK outputs

8 snapshot tests pin the full byte-stable output of each emit_*
function for three representative inputs. Catches subtle regressions
(ordering drift, whitespace changes, etc) that contains-style
assertions miss."
```

---

## Phase 5 — Pipeline composer

### Task 11: `EmitInput` composer (happy path)

**Files:**
- Modify: `src/buck/emit.rs`

Add a function `build_emit_input(...)` that takes a tree's resolved data and produces an `EmitInput`. Reuses S1's lockfile parsing, S2's wheel selector.

- [ ] **Step 1: Write the failing test**

Append to `src/buck/emit.rs::tests`:

```rust
#[test]
fn build_emit_input_from_synthetic_resolved() {
    // Build a minimal Config + Lockfile + projected ResolvedView in memory,
    // then call build_emit_input() and verify the resulting EmitInput.
    // This test exercises the cross-merge logic, not the pipeline below it
    // (which is tested via integration tests in tests/buckify.rs).

    use crate::config::{Config, Platform, PythonVersion, Tree};
    use crate::lock::types::{Lockfile, Package, Source, Wheel};
    use pep440_rs::Version;
    use pep508_rs::PackageName;
    use std::str::FromStr;
    use url::Url;

    let tree = Tree {
        name: "default".into(),
        manifest_path: "pyproject.toml".into(),
        third_party_dir: "third-party/python".into(),
        python_versions: vec![PythonVersion(3, 12)],
    };

    let mut platforms = std::collections::BTreeMap::new();
    platforms.insert("linux-x86_64-gnu".into(), Platform {
        target: "x86_64-unknown-linux-gnu".into(),
        manylinux: Some("2_17".into()),
        musllinux: None,
        macos_min: None,
    });

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
                name: PackageName::from_str("certifi").unwrap(),
                version: Version::from_str("2025.4.26").unwrap(),
                source: Source::Registry {
                    url: Url::parse("https://pypi.org/simple").unwrap(),
                },
                dependencies: vec![],
                sdist: None,
                wheels: vec![Wheel {
                    url: Url::parse(
                        "https://files.pythonhosted.org/p/certifi-2025.4.26-py3-none-any.whl"
                    ).unwrap(),
                    hash: "sha256:abc".into(),
                    size: None,
                    filename: "certifi-2025.4.26-py3-none-any.whl".into(),
                }],
                metadata: None,
            },
        ],
    };

    let input = build_emit_input(&config, &tree, &lockfile)
        .expect("build_emit_input succeeds");

    assert_eq!(input.tree, "default");
    assert_eq!(input.third_party_dir, "third-party/python");
    assert_eq!(input.configs, vec![ConfigName::new("3.12", "linux-x86_64-gnu")]);
    assert_eq!(input.packages.len(), 1);
    let pkg = &input.packages[0];
    assert_eq!(pkg.name, "certifi");
    assert_eq!(pkg.version, "2025.4.26");
    assert!(pkg.deps.is_empty());
    assert_eq!(pkg.wheels.len(), 1);
    let wheel = pkg.wheels.values().next().unwrap();
    assert!(wheel.url.contains("certifi-2025.4.26"));
    assert_eq!(wheel.hash, "sha256:abc");
}
```

- [ ] **Step 2: Run — verify fails**

Run: `cargo test --lib buck::emit::tests::build_emit_input_from_synthetic_resolved`
Expected: FAIL — function doesn't exist.

- [ ] **Step 3: Implement `build_emit_input` happy path**

Append to `src/buck/emit.rs`:

```rust
use crate::config::{Config, Tree};
use crate::lock::types::Lockfile;

/// Build an EmitInput for one tree by walking every (platform, python) cell,
/// picking wheels, and cross-merging into per-package wheel maps.
pub fn build_emit_input(
    config: &Config,
    tree: &Tree,
    lockfile: &Lockfile,
) -> anyhow::Result<EmitInput> {
    use crate::lock::graph::{build as build_graph, detect_cycles};
    use crate::lock::resolved::project as project_view;
    use crate::wheel::{build_compatible_tags, pick_wheel, PickResult};
    use std::collections::BTreeMap;

    let graph = build_graph(lockfile)?;
    detect_cycles(&graph)?;
    let view = project_view(&graph, config, tree)?;

    // Build the (name, version) -> &[Wheel] index from the lockfile.
    let mut wheel_index: BTreeMap<(String, String), &[crate::lock::types::Wheel]> = BTreeMap::new();
    for pkg in &lockfile.packages {
        wheel_index.insert(
            (pkg.name.as_ref().to_string(), pkg.version.to_string()),
            &pkg.wheels,
        );
    }

    // Sorted configs Vec for the EmitInput.
    let mut configs: Vec<ConfigName> = Vec::new();
    for (plat_name, _) in &config.platforms {
        for py in &tree.python_versions {
            configs.push(ConfigName::new(&format!("{}.{}", py.0, py.1), plat_name));
        }
    }
    configs.sort();

    // Per-package accumulator: name@version -> (deps_per_cell, wheels_per_cell).
    type PkgKey = (String, String);
    let mut pkg_wheels: BTreeMap<PkgKey, BTreeMap<ConfigName, EmitWheel>> = BTreeMap::new();
    let mut pkg_deps_per_cell: BTreeMap<PkgKey, BTreeMap<ConfigName, Vec<String>>> = BTreeMap::new();

    for resolved_cfg in &view.configs {
        let plat_name = &resolved_cfg.platform;
        let plat = config
            .platforms
            .get(plat_name)
            .ok_or_else(|| anyhow::anyhow!("platform `{}` missing from config", plat_name))?;
        let py = crate::config::PythonVersion::from_str(&resolved_cfg.python_version)
            .map_err(anyhow::Error::msg)?;
        let cfg_name = ConfigName::new(&resolved_cfg.python_version, plat_name);
        let compat = build_compatible_tags(plat, py);

        for pkg in &resolved_cfg.packages {
            let key: PkgKey = (pkg.name.clone(), pkg.version.clone());
            // First-party packages have no wheels and are not emitted as third-party.
            let wheels = wheel_index.get(&key).copied().unwrap_or(&[]);
            if wheels.is_empty() {
                continue;
            }
            match pick_wheel(wheels, &compat) {
                PickResult::Picked { wheel, .. } => {
                    pkg_wheels
                        .entry(key.clone())
                        .or_default()
                        .insert(cfg_name.clone(), EmitWheel {
                            url: wheel.url.to_string(),
                            hash: wheel.hash.clone(),
                        });
                    pkg_deps_per_cell
                        .entry(key.clone())
                        .or_default()
                        .insert(cfg_name.clone(), pkg.deps.clone());
                }
                PickResult::NoWheel => {
                    anyhow::bail!(
                        "package '{}-{}' has no wheel for cell ({}, {}) and S3 does \
                         not yet handle native sdists. Restrict the affected \
                         python_versions or platforms in muntjac.toml until S5 lands.",
                        pkg.name, pkg.version, resolved_cfg.python_version, plat_name
                    );
                }
            }
        }
    }

    // Cross-cell dep equality check + build EmitPackage list.
    let mut packages: Vec<EmitPackage> = Vec::new();
    for (key, wheel_map) in pkg_wheels {
        let cells_deps = &pkg_deps_per_cell[&key];
        let mut iter = cells_deps.iter();
        let (first_cell, first_deps) = iter.next().expect("at least one cell per package");
        for (cell, deps) in iter {
            if deps != first_deps {
                anyhow::bail!(
                    "package '{}-{}' has different deps across cells:\n  {} -> {:?}\n  {} -> {:?}\n\
                     Per-cell select()-driven deps are deferred to S4.",
                    key.0, key.1, first_cell, first_deps, cell, deps
                );
            }
        }
        let mut deps: Vec<String> = first_deps.iter().map(|d| format!(":{}", d)).collect();
        deps.sort();
        packages.push(EmitPackage {
            name: key.0,
            version: key.1,
            deps,
            wheels: wheel_map,
        });
    }

    packages.sort_by(|a, b| a.name.cmp(&b.name).then(a.version.cmp(&b.version)));

    Ok(EmitInput {
        tree: tree.name.clone(),
        third_party_dir: tree.third_party_dir.to_string_lossy().into_owned(),
        configs,
        packages,
    })
}
```

Notes for the implementer:
- `lock::graph::build`, `detect_cycles`, `lock::resolved::project` are the existing free functions used by `pick_wheels.rs`. Use the same import shape as that file. Adapt if names differ.
- The pseudocode treats `pkg.deps` as `Vec<String>` of dep names. The actual `ResolvedPackage.deps` shape might be different — inspect `src/lock/resolved.rs` and adapt. The composer needs the *reachable registry/git dep names* for the package in question.
- If `resolved_cfg.python_version` is already a `PythonVersion` struct (not a string), drop the `from_str` parsing.
- `tree.third_party_dir` is a `PathBuf` per S0; we render it to a string via `to_string_lossy()`. If the user has non-UTF-8 paths, that's their bug. (Validate upstream in `Config::validate` if it ever matters.)

- [ ] **Step 4: Run — passes**

Run: `cargo test --lib buck::emit::tests::build_emit_input_from_synthetic_resolved`
Expected: PASS.

Full suite: `cargo test 2>&1 | tail -5`. Expected: 18 buck tests + 119 prior = 137-ish.

- [ ] **Step 5: Commit**

```bash
git add src/buck/emit.rs
git commit -m "feat(s3): build_emit_input composes pipeline into EmitInput

Walks each (platform, python) cell in a tree, picks wheels, and
cross-merges per-package into BTreeMap<ConfigName, EmitWheel>. Sorted
by (name, version) then ConfigName for byte-stable output.

Cross-cell dep-set equality is checked (mismatches deferred to S4
with a clear error). NoWheel outcomes error pointing at S5."
```

---

### Task 12: Composer error paths

**Files:**
- Modify: `src/buck/emit.rs::tests`

The happy-path test confirms the composer works on clean input. T12 confirms the two error paths fire correctly.

- [ ] **Step 1: Write failing tests for both error paths**

Append to `src/buck/emit.rs::tests`:

```rust
#[test]
fn build_emit_input_errors_on_no_wheel() {
    // Synthetic lockfile with a package that has cp310-only wheels;
    // muntjac.toml says python_versions = ["3.12"]. NoWheel.

    use crate::config::{Config, Platform, PythonVersion, Tree};
    use crate::lock::types::{Lockfile, Package, Source, Wheel};
    use pep440_rs::Version;
    use pep508_rs::PackageName;
    use std::str::FromStr;
    use url::Url;

    let tree = Tree {
        name: "default".into(),
        manifest_path: "pyproject.toml".into(),
        third_party_dir: "third-party/python".into(),
        python_versions: vec![PythonVersion(3, 12)],
    };
    let mut platforms = std::collections::BTreeMap::new();
    platforms.insert("linux-x86_64-gnu".into(), Platform {
        target: "x86_64-unknown-linux-gnu".into(),
        manylinux: Some("2_17".into()),
        musllinux: None,
        macos_min: None,
    });
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
        packages: vec![Package {
            name: PackageName::from_str("ancient-pkg").unwrap(),
            version: Version::from_str("0.1.0").unwrap(),
            source: Source::Registry {
                url: Url::parse("https://pypi.org/simple").unwrap(),
            },
            dependencies: vec![],
            sdist: None,
            wheels: vec![Wheel {
                url: Url::parse("https://example.com/ancient_pkg-0.1.0-cp310-cp310-manylinux_2_17_x86_64.whl").unwrap(),
                hash: "sha256:aaaa".into(),
                size: None,
                filename: "ancient_pkg-0.1.0-cp310-cp310-manylinux_2_17_x86_64.whl".into(),
            }],
            metadata: None,
        }],
    };

    let err = build_emit_input(&config, &tree, &lockfile)
        .expect_err("should fail on NoWheel");
    let msg = format!("{:#}", err);
    assert!(msg.contains("ancient-pkg"), "error must name the package: {}", msg);
    assert!(msg.contains("3.12"), "error must name the python version: {}", msg);
    assert!(msg.contains("linux-x86_64-gnu"), "error must name the platform: {}", msg);
    assert!(msg.contains("S5"), "error must point at S5: {}", msg);
}
```

(Dep-set mismatch is harder to synthesize without marker-gated deps in the lockfile. We rely on integration tests + the inline equality check rather than a synthetic test. The composer's logic is straightforward enough that the integration tests are sufficient evidence.)

- [ ] **Step 2: Run — verify fails**

Run: `cargo test --lib buck::emit::tests::build_emit_input_errors_on_no_wheel`
Expected: PASS (the error path is already implemented in T11; this test just verifies the message shape).

If the test FAILS because the message is missing one of the required substrings, fix the message in `build_emit_input` to include them. Re-run.

- [ ] **Step 3: Commit**

```bash
git add src/buck/emit.rs
git commit -m "test(s3): build_emit_input errors on NoWheel with cell-naming message

Pins the NoWheel error shape: must include package, python version,
platform, and S5 forward-reference. Dep-set mismatch coverage relies
on the inline check + integration tests."
```

---

## Phase 6 — Filesystem writer

### Task 13: `write.rs` atomic file writer

**Files:**
- Create: `src/buck/write.rs`

Takes an `EmitOutput` plus a target directory; creates `config/` subdir; writes each output file atomically (write to `.tmp`, rename to final).

- [ ] **Step 1: Write the failing test**

Create `src/buck/write.rs`:

```rust
//! Filesystem writer for EmitOutput.

use std::path::Path;

use super::emit::EmitOutput;

pub fn write_outputs(output: &EmitOutput, third_party_dir: &Path) -> anyhow::Result<()> {
    use anyhow::Context;
    std::fs::create_dir_all(third_party_dir)
        .with_context(|| format!("creating {}", third_party_dir.display()))?;
    let config_dir = third_party_dir.join("config");
    std::fs::create_dir_all(&config_dir)
        .with_context(|| format!("creating {}", config_dir.display()))?;

    atomic_write(&third_party_dir.join("BUCK"), &output.buck)?;
    atomic_write(&third_party_dir.join("muntjac.bzl"), &output.muntjac_bzl)?;
    atomic_write(&third_party_dir.join("PACKAGE"), &output.package_file)?;
    atomic_write(&config_dir.join("BUCK"), &output.config_buck)?;
    Ok(())
}

fn atomic_write(path: &Path, contents: &str) -> anyhow::Result<()> {
    use anyhow::Context;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, contents)
        .with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .with_context(|| format!("renaming {} -> {}", tmp.display(), path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_outputs_creates_all_four_files() {
        let tmp = tempfile::tempdir().unwrap();
        let tpd = tmp.path().join("third-party/python");

        let out = EmitOutput {
            buck: "BUCK_BODY\n".into(),
            muntjac_bzl: "BZL_BODY\n".into(),
            config_buck: "CONFIG_BODY\n".into(),
            package_file: "PACKAGE_BODY\n".into(),
        };

        write_outputs(&out, &tpd).unwrap();

        assert_eq!(std::fs::read_to_string(tpd.join("BUCK")).unwrap(), "BUCK_BODY\n");
        assert_eq!(std::fs::read_to_string(tpd.join("muntjac.bzl")).unwrap(), "BZL_BODY\n");
        assert_eq!(std::fs::read_to_string(tpd.join("PACKAGE")).unwrap(), "PACKAGE_BODY\n");
        assert_eq!(std::fs::read_to_string(tpd.join("config/BUCK")).unwrap(), "CONFIG_BODY\n");
        // No leftover .tmp files
        for entry in std::fs::read_dir(&tpd).unwrap() {
            let path = entry.unwrap().path();
            assert!(path.extension().map_or(true, |e| e != "tmp"),
                "leftover .tmp file: {:?}", path);
        }
    }

    #[test]
    fn write_outputs_overwrites_existing_files() {
        let tmp = tempfile::tempdir().unwrap();
        let tpd = tmp.path().join("third-party/python");

        let out1 = EmitOutput {
            buck: "OLD\n".into(),
            muntjac_bzl: String::new(),
            config_buck: String::new(),
            package_file: String::new(),
        };
        write_outputs(&out1, &tpd).unwrap();
        assert_eq!(std::fs::read_to_string(tpd.join("BUCK")).unwrap(), "OLD\n");

        let out2 = EmitOutput {
            buck: "NEW\n".into(),
            muntjac_bzl: String::new(),
            config_buck: String::new(),
            package_file: String::new(),
        };
        write_outputs(&out2, &tpd).unwrap();
        assert_eq!(std::fs::read_to_string(tpd.join("BUCK")).unwrap(), "NEW\n");
    }
}
```

`use super::emit::EmitOutput;` ensures the type resolves; `pub mod write;` was added in T4's mod.rs.

- [ ] **Step 2: Verify tempfile is a dev-dep**

Run: `grep tempfile Cargo.toml`
Expected: present.

- [ ] **Step 3: Run — passes**

Run: `cargo test --lib buck::write`
Expected: 2 tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/buck/write.rs
git commit -m "feat(s3): atomic file writer for EmitOutput

write_outputs creates the target dir + config/ subdir, then writes
each of the four output files atomically (write to .tmp + rename).
Overwrites existing files unconditionally — they're @generated."
```

---

## Phase 7 — CLI wiring

### Task 14: Wire `Command::Buckify` to the new pipeline

**Files:**
- Create: `src/cli/buckify.rs`
- Modify: `src/cli/mod.rs` (replace `stub::run("buckify", "S3+")` with `buckify::run(...)`)

- [ ] **Step 1: Create `src/cli/buckify.rs`**

```rust
//! `muntjac buckify` — read uv.lock + muntjac.toml, emit BUCK + muntjac.bzl + config/BUCK + PACKAGE.

use anyhow::Context;
use std::str::FromStr;

use crate::buck::{build_emit_input, write::write_outputs, BuckEmitter, StringTemplateEmitter};
use crate::cli::Globals;
use crate::config::Config;
use crate::lock::parser::parse as parse_lockfile;

pub fn run(globals: &Globals) -> anyhow::Result<()> {
    let workdir = globals.workdir().context("resolving working directory")?;
    let config_text = std::fs::read_to_string(workdir.join("muntjac.toml"))
        .context("reading muntjac.toml")?;
    let config = Config::from_str(&config_text)
        .context("parsing muntjac.toml")?;

    let emitter = StringTemplateEmitter;

    for tree in &config.trees {
        if let Some(filter) = &globals.tree {
            if &tree.name != filter {
                continue;
            }
        }

        // Resolve uv.lock path relative to manifest_path's directory.
        let manifest = workdir.join(&tree.manifest_path);
        let lock_path = manifest
            .parent()
            .unwrap_or(workdir.as_path())
            .join("uv.lock");
        let lock_text = std::fs::read_to_string(&lock_path)
            .with_context(|| format!("reading {}", lock_path.display()))?;
        let lockfile = parse_lockfile(&lock_text)
            .with_context(|| format!("parsing {}", lock_path.display()))?;

        let input = build_emit_input(&config, tree, &lockfile)?;
        let output = emitter.emit(&input);

        let third_party_dir = workdir.join(&tree.third_party_dir);
        write_outputs(&output, &third_party_dir)?;
    }
    Ok(())
}
```

Notes:
- `crate::lock::parser::parse` is the existing entry point used by `pick_wheels.rs`. Use whatever it's actually named.
- Add `pub use emit::build_emit_input;` to `src/buck/mod.rs` so the call site at `crate::buck::build_emit_input` resolves.
- The `make_emitter` could be parameterized by config later (a `buck.emitter` setting); for now, hard-code `StringTemplateEmitter`.

- [ ] **Step 2: Wire into Command::Buckify in `src/cli/mod.rs`**

Find the dispatch for `Command::Buckify` (currently `stub::run("buckify", "S3+")`). Replace with:

```rust
Command::Buckify => buckify::run(&cli.globals),
```

Add `pub mod buckify;` at the top of `src/cli/mod.rs` alongside other CLI modules.

- [ ] **Step 3: Smoke test — build compiles, help works**

Run: `cargo build 2>&1 | tail -5`
Expected: PASS.

Run: `cargo run --quiet -- buckify --help 2>&1 | head -10`
Expected: clap prints the subcommand help. (No extra flags in S3 beyond globals.)

- [ ] **Step 4: Run full test suite**

Run: `cargo test 2>&1 | tail -5`
Expected: all prior tests still pass (~138 lib + buck tests; integration tests in T15/T16).

- [ ] **Step 5: Commit**

```bash
git add src/cli/buckify.rs src/cli/mod.rs src/buck/mod.rs
git commit -m "feat(s3): wire buckify command to the new pipeline

cli/buckify.rs::run loads config, iterates trees (with --tree filter),
parses each tree's uv.lock, composes EmitInput via build_emit_input,
emits via StringTemplateEmitter, writes via write_outputs."
```

---

## Phase 8 — Integration fixtures

### Task 15: Fixture `01-pure-python` golden

**Files:**
- Create: `tests/fixtures/buck/01-pure-python/{muntjac.toml, pyproject.toml, uv.lock, expected/}`
- Create: `tests/fixtures/buck/README.md`
- Create: `tests/buckify.rs`

- [ ] **Step 1: Generate the lockfile**

```bash
mkdir -p /tmp/muntjac-buck-fixture-01 && cd /tmp/muntjac-buck-fixture-01
cat > pyproject.toml <<'EOF'
[project]
name = "fixture-01"
version = "0.1.0"
requires-python = ">=3.11"
dependencies = ["certifi==2025.4.26"]
EOF
uv lock 2>&1 | tail -3
```

(Use `certifi` or another small pure-python package. Pick one that's compact in the lockfile.)

Copy `pyproject.toml` and `uv.lock` into `tests/fixtures/buck/01-pure-python/`.

- [ ] **Step 2: Write `muntjac.toml`**

`tests/fixtures/buck/01-pure-python/muntjac.toml`:

```toml
manifest_path   = "pyproject.toml"
third_party_dir = "third-party/python"
python_versions = ["3.11", "3.12"]

[platforms.linux-x86_64-gnu]
target = "x86_64-unknown-linux-gnu"
manylinux = "2_17"
```

- [ ] **Step 3: Write fixture README**

`tests/fixtures/buck/README.md`:

```markdown
# BUCK emitter fixtures

Each directory contains:
- `muntjac.toml` — config (platforms + python versions to exercise)
- `pyproject.toml` — input used to regenerate `uv.lock`
- `uv.lock` — **frozen artifact**
- `expected/` — golden output files (BUCK, muntjac.bzl, PACKAGE, config/BUCK)

See `tests/fixtures/lock/README.md` for the broader frozen-artifact
convention. The same rule applies: regenerating `uv.lock` requires
regenerating all goldens in the same commit.

## Regenerating goldens

If the BUCK emitter output shape changes:

```bash
cd tests/fixtures/buck/<fixture>
rm -rf third-party/  # clean any prior buckify output
cargo run --quiet --manifest-path=../../../../Cargo.toml -- \
  -C $PWD buckify
mv third-party/python/* expected/   # or diff + manually update
```
```

- [ ] **Step 4: Create integration test framework**

Create `tests/buckify.rs`:

```rust
//! Integration tests for `muntjac buckify`.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/buck")
        .join(name)
}

fn copy_fixture_to(src: &Path, dst: &Path) {
    fn copy_dir(src: &Path, dst: &Path) {
        std::fs::create_dir_all(dst).unwrap();
        for entry in std::fs::read_dir(src).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let name = entry.file_name();
            if path.is_dir() {
                // Skip `expected/` and `third-party/` — they're test outputs.
                if name == "expected" || name == "third-party" {
                    continue;
                }
                copy_dir(&path, &dst.join(&name));
            } else {
                std::fs::copy(&path, dst.join(&name)).unwrap();
            }
        }
    }
    copy_dir(src, dst);
}

fn run_buckify(workdir: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_muntjac"))
        .arg("-C").arg(workdir)
        .arg("buckify")
        .output()
        .expect("run muntjac buckify")
}

fn assert_files_match(out_dir: &Path, golden_dir: &Path) {
    for entry in walkdir::WalkDir::new(golden_dir) {
        let entry = entry.unwrap();
        if !entry.file_type().is_file() {
            continue;
        }
        let rel = entry.path().strip_prefix(golden_dir).unwrap();
        let out_path = out_dir.join(rel);
        assert!(out_path.exists(), "output file missing: {}", out_path.display());
        let actual = std::fs::read_to_string(&out_path).unwrap();
        let expected = std::fs::read_to_string(entry.path()).unwrap();
        assert_eq!(actual, expected, "diff at {}", rel.display());
    }
}

#[test]
fn fixture_01_pure_python_golden() {
    let fix = fixture("01-pure-python");
    let tmp = tempfile::tempdir().unwrap();
    copy_fixture_to(&fix, tmp.path());
    let out = run_buckify(tmp.path());
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    assert_files_match(&tmp.path().join("third-party/python"), &fix.join("expected"));
}
```

Note: `walkdir` may or may not already be a dev-dep. If not: `cargo add --dev walkdir`.

- [ ] **Step 5: Run buckify against the fixture, generate the golden files**

```bash
cd tests/fixtures/buck/01-pure-python
mkdir -p expected expected/config
# Run from /tmp to avoid polluting the fixture dir with state we don't want committed.
tmp=$(mktemp -d)
cp pyproject.toml uv.lock muntjac.toml "$tmp/"
cargo run --quiet --manifest-path=../../../../Cargo.toml -- -C "$tmp" buckify
# Copy generated files to expected/
cp "$tmp/third-party/python/BUCK"        expected/BUCK
cp "$tmp/third-party/python/muntjac.bzl" expected/muntjac.bzl
cp "$tmp/third-party/python/PACKAGE"     expected/PACKAGE
cp "$tmp/third-party/python/config/BUCK" expected/config/BUCK
```

Sanity-check the generated files:
- `BUCK`: contains one `pypi_package(name = "certifi", ...)` call with two wheels-dict entries (py311, py312).
- `muntjac.bzl`: `_CONFIGS = ["py311-linux-x86_64-gnu", "py312-linux-x86_64-gnu"]`.
- `config/BUCK`: two `config_setting` blocks + `python_version` axis with py311/py312, `platform` axis with linux-x86_64-gnu.
- `PACKAGE`: placeholder header.

If anything looks wrong, STOP and investigate emit functions.

- [ ] **Step 6: Run the integration test**

Run: `cargo test --test buckify fixture_01_pure_python_golden 2>&1 | tail -10`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add tests/fixtures/buck/ tests/buckify.rs Cargo.toml
git commit -m "test(s3): fixture 01-pure-python golden + integration test framework

Real certifi lockfile, 1 platform x 2 pythons. Goldens cover all four
output files. Test copies fixture into tempdir, runs muntjac buckify,
and byte-compares each file against expected/."
```

---

### Task 16: Fixture `10-determinism`

**Files:**
- Modify: `tests/buckify.rs`
- (Optional) Create: `tests/fixtures/buck/10-determinism/` — or reuse 01

- [ ] **Step 1: Decide reuse vs. duplicate**

The simplest approach: the `10-determinism` test reuses `01-pure-python`'s inputs and just asserts byte-identical output across two runs. No new fixture directory needed.

If you'd rather have an explicit `10-determinism/` directory pointing at the same lockfile, that's fine too but adds maintenance — the inputs would need to be kept in sync. Default: reuse 01.

- [ ] **Step 2: Add the determinism test**

Append to `tests/buckify.rs`:

```rust
#[test]
fn fixture_10_determinism_two_runs_byte_identical() {
    let fix = fixture("01-pure-python");

    let tmp_a = tempfile::tempdir().unwrap();
    let tmp_b = tempfile::tempdir().unwrap();
    copy_fixture_to(&fix, tmp_a.path());
    copy_fixture_to(&fix, tmp_b.path());

    let out_a = run_buckify(tmp_a.path());
    assert!(out_a.status.success(), "run a failed: {}", String::from_utf8_lossy(&out_a.stderr));
    let out_b = run_buckify(tmp_b.path());
    assert!(out_b.status.success(), "run b failed: {}", String::from_utf8_lossy(&out_b.stderr));

    let tpd_a = tmp_a.path().join("third-party/python");
    let tpd_b = tmp_b.path().join("third-party/python");

    for rel in ["BUCK", "muntjac.bzl", "PACKAGE", "config/BUCK"] {
        let bytes_a = std::fs::read(tpd_a.join(rel)).unwrap();
        let bytes_b = std::fs::read(tpd_b.join(rel)).unwrap();
        assert_eq!(bytes_a, bytes_b, "{} differs across runs", rel);
    }
}
```

- [ ] **Step 3: Run the test**

Run: `cargo test --test buckify fixture_10_determinism 2>&1 | tail -10`
Expected: PASS.

Full suite: `cargo test 2>&1 | tail -5`. Expected: ~140 tests pass.

- [ ] **Step 4: Commit**

```bash
git add tests/buckify.rs
git commit -m "test(s3): fixture 10 — byte-identical output across two buckify runs

Reuses fixture 01-pure-python's inputs. Runs buckify into two
tempdirs and asserts every output file is byte-identical."
```

---

## Phase 9 — Close out

### Task 17: Update `TECH_DEBT.md`

**Files:**
- Modify: `docs/superpowers/TECH_DEBT.md`

Move three items to `## Resolved` with commit SHAs.

- [ ] **Step 1: Identify the closing commits**

Run: `git log --oneline | head -25` to find the SHAs for:
- T1 (`render_tag` consolidation): closes "`render_tag` duplicated between handler and snapshot tests".
- T2 (`MIN_SUPPORTED_PY_MINOR`): closes "`expand_requires_python` floor of 3.11 is hard-coded".
- T3 (`#[serde(default)]`): closes "`RawConfig::platforms` has no `#[serde(default)]`".

- [ ] **Step 2: Move the three items**

Read `docs/superpowers/TECH_DEBT.md`. For each of the three resolved items, cut from `## Open` and paste into `## Resolved` with this format:

```markdown
### <item title>
- **Resolved:** S3, commit `<sha>`
- **Summary:** <1-2 sentence summary of the fix>
```

Example for the render_tag item:

```markdown
### `render_tag` duplicated between handler and snapshot tests
- **Resolved:** S3, commit `<T1-sha>`
- **Summary:** Implemented `Display` for `Tag`/`PythonTag`/`AbiTag`/`PlatformTag`. The duplicate `render_tag` in `src/cli/debug/pick_wheels.rs` and the test-only `render_*` helpers in `src/wheel/compat.rs` were both deleted; all call sites now use `tag.to_string()`.
```

The `Open` section should retain everything else.

- [ ] **Step 3: Commit**

```bash
git add docs/superpowers/TECH_DEBT.md
git commit -m "docs: move 3 S3-resolved tech-debt items to Resolved section

render_tag duplication (Display for Tag), expand_requires_python
floor doc, and RawConfig::platforms #[serde(default)] all closed
in S3."
```

---

### Task 18: Mark S3 shipped in roadmap + tag

**Files:**
- Modify: `docs/superpowers/specs/2026-05-20-muntjac-roadmap.md`

- [ ] **Step 1: Count commits + tests**

```bash
# Count commits since S2's head
git log --oneline 137f7a2..HEAD | wc -l
# Confirm test count
cargo test 2>&1 | grep "test result" | awk '{sum_p += $4} END {print sum_p}'
```

Note the numbers for the roadmap row.

- [ ] **Step 2: Update the roadmap S3 row**

Find:

```
| S3 | (not yet written) | (not yet written) | ⬜ next |
| S4 | (not yet written) | (not yet written) | ⬜ blocked on S3 |
```

Replace with:

```
| S3 | [2026-05-21-muntjac-s3-buck-emitter-design.md](./2026-05-21-muntjac-s3-buck-emitter-design.md) | [2026-05-21-muntjac-s3-buck-emitter.md](../plans/2026-05-21-muntjac-s3-buck-emitter.md) | ✅ shipped (tag `s3-complete`, <N> commits, <M> tests) |
| S4 | (not yet written) | (not yet written) | ⬜ next |
```

Substitute the actual `<N>` (commit count) and `<M>` (test count) from Step 1.

- [ ] **Step 3: Final verification**

```bash
cargo test 2>&1 | tail -5
cargo clippy --all-targets -- -D warnings 2>&1 | tail -5
cargo fmt --check 2>&1 | tail -5
```

All should pass with no warnings.

- [ ] **Step 4: Tag and commit**

```bash
git add docs/superpowers/specs/2026-05-20-muntjac-roadmap.md
git commit -m "docs: mark S3 shipped in roadmap index

S3 (first BUCK emitter, pure-python single-platform) shipped.
Tagged s3-complete. S4 (multi-platform BUCK) is next."
git tag s3-complete
```

---

## Verification gate

Before declaring S3 complete:

```bash
cargo test                                 # all ~140 tests pass
cargo clippy --all-targets -- -D warnings  # no warnings
cargo fmt --check                          # formatting clean
```

Hand-verify the demo:

```bash
tmp=$(mktemp -d)
cp tests/fixtures/buck/01-pure-python/{muntjac.toml,pyproject.toml,uv.lock} "$tmp/"
cargo run --quiet -- -C "$tmp" buckify
ls -la "$tmp/third-party/python/"  # BUCK, muntjac.bzl, PACKAGE, config/
cat "$tmp/third-party/python/BUCK"  # human-readable, alphabetic, with @generated header
```

Re-run buckify; output should be byte-identical to the first run.

Exit criteria from spec §11 all hold green.
