# S11 — Multi-tree support

**Status:** Designed 2026-05-25. Pending implementation plan.

**Ships as:** v0.2.0 (first pre-announce Phase 2 stage; build order 1/3, S11 → S9 → S10).

**Predecessor:** S8a (launch polish, v0.1.0 published-but-unannounced).

**Gates:** the maintainer's dogfood migration of a real multi-Python project, which is itself the gate on the public announcement.

---

## 1. Goal

Make muntjac's multi-tree support actually compose at the Buck level, fill the per-command gaps, and prove it end-to-end. A monorepo with incompatible dependency universes — e.g. a legacy service that needs numpy 1.x and a modern service that needs numpy 2.x — can declare one `[tree.<name>]` block per universe, run `muntjac buckify` once, and `buck2 build` both, with the same package at conflicting versions coexisting via distinct Buck target paths.

### 1.1 Already built (no work needed)

The `Tree` abstraction was baked in early (S1–S6), so much of multi-tree already exists:

- **Config model:** `Config.trees: Vec<Tree>`; `[tree.<name>]` blocks parse; flat top-level config maps to a single `"default"` tree; mixing flat + `[tree.*]` shapes is rejected (`ConfigError::IncompatibleShape`). `[platforms]`, `[fixups]`, `[buck]`, `[lockfile]` are top-level shared; `manifest_path` / `third_party_dir` / `python_versions` are per-tree.
- **`buckify`:** loops over `config.trees`, filters by `--tree`.
- **`debug print-deps` / `debug pick-wheels`:** tree-aware with `--tree` filtering.
- **`config check`:** reports every tree.
- **Emitter:** carries `tree.name` through `EmitInput`.

### 1.2 In scope (the actual work)

1. **Emitter split** — separate shared-cfg emission (once) from per-tree package emission. Add `[buck] cfg_dir` with common-ancestor derivation. *(The architectural core.)*
2. **`vendor`** — process all trees by default (currently first-tree-only).
3. **`fixups show`** — print per-tree effective-fixup blocks (currently first-tree-only).
4. **`--tree` consistency** — unknown `--tree` is a hard error everywhere (buckify currently skips silently); factored into one `resolve_trees` helper.
5. **Validation** — reject two trees sharing a `third_party_dir`; reject a tree whose `third_party_dir` equals the resolved `cfg_dir` (multi-tree only).
6. **`10-multi-tree` fixture** — two trees, same Python, numpy 1.x vs 2.x; `buckify` + `buck2 build` both green.
7. **Docs + version** — README multi-tree section; bump to v0.2.0.

### 1.3 Out of scope (deferred)

| Item | Reason |
|------|--------|
| `muntjac init` multi-tree scaffolding | Stays single-tree (the 99% case); multi-tree is a documented hand-edit. YAGNI. |
| Cross-tree version-coherence enforcement | Trees are islands by design (design spec §3); no enforced coherence. |
| Dual-Python fixture (legacy 3.10 + modern 3.12) | Heavier CI (two toolchains); the same-Python conflicting-version fixture proves the composition. Logged as a possible post-S11 follow-up. |
| `fixups update` per-tree behavior | Registry + `registry_rev` are top-level shared, so update is tree-independent; `--tree` is accepted-but-ignored. |

### 1.4 Exit criteria

1. `10-multi-tree` buckifies and `buck2 build`s both trees green in CI on all three matrix runners (same package, conflicting versions, distinct target paths).
2. Single-tree emitter output is **byte-identical** to pre-S11 (existing snapshot fixtures 01–09 pass unchanged — the backward-compat proof).
3. `vendor` and `fixups show` operate across all trees; `--tree <unknown>` errors everywhere with the available-names list.
4. `Config::validate` rejects colliding `third_party_dir`s and (multi-tree) a `third_party_dir == cfg_dir` collision.
5. README documents the multi-tree config shape; `Cargo.toml.version == "0.2.0"`; CHANGELOG has a `## [0.2.0]` section.

---

## 2. Config model

The config layer is mostly in place. The one **new field** is `[buck] cfg_dir` (optional).

### 2.1 Multi-tree config shape

```toml
[platforms]   # shared across all trees (unchanged)
linux-x86_64-gnu  = { target = "x86_64-unknown-linux-gnu",  manylinux = "2_17" }
linux-aarch64-gnu = { target = "aarch64-unknown-linux-gnu", manylinux = "2_17" }
macos-arm64       = { target = "aarch64-apple-darwin",      macos_min = "11.0" }

[fixups]      # shared (unchanged)
registry     = "github.com/rsJames-ttrpg/muntjac-fixups"
registry_rev = "abc123…"

[buck]
file_name = "BUCK"
cfg_dir   = "third-party/python"   # NEW, optional — where shared config/ + wiring.bzl land

[tree.modern]
manifest_path   = "pyproject.toml"
third_party_dir = "third-party/python/modern"
python_versions = ["3.12"]

[tree.legacy]
manifest_path   = "legacy/pyproject.toml"
third_party_dir = "third-party/python/legacy"
python_versions = ["3.12"]
```

### 2.2 `cfg_dir` resolution

A derived accessor (e.g. `Config::cfg_dir() -> PathBuf`) resolves the shared-cfg location:

1. If `[buck] cfg_dir` is set explicitly → use it.
2. Else → the longest common path-ancestor of all trees' `third_party_dir`s.
3. Single tree: the common ancestor *is* that tree's `third_party_dir`, so the cfg lands at `<third_party_dir>/config/BUCK` + `<third_party_dir>/wiring.bzl` — **exactly as today**.

Examples:
- Single tree `third_party_dir = "third-party/python"` → `cfg_dir = "third-party/python"` (unchanged).
- Trees at `third-party/python/{modern,legacy}` → common ancestor `third-party/python` → `cfg_dir = "third-party/python"`.
- Trees at unrelated paths (`apps/a/tp`, `apps/b/tp`) → common ancestor `apps` (or repo root); set `[buck] cfg_dir` explicitly if that's undesirable.

### 2.3 python_versions union

The shared cfg's `python_version` constraint axis is the **union** of every tree's `python_versions` (sorted, deduped). The platform axis is the shared `[platforms]` (already global). So `config/BUCK` carries `config_setting`s for the full `(union-of-python-versions × platforms)` grid; a tree that only uses 3.12 simply never references the 3.10 settings.

### 2.4 New validation rules

In `Config::validate` (typed `ConfigError`, `thiserror`-derived, byte-locked messages like the rest):

- **`DuplicateTreeDir`** — two trees with the same `third_party_dir`. They'd clobber each other's `BUCK`/`muntjac.bzl`. Message names the dir and the colliding tree names.
- **`TreeDirIsCfgDir`** — a tree whose `third_party_dir` equals the resolved `cfg_dir`. The tree's package output would collide with the shared `config/` + `wiring.bzl`. **Fires only when there's more than one tree** — in the single-tree case `cfg_dir == third_party_dir` by construction and is correct (cfg and package share the dir, as today).
- **Unknown `--tree`** — not a `validate` rule but a runtime resolution error (see §4); lists available tree names.

---

## 3. Emitter split

Today the emitter produces one `EmitOutput { buck, muntjac_bzl, config_buck, wiring_bzl }` per tree, and `write_outputs` writes all four into that tree's `third_party_dir`. With N trees that yields N conflicting `config/BUCK` + `wiring.bzl` pairs — N cfg axes the root PACKAGE can't reconcile (it calls `set_cfg_modifiers` once, loading one `wiring.bzl`).

The fix separates the cfg machinery (shared, emit once) from the package machinery (per-tree).

### 3.1 Shared-cfg emission

A new function (e.g. `emit_shared_cfg` in `src/buck/`):

- **Input:** `python_versions_union` (sorted-deduped across all trees), the shared `[platforms]`, and `cfg_dir` (as a cell-relative path).
- **Output:** a `SharedCfgOutput { config_buck, wiring_bzl }`.
  - `config_buck`: `constraint_setting(python_version)` + `constraint_value`s for each union python version, `constraint_setting(platform)` + `constraint_value`s for each platform, and `config_setting`s over the full `union-py × platforms` grid.
  - `wiring_bzl`: `MUNTJAC_HOST_MODIFIERS` mapping host os/arch → `root//<cfg_dir>/config:<platform>`.
- **Written once** to `<cfg_dir>/config/BUCK` + `<cfg_dir>/wiring.bzl`.

### 3.2 Per-tree package emission

The existing `build_emit_input` + `emit`, minus the cfg fields:

- `EmitOutput` keeps `buck` + `muntjac_bzl`; `config_buck` / `wiring_bzl` move out to `SharedCfgOutput`.
- The per-tree `BUCK` / `muntjac.bzl` reference config_settings by **`cfg_dir`-relative label** (`//<cfg_dir>/config:py312-linux-x86_64-gnu`) instead of tree-relative. This is the one threading change: the emit context (`BuildEmitContext`) gains a `cfg_dir` field so label generation points at the shared cfg package.

### 3.3 `buckify` orchestration

```text
parse config → resolve cfg_dir → compute python_versions union
emit shared cfg once → write <cfg_dir>/{config/BUCK, wiring.bzl}
for each tree in resolve_trees(config, --tree):
    emit per-tree BUCK + muntjac.bzl → write <tree.third_party_dir>/{BUCK, muntjac.bzl}
```

The shared cfg is always (re)written, even under `--tree X`, so a single-tree regeneration keeps the cfg consistent with the union of all configured trees.

### 3.4 Backward-compatibility guarantee

For a single tree:
- `cfg_dir == third_party_dir` (§2.2 case 3),
- `python_versions_union == that tree's versions`,
- the platform axis is unchanged,

so the four files land in the same places with identical bytes. **The existing single-tree snapshot fixtures (01–09) must pass unchanged** — this is the regression proof, run before merging the split. `wiring.bzl`'s `root//<cfg_dir>/config:<platform>` resolves to the same `root//<third_party_dir>/config:...` it does today.

### 3.5 Consumer-facing upside

All trees' `python_binary` consumers reference one shared modifier path (`//<cfg_dir>/config:py312`), regardless of which tree they pull targets from. One cfg vocabulary for the whole repo.

---

## 4. Command tree-scoping

A shared helper centralizes tree resolution so every command errors identically:

```rust
// Returns the trees a command should operate on, or an error naming
// available trees if --tree names a nonexistent one.
fn resolve_trees<'a>(config: &'a Config, tree_filter: Option<&str>)
    -> Result<Vec<&'a Tree>>
```

- `tree_filter == None` → all trees.
- `tree_filter == Some(name)` and found → `vec![that tree]`.
- `tree_filter == Some(name)` and not found → error listing available tree names.

| Command | After S11 |
|---------|-----------|
| `buckify` | shared cfg once + per-tree packages for `resolve_trees(...)`; unknown `--tree` → error (was silent skip) |
| `vendor` | prebakes every tree in `resolve_trees(...)` (was first-tree-only); unknown `--tree` → error |
| `fixups show <pkg>` | no `--tree` → labeled effective-fixup block **per tree** (community shared, local per-tree); `--tree X` → just that tree (was first-tree-only) |
| `fixups update` | **unchanged** — registry + `registry_rev` top-level shared; tree-independent; `--tree` accepted-but-ignored (documented) |
| `config check` | unchanged (reports all trees) |
| `debug print-deps` / `pick-wheels` | unchanged (already tree-aware) |
| `init` | unchanged (single-tree scaffold; multi-tree by hand-edit) |

`vendor`'s current `.trees.first()` and `fixups show`'s current `.trees.first()` are both replaced by `resolve_trees`.

---

## 5. Testing

### 5.1 `10-multi-tree` fixture

> The design spec (`2026-05-20-muntjac-design.md` §line 496) calls this `08-multi-tree`, but S7a/S7b consumed fixtures `06`–`09` (`06-community-fixup`, `07-allow-local-overrides-false`, `08-replace-community`, `09-git-registry`). Next free number is `10`.

`tests/fixtures/buck/10-multi-tree/`:

- Two trees, both Python 3.12, sharing `[platforms]` + `[fixups] registry = "none"`.
- `tree.modern` → numpy 2.x; `tree.legacy` → numpy 1.x. A distinct `uv.lock` per tree under each tree's manifest directory.
- `cfg_dir = third-party/python` (common ancestor of `third-party/python/{modern,legacy}`).
- Ships the prelude/toolchains/.buckconfig/PACKAGE scaffolding (reusing fixture 02's pattern), with the root PACKAGE loading the single shared `//third-party/python:wiring.bzl`.

### 5.2 CI (extends `ci.yml`, all three matrix runners)

Per [[feedback_ci_no_arch_polymorphism]], the demo runs on every matrix runner, not behind an arch conditional.

- `muntjac buckify` on the fixture → assert the shared `third-party/python/{config/BUCK,wiring.bzl}` exist once, and per-tree `third-party/python/{modern,legacy}/{BUCK,muntjac.bzl}` exist.
- `buck2 run //tests/smoke:modern_demo` (imports numpy, asserts `np.__version__` starts with `2.`) **and** `//tests/smoke:legacy_demo` (asserts `1.`) — proves both version universes build and run in one Buck project via distinct target paths.

### 5.3 Snapshot + unit tests (`cargo test`)

The durable regression net per [[feedback_tests_over_reviewers]]:

- `10-multi-tree` byte-snapshot of all emitted files (shared cfg + both trees' packages).
- **Backward-compat:** existing single-tree snapshots (01–09) pass unchanged — run before merging the emitter split.
- `Config::validate` unit tests: duplicate `third_party_dir` rejected; multi-tree `third_party_dir == cfg_dir` rejected; single-tree `cfg_dir == third_party_dir` accepted.
- `cfg_dir` derivation unit tests: common-ancestor of `{a/b/modern, a/b/legacy}` = `a/b`; single tree = its own dir; explicit `[buck] cfg_dir` override wins.
- `resolve_trees` helper: all-trees default, `--tree` filter, unknown-name error message.

---

## 6. Docs + versioning

- **README:** add a "Multi-tree" subsection showing the `[tree.<name>]` config shape and the target-path isolation guarantee (`//third-party/python/modern:numpy` vs `//third-party/python/legacy:numpy`).
- **Cargo.toml:** bump `version` to `0.2.0`.
- **CHANGELOG.md:** new `## [0.2.0]` section describing multi-tree support; `[Unreleased]` link refresh.
- **Release:** cut v0.2.0 to crates.io via the existing `release.yml` tag-push flow at stage close (per the roadmap's cut-each-announce-once cadence). The public announcement still waits for S10 + dogfood.
- **Roadmap:** mark S11 ✅ shipped at stage close with commit count + tag.

The design spec already documents multi-tree semantics (`2026-05-20-muntjac-design.md` §3 "Multi-tree"); S11 makes the implementation match.

---

## 7. Plan touch list

**Modify:**
- `src/config.rs` — `[buck] cfg_dir` field on `BuckConfig`; `Config::cfg_dir()` derivation; `DuplicateTreeDir` + `TreeDirIsCfgDir` validation.
- `src/error.rs` — two new `ConfigError` variants.
- `src/buck/emit.rs` — split `EmitOutput` (drop cfg fields); add `SharedCfgOutput` + `emit_shared_cfg`; thread `cfg_dir` into `BuildEmitContext` + label generation.
- `src/buck/` write path — `write_outputs` splits into shared-cfg write + per-tree package write (or a new `write_shared_cfg`).
- `src/cli/buckify.rs` — orchestration per §3.3.
- `src/cli/vendor.rs` — replace `.first()` with `resolve_trees` loop.
- `src/cli/fixups.rs` — `show` per-tree blocks via `resolve_trees`; `update` documents tree-independence.
- `src/cli/mod.rs` (or a shared module) — `resolve_trees` helper.

**Create:**
- `tests/fixtures/buck/10-multi-tree/` — fixture inputs + expected outputs + smoke targets.
- Snapshot test in `tests/buckify.rs` for `10-multi-tree`.

**Update:**
- `.github/workflows/ci.yml` — `10-multi-tree` buckify + buck2 build steps (all runners).
- `README.md`, `CHANGELOG.md`, `Cargo.toml`, roadmap — per §6.

---

## 8. References

- Design spec §3 "Multi-tree" + §6 (emitter) + §line 549 (multi-tree composability): [`2026-05-20-muntjac-design.md`](./2026-05-20-muntjac-design.md)
- Roadmap S11 row + Phase 2 re-sequencing: [`2026-05-20-muntjac-roadmap.md`](./2026-05-20-muntjac-roadmap.md)
- Existing single-tree emitter + fixtures (the backward-compat baseline): `src/buck/emit.rs`, `tests/fixtures/buck/01`–`09`
