# S11 — Multi-tree support — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make muntjac's already-partial multi-tree support compose at the Buck level by emitting the shared cfg machinery once (not per-tree), fill the per-command gaps (`vendor`, `fixups show`), add validation, and prove it with a two-tree fixture that builds conflicting numpy versions in one Buck project.

**Architecture:** The cfg axis (python_version × platform constraint/config_setting/wiring machinery) moves from per-tree emission to a single shared `cfg_dir` location, so the root `PACKAGE`'s single `set_cfg_modifiers` call composes. `cfg_dir` defaults to the longest common ancestor of all trees' `third_party_dir`s (single tree → its own dir, so output stays byte-identical). Per-tree emission keeps `BUCK` + `muntjac.bzl`, referencing config_settings by `cfg_dir`-relative labels.

**Tech Stack:** Rust 2024, `thiserror` typed errors, `serde`/`toml` config, `insta` snapshots, Buck2 + Starlark emission, `assert_cmd` integration tests.

**Spec:** [`docs/superpowers/specs/2026-05-25-muntjac-s11-multi-tree-design.md`](../specs/2026-05-25-muntjac-s11-multi-tree-design.md)

---

## File Structure

**Modify:**
- `src/config.rs` — `cfg_dir` field on `BuckConfig`; `Config::cfg_dir()` derivation; `python_versions_union()`; two validation rules.
- `src/error.rs` — two `ConfigError` variants (`DuplicateTreeDir`, `TreeDirIsCfgDir`).
- `src/buck/emit.rs` — `EmitInput.cfg_dir` field; `SharedCfgInput` struct + `build_shared_cfg_input`; drop cfg fields from `EmitOutput`; add `SharedCfgOutput`.
- `src/buck/string_writer.rs` — switch config-label sites from `third_party_dir` to `cfg_dir`; extract `emit_config_buck`/`emit_wiring_bzl` into a standalone `emit_shared_cfg`.
- `src/buck/mod.rs` — `write_outputs` drops cfg files; new `write_shared_cfg`; re-exports.
- `src/cli/mod.rs` — `resolve_trees` helper.
- `src/cli/buckify.rs` — orchestrate shared-cfg-once + per-tree packages.
- `src/cli/vendor.rs` — loop trees via `resolve_trees`.
- `src/cli/fixups.rs` — `show` per-tree blocks via `resolve_trees`.
- `.github/workflows/ci.yml` — `10-multi-tree` buckify + buck2 build steps.
- `README.md`, `CHANGELOG.md`, `Cargo.toml`, roadmap — docs + version.

**Create:**
- `tests/fixtures/buck/10-multi-tree/` — two-tree fixture + expected outputs + smoke targets.
- Snapshot test in `tests/buckify.rs`.

---

## Task 1: `[buck] cfg_dir` field + `Config::cfg_dir()` derivation

**Files:**
- Modify: `src/config.rs` (BuckConfig ~148-166; add methods on `impl Config`)

- [ ] **Step 1: Write failing tests for cfg_dir derivation**

Add to `src/config.rs` `mod tests` (near the existing `parses_multi_tree` test ~493):

```rust
#[test]
fn cfg_dir_single_tree_is_third_party_dir() {
    let toml = r#"
manifest_path = "../pyproject.toml"
third_party_dir = "third-party/python"
python_versions = ["3.12"]
[platforms]
macos-arm64 = { target = "aarch64-apple-darwin", macos_min = "11.0" }
"#;
    let config: Config = toml.parse().unwrap();
    assert_eq!(config.cfg_dir(), std::path::PathBuf::from("third-party/python"));
}

#[test]
fn cfg_dir_multi_tree_is_common_ancestor() {
    let toml = r#"
[platforms]
macos-arm64 = { target = "aarch64-apple-darwin", macos_min = "11.0" }
[tree.modern]
manifest_path = "pyproject.toml"
third_party_dir = "third-party/python/modern"
python_versions = ["3.12"]
[tree.legacy]
manifest_path = "legacy/pyproject.toml"
third_party_dir = "third-party/python/legacy"
python_versions = ["3.12"]
"#;
    let config: Config = toml.parse().unwrap();
    assert_eq!(config.cfg_dir(), std::path::PathBuf::from("third-party/python"));
}

#[test]
fn cfg_dir_explicit_override_wins() {
    let toml = r#"
[platforms]
macos-arm64 = { target = "aarch64-apple-darwin", macos_min = "11.0" }
[buck]
cfg_dir = "buck/cfg"
[tree.a]
manifest_path = "a/pyproject.toml"
third_party_dir = "apps/a/tp"
python_versions = ["3.12"]
[tree.b]
manifest_path = "b/pyproject.toml"
third_party_dir = "apps/b/tp"
python_versions = ["3.12"]
"#;
    let config: Config = toml.parse().unwrap();
    assert_eq!(config.cfg_dir(), std::path::PathBuf::from("buck/cfg"));
}

#[test]
fn python_versions_union_dedupes_and_sorts() {
    let toml = r#"
[platforms]
macos-arm64 = { target = "aarch64-apple-darwin", macos_min = "11.0" }
[tree.modern]
manifest_path = "pyproject.toml"
third_party_dir = "tp/modern"
python_versions = ["3.12", "3.11"]
[tree.legacy]
manifest_path = "legacy/pyproject.toml"
third_party_dir = "tp/legacy"
python_versions = ["3.11"]
"#;
    let config: Config = toml.parse().unwrap();
    assert_eq!(
        config.python_versions_union(),
        vec![PythonVersion(3, 11), PythonVersion(3, 12)]
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib config::tests::cfg_dir 2>&1 | tail -20`
Expected: FAIL — `no method named cfg_dir` / `python_versions_union`.

- [ ] **Step 3: Add `cfg_dir` field to `BuckConfig`**

In `src/config.rs`, `BuckConfig` (currently ~148-153) becomes:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct BuckConfig {
    #[serde(default = "default_buck_file_name")]
    pub file_name: String,
    #[serde(default)]
    pub vendor: bool,
    /// Where the shared cfg (config/ + wiring.bzl) is written. `None` →
    /// derived as the longest common ancestor of trees' third_party_dirs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cfg_dir: Option<PathBuf>,
}
```

Update the `Default` impl (~155-162) to add `cfg_dir: None,`.

- [ ] **Step 4: Add `cfg_dir()` + `python_versions_union()` methods**

Add a new `impl Config` block in `src/config.rs` (after the `dedupe_include_groups` block ~315):

```rust
impl Config {
    /// Resolve where the shared cfg (config/ + wiring.bzl) is written.
    /// Explicit `[buck] cfg_dir` wins; else the longest common path-ancestor
    /// of all trees' third_party_dirs. For a single tree this is that tree's
    /// own third_party_dir, keeping output byte-identical to pre-S11.
    pub fn cfg_dir(&self) -> PathBuf {
        if let Some(explicit) = &self.buck.cfg_dir {
            return explicit.clone();
        }
        let mut dirs = self.trees.iter().map(|t| t.third_party_dir.as_path());
        let first = dirs.next().expect("validated: at least one tree");
        let mut common: Vec<std::path::Component> = first.components().collect();
        for d in dirs {
            let comps: Vec<_> = d.components().collect();
            let keep = common
                .iter()
                .zip(comps.iter())
                .take_while(|(a, b)| a == b)
                .count();
            common.truncate(keep);
        }
        common.iter().collect()
    }

    /// Union of every tree's python_versions, sorted and deduped.
    pub fn python_versions_union(&self) -> Vec<PythonVersion> {
        let mut set: std::collections::BTreeSet<PythonVersion> =
            std::collections::BTreeSet::new();
        for t in &self.trees {
            for v in &t.python_versions {
                set.insert(v.clone());
            }
        }
        set.into_iter().collect()
    }
}
```

Note: `PythonVersion` needs `Ord`/`PartialOrd` for `BTreeSet`. It currently derives `Hash` but check for `Ord` — if absent, add `PartialOrd, Ord` to its derive list (it's a `(u8, u8)` tuple struct ~174, so derived ordering is correct: major then minor).

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib config::tests 2>&1 | tail -20`
Expected: PASS, including the four new tests and all pre-existing config tests.

- [ ] **Step 6: Commit**

```sh
cd /home/jackm/repos/muntjac
git add src/config.rs
git commit -m "$(cat <<'EOF'
feat(s11): [buck] cfg_dir field + Config::cfg_dir/python_versions_union

cfg_dir resolves where the shared cfg machinery lands: explicit
[buck] cfg_dir wins, else the longest common ancestor of trees'
third_party_dirs. Single tree → its own dir (byte-identical to today).
python_versions_union collects + sorts + dedupes across trees for the
shared config_setting grid.
EOF
)"
```

---

## Task 2: Validation rules — DuplicateTreeDir + TreeDirIsCfgDir

**Files:**
- Modify: `src/error.rs` (ConfigError ~6-36)
- Modify: `src/config.rs` (`Config::validate` ~317)

- [ ] **Step 1: Write failing tests**

Add to `src/config.rs` `mod tests`:

```rust
#[test]
fn rejects_duplicate_tree_dir() {
    let toml = r#"
[platforms]
macos-arm64 = { target = "aarch64-apple-darwin", macos_min = "11.0" }
[tree.a]
manifest_path = "a/pyproject.toml"
third_party_dir = "tp/shared"
python_versions = ["3.12"]
[tree.b]
manifest_path = "b/pyproject.toml"
third_party_dir = "tp/shared"
python_versions = ["3.12"]
"#;
    let err = toml.parse::<Config>().unwrap_err();
    assert!(matches!(err, crate::error::ConfigError::DuplicateTreeDir { .. }));
}

#[test]
fn rejects_tree_dir_equal_to_cfg_dir_when_multi_tree() {
    // cfg_dir derives to "tp" (common ancestor of tp + tp/legacy); tree "a"
    // sits exactly at "tp", colliding with the shared cfg location.
    let toml = r#"
[platforms]
macos-arm64 = { target = "aarch64-apple-darwin", macos_min = "11.0" }
[tree.a]
manifest_path = "a/pyproject.toml"
third_party_dir = "tp"
python_versions = ["3.12"]
[tree.legacy]
manifest_path = "legacy/pyproject.toml"
third_party_dir = "tp/legacy"
python_versions = ["3.12"]
"#;
    let err = toml.parse::<Config>().unwrap_err();
    assert!(matches!(err, crate::error::ConfigError::TreeDirIsCfgDir { .. }));
}

#[test]
fn single_tree_cfg_dir_equals_third_party_dir_is_ok() {
    // Single tree: cfg_dir == third_party_dir by construction; must NOT error.
    let toml = r#"
manifest_path = "../pyproject.toml"
third_party_dir = "third-party/python"
python_versions = ["3.12"]
[platforms]
macos-arm64 = { target = "aarch64-apple-darwin", macos_min = "11.0" }
"#;
    assert!(toml.parse::<Config>().is_ok());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib config::tests::rejects_duplicate_tree_dir config::tests::rejects_tree_dir config::tests::single_tree_cfg 2>&1 | tail -20`
Expected: FAIL — variants don't exist / no error raised.

- [ ] **Step 3: Add the ConfigError variants**

In `src/error.rs`, add to `enum ConfigError` (after `IncompatibleShape` ~16):

```rust
    #[error("trees {trees:?} share third_party_dir `{dir}`; each tree needs a distinct directory")]
    DuplicateTreeDir { dir: String, trees: Vec<String> },

    #[error(
        "tree `{tree}`'s third_party_dir `{dir}` collides with the shared cfg_dir; move the tree under a subdirectory or set [buck] cfg_dir explicitly"
    )]
    TreeDirIsCfgDir { tree: String, dir: String },
```

- [ ] **Step 4: Add the checks to `Config::validate`**

In `src/config.rs`, near the top of `Config::validate` (before the platform loop ~319), add:

```rust
        // Reject two trees sharing a third_party_dir (they'd clobber output).
        {
            let mut by_dir: std::collections::BTreeMap<PathBuf, Vec<String>> =
                std::collections::BTreeMap::new();
            for t in &self.trees {
                by_dir
                    .entry(t.third_party_dir.clone())
                    .or_default()
                    .push(t.name.clone());
            }
            if let Some((dir, names)) = by_dir.iter().find(|(_, v)| v.len() > 1) {
                return Err(crate::error::ConfigError::DuplicateTreeDir {
                    dir: dir.display().to_string(),
                    trees: names.clone(),
                });
            }
        }
        // Multi-tree only: reject a tree whose dir equals the shared cfg_dir.
        if self.trees.len() > 1 {
            let cfg_dir = self.cfg_dir();
            if let Some(t) = self.trees.iter().find(|t| t.third_party_dir == cfg_dir) {
                return Err(crate::error::ConfigError::TreeDirIsCfgDir {
                    tree: t.name.clone(),
                    dir: t.third_party_dir.display().to_string(),
                });
            }
        }
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib config::tests 2>&1 | tail -20`
Expected: PASS (new + existing).

- [ ] **Step 6: Add error-message exactness tests**

Add to `src/error.rs` `mod tests`:

```rust
    #[test]
    fn duplicate_tree_dir_message_is_exact() {
        let e = ConfigError::DuplicateTreeDir {
            dir: "tp/shared".into(),
            trees: vec!["a".into(), "b".into()],
        };
        assert_eq!(
            e.to_string(),
            "trees [\"a\", \"b\"] share third_party_dir `tp/shared`; each tree needs a distinct directory"
        );
    }

    #[test]
    fn tree_dir_is_cfg_dir_message_is_exact() {
        let e = ConfigError::TreeDirIsCfgDir {
            tree: "a".into(),
            dir: "tp".into(),
        };
        assert_eq!(
            e.to_string(),
            "tree `a`'s third_party_dir `tp` collides with the shared cfg_dir; move the tree under a subdirectory or set [buck] cfg_dir explicitly"
        );
    }
```

Run: `cargo test --lib error::tests 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 7: Commit**

```sh
cd /home/jackm/repos/muntjac
git add src/error.rs src/config.rs
git commit -m "$(cat <<'EOF'
feat(s11): validate tree dir collisions

DuplicateTreeDir: two trees can't share a third_party_dir (clobber).
TreeDirIsCfgDir: a tree's dir can't equal the shared cfg_dir (multi-tree
only) — its package output would collide with config/ + wiring.bzl.
Single-tree cfg_dir == third_party_dir stays valid.
EOF
)"
```

---

## Task 3: `resolve_trees` helper

**Files:**
- Modify: `src/cli/mod.rs` (add helper + re-export)
- Test: `tests/multi_tree.rs` (NEW integration test file)

- [ ] **Step 1: Write the helper**

Add to `src/cli/mod.rs` (after the `Globals` impl ~60):

```rust
use crate::config::{Config, Tree};

/// Resolve which trees a command operates on. `None` → all trees;
/// `Some(name)` → just that tree, or an error naming available trees.
pub fn resolve_trees<'a>(
    config: &'a Config,
    tree_filter: Option<&str>,
) -> Result<Vec<&'a Tree>> {
    match tree_filter {
        None => Ok(config.trees.iter().collect()),
        Some(name) => match config.trees.iter().find(|t| t.name == name) {
            Some(t) => Ok(vec![t]),
            None => {
                let available: Vec<&str> =
                    config.trees.iter().map(|t| t.name.as_str()).collect();
                anyhow::bail!(
                    "tree `{}` not found in muntjac.toml; available: {}",
                    name,
                    available.join(", ")
                )
            }
        },
    }
}
```

- [ ] **Step 2: Write a failing integration test**

Create `tests/multi_tree.rs`:

```rust
mod common;

use assert_cmd::prelude::*;
use common::muntjac;
use predicates::str::contains;
use std::fs;
use tempfile::tempdir;

const TWO_TREE_TOML: &str = r#"
[platforms]
macos-arm64 = { target = "aarch64-apple-darwin", macos_min = "11.0" }
[tree.modern]
manifest_path = "modern/pyproject.toml"
third_party_dir = "tp/modern"
python_versions = ["3.12"]
[tree.legacy]
manifest_path = "legacy/pyproject.toml"
third_party_dir = "tp/legacy"
python_versions = ["3.12"]
"#;

#[test]
fn buckify_unknown_tree_errors_with_available_names() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("muntjac.toml"), TWO_TREE_TOML).unwrap();
    muntjac()
        .arg("-C")
        .arg(dir.path())
        .arg("--tree")
        .arg("ghost")
        .arg("buckify")
        .assert()
        .failure()
        .stderr(contains("tree `ghost` not found"))
        .stderr(contains("modern"))
        .stderr(contains("legacy"));
}
```

- [ ] **Step 3: Run to verify it fails**

Run: `cargo test --test multi_tree buckify_unknown_tree 2>&1 | tail -20`
Expected: FAIL — buckify currently skips unknown trees silently (exits success with no output).

(The test passes once Task 5 wires `resolve_trees` into buckify. If you're running tasks in order, this test stays red until Task 5 — that's expected; note it and proceed. The helper itself compiles now.)

- [ ] **Step 4: Verify the helper compiles + unit-covers**

Add a unit test to `src/cli/mod.rs` `#[cfg(test)] mod tests` (create the module if absent):

```rust
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
}
```

Run: `cargo test --lib cli::tests::resolve_trees 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Commit**

```sh
cd /home/jackm/repos/muntjac
git add src/cli/mod.rs tests/multi_tree.rs
git commit -m "$(cat <<'EOF'
feat(s11): resolve_trees helper for uniform --tree scoping

None → all trees; Some(name) → that tree or an error listing available
names. Replaces the divergent .first()/skip logic across commands.
The buckify integration test stays red until Task 5 wires it in.
EOF
)"
```

---

## Task 4: Emitter split — cfg_dir field + shared-cfg emission

**Files:**
- Modify: `src/buck/emit.rs` (EmitInput ~10-16, EmitOutput ~88-94)
- Modify: `src/buck/string_writer.rs` (label sites + extract shared-cfg fns)

This is the architectural core. The label base for cfg references switches from `third_party_dir` to a new `cfg_dir`, and the cfg files are produced by a standalone emitter.

- [ ] **Step 1: Add `cfg_dir` to `EmitInput`; split `EmitOutput`**

In `src/buck/emit.rs`, `EmitInput` (~10-16) becomes:

```rust
#[derive(Debug, Clone)]
pub struct EmitInput {
    pub tree: String,
    pub third_party_dir: String,
    /// Cell-relative path to the shared cfg package (config_settings +
    /// wiring.bzl). Single tree: equals `third_party_dir`. Used for
    /// `//<cfg_dir>/config:...` and `//<cfg_dir>:wiring.bzl` references.
    pub cfg_dir: String,
    pub configs: Vec<ConfigName>,
    pub packages: Vec<EmitPackage>,
}
```

Replace `EmitOutput` (~88-94) with a per-tree output (no cfg fields) plus a shared-cfg output + input:

```rust
#[derive(Debug, Clone)]
pub struct EmitOutput {
    pub buck: String,
    pub muntjac_bzl: String,
}

/// Inputs to the shared-cfg emitter: the union python-version × platform
/// grid, plus the cfg_dir the labels are rooted at.
#[derive(Debug, Clone)]
pub struct SharedCfgInput {
    pub cfg_dir: String,
    pub configs: Vec<ConfigName>,
    /// Platform keys (sorted), used for the host-modifier mapping in wiring.bzl.
    pub platforms: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SharedCfgOutput {
    pub config_buck: String,
    pub wiring_bzl: String,
}
```

- [ ] **Step 2: Add `build_shared_cfg_input` to `emit.rs`**

Add after `build_emit_input` in `src/buck/emit.rs`:

```rust
/// Build the shared-cfg input from the full config (union python versions ×
/// all platforms) rooted at the resolved cfg_dir.
pub fn build_shared_cfg_input(config: &Config) -> SharedCfgInput {
    let cfg_dir = config.cfg_dir().to_string_lossy().into_owned();
    let mut configs: Vec<ConfigName> = Vec::new();
    for plat_name in config.platforms.keys() {
        for py in config.python_versions_union() {
            configs.push(ConfigName::new(&format!("{}.{}", py.0, py.1), plat_name));
        }
    }
    configs.sort();
    let platforms: Vec<String> = config.platforms.keys().cloned().collect();
    SharedCfgInput {
        cfg_dir,
        configs,
        platforms,
    }
}
```

- [ ] **Step 3: Set `cfg_dir` in `build_emit_input`**

In `build_emit_input` (`src/buck/emit.rs`), find where `EmitInput` is constructed (near the end of the function — it sets `tree`, `third_party_dir`, `configs`, `packages`). Add the `cfg_dir` field. The function signature gains the cfg_dir via the context; add a `cfg_dir` field to `BuildEmitContext`:

In `BuildEmitContext` (~111-120) add:
```rust
    /// Cell-relative cfg_dir for config-setting label generation. When
    /// `None`, falls back to the tree's third_party_dir (single-tree path).
    pub cfg_dir: Option<&'a str>,
```

In the `EmitInput { ... }` construction inside `build_emit_input`, set:
```rust
        cfg_dir: ctx
            .cfg_dir
            .map(|s| s.to_string())
            .unwrap_or_else(|| /* same string used for third_party_dir */ tree.third_party_dir.to_string_lossy().into_owned()),
```
Match whatever expression currently fills `third_party_dir:` for the fallback so single-tree is identical.

- [ ] **Step 4: Switch config-label sites in `string_writer.rs` from `third_party_dir` to `cfg_dir`**

In `src/buck/string_writer.rs`, the per-tree emitter (`emit` ~11-19) now returns only `buck` + `muntjac_bzl`:

```rust
    fn emit(&self, input: &EmitInput) -> EmitOutput {
        EmitOutput {
            buck: emit_buck(input),
            muntjac_bzl: emit_muntjac_bzl(input),
        }
    }
```

Then switch every **config-setting / wiring reference** from `input.third_party_dir` to `input.cfg_dir`. These are the sites (confirmed by grep):
- `emit_muntjac_bzl` (~48): the `let tpd = &input.third_party_dir;` used for `//{tpd}/config:...` and `//{tpd}:wiring.bzl` load — introduce `let cfg = &input.cfg_dir;` and use `cfg` for the config + wiring references, keeping `tpd` only for the package's own location if needed.
- `emit_muntjac_bzl` doc-comment example lines (~103, ~109): `//{}/config:py312` → use `cfg`.
- `write_pypi_package` select keys (~629): `format!("//{}/config:{}", pkg_third_party_dir, cell)` → must use cfg_dir. Thread cfg_dir into `write_pypi_package` (add a `cfg_dir: &str` param; call site in `emit_buck` ~41 passes `&input.cfg_dir`).
- `emit_buck` (~31): the `input.third_party_dir` used for any config reference → cfg_dir; the package target location stays third_party_dir.

**Important:** the package's OWN references (its target name, overlay source paths under its `third_party_dir`) stay `third_party_dir`. Only the **`/config:` and `:wiring.bzl`** references move to `cfg_dir`.

- [ ] **Step 5: Extract `emit_shared_cfg` (config_buck + wiring_bzl) into a standalone fn**

Rename/repurpose the existing `emit_config_buck(input: &EmitInput)` (~388) and `emit_wiring_bzl(input: &EmitInput)` (~455) to take `&SharedCfgInput` and read `cfg_dir`/`configs`/`platforms` from it instead of `EmitInput`. Then add the public entry point:

```rust
pub fn emit_shared_cfg(input: &SharedCfgInput) -> SharedCfgOutput {
    SharedCfgOutput {
        config_buck: emit_config_buck(input),
        wiring_bzl: emit_wiring_bzl(input),
    }
}
```

In `emit_config_buck`/`emit_wiring_bzl`, replace `input.third_party_dir` → `input.cfg_dir` and derive platform names from `input.platforms` (where they previously derived from `input.configs`/`third_party_dir`). The `root//{}/config:{}` line (~597) uses `input.cfg_dir`.

- [ ] **Step 6: Update in-file tests in `string_writer.rs`**

The `mod tests` builds `EmitInput { third_party_dir: "third-party/python", ... }` (~704, ~722) and asserts on `out.config_buck`/`out.wiring_bzl` (~743-744, ~824+, ~892+). Update:
- Add `cfg_dir: "third-party/python".into(),` to every `EmitInput { ... }` literal.
- The config_buck/wiring_bzl assertions move to build a `SharedCfgInput { cfg_dir: "third-party/python".into(), configs: <same>, platforms: vec!["linux-aarch64-gnu".into(), ...] }` and call `emit_shared_cfg(&sci)`, asserting on `.config_buck` / `.wiring_bzl`.
- `emitter_returns_four_strings` (~739) becomes `emitter_returns_two_strings` asserting `out.buck` + `out.muntjac_bzl` only.

- [ ] **Step 7: Build + run buck unit tests**

Run: `cargo test --lib buck:: 2>&1 | tail -30`
Expected: PASS. If label assertions fail, confirm the `/config:` and `:wiring.bzl` sites use `cfg_dir` and the package-location sites still use `third_party_dir`.

- [ ] **Step 8: Commit**

```sh
cd /home/jackm/repos/muntjac
git add src/buck/emit.rs src/buck/string_writer.rs
git commit -m "$(cat <<'EOF'
feat(s11): split shared-cfg emission from per-tree packages

EmitInput gains cfg_dir; EmitOutput drops config_buck/wiring_bzl (now in
SharedCfgOutput via emit_shared_cfg(SharedCfgInput)). Config-setting and
wiring references switch from third_party_dir to cfg_dir; package-location
references stay on third_party_dir. Single-tree (cfg_dir == third_party_dir)
is byte-identical — locked by the snapshot fixtures in Task 9.
EOF
)"
```

---

## Task 5: buckify orchestration + write split

**Files:**
- Modify: `src/buck/mod.rs` (`write_outputs`; add `write_shared_cfg`; exports)
- Modify: `src/cli/buckify.rs`

- [ ] **Step 1: Update `write_outputs` + add `write_shared_cfg` in `src/buck/mod.rs`**

`write_outputs` currently writes `BUCK`, `muntjac.bzl`, `config/BUCK`, `wiring.bzl` to `third_party_dir`. Change it to write only the per-tree files:

```rust
/// Write per-tree package files (BUCK + muntjac.bzl) into third_party_dir.
pub fn write_outputs(out: &EmitOutput, third_party_dir: &std::path::Path) -> anyhow::Result<()> {
    use anyhow::Context;
    std::fs::create_dir_all(third_party_dir)
        .with_context(|| format!("creating {}", third_party_dir.display()))?;
    std::fs::write(third_party_dir.join("BUCK"), &out.buck)
        .with_context(|| format!("writing {}/BUCK", third_party_dir.display()))?;
    std::fs::write(third_party_dir.join("muntjac.bzl"), &out.muntjac_bzl)
        .with_context(|| format!("writing {}/muntjac.bzl", third_party_dir.display()))?;
    Ok(())
}

/// Write the shared cfg files (config/BUCK + wiring.bzl) into cfg_dir.
pub fn write_shared_cfg(
    out: &SharedCfgOutput,
    cfg_dir: &std::path::Path,
) -> anyhow::Result<()> {
    use anyhow::Context;
    let config_dir = cfg_dir.join("config");
    std::fs::create_dir_all(&config_dir)
        .with_context(|| format!("creating {}", config_dir.display()))?;
    std::fs::write(config_dir.join("BUCK"), &out.config_buck)
        .with_context(|| format!("writing {}/BUCK", config_dir.display()))?;
    std::fs::write(cfg_dir.join("wiring.bzl"), &out.wiring_bzl)
        .with_context(|| format!("writing {}/wiring.bzl", cfg_dir.display()))?;
    Ok(())
}
```

Add `SharedCfgInput`, `SharedCfgOutput`, `emit_shared_cfg`, `build_shared_cfg_input`, `write_shared_cfg` to the `pub use` re-exports in `src/buck/mod.rs` (match the existing export style for `BuildEmitContext`, `build_emit_input`, `write_outputs`).

- [ ] **Step 2: Rewrite `buckify::run` orchestration**

Replace `src/cli/buckify.rs::run` with shared-cfg-once + per-tree loop using `resolve_trees`:

```rust
pub fn run(globals: &Globals) -> Result<()> {
    let cwd = globals.workdir().context("resolving working directory")?;
    let cfg_path = cwd.join("muntjac.toml");
    let cfg_bytes =
        fs::read_to_string(&cfg_path).with_context(|| format!("reading {}", cfg_path.display()))?;
    let config =
        Config::from_str(&cfg_bytes).with_context(|| format!("parsing {}", cfg_path.display()))?;

    let emitter = StringTemplateEmitter;

    // 1. Emit the shared cfg once.
    let cfg_dir_rel = config.cfg_dir();
    let shared = crate::buck::emit_shared_cfg(&crate::buck::build_shared_cfg_input(&config));
    crate::buck::write_shared_cfg(&shared, &cwd.join(&cfg_dir_rel))?;
    let cfg_dir_str = cfg_dir_rel.to_string_lossy().into_owned();

    // 2. Emit per-tree packages.
    for tree in crate::cli::resolve_trees(&config, globals.tree.as_deref())? {
        let manifest_dir = cfg_path
            .parent()
            .unwrap_or(Path::new("."))
            .join(tree.manifest_path.parent().unwrap_or(Path::new("")));
        let lockfile_path = manifest_dir.join("uv.lock");
        let lock_bytes = fs::read_to_string(&lockfile_path)
            .with_context(|| format!("reading {}", lockfile_path.display()))?;
        let lockfile = lock::parser::parse(&lock_bytes)
            .with_context(|| format!("parsing {}", lockfile_path.display()))?;

        let third_party_dir = cwd.join(&tree.third_party_dir);
        let manifest_path = third_party_dir.join("prebake/.manifest.toml");
        let manifest = if manifest_path.is_file() {
            Some(crate::sdist::Manifest::load(&manifest_path)?)
        } else {
            None
        };
        let canonical_third_party_dir =
            std::fs::canonicalize(&third_party_dir).unwrap_or_else(|_| third_party_dir.clone());

        let fixups = crate::fixup::EffectiveFixups::load(
            &config.fixups.registry,
            &third_party_dir,
            config.fixups.allow_local_overrides,
            globals.no_network,
        )
        .with_context(|| format!("loading fixups for tree '{}'", tree.name))?;

        let input = build_emit_input(
            &config,
            tree,
            &lockfile,
            &BuildEmitContext {
                manifest: manifest.as_ref(),
                fixups: Some(&fixups),
                abs_third_party_dir: Some(&canonical_third_party_dir),
                cfg_dir: Some(&cfg_dir_str),
            },
        )?;
        let output = emitter.emit(&input);
        write_outputs(&output, &third_party_dir)?;
    }
    Ok(())
}
```

Keep the existing imports; add `use std::path::Path;` if not present (it is). Ensure `crate::cli::resolve_trees` path is correct (it's defined in `src/cli/mod.rs`).

- [ ] **Step 3: Verify single-tree buckify is byte-identical (fixture 02)**

Run:
```sh
cd /home/jackm/repos/muntjac/tests/fixtures/buck/02-numpy-pandas
git submodule update --init --recursive --depth 1 2>/dev/null || true
cargo run --quiet --manifest-path ../../../../Cargo.toml -- buckify
git -C ../../../.. diff --stat -- tests/fixtures/buck/02-numpy-pandas/third-party
```
Expected: **no diff** (regenerated output matches committed golden). If diff appears, the cfg_dir fallback in single-tree isn't matching the old third_party_dir path exactly — fix Task 4 Step 3.

Reset any churn: `git -C ../../../.. checkout -- tests/fixtures/buck/02-numpy-pandas/third-party`.

- [ ] **Step 4: Run the buckify integration suite + the Task 3 red test**

Run: `cargo test --test buckify 2>&1 | tail -20 && cargo test --test multi_tree buckify_unknown_tree 2>&1 | tail -10`
Expected: existing buckify snapshot tests PASS (byte-identical); `buckify_unknown_tree_errors_with_available_names` now PASSES (resolve_trees wired in).

- [ ] **Step 5: Commit**

```sh
cd /home/jackm/repos/muntjac
git add src/buck/mod.rs src/cli/buckify.rs
git commit -m "$(cat <<'EOF'
feat(s11): buckify emits shared cfg once + per-tree packages

write_outputs now writes only BUCK + muntjac.bzl per tree; new
write_shared_cfg writes config/BUCK + wiring.bzl once to cfg_dir.
buckify resolves cfg_dir, emits the shared cfg, then loops trees via
resolve_trees (unknown --tree now errors). Single-tree output verified
byte-identical against fixture 02.
EOF
)"
```

---

## Task 6: vendor multi-tree

**Files:**
- Modify: `src/cli/vendor.rs` (~21-183)

- [ ] **Step 1: Wrap the per-tree body in a `resolve_trees` loop**

In `src/cli/vendor.rs::run`, replace the single-tree resolution (~29-42, the `let tree: &Tree = match &globals.tree { ... }` block) with a loop over `resolve_trees`. Extract the existing per-tree body (steps 1–5, lines ~42-181) into a helper `fn vendor_tree(globals: &Globals, workdir: &Path, config_path: &Path, tree: &Tree) -> Result<()>` and call it per tree:

```rust
pub fn run(globals: &Globals) -> Result<()> {
    let workdir = globals.workdir().context("resolving working directory")?;
    let config_path = workdir.join("muntjac.toml");
    let config_text = std::fs::read_to_string(&config_path)
        .with_context(|| format!("reading {}", config_path.display()))?;
    let config = Config::from_str(&config_text)
        .with_context(|| format!("parsing {}", config_path.display()))?;

    for tree in crate::cli::resolve_trees(&config, globals.tree.as_deref())? {
        vendor_tree(globals, &workdir, &config_path, tree)
            .with_context(|| format!("vendoring tree '{}'", tree.name))?;
    }
    Ok(())
}

fn vendor_tree(
    globals: &Globals,
    workdir: &Path,
    config_path: &Path,
    tree: &Tree,
) -> Result<()> {
    let third_party_dir = workdir.join(&tree.third_party_dir);
    // ... existing body from "Step 1: lock freshness" through "Step 5: write manifest",
    //     unchanged, operating on this `tree`.
    Ok(())
}
```

Move the existing lines verbatim into `vendor_tree`; the only change is they now receive `tree`/`workdir`/`config_path` as params instead of computing a single tree.

- [ ] **Step 2: Build + smoke vendor on a single-tree fixture**

Run:
```sh
cargo build --locked 2>&1 | tail -5
cd /home/jackm/repos/muntjac/tests/fixtures/buck/04-pure-python-sdist
cargo run --quiet --manifest-path ../../../../Cargo.toml -- vendor 2>&1 | tail -10
git -C ../../../.. checkout -- tests/fixtures/buck/04-pure-python-sdist 2>/dev/null || true
```
Expected: builds; vendor runs without error on the single tree (behavior unchanged for single-tree).

- [ ] **Step 3: Run the vendor smoke test**

Run: `cargo test --test vendor_smoke 2>&1 | tail -15`
Expected: PASS (single-tree behavior preserved).

- [ ] **Step 4: Commit**

```sh
cd /home/jackm/repos/muntjac
git add src/cli/vendor.rs
git commit -m "$(cat <<'EOF'
feat(s11): vendor processes all trees

vendor::run now loops resolve_trees instead of operating on .first().
Per-tree work factored into vendor_tree(); single-tree behavior
unchanged. Unknown --tree errors via the shared helper.
EOF
)"
```

---

## Task 7: fixups show per-tree blocks

**Files:**
- Modify: `src/cli/fixups.rs` (`show` ~37-…)

- [ ] **Step 1: Loop trees in `show`**

In `src/cli/fixups.rs::show`, replace the `.trees.first()` resolution (~45-48) with a `resolve_trees` loop, printing a labeled block per tree:

```rust
fn show(package: String, globals: &Globals) -> Result<()> {
    let cwd = globals.workdir().context("resolving working directory")?;
    let cfg_path = cwd.join("muntjac.toml");
    let cfg_bytes = std::fs::read_to_string(&cfg_path)
        .with_context(|| format!("reading {}", cfg_path.display()))?;
    let config =
        Config::from_str(&cfg_bytes).with_context(|| format!("parsing {}", cfg_path.display()))?;

    let pkg_name = PackageName::from_str(&package)
        .with_context(|| format!("normalizing package name `{}`", package))?;

    let trees = crate::cli::resolve_trees(&config, globals.tree.as_deref())?;
    let multi = trees.len() > 1;
    for tree in trees {
        if multi {
            println!("# ===== tree: {} =====", tree.name);
        }
        let third_party_dir = cwd.join(&tree.third_party_dir);
        show_one_tree(&config, &third_party_dir, &tree.name, &pkg_name, globals)?;
    }
    Ok(())
}
```

Extract the existing body (from `EffectiveFixups::load` through the printing logic) into `fn show_one_tree(config: &Config, third_party_dir: &Path, tree_name: &str, pkg_name: &PackageName, globals: &Globals) -> Result<()>`, moving the current lines verbatim.

- [ ] **Step 2: Build + smoke `fixups show` on a single-tree fixture**

Run:
```sh
cargo build --locked 2>&1 | tail -5
cd /home/jackm/repos/muntjac/tests/fixtures/buck/06-community-fixup
cargo run --quiet --manifest-path ../../../../Cargo.toml -- fixups show fake-pkg 2>&1 | tail -15 || true
```
Expected: builds; single-tree output has NO `# ===== tree:` header (the `multi` guard suppresses it), preserving current single-tree output.

- [ ] **Step 3: Run the fixups-show smoke test**

Run: `cargo test --test fixups_show_smoke 2>&1 | tail -15`
Expected: PASS (single-tree output unchanged).

- [ ] **Step 4: Commit**

```sh
cd /home/jackm/repos/muntjac
git add src/cli/fixups.rs
git commit -m "$(cat <<'EOF'
feat(s11): fixups show prints per-tree blocks

show loops resolve_trees; with >1 tree it prefixes each tree's
effective-fixup block with a `# ===== tree: <name> =====` header.
Single-tree output unchanged (no header). update stays tree-independent.
EOF
)"
```

---

## Task 8: 10-multi-tree fixture inputs

**Files:**
- Create: `tests/fixtures/buck/10-multi-tree/` (muntjac.toml, two pyproject.toml + uv.lock, prelude submodule, toolchains, .buckconfig, PACKAGE, smoke targets, .gitignore)

- [ ] **Step 1: Scaffold the fixture from fixture 02**

Reuse fixture 02's Buck2 scaffolding (prelude submodule, toolchains/, .buckconfig). Run:

```sh
cd /home/jackm/repos/muntjac/tests/fixtures/buck
mkdir -p 10-multi-tree/modern 10-multi-tree/legacy 10-multi-tree/tests/smoke
cp -r 02-numpy-pandas/toolchains 10-multi-tree/toolchains
cp 02-numpy-pandas/.buckconfig 10-multi-tree/.buckconfig
# prelude submodule is shared via the repo's .gitmodules; reference the same path.
```

For the prelude, the fixture references the repo-level prelude submodule the same way fixture 02 does. Inspect `02-numpy-pandas/.buckconfig` `[cells] prelude = prelude` and mirror the submodule wiring (the plan's CI step runs `git submodule update --init`).

- [ ] **Step 2: Write `10-multi-tree/muntjac.toml`**

```toml
[platforms]
linux-x86_64-gnu  = { target = "x86_64-unknown-linux-gnu",  manylinux = "2_17" }
linux-aarch64-gnu = { target = "aarch64-unknown-linux-gnu", manylinux = "2_17" }
macos-arm64       = { target = "aarch64-apple-darwin",      macos_min = "11.0" }

[fixups]
registry = "none"

[buck]
file_name = "BUCK"

[tree.modern]
manifest_path   = "modern/pyproject.toml"
third_party_dir = "third-party/python/modern"
python_versions = ["3.12"]

[tree.legacy]
manifest_path   = "legacy/pyproject.toml"
third_party_dir = "third-party/python/legacy"
python_versions = ["3.12"]
```

- [ ] **Step 3: Write the two pyproject.toml + uv.lock pairs**

`10-multi-tree/modern/pyproject.toml`:
```toml
[project]
name = "modern-app"
version = "0.0.0"
requires-python = ">=3.12,<3.13"
dependencies = ["numpy>=2.1,<2.3"]
[build-system]
requires = ["hatchling"]
build-backend = "hatchling.build"
```

`10-multi-tree/legacy/pyproject.toml`:
```toml
[project]
name = "legacy-app"
version = "0.0.0"
requires-python = ">=3.12,<3.13"
dependencies = ["numpy>=1.26,<2"]
[build-system]
requires = ["hatchling"]
build-backend = "hatchling.build"
```

Generate the lockfiles with real uv (committed as frozen fixtures, per the repo's fixture convention):
```sh
cd /home/jackm/repos/muntjac/tests/fixtures/buck/10-multi-tree/modern && uv lock
cd ../legacy && uv lock
```
Verify `modern/uv.lock` pins numpy 2.x and `legacy/uv.lock` pins numpy 1.26.x. If `uv` is unavailable in the dev environment, hand-author minimal `uv.lock` files mirroring fixture 02's structure with the respective numpy versions + their wheels.

- [ ] **Step 4: Write the smoke targets**

`10-multi-tree/tests/smoke/BUCK`:
```python
python_binary(
    name = "modern_demo",
    main = "modern_demo.py",
    modifiers = ["//third-party/python/config:py312"],
    deps = ["//third-party/python/modern:numpy"],
)

python_binary(
    name = "legacy_demo",
    main = "legacy_demo.py",
    modifiers = ["//third-party/python/config:py312"],
    deps = ["//third-party/python/legacy:numpy"],
)
```

`10-multi-tree/tests/smoke/modern_demo.py`:
```python
import numpy as np
print("MODERN numpy", np.__version__)
assert np.__version__.startswith("2."), np.__version__
```

`10-multi-tree/tests/smoke/legacy_demo.py`:
```python
import numpy as np
print("LEGACY numpy", np.__version__)
assert np.__version__.startswith("1."), np.__version__
```

Note both `modifiers` use the **shared** `//third-party/python/config:py312` (the cfg_dir = common ancestor `third-party/python`), proving the shared-cfg model.

- [ ] **Step 5: Write the root PACKAGE + .gitignore**

`10-multi-tree/PACKAGE` (mirror fixture 02's, but load the shared `//third-party/python:wiring.bzl`):
```python
load(
    "@prelude//cfg/modifier:cfg_constructor.bzl",
    "cfg_constructor_post_constraint_analysis",
    "cfg_constructor_pre_constraint_analysis",
)
load("@prelude//cfg/modifier:common.bzl", "MODIFIER_METADATA_KEY")
load("@prelude//cfg/modifier:set_cfg_modifiers.bzl", "set_cfg_modifiers")
load("//third-party/python:wiring.bzl", "MUNTJAC_HOST_MODIFIERS")

set_cfg_constructor(
    stage0 = cfg_constructor_pre_constraint_analysis,
    stage1 = cfg_constructor_post_constraint_analysis,
    key = MODIFIER_METADATA_KEY,
    aliases = struct(),
    extra_data = struct(),
)
set_cfg_modifiers(cfg_modifiers = MUNTJAC_HOST_MODIFIERS)
```

`10-multi-tree/third-party/python/.gitignore` (allow-list pattern per TD-S7a-01; covers both trees' generated output + the shared cfg):
```
# muntjac-generated; commit nothing under here except this file.
*
!.gitignore
```

Wait — the shared cfg + per-tree BUCK are muntjac-generated and the snapshot test regenerates them into `expected/`. Do NOT commit generated BUCK/muntjac.bzl/config/wiring under the live tree; the snapshot lives in `expected/`. Add a `.gitignore` at `10-multi-tree/third-party/python/` ignoring `modern/BUCK`, `modern/muntjac.bzl`, `legacy/BUCK`, `legacy/muntjac.bzl`, `config/`, `wiring.bzl`, plus `buck-out/`. Mirror fixture 02's `.gitignore` conventions.

- [ ] **Step 6: Commit fixture inputs**

```sh
cd /home/jackm/repos/muntjac
git add tests/fixtures/buck/10-multi-tree
git commit -m "$(cat <<'EOF'
test(s11): 10-multi-tree fixture inputs

Two trees (modern: numpy 2.x, legacy: numpy 1.26.x) on Python 3.12,
sharing [platforms] + cfg_dir third-party/python. Smoke targets import
each tree's numpy and assert the major version. Root PACKAGE loads the
single shared wiring.bzl. (08 was taken by S7a; this is fixture 10.)
EOF
)"
```

---

## Task 9: 10-multi-tree snapshot test + backward-compat verification

**Files:**
- Modify: `tests/buckify.rs` (add `fixture_10_multi_tree_golden`)
- Create: `tests/fixtures/buck/10-multi-tree/expected/` (golden output)

- [ ] **Step 1: Add the snapshot test**

Append to `tests/buckify.rs`, mirroring `fixture_02_numpy_pandas_golden` (~98-116) exactly — same helpers: `fixture(name)`, `copy_fixture_to`, `run_buckify`, `assert_files_match(out_dir, golden_dir)` where the golden dir is `<fixture>/expected` compared against the emitted `third-party/python` subtree:

```rust
#[test]
fn fixture_10_multi_tree_golden() {
    let fix = fixture("10-multi-tree");
    let tmp = tempfile::tempdir().unwrap();
    copy_fixture_to(&fix, tmp.path());
    let out = run_buckify(tmp.path());
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // `expected/` mirrors the emitted `third-party/python` subtree: shared cfg
    // (config/BUCK + wiring.bzl) emitted once + per-tree modern/ + legacy/ packages.
    assert_files_match(
        &tmp.path().join("third-party/python"),
        &fix.join("expected"),
    );
}
```

- [ ] **Step 2: Generate the golden output**

`assert_files_match` compares the emitted `third-party/python` tree against `<fixture>/expected`, so `expected/` mirrors that subtree **directly** (no `third-party/python/` nesting inside `expected/`):

```sh
cd /home/jackm/repos/muntjac/tests/fixtures/buck/10-multi-tree
cargo run --quiet --manifest-path ../../../../Cargo.toml -- buckify
rm -rf expected && mkdir -p expected
cp -r third-party/python/config   expected/config
cp    third-party/python/wiring.bzl expected/wiring.bzl
mkdir -p expected/modern expected/legacy
cp third-party/python/modern/BUCK        expected/modern/BUCK
cp third-party/python/modern/muntjac.bzl expected/modern/muntjac.bzl
cp third-party/python/legacy/BUCK        expected/legacy/BUCK
cp third-party/python/legacy/muntjac.bzl expected/legacy/muntjac.bzl
```

Inspect the goldens: `expected/config/BUCK` should have `py312-*` config_settings for all three platforms; `expected/modern/BUCK` should reference `//third-party/python/config:py312-...` (the SHARED cfg, not `//third-party/python/modern/config:...`); same for `expected/legacy/BUCK`. This visual check is the proof the cfg-label threading is correct.

Note: confirm whether `copy_fixture_to` copies the `expected/` dir into the tempdir (if it copies everything, the emitted output and the golden coexist in the tempdir but `assert_files_match` reads the golden from `fix.join("expected")`, the source fixture — so it's fine). Mirror exactly what `fixture_02` relies on.

- [ ] **Step 3: Run the new snapshot + the full backward-compat suite**

Run: `cargo test --test buckify 2>&1 | tail -25`
Expected: `fixture_10_multi_tree_golden` PASS **and** every pre-existing `fixture_01..09` golden PASS unchanged (the backward-compat proof).

- [ ] **Step 4: Run the whole test suite**

Run: `cargo test --locked 2>&1 | tail -15`
Expected: all green.

- [ ] **Step 5: Commit**

```sh
cd /home/jackm/repos/muntjac
git add tests/buckify.rs tests/fixtures/buck/10-multi-tree/expected
git commit -m "$(cat <<'EOF'
test(s11): 10-multi-tree byte-snapshot + backward-compat proof

Golden snapshot of the shared cfg (config/BUCK + wiring.bzl emitted
once) + both trees' BUCK/muntjac.bzl. Per-tree BUCK references the
shared //third-party/python/config:py312-* settings. Existing
fixture_01..09 goldens pass unchanged — single-tree output is
byte-identical post-split.
EOF
)"
```

---

## Task 10: CI buckify + buck2 build for 10-multi-tree

**Files:**
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Add the multi-tree buckify + build steps**

After the existing fixture-02 buck2 steps in `ci.yml` (~96-102), add (NOT gated by `if: matrix.runner` — runs on all three runners, per the no-arch-polymorphism principle):

```yaml
      - name: muntjac buckify (10-multi-tree fixture)
        run: |
          set -euo pipefail
          cd tests/fixtures/buck/10-multi-tree
          git submodule update --init --recursive --depth 1
          cargo run --release --manifest-path ../../../../Cargo.toml -- buckify
          # Shared cfg emitted once; per-tree packages exist.
          test -f third-party/python/config/BUCK
          test -f third-party/python/wiring.bzl
          test -f third-party/python/modern/BUCK
          test -f third-party/python/legacy/BUCK

      - name: buck2 build + run both trees (10-multi-tree)
        run: |
          set -euo pipefail
          cd tests/fixtures/buck/10-multi-tree
          buck2 run //tests/smoke:modern_demo 2>&1 | tee /tmp/modern-out.txt
          grep -E "MODERN numpy 2\." /tmp/modern-out.txt
          buck2 run //tests/smoke:legacy_demo 2>&1 | tee /tmp/legacy-out.txt
          grep -E "LEGACY numpy 1\." /tmp/legacy-out.txt
```

- [ ] **Step 2: Validate the workflow YAML**

Run: `python3 -c "import yaml; yaml.safe_load(open('/home/jackm/repos/muntjac/.github/workflows/ci.yml'))"`
Expected: exits 0.

- [ ] **Step 3: Commit**

```sh
cd /home/jackm/repos/muntjac
git add .github/workflows/ci.yml
git commit -m "$(cat <<'EOF'
ci(s11): build both trees of 10-multi-tree on all runners

buckify the two-tree fixture, assert the shared cfg + per-tree packages
exist, then buck2 run both smoke targets and grep for numpy 2.x (modern)
and 1.x (legacy) — proving conflicting versions coexist in one Buck
project via distinct target paths. Runs on all three matrix runners
(no arch-conditional skip).
EOF
)"
```

---

## Task 11: Docs + version bump

**Files:**
- Modify: `README.md` (add Multi-tree subsection)
- Modify: `Cargo.toml` (version → 0.2.0)
- Modify: `CHANGELOG.md` (add `## [0.2.0]`)

- [ ] **Step 1: Add a Multi-tree section to README**

In `README.md`, after the "Configuration" section, add:

```markdown
## Multi-tree (incompatible dependency universes)

When parts of your monorepo can't share one resolution — say a legacy service needs `numpy<2` and a new one needs `numpy>=2` — declare one `[tree.<name>]` block per universe. `[platforms]` and `[fixups]` stay shared:

​```toml
[platforms]
macos-arm64 = { target = "aarch64-apple-darwin", macos_min = "11.0" }

[tree.modern]
manifest_path   = "pyproject.toml"
third_party_dir = "third-party/python/modern"
python_versions = ["3.12"]

[tree.legacy]
manifest_path   = "legacy/pyproject.toml"
third_party_dir = "third-party/python/legacy"
python_versions = ["3.12"]
​```

Each tree is an island — the same package at conflicting versions coexists via distinct Buck target paths (`//third-party/python/modern:numpy` vs `//third-party/python/legacy:numpy`). `muntjac buckify`/`vendor` process all trees by default; `--tree <name>` scopes to one. First-party rules pick a universe by which target path they depend on.
```

(The `​` before each fenced code block above is a zero-width marker to keep this plan's own fencing intact — write plain ``` in the actual README.)

- [ ] **Step 2: Bump Cargo.toml version**

Edit `Cargo.toml`: `version = "0.1.0"` → `version = "0.2.0"`.

Run: `cargo build --locked` (updates Cargo.lock's muntjac entry to 0.2.0). Stage both.

- [ ] **Step 3: Add CHANGELOG [0.2.0] section**

In `CHANGELOG.md`, between `## [Unreleased]` and `## [0.1.0]`, insert:

```markdown
## [0.2.0] — 2026-05-25

### Added

- Multi-tree support: `[tree.<name>]` blocks let one repo carry incompatible dependency universes, each with its own `uv.lock` + `third_party_dir`, picked by Buck target path. `buckify`/`vendor` process all trees by default; `--tree <name>` scopes to one. `fixups show` prints per-tree blocks.
- `[buck] cfg_dir` (optional) — overrides where the shared `config/` + `wiring.bzl` are written; defaults to the longest common ancestor of trees' `third_party_dir`s.

### Changed

- The Buck cfg machinery (constraint_settings, config_settings, host modifiers, wiring.bzl) is now emitted once at `cfg_dir` and shared across trees, so the root `PACKAGE`'s single `set_cfg_modifiers` call composes. Single-tree output is unchanged (byte-identical).
```

Update the bottom link refs: change `[Unreleased]` compare to `v0.2.0...HEAD`, and add `[0.2.0]: https://github.com/rsJames-ttrpg/muntjac/releases/tag/v0.2.0`.

- [ ] **Step 4: Verify publish-dry-run still passes**

Run: `cargo publish --dry-run --locked 2>&1 | tail -5`
Expected: succeeds (version 0.2.0, no metadata errors).

- [ ] **Step 5: Commit**

```sh
cd /home/jackm/repos/muntjac
git add README.md Cargo.toml Cargo.lock CHANGELOG.md
git commit -m "$(cat <<'EOF'
docs(s11): README multi-tree section + v0.2.0 CHANGELOG

Bump to 0.2.0. README documents the [tree.<name>] config shape + the
target-path isolation guarantee. CHANGELOG [0.2.0] covers multi-tree
support, [buck] cfg_dir, and the shared-cfg emission change (single-tree
byte-identical).
EOF
)"
```

---

## Task 12: Stage-close — roadmap mark-shipped + tag

**Run after Tasks 1–11 are merged and CI is green.**

**Files:**
- Modify: `docs/superpowers/specs/2026-05-20-muntjac-roadmap.md`

- [ ] **Step 1: Count commits since the previous tag**

Run: `cd /home/jackm/repos/muntjac && git log --oneline $(git describe --tags --abbrev=0)..HEAD | wc -l`
Record N (expected ~14: 1 spec + 1 plan + 11 task commits + this one).

- [ ] **Step 2: Update the roadmap S11 section**

In `docs/superpowers/specs/2026-05-20-muntjac-roadmap.md`, change the S11 heading:
```
### S11 — Multi-tree → v0.2.0 (next)
```
to:
```
### S11 — Multi-tree → v0.2.0 ✅ shipped
```
And after its body paragraph add (N from Step 1):
```

**Shipped:** N commits, tag `s11-complete`. cfg machinery emitted once at a shared cfg_dir (common-ancestor of trees' third_party_dirs, `[buck] cfg_dir` override); emitter split into shared-cfg + per-tree package emission; `resolve_trees` helper unifies `--tree` scoping (buckify/vendor all-trees, unknown errors); `fixups show` per-tree blocks; validation rejects dir collisions; 10-multi-tree fixture builds numpy 2.x + 1.x in one Buck project. Single-tree output byte-identical (fixtures 01–09 unchanged). cargo publish to crates.io as v0.2.0 is the maintainer's tag-cut action (cut-each-announce-once cadence).
```

- [ ] **Step 3: Update the specs index table row**

Change the S11 row:
```
| S11 | (not yet written) | (not yet written) | ⬜ next (build order 1/3 → v0.2.0) |
```
to:
```
| S11 | [2026-05-25-muntjac-s11-multi-tree-design.md](./2026-05-25-muntjac-s11-multi-tree-design.md) | [2026-05-25-muntjac-s11-multi-tree.md](../plans/2026-05-25-muntjac-s11-multi-tree.md) | ✅ shipped (tag `s11-complete`, N commits) |
```

- [ ] **Step 4: Commit + tag + push**

```sh
cd /home/jackm/repos/muntjac
git add docs/superpowers/specs/2026-05-20-muntjac-roadmap.md
git commit -m "$(cat <<'EOF'
docs(s11): mark ✅ shipped in roadmap

Multi-tree composes at the Buck level: shared cfg emitted once, per-tree
packages, all commands tree-aware. Next: S9 (vendor mode → v0.3.0).
crates.io publish of v0.2.0 + the dogfood migration are maintainer
actions; public announcement still waits for S10.
EOF
)"
git tag -a s11-complete -m "S11 — multi-tree support complete (v0.2.0)"
git push origin main
git push origin s11-complete
```

**Do NOT push a `v0.2.0` tag here** unless the maintainer is ready to cut the crates.io release — that triggers `release.yml`. `s11-complete` is the stage marker; `v0.2.0` is the release marker (maintainer's call, per spec §6).

---

## Self-Review

**Spec coverage:**
- §1.2 item 1 (emitter split + cfg_dir) → Tasks 1, 4, 5 ✓
- §1.2 item 2 (vendor all trees) → Task 6 ✓
- §1.2 item 3 (fixups show per-tree) → Task 7 ✓
- §1.2 item 4 (--tree consistency / resolve_trees) → Tasks 3, 5, 6, 7 ✓
- §1.2 item 5 (validation) → Task 2 ✓
- §1.2 item 6 (10-multi-tree fixture) → Tasks 8, 9, 10 ✓
- §1.2 item 7 (docs + version) → Task 11 ✓
- §2.2 cfg_dir derivation → Task 1 ✓
- §2.3 python_versions union → Task 1 ✓
- §2.4 validation rules → Task 2 ✓
- §3 emitter split → Tasks 4, 5 ✓
- §4 command scoping table → Tasks 3, 5, 6, 7 ✓
- §5 testing → Tasks 9 (snapshot + backward-compat), 10 (CI buck2 build) ✓
- §6 docs/versioning → Tasks 11, 12 ✓
- §1.4 exit criteria: (1) Task 10; (2) Task 9 Step 3; (3) Tasks 6/7 + Task 3; (4) Task 2; (5) Task 11 ✓

**Placeholder scan:** No TBD/TODO. The README fenced-block markers (`​`) are explicitly explained as zero-width artifacts to write as plain ``` — not a placeholder.

**Type consistency:** `resolve_trees(&Config, Option<&str>) -> Result<Vec<&Tree>>` used identically in Tasks 3/5/6/7. `SharedCfgInput`/`SharedCfgOutput`/`emit_shared_cfg`/`build_shared_cfg_input`/`write_shared_cfg` defined in Task 4/5, consumed in Task 5. `Config::cfg_dir() -> PathBuf` and `python_versions_union() -> Vec<PythonVersion>` defined Task 1, consumed Tasks 2, 4, 5. `BuildEmitContext.cfg_dir: Option<&str>` defined Task 4, set in Task 5.

**Note for the implementer:** Tasks 4 and 5 are the intricate ones — they modify long existing functions (`build_emit_input`, the `string_writer.rs` emitters). The plan gives exact field additions, the new struct/fn definitions in full, and the precise label sites to switch (`/config:` and `:wiring.bzl` → cfg_dir; package location → third_party_dir). The byte-identical single-tree check (Task 5 Step 3) and the fixture_01–09 backward-compat suite (Task 9 Step 3) are the safety nets — run them before committing each.
