# Muntjac S7a — Community Fixup Layering (Design)

> **Stage:** S7a (sub-stage of S7 per `2026-05-20-muntjac-roadmap.md`).
> **Status:** Design.
> **Prerequisite reading:** `2026-05-20-muntjac-design.md` §7 (fixup schema & the moat);
> `2026-05-23-muntjac-s6-local-fixups-design.md` (local-only fixup foundation).

---

## 1. Scope & non-goals

S7 (community fixup registry) is decomposed into two sub-stages.

**S7a — this spec.** Community fixups layer on top of local fixups via two
no-network registry modes: `registry = "none"` (default, community layer
empty) and `registry = "file://<abs>"` (community layer loaded from a
local directory checkout). The two-pass layering algorithm (community
fully resolved → local fully resolved → merge with local-wins-on-scalar,
community-first-on-list) is the substance. Three synthetic fixtures
exercise community-only / local-only / both, the `replace_community = true`
escape hatch, and `allow_local_overrides = false`.

**S7b — separate spec, follows.** Git fetch via `gix`; content-addressed
cache at `~/.cache/muntjac/fixups/<sha>/`; `muntjac fixups update`
command with structured diff output; `--offline` integration test.
Effectively: the network-and-cache layer that turns `registry =
"github.com/<owner>/<repo>"` into a usable mode.

The split is a capability boundary: S7a delivers "layered fixups work
if you supply the registry as a checkout"; S7b delivers "muntjac
fetches and pins the registry for you." S8 (launch polish) does not
shift — it now blocks on S7b instead of S7.

### 1.1 In scope for S7a

| Concern | Resolution |
|---|---|
| `RegistryConfig` typed enum (`None`, `FileUrl`, `Git`) | parsed at `Config::from_raw` time; replaces `FixupRegistry(String)` |
| `FileUrl` loader | `load_community(packages_dir)` walks `packages/<pkg>/fixups.toml` |
| `None` mode | `EffectiveFixups.community` is empty; no I/O |
| `Git` mode | declared in the enum; `EffectiveFixups::load` errors with `GitRegistryNotImplemented` |
| `replace_community = true` on local fixups | drops community layer for that package before merging |
| `replace_community = true` on community fixups | rejected at load with `ReplaceCommunityInCommunity` |
| `allow_local_overrides = false` | local layer not loaded; community is sole source |
| Per-layer `resolve_for_cell` reused unchanged from S6 | yes |
| Cross-layer `merge_resolved(community, local)` | new; field-by-field rules in §4 |
| `EffectiveFixups` facade with `.resolve()` method | hides layering from emitter |
| `BuildEmitContext<'a>` (TD-S6-04) | refactors `build_emit_input` from 6 positional → 3 + ctx |
| `muntjac fixups show <pkg>` layered output | two labeled TOML blocks (community / local) |
| Three new fixtures (`06`, `07`, `08`) | snapshot-only, synthetic packages |

### 1.2 Schema-only (parses but warns / future)

| Field / config | Status in S7a |
|---|---|
| `registry_rev` in `[fixups]` | parsed; stderr warning when `registry` is `none` or `file://` (S7b uses it) |
| `RegistryConfig::Git` | parses but `EffectiveFixups::load` errors with `GitRegistryNotImplemented` (S7b implements) |

### 1.3 Out of scope (deferred to S7b)

- `gix` dependency, git fetch logic.
- `~/.cache/muntjac/fixups/<sha>/` content-addressed cache.
- `muntjac fixups update` command.
- Structured diff output.
- `--offline` flag and air-gap integration test.
- Symlink-traversal hardening in `extract_tarball` (S5 TD): re-targeted to S7b because S7a doesn't introduce any tarball-extraction code — gix isn't introduced, and `load_community` walks a directory via `std::fs::read_dir`.

### 1.4 Out of scope (post-launch / deferred indefinitely)

- `--layer community|local` flag for `fixups show` — single command works for the moat-demo.
- `--explain` / per-cell `muntjac debug resolve-fixup` — useful for power users; not on the launch path.
- Splitting fixture 05 (TD-S6-05) — per-field unit-test coverage already exists in `src/buck/emit.rs::tests::`; the snapshot fixture for layering goes in fixtures 06/07/08, not 05.
- `entry_points = true` auto-discovery (TD-S6-02), non-`__main__` entry points (TD-S6-03), RECORD regeneration (TD-S6-01) — all post-launch.

---

## 2. Module layout & pipeline integration

### 2.1 Module layout delta

```
src/fixup/
├── mod.rs                  # re-exports +EffectiveFixups, +RegistryConfig
├── schema.rs               # FixupConfig gains `replace_community: bool` (lifted from
│                           #   FixupBody where S6 stub-declared it; semantics deferred to S7a)
├── loader.rs               # +pub fn load_community(packages_dir)
│                           # +pub(crate) fn load_one_fixup(toml_path) (factored)
├── layer.rs                # +EffectiveFixups { community, local }
│                           # +impl EffectiveFixups::{load, resolve}
│                           # +pub fn merge_resolved(community, local) -> ResolvedFixup
└── registry.rs             # NEW. RegistryConfig enum + load_registry() dispatcher

src/config.rs               # FixupRegistry(String) → RegistryConfig enum, parsed
                            #   at Config::from_raw; existing validate_registry
                            #   becomes the enum constructor.
                            # FixupsConfig.registry_rev: parsed-and-warned when
                            #   registry is not Git.

src/buck/emit.rs            # build_emit_input(config, tree, lockfile, &BuildEmitContext)
                            # — 6 positional → 3 positional + 1 ctx struct (TD-S6-04)

src/cli/buckify.rs          # call-site update for BuildEmitContext;
                            #   constructs EffectiveFixups::load(...)
src/cli/fixups.rs           # show prints community+local blocks (header comments)
src/cli/init.rs             # starter muntjac.toml gains hint comment under [fixups]

tests/fixtures/buck/
├── 06-community-fixup/     # NEW. Layering: list union, scalar override, cfg sections
│   ├── muntjac.toml        #   registry = "file://./registry" (root-resolved to abs)
│   ├── uv.lock
│   ├── pyproject.toml
│   ├── registry/packages/{pkg-a,pkg-b}/fixups.toml   # community layer
│   ├── third-party/python/fixups/{pkg-a,pkg-c}/fixups.toml  # local layer
│   ├── third-party/python/prebake/                   # sdist source for synthetic pkgs
│   ├── expected/
│   │   ├── BUCK
│   │   └── muntjac.bzl
│   └── .gitignore
├── 07-allow-local-overrides-false/     # NEW. Toggle: local layer suppressed
└── 08-replace-community/               # NEW. Per-package community drop

src/fixup/error.rs          # +ReplaceCommunityInCommunity, +GitRegistryNotImplemented,
                            #   +RegistryPathNotFound, +RegistryPathNotAbsolute
```

### 2.2 Pipeline (extends S5/S6's manifest+fixups threading)

```
cli/buckify.rs:
  1. parse muntjac.toml → Config (RegistryConfig now typed)
  2. parse uv.lock → Lockfile
  3. load prebake manifest → Option<&Manifest>                    ← S5
  4. EffectiveFixups::load(&config.fixups.registry,               ← REPLACES S6's load_local
                           &third_party_dir,
                           config.fixups.allow_local_overrides)
     → Result<EffectiveFixups>
        - dispatches RegistryConfig:
            None      → community = FixupSet::default()
            FileUrl(p) → community = load_community(p)
            Git { .. } → Err(GitRegistryNotImplemented)
        - allow_local_overrides:
            true  → local = load_local(third_party_dir)
            false → local = FixupSet::default()
        - validates: for each community fixup, if replace_community
              → Err(ReplaceCommunityInCommunity { file })
  5. canonicalize(third_party_dir) → abs_third_party_dir
  6. ctx = BuildEmitContext { manifest, fixups: Some(&eff), abs_third_party_dir: Some(...) }
  7. build_emit_input(&config, tree, &lockfile, &ctx) → EmitInput
  8. emitter.emit(input) → write files
```

Inside `build_emit_input`, per-package iteration calls
`ctx.fixups.unwrap().resolve(&pkg_name, &cfg_ctx)` (where `ctx.fixups`
is `Option<&EffectiveFixups>` to preserve the no-fixups test-friendly
path). The emitter remains ignorant of layering — it sees only
`ResolvedFixup`.

---

## 3. Public API surface

### 3.1 `RegistryConfig` (in `src/fixup/registry.rs`)

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryConfig {
    /// No community registry. EffectiveFixups.community is empty.
    None,
    /// Local checkout. Path resolved to absolute at Config parse time.
    FileUrl(PathBuf),
    /// Git registry. Declared in S7a, errors on use; implemented in S7b.
    Git { url: String, rev: Option<String> },
}
```

Replaces `pub struct FixupRegistry(pub String)` in `src/config.rs`.
Parsed by `Config::from_raw`; the existing `validate_registry` function
becomes the enum constructor (`parse_registry_config(s, registry_rev,
muntjac_toml_dir) -> Result<RegistryConfig, ConfigError>`). The
`registry_rev` field from `FixupsConfig` is consumed during enum
construction — folded into `RegistryConfig::Git { rev }` if the
registry is a git URL, ignored (with stderr warning) otherwise.

**`file://` URL handling:**
- `file:///abs/path` → `RegistryConfig::FileUrl(PathBuf::from("/abs/path"))` (RFC 8089).
- `file://./relative` or `file://relative` → **rejected**; RFC 8089 requires absolute paths after `file://`.
- For fixture ergonomics, `Config::from_str_with_root(toml_str, root_dir)` (existing S6 helper used to resolve `third_party_dir` relative paths) handles the case where users want config-relative paths: write `registry = "file://./registry"` in muntjac.toml and `from_str_with_root` resolves it against the config file's directory. The constructed `RegistryConfig::FileUrl` always holds an absolute path. The relative-form-in-text is therefore *parser sugar*, not a runtime concept.

**S0 scaffolding migration:** the existing `FixupRegistry(String)` wrapper is removed. `FixupsConfig.registry` becomes `RegistryConfig`. Callers in `src/config.rs::tests` (and the single CLI call-site in `init.rs`) get updated. No user-facing migration required — no released version uses the scaffolded `FixupRegistry`.

### 3.2 `EffectiveFixups` (in `src/fixup/layer.rs`)

```rust
pub struct EffectiveFixups {
    pub community: FixupSet,    // empty when registry == None
    pub local:     FixupSet,    // empty when allow_local_overrides == false
}

impl EffectiveFixups {
    pub fn load(
        registry: &RegistryConfig,
        third_party_dir: &Path,
        allow_local_overrides: bool,
    ) -> Result<Self, FixupError>;

    /// Resolve a package's fixup for one (version, platform, python) cell.
    /// Reads both layers, applies `replace_community = true` escape hatch,
    /// runs `resolve_for_cell` per layer, then `merge_resolved`.
    pub fn resolve(
        &self,
        pkg: &PackageName,
        ctx: &CfgContext<'_>,
    ) -> ResolvedFixup;
}
```

### 3.3 `merge_resolved` (in `src/fixup/layer.rs`)

```rust
pub fn merge_resolved(community: ResolvedFixup, local: ResolvedFixup) -> ResolvedFixup;
```

Field-by-field merge rules: see §4.2.

### 3.4 `FixupConfig` schema delta (in `src/fixup/schema.rs`)

Current S6 shape (hand-rolled `from_toml_str` peels cfg sections by
prefix-strip + TOML round-trip):

```rust
pub struct FixupConfig {
    pub top: FixupBody,                                // S6
    pub cfg_sections: Vec<(String, FixupBody)>,        // S6
    pub replace_community: bool,                       // NEW in S7a; defaults to false
}
```

`from_toml_str` is extended to recognize the top-level
`replace_community` key and route it to `FixupConfig.replace_community`
rather than `FixupBody`'s field set (otherwise `deny_unknown_fields` on
`FixupBody` would reject the key). `to_toml_string` emits
`replace_community = true` at the file head only when set (and skips
when `false`, the default), preserving round-trip equality for the
common case.

**S6 stub removal:** S6 declared `replace_community: bool` on
`FixupBody` per its §1.2 (parses but no-op). **S7a moves it to
`FixupConfig`** (file-level, outside per-cfg sections) since it applies
to the whole package fixup, not a section. The S6 stub on `FixupBody`
is removed; any community fixup author who wrote
`replace_community = true` in a cfg section (and no such user exists
since S6 was no-op and S7a is unreleased) gets an unknown-field error
under `FixupBody`'s `#[serde(deny_unknown_fields)]`. Migration: move
to the file's top level.

`replace_community` is consumed in `EffectiveFixups::resolve` (drops
community before per-layer resolve) and is **not** propagated to
`ResolvedFixup`.

### 3.5 `BuildEmitContext<'a>` (in `src/buck/emit.rs`, TD-S6-04)

```rust
#[derive(Debug, Clone, Default)]
pub struct BuildEmitContext<'a> {
    pub manifest: Option<&'a PrebakeManifest>,
    pub fixups: Option<&'a EffectiveFixups>,
    pub abs_third_party_dir: Option<&'a Path>,
}

pub fn build_emit_input(
    config: &Config,
    tree: &TreeConfig,
    lockfile: &Lockfile,
    ctx: &BuildEmitContext<'_>,
) -> Result<EmitInput, EmitError>;
```

Three positional inputs (the pure pipeline data: parsed config, tree
selection, parsed lockfile) plus one context struct (the contextual /
optional inputs). Call-site at `cli/buckify.rs` becomes readable;
S7b adds the cache directory to the context without a positional-arg
churn. S5's `Option<&PrebakeManifest>` and S6's `Option<&FixupSet>` +
`Option<&Path>` collapse from three positional optionals into the ctx.

Existing tests in `src/buck/emit.rs::tests::` (~30 tests touching
`build_emit_input`) mechanically migrate to construct
`BuildEmitContext::default()` (all `None`) and override only the
fields each test exercises. The migration is in-stage; not deferred.

---

## 4. Layering algorithm

### 4.1 Resolution (per `(pkg, version, platform, python_version)` cell)

```
EffectiveFixups::resolve(pkg, ctx) -> ResolvedFixup:
    local_cfg = self.local.get(pkg)

    // replace_community escape hatch: applied BEFORE community is read.
    community_cfg = match local_cfg {
        Some(c) if c.replace_community => None,
        _ => self.community.get(pkg),
    }

    community_resolved = match community_cfg {
        Some(cfg) => resolve_for_cell(cfg, ctx),   // S6 fn, reused unchanged
        None      => ResolvedFixup::default(),
    }

    local_resolved = match local_cfg {
        Some(cfg) => resolve_for_cell(cfg, ctx),   // S6 fn, reused unchanged
        None      => ResolvedFixup::default(),
    }

    merge_resolved(community_resolved, local_resolved)
```

**Two-pass design:** each layer is fully resolved (top + matching cfg
sections) into a `ResolvedFixup` before any cross-layer merge. This
guarantees local always wins on scalars, regardless of whether the
overriding value came from local's top or local's cfg section.

The interleaved alternative (`community.top → community.cfgs →
local.top → local.cfgs` into one accumulator) was rejected: a matching
community cfg section would run *after* local's top, shadowing local
scalars. Counterintuitive — local should always win.

### 4.2 `merge_resolved` field rules

| Field | Type | Rule | Rationale |
|---|---|---|---|
| `extra_deps` | `Vec<String>` | community ++ local, dedup preserved-first | additive merge with community-stable ordering |
| `omit_deps` | `Vec<String>` | community ++ local, dedup preserved-first | additive |
| `replace_deps` | `BTreeMap<String, String>` | extend (local key overwrites community) | per-dep override |
| `prefer_wheel` | `Option<String>` | local if `Some`, else community | local wins on scalar |
| `exclude_wheels` | `Vec<String>` | community ++ local, dedup preserved-first | additive (each glob is a separate constraint) |
| `overlay` | `Option<PathBuf>` | local if `Some`, else community | overlay is whole-wheel; can't merge two paths |
| `entry_points` | `Option<EntryPoints>` | local if `Some`, else community | binary list is whole-package |
| `visibility` | `Option<Vec<String>>` | local if `Some`, else community | visibility is whole-rule; merging confuses intent |
| `labels` | `Vec<String>` | community ++ local, dedup preserved-first | additive (labels are tags) |
| `runtime_env` | `BTreeMap<String, String>` | extend (local key overwrites community) | per-env-var override |

**Dedup-preserved-first:** `vec.dedup()` (Rust stdlib) requires
contiguity, so the implementation walks once with a tracking set,
yielding `[community items in order, then local items not seen]`. This
preserves community's intent (a careful author may have ordered deps
deliberately) while letting local additions slot in afterward.

**`replace_community` not in `ResolvedFixup`:** consumed at resolution
time. The merged ResolvedFixup contains no trace of which layer fields
came from — by design. `fixups show` (§6.1) prints the unmerged
layer-by-layer view for provenance.

### 4.3 Worked examples

**Example A — list union with cfg sections:**

```toml
# registry/packages/pkg-a/fixups.toml
extra_deps = ["//community:base"]
[cfg('target_os = "linux"')]
extra_deps = ["//community:linux-only"]
```

```toml
# third-party/python/fixups/pkg-a/fixups.toml
extra_deps = ["//local:base"]
[cfg('target_arch = "x86_64"')]
extra_deps = ["//local:x86-only"]
```

Cell `(linux, x86_64)` → resolve produces:
```rust
ResolvedFixup {
    extra_deps: ["//community:base", "//community:linux-only",
                 "//local:base", "//local:x86-only"],
    ..Default::default()
}
```

Cell `(macos, aarch64)` → community contributes only its top:
```rust
ResolvedFixup {
    extra_deps: ["//community:base", "//local:base"],
    ..Default::default()
}
```

**Example B — scalar override:**

```toml
# community
visibility = ["//community/visibility:..."]
overlay = "overlay-community"
```

```toml
# local
visibility = ["//local/visibility:..."]
# overlay unspecified
```

Resolved: `visibility = ["//local/visibility:..."]`, `overlay = "overlay-community"`.
Local wins on visibility (it was Some); community provides overlay (local was None).

**Example C — `replace_community`:**

```toml
# community
extra_deps = ["//community:huge", "//community:legacy"]
overlay = "overlay-community"
```

```toml
# local
replace_community = true
extra_deps = ["//local:only"]
```

Resolved: `extra_deps = ["//local:only"]`, `overlay = None`. The
community layer is dropped entirely; local stands alone.

---

## 5. Loader: layout convention & validation

### 5.1 Layout conventions

```
# Community registry (file:// mode in S7a)
<registry-checkout>/
└── packages/
    ├── numpy/fixups.toml
    ├── pillow/fixups.toml
    └── ...

# Local fixups (unchanged from S6)
<third_party_dir>/
└── fixups/
    ├── numpy/fixups.toml
    └── ...
```

The directory layouts differ deliberately: matches the design spec §7,
matches reindeer's convention for `fixups/` (local) vs `packages/`
(community), and reduces the chance of a user accidentally pointing
`registry = "file://./third-party/python"` at their own local tree
(which would be empty under `packages/`).

### 5.2 Loader entry points

```rust
// src/fixup/loader.rs

pub(crate) fn load_one_fixup(toml_path: &Path) -> Result<FixupConfig, FixupError>;
// Factored from current load_local internals: read file, parse,
// classify unknown-field vs other parse errors. Shared by both
// entry points.

pub fn load_local(third_party_dir: &Path) -> Result<FixupSet, FixupError>;
// Unchanged signature. Walks `<dir>/fixups/<pkg>/fixups.toml`.
// Internals delegate to load_one_fixup.

pub fn load_community(registry_dir: &Path) -> Result<FixupSet, FixupError>;
// NEW. Walks `<registry_dir>/packages/<pkg>/fixups.toml` — takes the
// registry root, joins `packages/` internally.
// Internals delegate to load_one_fixup.
// After load: walks the set, errors on any entry with
//   replace_community = true → ReplaceCommunityInCommunity { file }.
// Returns RegistryPathNotFound { path: <registry_dir>/packages }
//   if `<registry_dir>/packages` doesn't exist.
// (Difference from load_local: load_local returns empty if fixups/
//  doesn't exist; load_community errors if packages/ doesn't exist
//  because the user explicitly set registry = "file://<path>"
//  pointing at a non-checkout.)
```

PEP 503 normalization (via `PackageName::from_str`) and the
unknown-field error path are identical across both loaders.

### 5.3 `EffectiveFixups::load` dispatcher

```rust
impl EffectiveFixups {
    pub fn load(
        registry: &RegistryConfig,
        third_party_dir: &Path,
        allow_local_overrides: bool,
    ) -> Result<Self, FixupError> {
        let community = match registry {
            RegistryConfig::None => FixupSet::default(),
            RegistryConfig::FileUrl(registry_dir) => {
                // load_community joins `packages/` internally and
                // returns RegistryPathNotFound if the joined path
                // doesn't exist.
                load_community(registry_dir)?
            }
            RegistryConfig::Git { url, .. } => {
                return Err(FixupError::GitRegistryNotImplemented {
                    registry: url.clone(),
                });
            }
        };

        let local = if allow_local_overrides {
            load_local(third_party_dir)?
        } else {
            FixupSet::default()
        };

        Ok(Self { community, local })
    }
}
```

---

## 6. CLI changes

### 6.1 `muntjac fixups show <pkg>` extension

S6 prints the local fixup file as canonical TOML. S7a extends to print
both layers as labeled blocks:

```
# community: /home/jack/proj/registry/packages/pillow/fixups.toml
extra_deps = ["//third-party/c:libjpeg"]

[cfg('target_os = "linux"')]
extra_deps = ["//third-party/c:linux-extras"]

# local: /home/jack/proj/third-party/python/fixups/pillow/fixups.toml
extra_deps = ["//local:internal-shim"]
replace_community = false
```

- Both blocks shown if both layers have the package.
- Single block (no header comment) if only one layer has the package.
- Neither layer has it → exit 1 with `no fixup for package '<pkg>' (checked community at <path>, local at <path>)`.
- When `replace_community = true` is set on local, the community block is still printed (for inspection) followed by a comment line `# (community fixup above is disabled by replace_community = true)` before the local block.

The two `# <layer>: <path>` markers are line comments. The output is
not a single round-trippable TOML document (it's two concatenated); a
machine reader splits on the header comment lines. JSON output was
considered and rejected: the existing S6 `fixups show` is canonical
TOML, and the layered version should be the natural extension.
`--layer community|local` / `--format json` are deferred to post-S7b
polish.

### 6.2 `muntjac.toml` schema

```toml
[fixups]
registry = "file:///abs/path/to/checkout"   # or "none" (default) or "github.com/..." (errors in S7a)
allow_local_overrides = true                # default true; false skips local layer
# registry_rev is parsed but ignored when registry is "none" or "file://...";
# S7b uses it as the SHA pin for git registries.
```

- `registry` is the surface; the typed `RegistryConfig` is internal.
- `registry_rev` parses as `Option<String>` (existing); emits stderr warning when registry is `None` or `FileUrl`:

  ```
  [muntjac] warn: registry_rev is ignored when registry is "none" or "file://..."; effective in S7b for git-based registries
  ```

  Emitted once per `Config::from_raw` invocation.

### 6.3 `muntjac init` template

S0's starter `muntjac.toml` already includes `[fixups] registry = "none"`. S7a adds a hint comment:

```toml
[fixups]
registry = "none"
# When a community registry exists, set to "github.com/<owner>/muntjac-fixups"
# and run `muntjac fixups update` to pin a SHA. For air-gapped or
# pre-launch usage, use a local checkout: registry = "file:///abs/path".
allow_local_overrides = true
```

---

## 7. Error handling

### 7.1 New `FixupError` variants (in `src/fixup/error.rs`)

```rust
#[error("fixup file {file} sets `replace_community = true`, which is \
only valid in local fixups, not in the community registry")]
ReplaceCommunityInCommunity { file: PathBuf },

#[error("git-based community registry is not yet implemented (S7b); \
registry = {registry} requires either \"none\" or \"file://...\"")]
GitRegistryNotImplemented { registry: String },

#[error("community registry path {path} does not exist")]
RegistryPathNotFound { path: PathBuf },
```

### 7.2 New `ConfigError` variant (in `src/error.rs`)

```rust
#[error("registry path must be absolute (got `{path}`); use a full \
file:// URL or set registry to a path relative to muntjac.toml")]
RegistryPathNotAbsolute { path: String },
```

The existing `ConfigError::BadRegistry` covers malformed top-level
registry strings (e.g. `file:` without `//`, `github.com/onlyname`).
`RegistryPathNotAbsolute` is a separate variant because the error
message must distinguish "you wrote a malformed file:// URL" from "you
wrote a syntactically valid relative file:// URL, which is not allowed
per RFC 8089."

### 7.3 Error wording: locked

Per the S6 spec convention (saved ~3 review iterations on error-string
churn), the wording above is **locked byte-for-byte**. Tests assert
canonical messages via `assert_eq!`. Refactors that change error text
require updating both the spec and the assertions in the same commit.

---

## 8. Testing strategy

| Layer | Test type | Location | Count |
|---|---|---|---|
| `RegistryConfig` parse / dispatch | unit | `src/config.rs::tests` | ~6 (None / FileUrl abs / FileUrl root-resolved / Git form parsed / bad file:// rejected / bad github URL rejected) |
| `merge_resolved` per-field | unit | `src/fixup/layer.rs::tests` | 10 (one per field type per row in §4.2) |
| `EffectiveFixups::resolve` | unit | `src/fixup/layer.rs::tests` | ~8 (community-only / local-only / both / replace_community / allow_local_overrides=false equivalent / both with cfg sections / neither has pkg / both have empty top) |
| `EffectiveFixups::load` dispatch | unit | `src/fixup/layer.rs::tests` | ~5 (None → empty; FileUrl OK → community loaded; FileUrl missing → RegistryPathNotFound; Git → GitRegistryNotImplemented; allow_local=false → local empty) |
| `load_community` layout walk | unit | `src/fixup/loader.rs::tests` | ~5 (single / multi / PEP503 normalize / replace_community-rejected / missing packages/ dir) |
| `load_one_fixup` (factored) | unit | `src/fixup/loader.rs::tests` | refactor: existing local-side tests still cover via `load_local`; add 2 direct |
| `BuildEmitContext` migration | unit | `src/buck/emit.rs::tests` | mechanical refactor; all existing tests pass after migrating to `&BuildEmitContext` |
| `fixups show` layered output | integration | `tests/fixups_show_smoke.rs` | +3 (community-only, local-only, both labeled) |
| Fixture 06 — community ⊕ local layering | snapshot | `tests/buckify.rs::fixture_06_community_fixup_golden` | 1 (byte-exact `BUCK` + `muntjac.bzl`) |
| Fixture 07 — `allow_local_overrides = false` | snapshot | `tests/buckify.rs::fixture_07_allow_local_overrides_false` | 1 |
| Fixture 08 — `replace_community = true` | snapshot | `tests/buckify.rs::fixture_08_replace_community` | 1 |
| `RegistryConfig::Git` error in S7a | unit | `src/fixup/registry.rs::tests` | 2 (variant + canonical message) |

**Fixture 06 — three synthetic packages:**

| pkg | community has? | local has? | exercises |
|---|---|---|---|
| `pkg-a` | yes: extra_deps + cfg(target_os="linux") | yes: extra_deps + cfg(target_arch="x86_64") | full layering with cfg sections |
| `pkg-b` | yes: overlay + visibility + labels | no | community-only path; emitter sees community-sourced fields |
| `pkg-c` | no | yes: extra_deps + entry_points | local-only path; behaves like S6 fixture 05 |

Synthetic packages avoid PyPI dependencies, keep the fixture
self-contained, and (since this is snapshot-only, no `buck2 build`)
don't need real wheel content beyond the prebake stubs S6 fixture 05
already established the pattern for.

**Fixture 07:** one synthetic package `pkg-a` with conflicting
visibility in both layers. Two snapshot runs (current `buckify` does
not support per-test-case config; the fixture commits the config with
`allow_local_overrides = false` and the snapshot asserts community
wins). A second fixture variant under `07b-allow-local-overrides-true/`
would exercise the toggle — **deferred**: a unit test on
`EffectiveFixups::load` confirms the boolean flips `local` between
populated and empty, which is the entire mechanism.

**Fixture 08:** `pkg-a` with full community fixture; local
`replace_community = true` plus a thin `extra_deps`. Snapshot asserts
the emitted BUCK reflects only local.

**No buck2 e2e for S7a.** S6's fixture 05 buck2 smoke (commit
`7b3562b`) still runs in CI and confirms the http_file + overlay
infrastructure works. S7a's contribution is the layering algorithm,
which is observable in the muntjac.bzl/BUCK outputs — snapshot diffs
are sufficient. The S7b plan revisits this if `muntjac fixups update`
introduces buck2-observable behavior (likely not — fetch just produces
on-disk fixups that flow through this stage's pipeline).

---

## 9. Demo (exit criteria for S7a)

```bash
# From a clean checkout of muntjac, in a workspace using muntjac:
cat > muntjac.toml <<EOF
manifest_path   = "pyproject.toml"
third_party_dir = "third-party/python"
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target = "x86_64-unknown-linux-gnu"
manylinux = "2_17"

[fixups]
registry = "file:///abs/path/to/muntjac-fixups-checkout"
allow_local_overrides = true
EOF

mkdir -p third-party/python/fixups/pillow
cat > third-party/python/fixups/pillow/fixups.toml <<EOF
extra_deps = ["//local:my-shim"]
EOF

muntjac buckify
# → produces BUCK incorporating community fixup for pillow (overlay,
#   visibility from registry) PLUS the local extra_deps.

muntjac fixups show pillow
# → prints two labeled TOML blocks (community + local).
```

End-to-end: layered fixups produce a correct BUCK when given a registry
as a checkout; the moat works on the no-network path. S7b removes
the "as a checkout" qualifier.

---

## 10. Tech-debt items folded / re-targeted

| Item | Action in S7a |
|---|---|
| TD-S6-04 (`build_emit_input` 6 positional args) | **Resolved** by `BuildEmitContext` refactor (§3.5) |
| TD-S6-05 (split fixture 05 dual-personality) | **Deferred indefinitely.** Per-field unit-test coverage exists; layering goes in fixtures 06/07/08; the buck2 smoke in fixture 05 stays as-is |
| TD-S5 symlink-traversal in `extract_tarball` | **Re-target to S7b.** S7a doesn't introduce tarball extraction (gix is S7b; `load_community` uses `read_dir`) |
| Roadmap row at `2026-05-20-muntjac-roadmap.md:239` shows S6 "⬜ next" | **Fixed in S7a spec commit** (post-S6-ship stale doc) |

---

## 11. Open questions & risks

- **`registry_rev` warning noise:** parsing it in S7a and warning every
  invocation is verbose for users who set it speculatively in S0
  scaffolding. Mitigation: warning fires once per `Config::from_raw`
  (already standard) and disappears in S7b when the field becomes
  meaningful. Pre-launch user count is zero, so the noise window is
  literally just the muntjac maintainer's own dogfood configs.
- **`FileUrl(./relative)` rejection vs root-resolution:** the spec
  resolves config-relative paths via `Config::from_str_with_root`
  before constructing `RegistryConfig::FileUrl`, so the user-visible
  form `registry = "file://./registry"` works in test fixtures (which
  is the only place it matters in S7a). Production-style configs use
  absolute paths. If users hit "but I want config-relative paths and
  I'm not using fixtures," the answer is "use the test-fixture pattern
  in S7b once git URLs are also supported (which removes the
  hand-rolled-checkout need)."
- **Stale-cache hazard from removing `FixupRegistry(String)`:** none
  visible — no released version, no external consumers of `FixupRegistry`. The
  refactor is fully in-stage.
- **`replace_community = true` semantics edge case:** what if local
  has `replace_community = true` *AND* an empty body (no extra_deps,
  etc.)? Behavior: community is dropped; local resolves to
  `ResolvedFixup::default()`. The package gets no fixup at all. Useful
  for "I explicitly want vanilla behavior for this package, ignore
  community." Spec-document this as the canonical use case for an
  empty-with-flag local fixup.
- **Spec interpretation of "cfg sections evaluate independently from
  both layers, applied in order":** the design spec §7 wording is
  ambiguous. S7a resolves to the per-layer-then-merge interpretation
  (each layer fully resolved before cross-layer merge), which gives
  local-always-wins on scalars regardless of cfg-section ordering.
  Recorded here so a future reader of the design spec doesn't have to
  re-derive the choice.

---

## 12. What S7b will add (preview, not committed)

For continuity when reading this stage in isolation:

- `gix` dependency; `git fetch --depth=1` semantics for the configured `RegistryConfig::Git { url, rev }`.
- `~/.cache/muntjac/fixups/<sha>/`: content-addressed cache. Cache hit → skip fetch. Multiple revs coexist.
- `muntjac fixups update [--rev <rev>]`: fetch (default: HEAD of main), record new SHA, write back to `muntjac.toml`'s `registry_rev`, print structured diff (`+ pillow: …`, `- olefile: removed`).
- `--offline` global flag: refuses to fetch; uses cache only; errors with `OfflineButCacheMiss` if pinned SHA isn't cached.
- Symlink-traversal hardening (re-targeted S5 TD).
- Integration test: `--offline` with pre-warmed cache produces same BUCK as online.
