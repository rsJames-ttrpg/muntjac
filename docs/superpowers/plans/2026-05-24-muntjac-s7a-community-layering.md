# S7a — Community Fixup Layering Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a community fixup layer that merges with the existing local layer (S6), with two no-network registry modes (`registry = "none"`, `registry = "file://…"`). Per-layer-then-merge algorithm: each layer is fully resolved (top + matching cfg sections), then a cross-layer merge applies local-wins-on-scalar + community-first-on-list semantics. Git fetch defers to S7b.

**Architecture:** Three new pieces. (1) `RegistryConfig` typed enum replaces the existing `FixupRegistry(String)` wrapper in `muntjac.toml` config parsing. (2) `EffectiveFixups` facade in `src/fixup/layer.rs` owns both `FixupSet`s (community + local) and exposes a single `.resolve(pkg, ctx) -> ResolvedFixup` method that hides layering from the emitter. (3) `BuildEmitContext<'a>` struct collapses the 6 positional args of `build_emit_input` into 3 positional + 1 context (resolves TD-S6-04 from S6). Three synthetic snapshot fixtures (`06-community-fixup`, `07-allow-local-overrides-false`, `08-replace-community`) exercise the layering algorithm end-to-end.

**Tech Stack:** Rust 2024 edition, rust-version 1.85. `thiserror` for typed errors, `serde` for TOML config, `pep508_rs::PackageName` for PEP 503 normalization, `insta` for snapshot tests. Reuses S6's `resolve_for_cell` unchanged.

**Prerequisite reading:** `docs/superpowers/specs/2026-05-24-muntjac-s7a-community-layering-design.md` (locked design); `docs/superpowers/specs/2026-05-23-muntjac-s6-local-fixups-design.md` for local-layer foundations; `docs/superpowers/TECH_DEBT.md` TD-S6-04 (BuildEmitContext refactor).

---

## Phase 1 — Error variants

### Task 1: Add three new `FixupError` variants with locked messages

**Files:**
- Modify: `src/fixup/error.rs:75` (insert before `Io { ... }`)

- [ ] **Step 1: Write the failing tests**

Add to `src/fixup/error.rs:84` `mod tests`:

```rust
#[test]
fn replace_community_in_community_message_is_exact() {
    let e = FixupError::ReplaceCommunityInCommunity {
        file: PathBuf::from("registry/packages/pillow/fixups.toml"),
    };
    assert_eq!(
        e.to_string(),
        "fixup file registry/packages/pillow/fixups.toml sets `replace_community = true`, which is only valid in local fixups, not in the community registry"
    );
}

#[test]
fn git_registry_not_implemented_message_is_exact() {
    let e = FixupError::GitRegistryNotImplemented {
        registry: "github.com/jackmpcollins/muntjac-fixups".into(),
    };
    assert_eq!(
        e.to_string(),
        "git-based community registry is not yet implemented (S7b); registry = github.com/jackmpcollins/muntjac-fixups requires either \"none\" or \"file://...\""
    );
}

#[test]
fn registry_path_not_found_message_is_exact() {
    let e = FixupError::RegistryPathNotFound {
        path: PathBuf::from("/tmp/no/such/packages"),
    };
    assert_eq!(
        e.to_string(),
        "community registry path /tmp/no/such/packages does not exist"
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p muntjac fixup::error::tests::replace_community_in_community_message_is_exact -- --exact`

Expected: FAIL — `ReplaceCommunityInCommunity` variant does not exist.

- [ ] **Step 3: Add the three variants**

In `src/fixup/error.rs`, insert before the `Io { ... }` variant (around line 69, after `BadCfgAtom`):

```rust
#[error("fixup file {file} sets `replace_community = true`, which is only valid in local fixups, not in the community registry")]
ReplaceCommunityInCommunity { file: PathBuf },

#[error("git-based community registry is not yet implemented (S7b); registry = {registry} requires either \"none\" or \"file://...\"")]
GitRegistryNotImplemented { registry: String },

#[error("community registry path {path} does not exist")]
RegistryPathNotFound { path: PathBuf },
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p muntjac fixup::error::tests`

Expected: PASS, all error-message tests pass including the three new ones.

- [ ] **Step 5: Commit**

```bash
git add src/fixup/error.rs
git commit -m "feat(s7a): FixupError variants for community registry"
```

---

### Task 2: Add `ConfigError::RegistryPathNotAbsolute`

**Files:**
- Modify: `src/error.rs` (add variant)

- [ ] **Step 1: Look up current `ConfigError` to find the right insertion point**

Run: `grep -n 'BadRegistry\|enum ConfigError' src/error.rs`

Expected: `BadRegistry` variant exists; note its line for the insertion.

- [ ] **Step 2: Write the failing test**

Add to `src/error.rs` tests module (if no test module, add one):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn registry_path_not_absolute_message_is_exact() {
        let e = ConfigError::RegistryPathNotAbsolute {
            path: "./registry".into(),
        };
        assert_eq!(
            e.to_string(),
            "registry path must be absolute (got `./registry`); use a full file:// URL or set registry to a path relative to muntjac.toml"
        );
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p muntjac error::tests::registry_path_not_absolute_message_is_exact -- --exact`

Expected: FAIL — `RegistryPathNotAbsolute` variant does not exist.

- [ ] **Step 4: Add the variant**

Insert immediately after `BadRegistry(String)` in the `ConfigError` enum:

```rust
#[error("registry path must be absolute (got `{path}`); use a full file:// URL or set registry to a path relative to muntjac.toml")]
RegistryPathNotAbsolute { path: String },
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p muntjac error::tests::registry_path_not_absolute_message_is_exact -- --exact`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/error.rs
git commit -m "feat(s7a): ConfigError::RegistryPathNotAbsolute"
```

---

## Phase 2 — Schema delta: move `replace_community` from `FixupBody` to `FixupConfig`

### Task 3: Move `replace_community` to file-level on `FixupConfig`

**Files:**
- Modify: `src/fixup/schema.rs` (lines 16-19 `FixupConfig` struct; lines 24-60 `FixupBody`; `from_toml_str` lines 97-114; `to_toml_string` lines 118-131; tests 204-215)

S6 declared `replace_community: bool` on `FixupBody` per its §1.2 (parses but no-op). S7a moves it to `FixupConfig` so it applies at the file level, not in cfg sections.

- [ ] **Step 1: Update the existing tests to assert new placement**

In `src/fixup/schema.rs`, replace the two existing tests (lines 204-215):

```rust
#[test]
fn parses_replace_community_default_false() {
    let toml = r#"extra_deps = ["//x:y"]"#;
    let cfg = FixupConfig::from_toml_str(toml).unwrap();
    assert!(!cfg.replace_community);  // CHANGED: was cfg.top.replace_community
}

#[test]
fn parses_replace_community_true() {
    let toml = r#"replace_community = true
extra_deps = ["//x:y"]"#;
    let cfg = FixupConfig::from_toml_str(toml).unwrap();
    assert!(cfg.replace_community);  // CHANGED: was cfg.top.replace_community
    assert_eq!(cfg.top.extra_deps, vec!["//x:y"]);
}

#[test]
fn replace_community_inside_cfg_section_errors_as_unknown_field() {
    let toml = r#"
        ["cfg(target_os = \"linux\")"]
        replace_community = true
    "#;
    let err = FixupConfig::from_toml_str(toml).unwrap_err();
    assert!(
        err.to_string().contains("replace_community"),
        "expected unknown-field error mentioning replace_community, got: {}",
        err
    );
}

#[test]
fn replace_community_round_trips() {
    let toml = "replace_community = true\nextra_deps = [\"//a:b\"]\n";
    let cfg = FixupConfig::from_toml_str(toml).unwrap();
    let out = cfg.to_toml_string().unwrap();
    let cfg2 = FixupConfig::from_toml_str(&out).unwrap();
    assert_eq!(cfg, cfg2);
    assert!(cfg2.replace_community);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p muntjac fixup::schema::tests::parses_replace_community_default_false -- --exact`

Expected: FAIL — `cfg.replace_community` doesn't exist; `FixupConfig` has no such field.

- [ ] **Step 3: Remove `replace_community` from `FixupBody`**

In `src/fixup/schema.rs`, lines 57-60, **delete** the `replace_community: bool` field and the `is_false` helper (the helper stays only if used elsewhere — grep first).

Run: `grep -n 'is_false' src/fixup/schema.rs`. If `is_false` is only used by the deleted field, remove the helper too.

Final shape after delete:

```rust
#[serde(skip_serializing_if = "BTreeMap::is_empty")]
pub runtime_env: BTreeMap<String, String>,

#[serde(skip_serializing_if = "Option::is_none")]
pub sdist: Option<SdistFixup>,
}  // end of FixupBody
```

- [ ] **Step 4: Add `replace_community` to `FixupConfig`**

In `src/fixup/schema.rs`, lines 14-19, update `FixupConfig`:

```rust
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FixupConfig {
    pub top: FixupBody,
    /// Ordered list of `['cfg(<expr>)']` sections.
    pub cfg_sections: Vec<(String, FixupBody)>,
    /// File-level opt-out: drops the community fixup for this package
    /// before merging. Validated as local-only by `load_community`.
    pub replace_community: bool,
}
```

- [ ] **Step 5: Update `from_toml_str` to recognize the top-level key**

In `src/fixup/schema.rs`, replace the existing `from_toml_str` body (lines 97-114) with:

```rust
pub fn from_toml_str(s: &str) -> Result<Self, toml::de::Error> {
    let raw: toml::Table = toml::from_str(s)?;
    let mut top_table = toml::Table::new();
    let mut cfg_sections: Vec<(String, FixupBody)> = Vec::new();
    let mut replace_community = false;

    for (key, value) in raw {
        if key == "replace_community" {
            // Top-level only; FixupBody's deny_unknown_fields rejects it
            // when nested inside a cfg() section.
            replace_community = value
                .as_bool()
                .ok_or_else(|| serde::de::Error::custom(
                    "replace_community must be a boolean"
                ))?;
        } else if let Some(predicate) =
            key.strip_prefix("cfg(").and_then(|t| t.strip_suffix(')'))
        {
            let body: FixupBody = value.try_into()?;
            cfg_sections.push((predicate.to_string(), body));
        } else {
            top_table.insert(key, value);
        }
    }

    let top: FixupBody = toml::Value::Table(top_table).try_into()?;
    Ok(FixupConfig {
        top,
        cfg_sections,
        replace_community,
    })
}
```

Add at the top of the file (if not present): `use serde::de::Error as _;` — actually no, `serde::de::Error::custom` is invoked as a method on the `Error` trait; ensure `serde::de::Error` is in scope via `use serde::de::Error;` if needed. The compiler will tell you.

- [ ] **Step 6: Update `to_toml_string` to emit `replace_community` at file head**

In `src/fixup/schema.rs`, replace the existing `to_toml_string` body (lines 118-131) with:

```rust
pub fn to_toml_string(&self) -> Result<String, toml::ser::Error> {
    let mut out = String::new();
    if self.replace_community {
        out.push_str("replace_community = true\n");
    }
    out.push_str(&toml::to_string_pretty(&self.top)?);
    for (predicate, body) in &self.cfg_sections {
        let body_str = toml::to_string_pretty(body)?;
        if body_str.trim().is_empty() {
            continue;
        }
        out.push_str(&format!("\n[\"cfg({})\"]\n", predicate));
        out.push_str(&body_str);
    }
    Ok(out)
}
```

- [ ] **Step 7: Run all schema tests**

Run: `cargo test -p muntjac fixup::schema`

Expected: PASS for all 9 tests (including the 4 updated/added in step 1).

- [ ] **Step 8: Check for downstream breakage**

Run: `cargo check --all-targets 2>&1 | head -30`

Expected: should compile cleanly. If anything references `body.replace_community` (where `body` is `&FixupBody`), it's a compile error — update the call site to use `FixupConfig::replace_community`. Most likely call sites are in `src/fixup/layer.rs::merge_into` (S6 no-op'd the field) and any test fixtures.

- [ ] **Step 9: Run the full fixup module test suite**

Run: `cargo test -p muntjac fixup::`

Expected: PASS, no regressions in cfg/layer/loader/overlay/validate.

- [ ] **Step 10: Commit**

```bash
git add src/fixup/schema.rs
git commit -m "refactor(s7a): move replace_community from FixupBody to FixupConfig"
```

---

## Phase 3 — `RegistryConfig` typed enum

### Task 4: Create `src/fixup/registry.rs` with `RegistryConfig` enum and parser

**Files:**
- Create: `src/fixup/registry.rs`
- Modify: `src/fixup/mod.rs` (add `pub mod registry;` and re-export)

- [ ] **Step 1: Write the failing tests in a new file**

Create `src/fixup/registry.rs`:

```rust
//! `RegistryConfig` — typed parse of the `[fixups] registry = "…"` field.
//!
//! Three forms:
//!   - `"none"` — no community layer.
//!   - `"file:///abs/path"` — local checkout (S7a).
//!   - `"github.com/<owner>/<repo>"` — git registry (S7b; declared, errors on use).

use std::path::PathBuf;

/// Parsed registry configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryConfig {
    /// No community registry; community layer is empty.
    None,
    /// Local checkout (path is absolute).
    FileUrl(PathBuf),
    /// Git-hosted registry. Declared in S7a; `EffectiveFixups::load`
    /// errors with `GitRegistryNotImplemented` until S7b lands.
    Git { url: String, rev: Option<String> },
}

/// Parse the raw `registry` string (and optional `registry_rev`) from
/// `muntjac.toml` into a typed `RegistryConfig`.
///
/// Returns:
///   - `RegistryConfig::None`            on `"none"`
///   - `RegistryConfig::FileUrl(abs)`    on `"file:///abs/path"`
///   - `RegistryConfig::Git { url, rev}` on `"github.com/<owner>/<repo>"`
///   - `Err(BadRegistry)` for malformed strings.
///   - `Err(RegistryPathNotAbsolute)` for `"file://relative"` /  `"file://./x"`.
pub fn parse_registry_config(
    raw: &str,
    registry_rev: Option<&str>,
) -> Result<RegistryConfig, crate::error::ConfigError> {
    if raw == "none" {
        return Ok(RegistryConfig::None);
    }
    if let Some(rest) = raw.strip_prefix("file://") {
        // RFC 8089: an absolute path begins with `/`.
        if !rest.starts_with('/') {
            return Err(crate::error::ConfigError::RegistryPathNotAbsolute {
                path: rest.to_string(),
            });
        }
        return Ok(RegistryConfig::FileUrl(PathBuf::from(rest)));
    }
    if let Some(rest) = raw.strip_prefix("github.com/") {
        let parts: Vec<&str> = rest.split('/').collect();
        if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
            return Ok(RegistryConfig::Git {
                url: raw.to_string(),
                rev: registry_rev.map(|s| s.to_string()),
            });
        }
    }
    Err(crate::error::ConfigError::BadRegistry(raw.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_none() {
        assert_eq!(parse_registry_config("none", None).unwrap(), RegistryConfig::None);
    }

    #[test]
    fn parses_file_url_abs() {
        let got = parse_registry_config("file:///abs/path/to/registry", None).unwrap();
        assert_eq!(got, RegistryConfig::FileUrl(PathBuf::from("/abs/path/to/registry")));
    }

    #[test]
    fn rejects_file_url_relative() {
        let err = parse_registry_config("file://relative", None).unwrap_err();
        assert!(matches!(err, crate::error::ConfigError::RegistryPathNotAbsolute { .. }));
    }

    #[test]
    fn rejects_file_url_dot_slash() {
        let err = parse_registry_config("file://./registry", None).unwrap_err();
        assert!(matches!(err, crate::error::ConfigError::RegistryPathNotAbsolute { .. }));
    }

    #[test]
    fn parses_github_url_with_rev() {
        let got =
            parse_registry_config("github.com/jackmpcollins/muntjac-fixups", Some("abc123"))
                .unwrap();
        assert_eq!(
            got,
            RegistryConfig::Git {
                url: "github.com/jackmpcollins/muntjac-fixups".into(),
                rev: Some("abc123".into()),
            }
        );
    }

    #[test]
    fn parses_github_url_no_rev() {
        let got = parse_registry_config("github.com/o/r", None).unwrap();
        match got {
            RegistryConfig::Git { url, rev } => {
                assert_eq!(url, "github.com/o/r");
                assert_eq!(rev, None);
            }
            other => panic!("expected Git, got {:?}", other),
        }
    }

    #[test]
    fn rejects_malformed_github_url() {
        let err = parse_registry_config("github.com/onlyname", None).unwrap_err();
        assert!(matches!(err, crate::error::ConfigError::BadRegistry(_)));
    }

    #[test]
    fn rejects_bare_string() {
        let err = parse_registry_config("not-a-url", None).unwrap_err();
        assert!(matches!(err, crate::error::ConfigError::BadRegistry(_)));
    }
}
```

- [ ] **Step 2: Wire the module into `src/fixup/mod.rs`**

In `src/fixup/mod.rs`, add (alphabetical insertion after `overlay`):

```rust
pub mod registry;
```

And add a re-export (alphabetical, after `discover_overlay_files`):

```rust
pub use registry::{RegistryConfig, parse_registry_config};
```

- [ ] **Step 3: Run the new tests**

Run: `cargo test -p muntjac fixup::registry`

Expected: PASS for all 8 tests.

- [ ] **Step 4: Verify the module compiles in the existing build**

Run: `cargo check --all-targets 2>&1 | head -10`

Expected: clean compile.

- [ ] **Step 5: Commit**

```bash
git add src/fixup/registry.rs src/fixup/mod.rs
git commit -m "feat(s7a): RegistryConfig enum + parse_registry_config"
```

---

### Task 5: Migrate `FixupsConfig.registry` from `FixupRegistry(String)` to `RegistryConfig`

**Files:**
- Modify: `src/config.rs` (lines 64-89 `FixupsConfig`/`FixupRegistry`; line 270 `validate_registry` call; lines 378-393 `validate_registry`; tests at lines 551, 569)

The S0 scaffolding has `FixupRegistry(pub String)` and a String-shaped `validate_registry`. S7a replaces both with the typed enum from Task 4. The `validate` step in `Config::from_str` no longer needs to re-validate (the parse-time call covers it).

- [ ] **Step 1: Update the existing `validates_registry_form` test to expect the new path**

In `src/config.rs:551`, the existing `validates_registry_form` test asserts that a malformed registry produces a Parse-time error. The shape is unchanged. Update to assert the new variant if the test does explicit match-binding (most likely uses `is_err()` only — leave it).

Run: `grep -n -A12 'fn validates_registry_form' src/config.rs`

If the test only does `.is_err()` or asserts on the surface error message, it should pass with the new wiring. No changes needed in this step — proceed.

- [ ] **Step 2: Write the failing test for `RegistryConfig` integration into `Config`**

Add to `src/config.rs::tests`:

```rust
#[test]
fn config_parses_file_url_registry() {
    let toml = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target    = "x86_64-unknown-linux-gnu"
manylinux = "2_17"

[fixups]
registry = "file:///abs/path/to/checkout"
"#;
    let config = Config::from_str(toml).expect("parse");
    use crate::fixup::RegistryConfig;
    assert_eq!(
        config.fixups.registry,
        RegistryConfig::FileUrl(std::path::PathBuf::from("/abs/path/to/checkout"))
    );
}

#[test]
fn config_defaults_registry_to_none() {
    let toml = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target    = "x86_64-unknown-linux-gnu"
manylinux = "2_17"
"#;
    let config = Config::from_str(toml).expect("parse");
    use crate::fixup::RegistryConfig;
    assert_eq!(config.fixups.registry, RegistryConfig::None);
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p muntjac config_parses_file_url_registry -- --exact`

Expected: FAIL — `config.fixups.registry` is still `FixupRegistry(String)`.

- [ ] **Step 4: Replace `FixupRegistry(String)` with `RegistryConfig`**

In `src/config.rs`, replace lines 64-89 with:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FixupsConfig {
    pub registry: crate::fixup::RegistryConfig,
    pub registry_rev: Option<String>,
    pub allow_local_overrides: bool,
}

// Derive Default manually because RegistryConfig doesn't have Default.
// Actually, give RegistryConfig a Default impl (see Task 4 below — already there
// because we use `#[derive(Default)]`? No: RegistryConfig is an enum without
// Default. Add it now to keep FixupsConfig::default() simple.):

// ... see Step 5 below for the impl Default for RegistryConfig change.

// Deserialize wrapper: parse from TOML's raw view (where registry is a
// String) into the typed FixupsConfig.
impl<'de> serde::Deserialize<'de> for FixupsConfig {
    fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct Raw {
            #[serde(default = "default_registry_str")]
            registry: String,
            #[serde(default)]
            registry_rev: Option<String>,
            #[serde(default = "default_true")]
            allow_local_overrides: bool,
        }
        let raw = Raw::deserialize(de)?;
        let registry =
            crate::fixup::parse_registry_config(&raw.registry, raw.registry_rev.as_deref())
                .map_err(serde::de::Error::custom)?;

        // Warn if registry_rev is set but registry is None or FileUrl
        // (only meaningful for Git mode in S7b).
        if raw.registry_rev.is_some() {
            match &registry {
                crate::fixup::RegistryConfig::None
                | crate::fixup::RegistryConfig::FileUrl(_) => {
                    eprintln!(
                        "[muntjac] warn: registry_rev is ignored when registry is \"none\" or \"file://...\"; effective in S7b for git-based registries"
                    );
                }
                crate::fixup::RegistryConfig::Git { .. } => {}
            }
        }

        Ok(FixupsConfig {
            registry,
            registry_rev: raw.registry_rev,
            allow_local_overrides: raw.allow_local_overrides,
        })
    }
}

fn default_registry_str() -> String {
    "none".into()
}
fn default_true() -> bool {
    true
}

// Serialize back to TOML for fixtures / round-trips. Map RegistryConfig to its
// canonical string form.
impl serde::Serialize for FixupsConfig {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("FixupsConfig", 3)?;
        let reg_str = match &self.registry {
            crate::fixup::RegistryConfig::None => "none".to_string(),
            crate::fixup::RegistryConfig::FileUrl(p) => {
                format!("file://{}", p.display())
            }
            crate::fixup::RegistryConfig::Git { url, .. } => url.clone(),
        };
        st.serialize_field("registry", &reg_str)?;
        st.serialize_field("registry_rev", &self.registry_rev)?;
        st.serialize_field("allow_local_overrides", &self.allow_local_overrides)?;
        st.end()
    }
}
```

- [ ] **Step 5: Add a `Default` impl for `RegistryConfig`**

Append to `src/fixup/registry.rs`:

```rust
impl Default for RegistryConfig {
    fn default() -> Self {
        RegistryConfig::None
    }
}
```

Update the `derive` on the enum to add `Default` *or* keep the manual impl — pick one. Manual impl is fine and explicit.

- [ ] **Step 6: Provide a `Default` for `FixupsConfig` that uses sensible defaults**

In `src/config.rs`, find any place that relies on `FixupsConfig::default()` (likely the `#[serde(default)]` annotation on `RawConfig.fixups`). The `#[derive(Default)]` works because `RegistryConfig: Default` (Step 5), `Option<String>: Default`, but `allow_local_overrides: bool` defaults to `false` not `true`.

Add a manual `Default` impl:

```rust
impl Default for FixupsConfig {
    fn default() -> Self {
        Self {
            registry: crate::fixup::RegistryConfig::None,
            registry_rev: None,
            allow_local_overrides: true,
        }
    }
}
```

Remove `Default` from the `#[derive(...)]` on `FixupsConfig` (or leave it — the manual impl wins).

- [ ] **Step 7: Remove the now-unused `FixupRegistry` type and `validate_registry` function**

In `src/config.rs`, delete:
- The `pub struct FixupRegistry(pub String)` (was around lines 77-79).
- `impl Default for FixupRegistry` (was around 81-85).
- `fn default_registry() -> FixupRegistry` (was around 87-89).
- `fn validate_registry(...)` (was around 378-393).
- The call `validate_registry(&self.fixups.registry)?;` in `Config::validate()` (was around line 270).

The parse-time check in `parse_registry_config` (Task 4) is now the sole validation path.

- [ ] **Step 8: Update existing test `validates_registry_form`**

The test at line ~551 (currently asserts that `registry = "https://example.com/whatever"` errors). Adjust it if its error path now happens at parse-deserialization time instead of validation time — `Config::from_str` returns the same `ConfigError::Parse(..)` (because deserialize errors fold into Parse) or `ConfigError::BadRegistry`. Run the test, see what changes.

- [ ] **Step 9: Update existing test `accepts_registry_forms`**

At line ~569: this test cycles through valid registry forms. With the typed enum, the forms `"file:///tmp/fixups"`, `"github.com/jackmpcollins/muntjac-fixups"`, `"none"` should all deserialize successfully. Confirm by running:

Run: `cargo test -p muntjac accepts_registry_forms -- --exact`

Expected: PASS (the forms are still accepted; only the internal type changed).

- [ ] **Step 10: Run the full config + fixup test suites**

Run: `cargo test -p muntjac config:: fixup::`

Expected: PASS. Any FAIL → investigate and fix in this step (most likely a test that builds `FixupRegistry("...")` directly — replace with `RegistryConfig::FileUrl(...)` or whichever variant fits).

- [ ] **Step 11: Compile-check the whole project**

Run: `cargo check --all-targets 2>&1 | grep -E 'error|warning: unused' | head -30`

Expected: no errors. Warnings about unused imports are OK to leave for the next task to mop up.

- [ ] **Step 12: Commit**

```bash
git add src/config.rs src/fixup/registry.rs
git commit -m "feat(s7a): FixupsConfig.registry → RegistryConfig typed enum"
```

---

### Task 6: registry_rev stderr warning when registry is None or FileUrl

This was folded into Task 5 (Step 4, `Deserialize` impl). Verify the warning fires correctly.

- [ ] **Step 1: Write the integration test for the warning**

The warning is via `eprintln!`, which is harder to assert on. Add a unit-level test that exercises the path. Add to `src/config.rs::tests`:

```rust
#[test]
fn registry_rev_with_none_registry_parses_ok() {
    // This invocation triggers the stderr warning (we can't easily
    // capture it in-process; assert behavior: parse succeeds, value
    // is preserved on the struct).
    let toml = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target    = "x86_64-unknown-linux-gnu"
manylinux = "2_17"

[fixups]
registry = "none"
registry_rev = "abc123"
"#;
    let config = Config::from_str(toml).expect("parse");
    use crate::fixup::RegistryConfig;
    assert_eq!(config.fixups.registry, RegistryConfig::None);
    assert_eq!(config.fixups.registry_rev.as_deref(), Some("abc123"));
}

#[test]
fn registry_rev_with_git_registry_no_warning() {
    let toml = r#"
manifest_path   = "../pyproject.toml"
third_party_dir = "."
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target    = "x86_64-unknown-linux-gnu"
manylinux = "2_17"

[fixups]
registry = "github.com/o/r"
registry_rev = "abc123"
"#;
    let config = Config::from_str(toml).expect("parse");
    use crate::fixup::RegistryConfig;
    match &config.fixups.registry {
        RegistryConfig::Git { url, rev } => {
            assert_eq!(url, "github.com/o/r");
            assert_eq!(rev.as_deref(), Some("abc123"));
        }
        other => panic!("expected Git, got {:?}", other),
    }
}
```

- [ ] **Step 2: Run the new tests**

Run: `cargo test -p muntjac registry_rev_with -- --exact 2>&1 | tail -20`

Expected: PASS. The warning will print to stderr during the `none` test; test still passes (only checks parsed values).

- [ ] **Step 3: Commit**

```bash
git add src/config.rs
git commit -m "test(s7a): cover registry_rev warning path for none/git"
```

---

## Phase 4 — Loader: factor + add `load_community`

### Task 7: Factor `load_one_fixup` helper from `load_local` internals

**Files:**
- Modify: `src/fixup/loader.rs` (lines 38-110 `load_local`; factor inner per-file logic)

- [ ] **Step 1: Write the failing direct test for the factored helper**

Add to `src/fixup/loader.rs::tests`:

```rust
#[test]
fn load_one_fixup_loads_single_file() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("fixups.toml");
    fs::write(&path, r#"extra_deps = ["//x:y"]"#).unwrap();

    let cfg = super::load_one_fixup(&path).expect("loads");
    assert_eq!(cfg.top.extra_deps, vec!["//x:y"]);
}

#[test]
fn load_one_fixup_unknown_field_is_typed() {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("fixups.toml");
    fs::write(&path, r#"unknown_thing = []"#).unwrap();

    let err = super::load_one_fixup(&path).unwrap_err();
    match err {
        FixupError::UnknownField { field, .. } => assert_eq!(field, "unknown_thing"),
        other => panic!("expected UnknownField, got {:?}", other),
    }
}
```

- [ ] **Step 2: Run the failing tests**

Run: `cargo test -p muntjac fixup::loader::tests::load_one_fixup_loads_single_file -- --exact`

Expected: FAIL — `load_one_fixup` does not exist.

- [ ] **Step 3: Extract the helper**

In `src/fixup/loader.rs`, add `pub(crate) fn load_one_fixup` above `load_local`:

```rust
/// Read and parse a single `fixups.toml` file. Shared internals between
/// `load_local` and `load_community`.
pub(crate) fn load_one_fixup(toml_path: &Path) -> Result<FixupConfig, FixupError> {
    let body = std::fs::read_to_string(toml_path).map_err(|e| FixupError::Io {
        path: toml_path.to_path_buf(),
        source: e,
    })?;

    FixupConfig::from_toml_str(&body).map_err(|source| {
        let msg = source.to_string();
        if let Some(field) = extract_unknown_field(&msg) {
            FixupError::UnknownField {
                file: toml_path.to_path_buf(),
                field,
            }
        } else {
            FixupError::ParseError {
                file: toml_path.to_path_buf(),
                source,
            }
        }
    })
}
```

- [ ] **Step 4: Refactor `load_local` to use it**

Replace the inner read-and-parse block in `load_local` (around lines 85-104). The new body of the loop becomes:

```rust
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

    let pkg_name = PackageName::from_str(pkg_dir_name).map_err(|e| FixupError::Io {
        path: path.clone(),
        source: std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()),
    })?;

    let config = load_one_fixup(&toml_path)?;
    fixups.insert(pkg_name, config);
}
```

- [ ] **Step 5: Run all loader tests**

Run: `cargo test -p muntjac fixup::loader`

Expected: PASS — existing tests still pass, two new ones for `load_one_fixup` pass.

- [ ] **Step 6: Commit**

```bash
git add src/fixup/loader.rs
git commit -m "refactor(s7a): factor load_one_fixup from load_local"
```

---

### Task 8: Implement `load_community`

**Files:**
- Modify: `src/fixup/loader.rs` (add `load_community` after `load_local`; add tests)
- Modify: `src/fixup/mod.rs` (re-export `load_community`)

- [ ] **Step 1: Write the failing tests**

Add to `src/fixup/loader.rs::tests`:

```rust
#[test]
fn load_community_walks_packages_subdir() {
    let tmp = TempDir::new().unwrap();
    write(tmp.path(), "packages/pillow/fixups.toml", r#"extra_deps = ["//x:y"]"#);
    write(tmp.path(), "packages/numpy/fixups.toml", r#"extra_deps = ["//a:b"]"#);

    let set = load_community(tmp.path()).expect("loads");
    assert_eq!(set.len(), 2);
    let pillow = PackageName::from_str("pillow").unwrap();
    assert_eq!(set.get(&pillow).unwrap().top.extra_deps, vec!["//x:y"]);
}

#[test]
fn load_community_normalizes_pep503_names() {
    let tmp = TempDir::new().unwrap();
    write(tmp.path(), "packages/Pillow/fixups.toml", r#"extra_deps = []"#);
    let set = load_community(tmp.path()).expect("loads");
    assert!(set.get(&PackageName::from_str("pillow").unwrap()).is_some());
}

#[test]
fn load_community_rejects_replace_community_on_community_side() {
    let tmp = TempDir::new().unwrap();
    write(
        tmp.path(),
        "packages/evil/fixups.toml",
        "replace_community = true\nextra_deps = []",
    );
    let err = load_community(tmp.path()).unwrap_err();
    match err {
        FixupError::ReplaceCommunityInCommunity { file } => {
            assert!(file.ends_with("packages/evil/fixups.toml"));
        }
        other => panic!("expected ReplaceCommunityInCommunity, got {:?}", other),
    }
}

#[test]
fn load_community_errors_if_packages_dir_missing() {
    let tmp = TempDir::new().unwrap();
    // tmp/packages does not exist.
    let err = load_community(tmp.path()).unwrap_err();
    match err {
        FixupError::RegistryPathNotFound { path } => {
            assert!(path.ends_with("packages"));
        }
        other => panic!("expected RegistryPathNotFound, got {:?}", other),
    }
}

#[test]
fn load_community_loads_zero_packages_ok() {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir_all(tmp.path().join("packages")).unwrap();
    let set = load_community(tmp.path()).expect("loads");
    assert!(set.is_empty());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p muntjac fixup::loader::tests::load_community_walks_packages_subdir -- --exact`

Expected: FAIL — `load_community` does not exist.

- [ ] **Step 3: Implement `load_community`**

Add to `src/fixup/loader.rs` after `load_local`:

```rust
/// Load every `<registry_dir>/packages/<pkg>/fixups.toml` into a
/// `FixupSet`. Package directory names are PEP 503-normalized.
///
/// Differs from `load_local` in two ways:
///   1. Walks `<registry_dir>/packages/` (not `<registry_dir>/fixups/`).
///   2. Errors with `RegistryPathNotFound` if `<registry_dir>/packages/`
///      doesn't exist (because the user explicitly pointed registry =
///      "file://<registry_dir>" at this path — silent emptiness would
///      mask a configuration mistake).
///   3. Rejects any fixup with `replace_community = true`; that flag is
///      a local-only opt-out.
pub fn load_community(registry_dir: &Path) -> Result<FixupSet, FixupError> {
    let packages_dir = registry_dir.join("packages");
    if !packages_dir.is_dir() {
        return Err(FixupError::RegistryPathNotFound {
            path: packages_dir,
        });
    }

    let mut fixups = BTreeMap::new();
    let entries = std::fs::read_dir(&packages_dir).map_err(|e| FixupError::Io {
        path: packages_dir.clone(),
        source: e,
    })?;

    for entry in entries {
        let entry = entry.map_err(|e| FixupError::Io {
            path: packages_dir.clone(),
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

        let pkg_name = PackageName::from_str(pkg_dir_name).map_err(|e| FixupError::Io {
            path: path.clone(),
            source: std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()),
        })?;

        let config = load_one_fixup(&toml_path)?;

        if config.replace_community {
            return Err(FixupError::ReplaceCommunityInCommunity { file: toml_path });
        }

        fixups.insert(pkg_name, config);
    }

    Ok(FixupSet::from_map_internal(fixups))
}
```

- [ ] **Step 4: Add a private constructor on `FixupSet` for the loader path**

`FixupSet::from_map_for_test` is `#[cfg(test)]`. Loader needs a non-test constructor. In `src/fixup/loader.rs`, add to the `impl FixupSet` block:

```rust
impl FixupSet {
    // ... existing methods ...

    /// Internal constructor — used by `load_local` and `load_community`.
    pub(crate) fn from_map_internal(
        fixups: BTreeMap<PackageName, FixupConfig>,
    ) -> Self {
        Self { fixups }
    }
}
```

Then update `load_local` to use `FixupSet::from_map_internal(fixups)` at its return site (it currently returns `FixupSet { fixups }` — change to `Ok(FixupSet::from_map_internal(fixups))` for symmetry). This step is optional cleanup; functionality unchanged.

- [ ] **Step 5: Re-export `load_community` from the module**

In `src/fixup/mod.rs`, update the `pub use loader::...` line:

```rust
pub use loader::{FixupSet, load_community, load_local};
```

- [ ] **Step 6: Run all loader tests**

Run: `cargo test -p muntjac fixup::loader`

Expected: PASS — all existing tests + 5 new ones.

- [ ] **Step 7: Commit**

```bash
git add src/fixup/loader.rs src/fixup/mod.rs
git commit -m "feat(s7a): load_community with packages/ layout + replace_community guard"
```

---

## Phase 5 — Cross-layer merge

### Task 9: Implement `merge_resolved`

**Files:**
- Modify: `src/fixup/layer.rs` (add `merge_resolved` + tests)
- Modify: `src/fixup/mod.rs` (re-export)

- [ ] **Step 1: Write the failing tests, one per field rule**

Add to `src/fixup/layer.rs::tests`:

```rust
#[test]
fn merge_resolved_extra_deps_community_first_dedup() {
    let c = ResolvedFixup {
        extra_deps: vec!["//c:base".into(), "//shared:dep".into()],
        ..Default::default()
    };
    let l = ResolvedFixup {
        extra_deps: vec!["//shared:dep".into(), "//l:base".into()],
        ..Default::default()
    };
    let merged = super::merge_resolved(c, l);
    assert_eq!(
        merged.extra_deps,
        vec!["//c:base", "//shared:dep", "//l:base"]
    );
}

#[test]
fn merge_resolved_omit_deps_community_first_dedup() {
    let c = ResolvedFixup {
        omit_deps: vec!["one".into(), "two".into()],
        ..Default::default()
    };
    let l = ResolvedFixup {
        omit_deps: vec!["two".into(), "three".into()],
        ..Default::default()
    };
    let merged = super::merge_resolved(c, l);
    assert_eq!(merged.omit_deps, vec!["one", "two", "three"]);
}

#[test]
fn merge_resolved_replace_deps_local_wins_on_key_collision() {
    let mut c_rd = std::collections::BTreeMap::new();
    c_rd.insert("foo".to_string(), "//c:foo".to_string());
    c_rd.insert("only-community".to_string(), "//c:only".to_string());
    let c = ResolvedFixup {
        replace_deps: c_rd,
        ..Default::default()
    };
    let mut l_rd = std::collections::BTreeMap::new();
    l_rd.insert("foo".to_string(), "//l:foo".to_string());
    l_rd.insert("only-local".to_string(), "//l:only".to_string());
    let l = ResolvedFixup {
        replace_deps: l_rd,
        ..Default::default()
    };
    let merged = super::merge_resolved(c, l);
    assert_eq!(merged.replace_deps.get("foo").map(|s| s.as_str()), Some("//l:foo"));
    assert_eq!(merged.replace_deps.get("only-community").map(|s| s.as_str()), Some("//c:only"));
    assert_eq!(merged.replace_deps.get("only-local").map(|s| s.as_str()), Some("//l:only"));
}

#[test]
fn merge_resolved_prefer_wheel_local_some_wins() {
    let c = ResolvedFixup {
        prefer_wheel: Some("sha256:aaa".into()),
        ..Default::default()
    };
    let l = ResolvedFixup {
        prefer_wheel: Some("sha256:bbb".into()),
        ..Default::default()
    };
    let merged = super::merge_resolved(c, l);
    assert_eq!(merged.prefer_wheel.as_deref(), Some("sha256:bbb"));
}

#[test]
fn merge_resolved_prefer_wheel_community_kept_when_local_none() {
    let c = ResolvedFixup {
        prefer_wheel: Some("sha256:aaa".into()),
        ..Default::default()
    };
    let l = ResolvedFixup::default();
    let merged = super::merge_resolved(c, l);
    assert_eq!(merged.prefer_wheel.as_deref(), Some("sha256:aaa"));
}

#[test]
fn merge_resolved_exclude_wheels_concat_dedup() {
    let c = ResolvedFixup {
        exclude_wheels: vec!["*win32*".into()],
        ..Default::default()
    };
    let l = ResolvedFixup {
        exclude_wheels: vec!["*win32*".into(), "*cuda*".into()],
        ..Default::default()
    };
    let merged = super::merge_resolved(c, l);
    assert_eq!(merged.exclude_wheels, vec!["*win32*", "*cuda*"]);
}

#[test]
fn merge_resolved_overlay_local_some_wins() {
    let c = ResolvedFixup {
        overlay: Some(std::path::PathBuf::from("c-overlay")),
        ..Default::default()
    };
    let l = ResolvedFixup {
        overlay: Some(std::path::PathBuf::from("l-overlay")),
        ..Default::default()
    };
    let merged = super::merge_resolved(c, l);
    assert_eq!(
        merged.overlay.as_deref(),
        Some(std::path::Path::new("l-overlay"))
    );
}

#[test]
fn merge_resolved_entry_points_local_some_wins() {
    let c = ResolvedFixup {
        entry_points: Some(crate::fixup::EntryPoints::Named(vec!["c-bin".into()])),
        ..Default::default()
    };
    let l = ResolvedFixup {
        entry_points: Some(crate::fixup::EntryPoints::Named(vec!["l-bin".into()])),
        ..Default::default()
    };
    let merged = super::merge_resolved(c, l);
    match merged.entry_points {
        Some(crate::fixup::EntryPoints::Named(names)) => {
            assert_eq!(names, vec!["l-bin".to_string()]);
        }
        other => panic!("expected Named, got {:?}", other),
    }
}

#[test]
fn merge_resolved_visibility_local_some_wins() {
    let c = ResolvedFixup {
        visibility: Some(vec!["//c:...".into()]),
        ..Default::default()
    };
    let l = ResolvedFixup {
        visibility: Some(vec!["//l:...".into()]),
        ..Default::default()
    };
    let merged = super::merge_resolved(c, l);
    assert_eq!(merged.visibility, Some(vec!["//l:...".to_string()]));
}

#[test]
fn merge_resolved_labels_concat_dedup() {
    let c = ResolvedFixup {
        labels: vec!["tag-a".into(), "tag-b".into()],
        ..Default::default()
    };
    let l = ResolvedFixup {
        labels: vec!["tag-b".into(), "tag-c".into()],
        ..Default::default()
    };
    let merged = super::merge_resolved(c, l);
    assert_eq!(merged.labels, vec!["tag-a", "tag-b", "tag-c"]);
}

#[test]
fn merge_resolved_runtime_env_local_overrides_key() {
    let mut c_re = std::collections::BTreeMap::new();
    c_re.insert("FOO".to_string(), "c".to_string());
    c_re.insert("BAR".to_string(), "c".to_string());
    let c = ResolvedFixup {
        runtime_env: c_re,
        ..Default::default()
    };
    let mut l_re = std::collections::BTreeMap::new();
    l_re.insert("FOO".to_string(), "l".to_string());
    l_re.insert("BAZ".to_string(), "l".to_string());
    let l = ResolvedFixup {
        runtime_env: l_re,
        ..Default::default()
    };
    let merged = super::merge_resolved(c, l);
    assert_eq!(merged.runtime_env.get("FOO").map(|s| s.as_str()), Some("l"));
    assert_eq!(merged.runtime_env.get("BAR").map(|s| s.as_str()), Some("c"));
    assert_eq!(merged.runtime_env.get("BAZ").map(|s| s.as_str()), Some("l"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p muntjac fixup::layer::tests::merge_resolved -- --skip resolve_`

Expected: FAIL — `merge_resolved` function does not exist.

- [ ] **Step 3: Implement `merge_resolved`**

Add to `src/fixup/layer.rs`:

```rust
/// Cross-layer merge. Community is contributed first; local overrides
/// scalar/Option fields and extends list/map fields.
///
/// Rules:
///   - Vec<String>: community ++ local, dedup preserved-first
///   - BTreeMap<String, String>: extend (local key overwrites community)
///   - Option<T>: local wins if Some, else community
pub fn merge_resolved(community: ResolvedFixup, local: ResolvedFixup) -> ResolvedFixup {
    ResolvedFixup {
        extra_deps: concat_dedup(community.extra_deps, local.extra_deps),
        omit_deps: concat_dedup(community.omit_deps, local.omit_deps),
        replace_deps: extend_map(community.replace_deps, local.replace_deps),
        prefer_wheel: local.prefer_wheel.or(community.prefer_wheel),
        exclude_wheels: concat_dedup(community.exclude_wheels, local.exclude_wheels),
        overlay: local.overlay.or(community.overlay),
        entry_points: local.entry_points.or(community.entry_points),
        visibility: local.visibility.or(community.visibility),
        labels: concat_dedup(community.labels, local.labels),
        runtime_env: extend_map(community.runtime_env, local.runtime_env),
    }
}

/// Concatenate two Vec<String>, dedup preserving first occurrence.
fn concat_dedup(a: Vec<String>, b: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::with_capacity(a.len() + b.len());
    for s in a.into_iter().chain(b) {
        if seen.insert(s.clone()) {
            out.push(s);
        }
    }
    out
}

fn extend_map(
    mut a: std::collections::BTreeMap<String, String>,
    b: std::collections::BTreeMap<String, String>,
) -> std::collections::BTreeMap<String, String> {
    a.extend(b);
    a
}
```

- [ ] **Step 4: Run all layer tests**

Run: `cargo test -p muntjac fixup::layer`

Expected: PASS, all existing S6 tests + 11 new `merge_resolved_*` tests.

- [ ] **Step 5: Commit**

```bash
git add src/fixup/layer.rs
git commit -m "feat(s7a): merge_resolved with per-field cross-layer rules"
```

---

## Phase 6 — EffectiveFixups facade

### Task 10: `EffectiveFixups` struct + `::resolve` with `replace_community` escape hatch

**Files:**
- Modify: `src/fixup/layer.rs` (add `EffectiveFixups`)
- Modify: `src/fixup/mod.rs` (re-export)

- [ ] **Step 1: Write the failing tests**

Add to `src/fixup/layer.rs::tests`:

```rust
fn ctx_linux() -> CfgContext<'static> {
    use pep440_rs::Version;
    use std::str::FromStr;
    // We need static lifetime; use leaking for test ergonomics.
    let v: &'static Version = Box::leak(Box::new(Version::from_str("1.0").unwrap()));
    let py: Version = Version::from_str("3.12").unwrap();
    CfgContext {
        package_version: v,
        python_version: py,
        target_os: "linux",
        target_arch: "x86_64",
        target_env: "gnu",
    }
}

fn fixup_with(extra_deps: Vec<&str>) -> crate::fixup::FixupConfig {
    crate::fixup::FixupConfig {
        top: crate::fixup::FixupBody {
            extra_deps: extra_deps.into_iter().map(String::from).collect(),
            ..Default::default()
        },
        cfg_sections: vec![],
        replace_community: false,
    }
}

#[test]
fn effective_fixups_resolve_returns_default_when_neither_layer_has_pkg() {
    use crate::fixup::loader::FixupSet;
    let eff = super::EffectiveFixups {
        community: FixupSet::default(),
        local: FixupSet::default(),
    };
    let pkg = pep508_rs::PackageName::from_str("missing").unwrap();
    let rf = eff.resolve(&pkg, &ctx_linux());
    assert!(rf.extra_deps.is_empty());
    assert!(rf.replace_deps.is_empty());
    assert!(rf.overlay.is_none());
}

#[test]
fn effective_fixups_resolve_community_only() {
    use crate::fixup::loader::FixupSet;
    let mut comm = std::collections::BTreeMap::new();
    let pkg = pep508_rs::PackageName::from_str("pkg").unwrap();
    comm.insert(pkg.clone(), fixup_with(vec!["//c:base"]));
    let eff = super::EffectiveFixups {
        community: FixupSet::from_map_for_test(comm),
        local: FixupSet::default(),
    };
    let rf = eff.resolve(&pkg, &ctx_linux());
    assert_eq!(rf.extra_deps, vec!["//c:base"]);
}

#[test]
fn effective_fixups_resolve_local_only() {
    use crate::fixup::loader::FixupSet;
    let mut loc = std::collections::BTreeMap::new();
    let pkg = pep508_rs::PackageName::from_str("pkg").unwrap();
    loc.insert(pkg.clone(), fixup_with(vec!["//l:base"]));
    let eff = super::EffectiveFixups {
        community: FixupSet::default(),
        local: FixupSet::from_map_for_test(loc),
    };
    let rf = eff.resolve(&pkg, &ctx_linux());
    assert_eq!(rf.extra_deps, vec!["//l:base"]);
}

#[test]
fn effective_fixups_resolve_both_layers_concat() {
    use crate::fixup::loader::FixupSet;
    let pkg = pep508_rs::PackageName::from_str("pkg").unwrap();
    let mut comm = std::collections::BTreeMap::new();
    comm.insert(pkg.clone(), fixup_with(vec!["//c:base"]));
    let mut loc = std::collections::BTreeMap::new();
    loc.insert(pkg.clone(), fixup_with(vec!["//l:base"]));
    let eff = super::EffectiveFixups {
        community: FixupSet::from_map_for_test(comm),
        local: FixupSet::from_map_for_test(loc),
    };
    let rf = eff.resolve(&pkg, &ctx_linux());
    assert_eq!(rf.extra_deps, vec!["//c:base", "//l:base"]);
}

#[test]
fn effective_fixups_resolve_replace_community_drops_community() {
    use crate::fixup::loader::FixupSet;
    let pkg = pep508_rs::PackageName::from_str("pkg").unwrap();
    let mut comm = std::collections::BTreeMap::new();
    comm.insert(pkg.clone(), fixup_with(vec!["//c:never-merged"]));

    let mut loc = std::collections::BTreeMap::new();
    let mut local_cfg = fixup_with(vec!["//l:only"]);
    local_cfg.replace_community = true;
    loc.insert(pkg.clone(), local_cfg);

    let eff = super::EffectiveFixups {
        community: FixupSet::from_map_for_test(comm),
        local: FixupSet::from_map_for_test(loc),
    };
    let rf = eff.resolve(&pkg, &ctx_linux());
    assert_eq!(rf.extra_deps, vec!["//l:only"]);  // community discarded
}

#[test]
fn effective_fixups_resolve_replace_community_with_empty_local_yields_empty() {
    use crate::fixup::loader::FixupSet;
    let pkg = pep508_rs::PackageName::from_str("pkg").unwrap();
    let mut comm = std::collections::BTreeMap::new();
    comm.insert(pkg.clone(), fixup_with(vec!["//c:base"]));

    let mut loc = std::collections::BTreeMap::new();
    let mut local_cfg = crate::fixup::FixupConfig::default();
    local_cfg.replace_community = true;
    loc.insert(pkg.clone(), local_cfg);

    let eff = super::EffectiveFixups {
        community: FixupSet::from_map_for_test(comm),
        local: FixupSet::from_map_for_test(loc),
    };
    let rf = eff.resolve(&pkg, &ctx_linux());
    assert!(rf.extra_deps.is_empty());
    assert!(rf.overlay.is_none());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p muntjac fixup::layer::tests::effective_fixups -- --skip merge`

Expected: FAIL — `EffectiveFixups` does not exist.

- [ ] **Step 3: Implement `EffectiveFixups` + `::resolve`**

Add to `src/fixup/layer.rs`:

```rust
use crate::fixup::loader::FixupSet;
use pep508_rs::PackageName;

/// Two-layer fixup facade. Owns the community and local `FixupSet`s
/// and exposes a single `.resolve(pkg, ctx)` entry point that hides
/// the layering from the emitter.
#[derive(Debug, Default, Clone)]
pub struct EffectiveFixups {
    pub community: FixupSet,
    pub local: FixupSet,
}

impl EffectiveFixups {
    /// Resolve a package's fixup for one cell. Always returns a
    /// `ResolvedFixup` — default if neither layer has the package.
    ///
    /// Algorithm (per design spec §4.1):
    ///   1. Look up local first; if it has `replace_community = true`,
    ///      drop the community layer for this package.
    ///   2. Fully resolve each layer via `resolve_for_cell`.
    ///   3. Cross-layer merge via `merge_resolved`.
    pub fn resolve(&self, pkg: &PackageName, ctx: &CfgContext<'_>) -> ResolvedFixup {
        let local_cfg = self.local.get(pkg);
        let community_cfg = match local_cfg {
            Some(c) if c.replace_community => None,
            _ => self.community.get(pkg),
        };

        let community_resolved = community_cfg
            .map(|cfg| resolve_for_cell(cfg, ctx))
            .unwrap_or_default();
        let local_resolved = local_cfg
            .map(|cfg| resolve_for_cell(cfg, ctx))
            .unwrap_or_default();

        merge_resolved(community_resolved, local_resolved)
    }
}
```

Note: `FixupSet` needs a `Default` impl so `EffectiveFixups::default()` works. Verify:

Run: `grep -n 'Default' src/fixup/loader.rs | head`

Expected: `#[derive(Debug, Default, Clone)]` already present on `FixupSet` (line 12).

- [ ] **Step 4: Re-export `EffectiveFixups` + `merge_resolved`**

In `src/fixup/mod.rs`, update the `pub use layer::` line:

```rust
pub use layer::{EffectiveFixups, ResolvedFixup, merge_resolved, resolve_for_cell};
```

- [ ] **Step 5: Run all layer tests**

Run: `cargo test -p muntjac fixup::layer`

Expected: PASS — all 6 `effective_fixups_*` tests pass, plus existing.

- [ ] **Step 6: Commit**

```bash
git add src/fixup/layer.rs src/fixup/mod.rs
git commit -m "feat(s7a): EffectiveFixups facade with resolve()"
```

---

### Task 11: `EffectiveFixups::load` dispatcher

**Files:**
- Modify: `src/fixup/layer.rs` (add `EffectiveFixups::load`)

- [ ] **Step 1: Write the failing tests**

Add to `src/fixup/layer.rs::tests`:

```rust
#[test]
fn effective_fixups_load_none_yields_empty_community() {
    use crate::fixup::RegistryConfig;
    use tempfile::TempDir;
    let tmp = TempDir::new().unwrap();
    let eff = super::EffectiveFixups::load(&RegistryConfig::None, tmp.path(), true)
        .expect("loads");
    assert!(eff.community.is_empty());
    assert!(eff.local.is_empty());  // no fixups/ subdir
}

#[test]
fn effective_fixups_load_file_url_walks_packages() {
    use crate::fixup::RegistryConfig;
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let registry_dir = tmp.path().join("registry");
    let tpd = tmp.path().join("third-party/python");
    std::fs::create_dir_all(registry_dir.join("packages/pillow")).unwrap();
    std::fs::write(
        registry_dir.join("packages/pillow/fixups.toml"),
        r#"extra_deps = ["//c:libjpeg"]"#,
    )
    .unwrap();
    std::fs::create_dir_all(&tpd).unwrap();

    let registry = RegistryConfig::FileUrl(registry_dir);
    let eff = super::EffectiveFixups::load(&registry, &tpd, true).expect("loads");
    let pillow = pep508_rs::PackageName::from_str("pillow").unwrap();
    assert!(eff.community.get(&pillow).is_some());
}

#[test]
fn effective_fixups_load_file_url_missing_packages_errors() {
    use crate::fixup::RegistryConfig;
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let registry = RegistryConfig::FileUrl(tmp.path().to_path_buf());
    let err = super::EffectiveFixups::load(&registry, tmp.path(), true).unwrap_err();
    match err {
        crate::fixup::FixupError::RegistryPathNotFound { .. } => {}
        other => panic!("expected RegistryPathNotFound, got {:?}", other),
    }
}

#[test]
fn effective_fixups_load_git_errors_in_s7a() {
    use crate::fixup::RegistryConfig;
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let registry = RegistryConfig::Git {
        url: "github.com/x/y".into(),
        rev: None,
    };
    let err = super::EffectiveFixups::load(&registry, tmp.path(), true).unwrap_err();
    match err {
        crate::fixup::FixupError::GitRegistryNotImplemented { registry } => {
            assert_eq!(registry, "github.com/x/y");
        }
        other => panic!("expected GitRegistryNotImplemented, got {:?}", other),
    }
}

#[test]
fn effective_fixups_load_allow_local_overrides_false_skips_local() {
    use crate::fixup::RegistryConfig;
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    let tpd = tmp.path().join("third-party/python");
    std::fs::create_dir_all(tpd.join("fixups/pillow")).unwrap();
    std::fs::write(
        tpd.join("fixups/pillow/fixups.toml"),
        r#"extra_deps = ["//local:pillow"]"#,
    )
    .unwrap();

    let eff = super::EffectiveFixups::load(&RegistryConfig::None, &tpd, false)
        .expect("loads");
    assert!(eff.local.is_empty());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p muntjac fixup::layer::tests::effective_fixups_load -- --skip resolve`

Expected: FAIL — `EffectiveFixups::load` does not exist.

- [ ] **Step 3: Implement `EffectiveFixups::load`**

In `src/fixup/layer.rs`, add to the `impl EffectiveFixups` block:

```rust
impl EffectiveFixups {
    /// Load both layers per the registry configuration and the
    /// `allow_local_overrides` flag.
    ///
    /// - `RegistryConfig::None`        → community is empty.
    /// - `RegistryConfig::FileUrl(p)`  → community loaded from `<p>/packages/`.
    /// - `RegistryConfig::Git { .. }`  → errors with `GitRegistryNotImplemented` (S7b implements).
    /// - `allow_local_overrides=false` → local is empty regardless of disk state.
    pub fn load(
        registry: &crate::fixup::RegistryConfig,
        third_party_dir: &std::path::Path,
        allow_local_overrides: bool,
    ) -> Result<Self, crate::fixup::FixupError> {
        let community = match registry {
            crate::fixup::RegistryConfig::None => FixupSet::default(),
            crate::fixup::RegistryConfig::FileUrl(registry_dir) => {
                crate::fixup::load_community(registry_dir)?
            }
            crate::fixup::RegistryConfig::Git { url, .. } => {
                return Err(crate::fixup::FixupError::GitRegistryNotImplemented {
                    registry: url.clone(),
                });
            }
        };

        let local = if allow_local_overrides {
            crate::fixup::load_local(third_party_dir)?
        } else {
            FixupSet::default()
        };

        Ok(Self { community, local })
    }
}
```

- [ ] **Step 4: Run all layer tests**

Run: `cargo test -p muntjac fixup::layer`

Expected: PASS — 5 `effective_fixups_load_*` tests + everything prior.

- [ ] **Step 5: Commit**

```bash
git add src/fixup/layer.rs
git commit -m "feat(s7a): EffectiveFixups::load dispatcher"
```

---

## Phase 7 — `BuildEmitContext<'a>` refactor (TD-S6-04)

### Task 12: Introduce `BuildEmitContext` and migrate `build_emit_input` signature

**Files:**
- Modify: `src/buck/emit.rs` (lines 122-132 `build_emit_input` signature; internal references at lines 142, 192, 424)

- [ ] **Step 1: Add `BuildEmitContext` struct**

In `src/buck/emit.rs`, insert before `build_emit_input` (around line 122):

```rust
/// Optional/contextual inputs to the emit pipeline. Collapses three
/// `Option<&_>` positional arguments accumulated across S5+S6 into a
/// single struct so S7a's `EffectiveFixups` slots in without further
/// positional-arg churn. (TD-S6-04 resolution.)
#[derive(Debug, Clone, Default)]
pub struct BuildEmitContext<'a> {
    /// S5: prebake manifest for sdist routing. `None` when no manifest exists.
    pub manifest: Option<&'a crate::sdist::Manifest>,
    /// S6 (now layered in S7a): community + local fixups via the
    /// `EffectiveFixups` facade. `None` is equivalent to "no fixups".
    pub fixups: Option<&'a crate::fixup::EffectiveFixups>,
    /// Absolute path to the resolved `third_party_dir` for overlay walk
    /// and other filesystem ops. `None` falls back to `tree.third_party_dir`
    /// (works when already absolute; e.g. in unit tests with TempDir).
    pub abs_third_party_dir: Option<&'a std::path::Path>,
}
```

- [ ] **Step 2: Update `build_emit_input` signature**

In `src/buck/emit.rs:122`, change:

```rust
pub fn build_emit_input(
    config: &Config,
    tree: &Tree,
    lockfile: &Lockfile,
    manifest: Option<&crate::sdist::Manifest>,
    fixups: Option<&crate::fixup::FixupSet>,
    abs_third_party_dir: Option<&std::path::Path>,
) -> anyhow::Result<EmitInput> {
```

to:

```rust
pub fn build_emit_input(
    config: &Config,
    tree: &Tree,
    lockfile: &Lockfile,
    ctx: &BuildEmitContext<'_>,
) -> anyhow::Result<EmitInput> {
    // Locals for backwards-compatible body references.
    let manifest = ctx.manifest;
    let fixups = ctx.fixups;
    let abs_third_party_dir = ctx.abs_third_party_dir;
```

This local rebinding lets the existing body (which uses `manifest`, `fixups`, `abs_third_party_dir` as bare names) keep working. **Important:** the `fixups` local is currently typed `Option<&FixupSet>` and the body uses `fs.get(...)` directly. After this signature change, `fixups` is `Option<&EffectiveFixups>` — the body must call `ef.resolve(...)` instead. That migration is in Task 13.

For Step 2, **temporarily change `fixups` field to `Option<&FixupSet>`** so this signature change compiles in isolation:

```rust
pub struct BuildEmitContext<'a> {
    pub manifest: Option<&'a crate::sdist::Manifest>,
    pub fixups: Option<&'a crate::fixup::FixupSet>,  // TEMP — Task 13 swaps to EffectiveFixups
    pub abs_third_party_dir: Option<&'a std::path::Path>,
}
```

- [ ] **Step 3: Migrate every test call site inside `src/buck/emit.rs`**

There are 8 in-file call sites (per `grep` from the planning phase, lines 750, 852, 986, 1117, 1220, 1337, 1426, 1521, 1635, 1758, 1793, 1870, 1934 — verify count fresh):

Run: `grep -c 'build_emit_input(' src/buck/emit.rs`

For each call site, the migration pattern is:

Before:
```rust
build_emit_input(&config, &tree, &lockfile, None, Some(&fixups), None)
```

After:
```rust
build_emit_input(
    &config,
    &tree,
    &lockfile,
    &BuildEmitContext {
        manifest: None,
        fixups: Some(&fixups),
        abs_third_party_dir: None,
    },
)
```

And the `None, None, None` baseline becomes `&BuildEmitContext::default()`.

Apply the migration to all call sites. For each, copy the existing optional-args pattern into the new struct-literal form.

- [ ] **Step 4: Migrate the buckify CLI call site**

In `src/cli/buckify.rs:50-57`:

Before:
```rust
let input = build_emit_input(
    &config,
    tree,
    &lockfile,
    manifest.as_ref(),
    Some(&fixups),
    Some(&third_party_dir),
)?;
```

After (`fixups` still `&FixupSet` until Task 13 swaps it to `EffectiveFixups`):
```rust
use crate::buck::BuildEmitContext;
let input = build_emit_input(
    &config,
    tree,
    &lockfile,
    &BuildEmitContext {
        manifest: manifest.as_ref(),
        fixups: Some(&fixups),
        abs_third_party_dir: Some(&third_party_dir),
    },
)?;
```

Also add `BuildEmitContext` to the `use crate::buck::` line at the top.

- [ ] **Step 5: Re-export `BuildEmitContext` from `src/buck/mod.rs`**

Run: `grep -n 'build_emit_input\|emit::' src/buck/mod.rs`

Find the re-export line for `build_emit_input` and add `BuildEmitContext` alongside:

```rust
pub use emit::{BuildEmitContext, /* ... existing items ... */, build_emit_input};
```

- [ ] **Step 6: Compile**

Run: `cargo build 2>&1 | tail -20`

Expected: clean build.

- [ ] **Step 7: Run the full test suite**

Run: `cargo test 2>&1 | grep -E '^test result' | head -20`

Expected: PASS, no regressions. Tests should run as before — signature change only.

- [ ] **Step 8: Commit**

```bash
git add src/buck/emit.rs src/buck/mod.rs src/cli/buckify.rs
git commit -m "refactor(s7a): build_emit_input takes BuildEmitContext (TD-S6-04)"
```

---

### Task 13: Switch `BuildEmitContext.fixups` to `EffectiveFixups`; migrate emit body

**Files:**
- Modify: `src/buck/emit.rs` (struct definition + lines 192-205 per-cell resolve + lines 424-453 first-cell-rf)
- Modify: `src/cli/buckify.rs` (construct `EffectiveFixups::load(...)` instead of `load_local(...)`)
- Modify: every test call site in `src/buck/emit.rs::tests::` that uses `Some(&fixups)`

- [ ] **Step 1: Write the failing test for layering through build_emit_input**

Add to `src/buck/emit.rs::tests` (find an appropriate spot near the other `FixupSet`-using tests, e.g. after line 1521's test):

```rust
#[test]
fn build_emit_input_applies_community_and_local_extra_deps() {
    use crate::fixup::{EffectiveFixups, FixupBody, FixupConfig, FixupSet};
    use std::collections::BTreeMap;

    let config = make_minimal_config_single_cell();  // existing helper in this test module
    let tree = config.trees[0].clone();
    let lockfile = make_minimal_lockfile_with_one_wheel("pkg-a");  // existing helper

    let pkg_name = pep508_rs::PackageName::from_str("pkg-a").unwrap();

    let mut comm_map = BTreeMap::new();
    comm_map.insert(
        pkg_name.clone(),
        FixupConfig {
            top: FixupBody {
                extra_deps: vec!["//c:base".into()],
                ..Default::default()
            },
            cfg_sections: vec![],
            replace_community: false,
        },
    );

    let mut loc_map = BTreeMap::new();
    loc_map.insert(
        pkg_name.clone(),
        FixupConfig {
            top: FixupBody {
                extra_deps: vec!["//l:base".into()],
                ..Default::default()
            },
            cfg_sections: vec![],
            replace_community: false,
        },
    );

    let eff = EffectiveFixups {
        community: FixupSet::from_map_for_test(comm_map),
        local: FixupSet::from_map_for_test(loc_map),
    };

    let input = build_emit_input(
        &config,
        &tree,
        &lockfile,
        &BuildEmitContext {
            manifest: None,
            fixups: Some(&eff),
            abs_third_party_dir: None,
        },
    )
    .expect("build_emit_input succeeds");

    let pkg_a = input.packages.iter().find(|p| p.name == "pkg-a").expect("pkg-a emitted");
    let deps_str = format!("{:?}", pkg_a.deps);
    assert!(deps_str.contains("//c:base"), "expected //c:base in {}", deps_str);
    assert!(deps_str.contains("//l:base"), "expected //l:base in {}", deps_str);
}
```

(Note: the helper function names `make_minimal_config_single_cell` and `make_minimal_lockfile_with_one_wheel` are stand-ins — check what helpers already exist in `src/buck/emit.rs::tests::` and reuse those names. If no such helpers exist, copy the inline construction pattern from `build_emit_input_from_synthetic_resolved` (line 670).)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p muntjac build_emit_input_applies_community_and_local_extra_deps -- --exact`

Expected: FAIL — `BuildEmitContext.fixups` is still `Option<&FixupSet>`; the test passes `&EffectiveFixups` which doesn't typecheck.

- [ ] **Step 3: Switch `BuildEmitContext.fixups` field type**

In `src/buck/emit.rs`, change the field declared in Task 12 Step 2:

Before:
```rust
pub fixups: Option<&'a crate::fixup::FixupSet>,
```

After:
```rust
pub fixups: Option<&'a crate::fixup::EffectiveFixups>,
```

- [ ] **Step 4: Migrate the per-cell resolve (line ~192-205)**

In `src/buck/emit.rs:192-205`, replace the existing `resolved_fixup` lookup block with:

```rust
let resolved_fixup: Option<crate::fixup::ResolvedFixup> = fixups.and_then(|eff| {
    let name = pep508_rs::PackageName::from_str(&pkg.name).ok()?;
    let pkg_ver = PepVersion::from_str(&pkg.version).ok()?;
    let py_ver = PepVersion::from_str(&resolved_cfg.python_version).ok()?;
    let ctx = CfgContext {
        package_version: &pkg_ver,
        python_version: py_ver,
        target_os: &os,
        target_arch: &arch,
        target_env: &env,
    };
    let rf = eff.resolve(&name, &ctx);
    // Preserve the "no fixup" semantics: only Some(rf) when at least one
    // layer had something to contribute. Default ResolvedFixup means
    // "nothing applies", so downstream skips the apply branches.
    if eff.community.get(&name).is_none() && eff.local.get(&name).is_none() {
        None
    } else {
        Some(rf)
    }
});
```

The `is_none() && is_none()` early-return preserves the S6 invariant that downstream apply-blocks are skipped when there's no fixup at all — without it, every package would get an empty-but-present ResolvedFixup, exercising apply code paths unnecessarily. Performance neutral but keeps behavior identical for the no-fixup case.

- [ ] **Step 5: Migrate the first-cell resolve (line ~424-453)**

In `src/buck/emit.rs:424-456`, replace the existing `first_cell_rf` block with:

```rust
let first_cell_rf: Option<crate::fixup::ResolvedFixup> = if let Some(eff) = fixups {
    let pkg_name = pep508_rs::PackageName::from_str(&key.0).ok();
    pkg_name.and_then(|n| {
        // Skip if neither layer has this package.
        if eff.community.get(&n).is_none() && eff.local.get(&n).is_none() {
            return None;
        }
        let first_cfg = &configs[0];
        let s_cfg = first_cfg.as_str();
        let (py_str, plat_name) = match s_cfg.split_once('-') {
            Some((p, r)) => (p.trim_start_matches("py"), r),
            None => ("", ""),
        };
        let py_str_dotted = if py_str.len() >= 2 {
            format!("{}.{}", &py_str[0..1], &py_str[1..])
        } else {
            py_str.to_string()
        };
        let plat = config.platforms.get(plat_name).expect("platform present");
        let (arch, os, env) = crate::fixup::cfg::split_target_triple(&plat.target);
        let pkg_ver = pep440_rs::Version::from_str(&key.1).expect("valid version");
        let py_ver = pep440_rs::Version::from_str(&py_str_dotted).expect("valid python version");
        let ctx = crate::fixup::CfgContext {
            package_version: &pkg_ver,
            python_version: py_ver,
            target_os: &os,
            target_arch: &arch,
            target_env: &env,
        };
        Some(eff.resolve(&n, &ctx))
    })
} else {
    None
};
```

- [ ] **Step 6: Migrate every in-file test call site that constructs `FixupSet` for `fixups`**

For each test (there are ~10 — search `Some(&fixups)` in `src/buck/emit.rs`), wrap the existing `FixupSet` in an `EffectiveFixups`:

Before:
```rust
let fixups = FixupSet::from_map_for_test(fixups_map);
let input = build_emit_input(
    &config,
    &tree,
    &lockfile,
    &BuildEmitContext {
        manifest: None,
        fixups: Some(&fixups),
        abs_third_party_dir: None,
    },
)?;
```

After:
```rust
let fixups = FixupSet::from_map_for_test(fixups_map);
let eff = crate::fixup::EffectiveFixups {
    community: crate::fixup::FixupSet::default(),
    local: fixups,
};
let input = build_emit_input(
    &config,
    &tree,
    &lockfile,
    &BuildEmitContext {
        manifest: None,
        fixups: Some(&eff),
        abs_third_party_dir: None,
    },
)?;
```

(Apply this systematically: tests are local-only as a default migration. The new test from Step 1 already constructs both layers.)

- [ ] **Step 7: Migrate `src/cli/buckify.rs` to construct `EffectiveFixups::load`**

In `src/cli/buckify.rs`, replace the fixup loading block (lines 48-57) with:

```rust
let canonical_third_party_dir = std::fs::canonicalize(&third_party_dir)
    .unwrap_or_else(|_| third_party_dir.clone());

let fixups = crate::fixup::EffectiveFixups::load(
    &config.fixups.registry,
    &third_party_dir,
    config.fixups.allow_local_overrides,
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
    },
)?;
```

- [ ] **Step 8: Compile and run all tests**

Run: `cargo build 2>&1 | tail -10`

Expected: clean build.

Run: `cargo test 2>&1 | grep -E '^test result' | head -20`

Expected: PASS — all existing tests + the new `build_emit_input_applies_community_and_local_extra_deps` test.

If any test fails: most likely a test that constructs `Some(&fixups: &FixupSet)` directly without the wrapper migration in Step 6. Find the call site and apply the wrapper.

- [ ] **Step 9: Commit**

```bash
git add src/buck/emit.rs src/cli/buckify.rs
git commit -m "feat(s7a): build_emit_input consumes EffectiveFixups via context"
```

---

## Phase 8 — CLI updates

### Task 14: `muntjac fixups show <pkg>` — labeled community + local blocks

**Files:**
- Modify: `src/cli/fixups.rs` (replace the `show` function)

- [ ] **Step 1: Write the failing smoke tests for layered output**

In `tests/fixups_show_smoke.rs` (existing), add three test cases:

```rust
#[test]
fn fixups_show_community_only() {
    let tmp = tempfile::TempDir::new().unwrap();
    let cwd = tmp.path();

    // muntjac.toml with file:// registry
    let registry_dir = cwd.join("registry");
    std::fs::create_dir_all(registry_dir.join("packages/pillow")).unwrap();
    std::fs::write(
        registry_dir.join("packages/pillow/fixups.toml"),
        "extra_deps = [\"//c:libjpeg\"]\n",
    )
    .unwrap();

    std::fs::create_dir_all(cwd.join("third-party/python")).unwrap();
    let muntjac_toml = format!(
        r#"
manifest_path   = "pyproject.toml"
third_party_dir = "third-party/python"
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target    = "x86_64-unknown-linux-gnu"
manylinux = "2_17"

[fixups]
registry = "file://{}"
"#,
        registry_dir.canonicalize().unwrap().display()
    );
    std::fs::write(cwd.join("muntjac.toml"), muntjac_toml).unwrap();

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_muntjac"))
        .args(["-C", cwd.to_str().unwrap(), "fixups", "show", "pillow"])
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("# community:"), "no community header: {}", stdout);
    assert!(stdout.contains("//c:libjpeg"));
    assert!(!stdout.contains("# local:"), "should not have local block: {}", stdout);
}

#[test]
fn fixups_show_local_only() {
    let tmp = tempfile::TempDir::new().unwrap();
    let cwd = tmp.path();
    let tpd = cwd.join("third-party/python");
    std::fs::create_dir_all(tpd.join("fixups/pillow")).unwrap();
    std::fs::write(
        tpd.join("fixups/pillow/fixups.toml"),
        "extra_deps = [\"//local:shim\"]\n",
    )
    .unwrap();
    std::fs::write(
        cwd.join("muntjac.toml"),
        r#"
manifest_path   = "pyproject.toml"
third_party_dir = "third-party/python"
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target    = "x86_64-unknown-linux-gnu"
manylinux = "2_17"

[fixups]
registry = "none"
"#,
    )
    .unwrap();

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_muntjac"))
        .args(["-C", cwd.to_str().unwrap(), "fixups", "show", "pillow"])
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("//local:shim"));
    assert!(!stdout.contains("# community:"), "should not have community block: {}", stdout);
}

#[test]
fn fixups_show_both_layers() {
    let tmp = tempfile::TempDir::new().unwrap();
    let cwd = tmp.path();

    let registry_dir = cwd.join("registry");
    std::fs::create_dir_all(registry_dir.join("packages/pillow")).unwrap();
    std::fs::write(
        registry_dir.join("packages/pillow/fixups.toml"),
        "extra_deps = [\"//c:libjpeg\"]\n",
    )
    .unwrap();
    let tpd = cwd.join("third-party/python");
    std::fs::create_dir_all(tpd.join("fixups/pillow")).unwrap();
    std::fs::write(
        tpd.join("fixups/pillow/fixups.toml"),
        "extra_deps = [\"//local:shim\"]\n",
    )
    .unwrap();

    let muntjac_toml = format!(
        r#"
manifest_path   = "pyproject.toml"
third_party_dir = "third-party/python"
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target    = "x86_64-unknown-linux-gnu"
manylinux = "2_17"

[fixups]
registry = "file://{}"
"#,
        registry_dir.canonicalize().unwrap().display()
    );
    std::fs::write(cwd.join("muntjac.toml"), muntjac_toml).unwrap();

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_muntjac"))
        .args(["-C", cwd.to_str().unwrap(), "fixups", "show", "pillow"])
        .output()
        .unwrap();
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("# community:"));
    assert!(stdout.contains("//c:libjpeg"));
    assert!(stdout.contains("# local:"));
    assert!(stdout.contains("//local:shim"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test fixups_show_smoke fixups_show_community_only -- --exact`

Expected: FAIL — current `show` only reads from `load_local`.

- [ ] **Step 3: Rewrite the `show` function**

Replace `src/cli/fixups.rs::show` with:

```rust
fn show(package: String, globals: &Globals) -> Result<()> {
    let cwd = globals.workdir().context("resolving working directory")?;
    let cfg_path = cwd.join("muntjac.toml");
    let cfg_bytes = std::fs::read_to_string(&cfg_path)
        .with_context(|| format!("reading {}", cfg_path.display()))?;
    let config =
        Config::from_str(&cfg_bytes).with_context(|| format!("parsing {}", cfg_path.display()))?;

    let tree = config
        .trees
        .first()
        .ok_or_else(|| anyhow!("no trees in muntjac.toml"))?;
    let third_party_dir = cwd.join(&tree.third_party_dir);

    let pkg_name = PackageName::from_str(&package)
        .with_context(|| format!("normalizing package name `{}`", package))?;

    let eff = fixup::EffectiveFixups::load(
        &config.fixups.registry,
        &third_party_dir,
        config.fixups.allow_local_overrides,
    )
    .with_context(|| format!("loading layered fixups for tree '{}'", tree.name))?;

    let community_cfg = eff.community.get(&pkg_name);
    let local_cfg = eff.local.get(&pkg_name);

    if community_cfg.is_none() && local_cfg.is_none() {
        // Reconstruct paths for the diagnostic.
        let community_path = match &config.fixups.registry {
            fixup::RegistryConfig::None => "(none)".to_string(),
            fixup::RegistryConfig::FileUrl(p) => p.join("packages").display().to_string(),
            fixup::RegistryConfig::Git { url, .. } => format!("git: {}", url),
        };
        let local_path = third_party_dir.join("fixups").display().to_string();
        anyhow::bail!(
            "no fixup for package '{}' (checked community at {}, local at {})",
            package,
            community_path,
            local_path,
        );
    }

    // Print labeled blocks.
    let both_present = community_cfg.is_some() && local_cfg.is_some();

    if let Some(c) = community_cfg {
        if both_present {
            // Source path for the header.
            if let fixup::RegistryConfig::FileUrl(p) = &config.fixups.registry {
                let community_file = p.join("packages").join(package.to_lowercase()).join("fixups.toml");
                println!("# community: {}", community_file.display());
            } else {
                println!("# community:");
            }
        }
        print!("{}", c.to_toml_string().context("re-emitting community fixup as TOML")?);
        if both_present {
            println!();
            if local_cfg.map_or(false, |l| l.replace_community) {
                println!("# (community fixup above is disabled by replace_community = true)");
            }
        }
    }

    if let Some(l) = local_cfg {
        if both_present {
            let local_file = third_party_dir
                .join("fixups")
                .join(package.to_lowercase())
                .join("fixups.toml");
            println!("# local: {}", local_file.display());
        }
        print!("{}", l.to_toml_string().context("re-emitting local fixup as TOML")?);
    }

    Ok(())
}
```

- [ ] **Step 4: Update imports in `src/cli/fixups.rs`**

The function now references `fixup::EffectiveFixups`, `fixup::RegistryConfig`. Confirm the existing `use crate::fixup;` import covers them (it does — the module re-exports both).

- [ ] **Step 5: Build and test**

Run: `cargo build 2>&1 | tail -5`

Expected: clean.

Run: `cargo test --test fixups_show_smoke`

Expected: PASS — the 3 new tests + the 1 from S6 (which should still pass; it's a local-only no-registry case).

- [ ] **Step 6: Commit**

```bash
git add src/cli/fixups.rs tests/fixups_show_smoke.rs
git commit -m "feat(s7a): fixups show prints layered community + local blocks"
```

---

### Task 15: `muntjac init` template hint comment

**Files:**
- Modify: `src/cli/init.rs` (find the starter `[fixups]` block)

- [ ] **Step 1: Find the current init template**

Run: `grep -n -A6 '\[fixups\]' src/cli/init.rs`

Expected: shows current starter `[fixups]` block.

- [ ] **Step 2: Update the template**

In `src/cli/init.rs`, change the `[fixups]` block in the starter template string to:

```toml
[fixups]
registry = "none"
# When a community registry exists, set to "github.com/<owner>/muntjac-fixups"
# and run `muntjac fixups update` to pin a SHA. For air-gapped or
# pre-launch usage, use a local checkout: registry = "file:///abs/path".
allow_local_overrides = true
```

- [ ] **Step 3: Update any existing init template snapshot test**

Run: `grep -rn 'allow_local_overrides\|registry = "none"' tests/init.rs src/cli/init.rs`

If there's a test asserting the exact starter file content (likely in `tests/init.rs`), update its expected text to include the new comment.

- [ ] **Step 4: Run init tests**

Run: `cargo test --test init`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/cli/init.rs tests/init.rs
git commit -m "docs(s7a): muntjac init template hints at registry usage"
```

---

## Phase 9 — Fixtures

### Task 16: Fixture 06-community-fixup inputs

**Files:**
- Create: `tests/fixtures/buck/06-community-fixup/muntjac.toml`
- Create: `tests/fixtures/buck/06-community-fixup/pyproject.toml`
- Create: `tests/fixtures/buck/06-community-fixup/uv.lock`
- Create: `tests/fixtures/buck/06-community-fixup/registry/packages/pkg-a/fixups.toml`
- Create: `tests/fixtures/buck/06-community-fixup/registry/packages/pkg-b/fixups.toml`
- Create: `tests/fixtures/buck/06-community-fixup/third-party/python/fixups/pkg-a/fixups.toml`
- Create: `tests/fixtures/buck/06-community-fixup/third-party/python/fixups/pkg-c/fixups.toml`
- Create: `tests/fixtures/buck/06-community-fixup/third-party/python/prebake/.manifest.toml`
- Create: `tests/fixtures/buck/06-community-fixup/.gitignore`

This fixture exercises three synthetic packages — `pkg-a` (both layers + cfg sections), `pkg-b` (community-only), `pkg-c` (local-only).

- [ ] **Step 1: Examine fixture 05 for the synthetic-package pattern**

Run: `ls tests/fixtures/buck/05-local-fixup/ && cat tests/fixtures/buck/05-local-fixup/muntjac.toml`

Expected: shows the working pattern for muntjac.toml + lockfile + prebake stubs. Reuse this shape.

- [ ] **Step 2: Create the muntjac.toml**

The `registry` must resolve to an absolute path at test time. Since the fixture path is checked into source control, we can't bake an absolute path in. **The buckify test code computes the absolute path and writes a per-test muntjac.toml into a tempdir copy.** Bake `registry = "file://./registry"` in the source file — it will be rewritten at test setup.

Create `tests/fixtures/buck/06-community-fixup/muntjac.toml`:

```toml
manifest_path   = "pyproject.toml"
third_party_dir = "third-party/python"
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target    = "x86_64-unknown-linux-gnu"
manylinux = "2_17"

[fixups]
registry = "file:///REPLACED_AT_TEST_TIME"
allow_local_overrides = true
```

- [ ] **Step 3: Create the pyproject.toml + uv.lock**

Mirror fixture 05's layout. `pyproject.toml`:

```toml
[project]
name = "fixture-06-consumer"
version = "0.0.1"
requires-python = ">=3.12,<3.13"
dependencies = [
    "pkg-a==1.0.0",
    "pkg-b==1.0.0",
    "pkg-c==1.0.0",
]
```

`uv.lock`: copy the synthetic-package skeleton from fixture 05, replacing the package list with `pkg-a`, `pkg-b`, `pkg-c`. Each entry needs a sdist or wheel — use prebake stubs (see step 5).

- [ ] **Step 4: Write the community fixup files**

`tests/fixtures/buck/06-community-fixup/registry/packages/pkg-a/fixups.toml`:

```toml
extra_deps = ["//community:base"]

["cfg(target_os = \"linux\")"]
extra_deps = ["//community:linux-only"]
```

`tests/fixtures/buck/06-community-fixup/registry/packages/pkg-b/fixups.toml`:

```toml
extra_deps = ["//community:b-base"]
visibility = ["//community/visibility:..."]
labels = ["community-tag"]
```

- [ ] **Step 5: Write the local fixup files**

`tests/fixtures/buck/06-community-fixup/third-party/python/fixups/pkg-a/fixups.toml`:

```toml
extra_deps = ["//local:base"]

["cfg(target_arch = \"x86_64\")"]
extra_deps = ["//local:x86-only"]
```

`tests/fixtures/buck/06-community-fixup/third-party/python/fixups/pkg-c/fixups.toml`:

```toml
extra_deps = ["//local:c-base"]
entry_points = ["pkg-c"]
```

- [ ] **Step 6: Create the prebake manifest (synthetic stubs)**

Mirror fixture 05's `prebake/.manifest.toml` + stubs. Each synthetic package gets a 1-line wheel stub. Copy patterns from `tests/fixtures/buck/05-local-fixup/third-party/python/prebake/`.

- [ ] **Step 7: Create the .gitignore**

`tests/fixtures/buck/06-community-fixup/.gitignore`:

```
# muntjac-generated files (reproduced by `cargo run -- buckify`).
# The `registry/` and committed `fixups/` subtrees ARE committed.
/third-party/python/BUCK
/third-party/python/muntjac.bzl
/third-party/python/wiring.bzl
/third-party/python/config/

# buck2 build outputs (local + CI)
buck-out/
.buckd/

# uv's venv if developer creates one
.venv/
```

- [ ] **Step 8: Commit the fixture inputs (no expected/ yet)**

```bash
git add tests/fixtures/buck/06-community-fixup/
git commit -m "test(s7a): fixture 06-community-fixup inputs"
```

---

### Task 17: Snapshot test for fixture 06 + generate expected/

**Files:**
- Modify: `tests/buckify.rs` (add `fixture_06_community_fixup_golden`)
- Create: `tests/fixtures/buck/06-community-fixup/expected/BUCK`
- Create: `tests/fixtures/buck/06-community-fixup/expected/muntjac.bzl`
- Create: `tests/fixtures/buck/06-community-fixup/expected/wiring.bzl`

- [ ] **Step 1: Find the existing fixture-05 snapshot test as a template**

Run: `grep -n -A40 'fn fixture_05_local_fixup_golden\|fn fixture_04_pure_python_sdist_golden' tests/buckify.rs`

Expected: shows the existing pattern: copy fixture to tempdir, rewrite muntjac.toml absolute paths, run buckify, compare against `expected/`.

- [ ] **Step 2: Write the failing test**

Add to `tests/buckify.rs`:

```rust
#[test]
fn fixture_06_community_fixup_golden() {
    let fixture_src = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/buck/06-community-fixup");
    let tmp = tempfile::TempDir::new().unwrap();
    copy_fixture(&fixture_src, tmp.path());

    // Rewrite the file:// registry path in muntjac.toml to absolute.
    let abs_registry = tmp.path().join("registry");
    let muntjac_toml_path = tmp.path().join("muntjac.toml");
    let mut toml_bytes = std::fs::read_to_string(&muntjac_toml_path).unwrap();
    toml_bytes = toml_bytes.replace(
        "file:///REPLACED_AT_TEST_TIME",
        &format!("file://{}", abs_registry.display()),
    );
    std::fs::write(&muntjac_toml_path, toml_bytes).unwrap();

    let status = std::process::Command::new(env!("CARGO_BIN_EXE_muntjac"))
        .args(["-C", tmp.path().to_str().unwrap(), "buckify"])
        .status()
        .unwrap();
    assert!(status.success());

    let generated_buck = std::fs::read_to_string(tmp.path().join("third-party/python/BUCK")).unwrap();
    let expected_buck =
        std::fs::read_to_string(fixture_src.join("expected/BUCK")).unwrap();
    assert_eq!(generated_buck, expected_buck, "BUCK byte-diff");

    let generated_bzl =
        std::fs::read_to_string(tmp.path().join("third-party/python/muntjac.bzl")).unwrap();
    let expected_bzl =
        std::fs::read_to_string(fixture_src.join("expected/muntjac.bzl")).unwrap();
    assert_eq!(generated_bzl, expected_bzl, "muntjac.bzl byte-diff");

    // Sanity assertions: pkg-a must have both community and local deps;
    // pkg-b must have community-only; pkg-c must have local-only.
    assert!(generated_buck.contains("//community:base"), "missing community base in BUCK");
    assert!(generated_buck.contains("//local:base"), "missing local base in BUCK");
    assert!(generated_buck.contains("//community:b-base"), "missing pkg-b community");
    assert!(generated_buck.contains("//local:c-base"), "missing pkg-c local");
}
```

- [ ] **Step 3: Run the test to verify it fails (no expected/ yet)**

Run: `cargo test --test buckify fixture_06_community_fixup_golden -- --exact 2>&1 | tail -20`

Expected: FAIL — `expected/BUCK` doesn't exist.

- [ ] **Step 4: Generate expected/ by running buckify against a tempdir copy**

```bash
cp -r tests/fixtures/buck/06-community-fixup /tmp/06-fixture-stage
ABS_REG=/tmp/06-fixture-stage/registry
sed -i "s|file:///REPLACED_AT_TEST_TIME|file://${ABS_REG}|" /tmp/06-fixture-stage/muntjac.toml
cargo run -- -C /tmp/06-fixture-stage buckify
mkdir -p tests/fixtures/buck/06-community-fixup/expected
cp /tmp/06-fixture-stage/third-party/python/BUCK \
   /tmp/06-fixture-stage/third-party/python/muntjac.bzl \
   /tmp/06-fixture-stage/third-party/python/wiring.bzl \
   tests/fixtures/buck/06-community-fixup/expected/
```

- [ ] **Step 5: Inspect the generated files**

Run: `head -40 tests/fixtures/buck/06-community-fixup/expected/BUCK`

Verify: pkg-a deps include both `//community:base` AND `//local:base`; pkg-a on linux includes `//community:linux-only`; pkg-a on x86_64 includes `//local:x86-only`; pkg-b shows community-only fields; pkg-c shows local-only fields.

If anything looks wrong, the layering algorithm has a bug — go back to fix it before committing `expected/`.

- [ ] **Step 6: Re-run the test to verify it passes**

Run: `cargo test --test buckify fixture_06_community_fixup_golden -- --exact 2>&1 | tail -10`

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add tests/fixtures/buck/06-community-fixup/expected/ tests/buckify.rs
git commit -m "test(s7a): fixture 06-community-fixup byte-exact snapshot"
```

---

### Task 18: Fixture 07-allow-local-overrides-false

**Files:**
- Create: `tests/fixtures/buck/07-allow-local-overrides-false/` (full fixture tree)
- Modify: `tests/buckify.rs` (add `fixture_07_allow_local_overrides_false_golden`)

- [ ] **Step 1: Create the fixture tree**

Copy fixture 06 as a starting point:

```bash
cp -r tests/fixtures/buck/06-community-fixup tests/fixtures/buck/07-allow-local-overrides-false
```

Then trim:
- Delete `tests/fixtures/buck/07-allow-local-overrides-false/expected/` (will regenerate).
- Delete `tests/fixtures/buck/07-allow-local-overrides-false/third-party/python/fixups/pkg-c/` (we only need pkg-a to demonstrate).
- Delete `pkg-b`, `pkg-c` from `registry/packages/`.
- Update `pyproject.toml` and `uv.lock` to list only `pkg-a==1.0.0`.

The two layers should have *conflicting* visibility on `pkg-a`:

`registry/packages/pkg-a/fixups.toml`:
```toml
extra_deps = ["//community:base"]
visibility = ["//community:..."]
```

`third-party/python/fixups/pkg-a/fixups.toml`:
```toml
extra_deps = ["//local:base"]
visibility = ["//local:..."]
```

Update `muntjac.toml` to set `allow_local_overrides = false`:

```toml
[fixups]
registry = "file:///REPLACED_AT_TEST_TIME"
allow_local_overrides = false
```

- [ ] **Step 2: Generate expected/**

```bash
cp -r tests/fixtures/buck/07-allow-local-overrides-false /tmp/07-fixture-stage
ABS_REG=/tmp/07-fixture-stage/registry
sed -i "s|file:///REPLACED_AT_TEST_TIME|file://${ABS_REG}|" /tmp/07-fixture-stage/muntjac.toml
cargo run -- -C /tmp/07-fixture-stage buckify
mkdir -p tests/fixtures/buck/07-allow-local-overrides-false/expected
cp /tmp/07-fixture-stage/third-party/python/BUCK \
   /tmp/07-fixture-stage/third-party/python/muntjac.bzl \
   /tmp/07-fixture-stage/third-party/python/wiring.bzl \
   tests/fixtures/buck/07-allow-local-overrides-false/expected/
```

- [ ] **Step 3: Verify the generated output reflects `allow_local_overrides = false`**

Run: `grep -E 'community|local' tests/fixtures/buck/07-allow-local-overrides-false/expected/BUCK`

Expected: only `//community:base` and `//community:...` appear in pkg-a's deps/visibility. No `//local:` strings should be present.

If `//local:` appears, `allow_local_overrides = false` was not honored — investigate `EffectiveFixups::load` wiring.

- [ ] **Step 4: Write the snapshot test**

Add to `tests/buckify.rs`:

```rust
#[test]
fn fixture_07_allow_local_overrides_false_golden() {
    let fixture_src = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/buck/07-allow-local-overrides-false");
    let tmp = tempfile::TempDir::new().unwrap();
    copy_fixture(&fixture_src, tmp.path());

    let abs_registry = tmp.path().join("registry");
    let muntjac_toml_path = tmp.path().join("muntjac.toml");
    let mut toml_bytes = std::fs::read_to_string(&muntjac_toml_path).unwrap();
    toml_bytes = toml_bytes.replace(
        "file:///REPLACED_AT_TEST_TIME",
        &format!("file://{}", abs_registry.display()),
    );
    std::fs::write(&muntjac_toml_path, toml_bytes).unwrap();

    let status = std::process::Command::new(env!("CARGO_BIN_EXE_muntjac"))
        .args(["-C", tmp.path().to_str().unwrap(), "buckify"])
        .status()
        .unwrap();
    assert!(status.success());

    let generated_buck =
        std::fs::read_to_string(tmp.path().join("third-party/python/BUCK")).unwrap();
    let expected_buck =
        std::fs::read_to_string(fixture_src.join("expected/BUCK")).unwrap();
    assert_eq!(generated_buck, expected_buck);

    // Sanity: local visibility was suppressed.
    assert!(!generated_buck.contains("//local:..."), "local visibility leaked: {}", generated_buck);
    assert!(generated_buck.contains("//community:..."), "community visibility missing");
}
```

- [ ] **Step 5: Run the test**

Run: `cargo test --test buckify fixture_07_allow_local_overrides_false_golden -- --exact`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add tests/fixtures/buck/07-allow-local-overrides-false/ tests/buckify.rs
git commit -m "test(s7a): fixture 07-allow-local-overrides-false byte-exact snapshot"
```

---

### Task 19: Fixture 08-replace-community

**Files:**
- Create: `tests/fixtures/buck/08-replace-community/` (full fixture tree)
- Modify: `tests/buckify.rs` (add `fixture_08_replace_community_golden`)

- [ ] **Step 1: Create the fixture tree**

Same skeleton as fixture 07 (single package `pkg-a`, registry + local fixups). The local fixup sets `replace_community = true` and a narrow `extra_deps`:

`registry/packages/pkg-a/fixups.toml`:
```toml
extra_deps = ["//community:base", "//community:linux-only"]
overlay = "_unused_path"
visibility = ["//community:..."]
labels = ["community-tag"]
```

`third-party/python/fixups/pkg-a/fixups.toml`:
```toml
replace_community = true
extra_deps = ["//local:only"]
```

`muntjac.toml` registers `allow_local_overrides = true` (default) and `registry = "file://./registry"`.

- [ ] **Step 2: Generate expected/**

```bash
cp -r tests/fixtures/buck/08-replace-community /tmp/08-fixture-stage
ABS_REG=/tmp/08-fixture-stage/registry
sed -i "s|file:///REPLACED_AT_TEST_TIME|file://${ABS_REG}|" /tmp/08-fixture-stage/muntjac.toml
cargo run -- -C /tmp/08-fixture-stage buckify
mkdir -p tests/fixtures/buck/08-replace-community/expected
cp /tmp/08-fixture-stage/third-party/python/BUCK \
   /tmp/08-fixture-stage/third-party/python/muntjac.bzl \
   /tmp/08-fixture-stage/third-party/python/wiring.bzl \
   tests/fixtures/buck/08-replace-community/expected/
```

- [ ] **Step 3: Verify community is fully suppressed**

Run: `grep -E 'community|local' tests/fixtures/buck/08-replace-community/expected/BUCK`

Expected: `//local:only` is the only fixup-derived dep on pkg-a. No `//community:*`, no `community-tag`, no `community:...` visibility.

- [ ] **Step 4: Write the snapshot test**

Add to `tests/buckify.rs`:

```rust
#[test]
fn fixture_08_replace_community_golden() {
    let fixture_src = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/buck/08-replace-community");
    let tmp = tempfile::TempDir::new().unwrap();
    copy_fixture(&fixture_src, tmp.path());

    let abs_registry = tmp.path().join("registry");
    let muntjac_toml_path = tmp.path().join("muntjac.toml");
    let mut toml_bytes = std::fs::read_to_string(&muntjac_toml_path).unwrap();
    toml_bytes = toml_bytes.replace(
        "file:///REPLACED_AT_TEST_TIME",
        &format!("file://{}", abs_registry.display()),
    );
    std::fs::write(&muntjac_toml_path, toml_bytes).unwrap();

    let status = std::process::Command::new(env!("CARGO_BIN_EXE_muntjac"))
        .args(["-C", tmp.path().to_str().unwrap(), "buckify"])
        .status()
        .unwrap();
    assert!(status.success());

    let generated_buck =
        std::fs::read_to_string(tmp.path().join("third-party/python/BUCK")).unwrap();
    let expected_buck =
        std::fs::read_to_string(fixture_src.join("expected/BUCK")).unwrap();
    assert_eq!(generated_buck, expected_buck);

    // Community must be fully suppressed.
    assert!(generated_buck.contains("//local:only"));
    assert!(!generated_buck.contains("//community:"), "community fields leaked: {}", generated_buck);
}
```

- [ ] **Step 5: Run the test**

Run: `cargo test --test buckify fixture_08_replace_community_golden -- --exact`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add tests/fixtures/buck/08-replace-community/ tests/buckify.rs
git commit -m "test(s7a): fixture 08-replace-community byte-exact snapshot"
```

---

## Phase 10 — Cleanup, formatting, tag

### Task 20: Final cargo test sweep + cargo fmt + cargo clippy

- [ ] **Step 1: Run the full test suite**

Run: `cargo test 2>&1 | grep -E '^test result|FAILED' | tail -30`

Expected: all PASS.

- [ ] **Step 2: Run cargo fmt + clippy**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings 2>&1 | tail -20`

Expected: clean (the pre-commit hook from S5 will block commits that aren't, so this is a sanity check).

If clippy fires: fix in place (these are usually small).

- [ ] **Step 3: If fmt/clippy needed fixes, commit them**

```bash
git add -u
git commit -m "style(s7a): cargo fmt + clippy cleanup"
```

(Skip if no changes.)

- [ ] **Step 4: Update TECH_DEBT to mark TD-S6-04 resolved**

In `docs/superpowers/TECH_DEBT.md`, find the `TD-S6-04` entry under `## Open` and move it to `## Resolved`:

```markdown
### TD-S6-04: `build_emit_input` has 6 positional parameters
- **Resolved:** S7a, commit `<sha>` (use `git log --oneline | head -1` after Task 12 lands)
- **Summary:** Introduced `BuildEmitContext<'a>` bundling `manifest`, `fixups`,
  `abs_third_party_dir`. `build_emit_input` is now 3 positional + 1 context.
  All ~10 in-file test call sites and the `cli/buckify.rs` call site migrated
  in the same stage. S7b will add a cache directory to the context without
  positional-arg churn.
```

- [ ] **Step 5: Commit TECH_DEBT update**

```bash
git add docs/superpowers/TECH_DEBT.md
git commit -m "docs(s7a): TECH_DEBT — move TD-S6-04 to Resolved"
```

- [ ] **Step 6: Update the roadmap with the actual shipped commit count**

After everything is committed, count commits in S7a:

```bash
git log --oneline s6-complete..HEAD | wc -l
cargo test 2>&1 | grep -E '^test result' | awk '{sum+=$4} END {print sum}'
```

Update `docs/superpowers/specs/2026-05-20-muntjac-roadmap.md` table row for S7a:

```markdown
| S7a | [2026-05-24-muntjac-s7a-community-layering-design.md](./2026-05-24-muntjac-s7a-community-layering-design.md) | [2026-05-24-muntjac-s7a-community-layering.md](../plans/2026-05-24-muntjac-s7a-community-layering.md) | ✅ shipped (tag `s7a-complete`, N commits, N tests) |
```

And update the roadmap S7a section heading line ~133 with the shipped status (mirror the S6 heading at line 115).

- [ ] **Step 7: Commit and tag**

```bash
git add docs/superpowers/specs/2026-05-20-muntjac-roadmap.md
git commit -m "docs(s7a): mark S7a ✅ shipped in roadmap"
git tag s7a-complete
```

---

## Self-review

**Spec coverage check (run against `docs/superpowers/specs/2026-05-24-muntjac-s7a-community-layering-design.md`):**

| Spec section | Covered by |
|---|---|
| §1.1 In scope: RegistryConfig typed enum | Task 4, Task 5 |
| §1.1 FileUrl loader | Task 8 |
| §1.1 None mode | Task 5 (default), Task 11 |
| §1.1 Git mode declared/errors | Task 4 (enum), Task 11 (error path) |
| §1.1 replace_community on local drops community | Task 10 |
| §1.1 replace_community on community rejected | Task 8 |
| §1.1 allow_local_overrides = false | Task 11 |
| §1.1 Per-layer resolve_for_cell reused unchanged | Task 10 (uses, doesn't modify) |
| §1.1 Cross-layer merge_resolved | Task 9 |
| §1.1 EffectiveFixups facade with .resolve | Task 10 |
| §1.1 BuildEmitContext refactor | Task 12, Task 13 |
| §1.1 fixups show layered output | Task 14 |
| §1.1 Three new fixtures | Tasks 16-19 |
| §1.2 registry_rev stderr warning | Task 5 Step 4 (warning emit), Task 6 (tests) |
| §1.2 RegistryConfig::Git errors with GitRegistryNotImplemented | Task 4, Task 11 |
| §3.5 BuildEmitContext signature | Task 12 |
| §4 Algorithm + per-field merge rules | Tasks 9, 10 |
| §5 Loader layout + load_community + ReplaceCommunityInCommunity | Tasks 7, 8 |
| §6 CLI fixups show + init template | Tasks 14, 15 |
| §7 Error variants locked byte-for-byte | Task 1, Task 2 |
| §8 Testing strategy counts | Distributed across all tasks |
| §10 TD-S6-04 resolution | Tasks 12, 13, 20 (TECH_DEBT update) |
| §10 Roadmap row fix | (done in spec commit `45f1989`); S7a row updated in Task 20 |

**Placeholder scan:** No "TBD"/"TODO"/"fill in"/"similar to" patterns; every code step has concrete code.

**Type consistency:**
- `RegistryConfig` defined Task 4, used Tasks 5/11/14.
- `EffectiveFixups` defined Task 10, used Tasks 11/13/14.
- `BuildEmitContext` defined Task 12, used Tasks 13/14.
- `merge_resolved` signature defined Task 9, used Task 10.
- `load_community` signature defined Task 8, used Task 11.
- `parse_registry_config` signature defined Task 4, used Task 5.

All cross-references resolve.

---

## Execution

Plan complete and saved to `docs/superpowers/plans/2026-05-24-muntjac-s7a-community-layering.md`.

Per the established cadence ([[feedback_planning_cadence]]), execute via `superpowers:subagent-driven-development` — fresh subagent per task, two-stage review between tasks.
