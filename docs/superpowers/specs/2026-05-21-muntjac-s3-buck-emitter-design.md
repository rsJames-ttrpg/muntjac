# Muntjac S3 — First BUCK emitter (pure-python, single platform)

**Status:** draft v1 (2026-05-21)
**Companion to:** [`2026-05-20-muntjac-design.md`](./2026-05-20-muntjac-design.md), [`2026-05-20-muntjac-roadmap.md`](./2026-05-20-muntjac-roadmap.md)
**Position:** stage S3 of Phase 1
**Depends on:** S0 (scaffolding), S1 (lockfile parser & graph), S2 (wheel selector) — all shipped
**Feeds into:** S4 (multi-platform BUCK)

---

## 1. Scope

S3 turns the picked-wheel data from S2 into a working Buck2 build-rule output. Each `muntjac buckify` produces a tidy four-file set that downstream first-party `python_binary` targets can depend on without further tooling. The output is byte-stable, hand-readable, and `@generated`-marked so accidental hand-edits are visible in code review.

**In scope**

- A `muntjac buckify [--tree NAME]` command. Reads `muntjac.toml` + `uv.lock`, runs the S1/S2 pipeline (graph + resolved view + wheel picker), and writes four files: `<third_party_dir>/BUCK`, `<third_party_dir>/muntjac.bzl`, `<third_party_dir>/config/BUCK`, `<third_party_dir>/PACKAGE`.
- A `BuckEmitter` trait + a default `StringTemplateEmitter` implementation (hand-rolled `writeln!`-style writers). The trait makes the v1 emitter swappable in the future without re-architecting the pipeline.
- Single-platform × N-pythons cell cardinality (e.g. `linux-x86_64-gnu` × `[3.11, 3.12]`). Pure-python wheels (`py3-none-any`) only.
- Byte-stable deterministic output: sorted iteration everywhere; snapshot tests via `insta`; explicit determinism integration test.
- Folded-in tech debt: `render_tag` consolidation (Display for Tag), `expand_requires_python` floor doc, `RawConfig::platforms` `#[serde(default)]`.

**Out of scope**

- Multi-platform / native wheels — S4.
- Sdist handling (native sdist error path or prebake) — S5.
- Fixups (local or community-registry) — S6/S7.
- Vendor mode (`--vendor`) — S9.
- Actually running `buck2 build` against the output — that's an S4 CI exit criterion.
- Network calls — buckify embeds URLs into `http_file` but doesn't fetch them. `--no-network` and `--frozen` flags are no-ops for buckify in S3 scope (they may guard `uv lock` in S5+).
- First-party workspace-member emission — S3 only handles third-party deps; first-party `python_binary` targets remain hand-written.

---

## 2. Approach

The emitter uses **hand-rolled string formatting** with sorted iteration over `BTreeMap`/`Vec`. The output shape is a fixed set of four Starlark files with regular structure (loads, package decls, macro body, config_settings); a typed AST or templating engine buys nothing at this scale and adds churn-prone machinery.

The emitter logic lives behind a `BuckEmitter` trait so a future implementation (typed AST, alternative output schemes for other build systems) can plug in without touching the CLI or pipeline-composer. The v1 implementation is `StringTemplateEmitter`.

Determinism is non-negotiable. Every map iteration uses `BTreeMap`. Every list is sorted before emission. No timestamps, usernames, or PIDs leak into the output. A snapshot test catches accidental introduction of unsorted iteration on the first run.

---

## 3. Module layout

```
src/
  buck/
    mod.rs            — re-exports the trait + default impl + run() entry point
    emit.rs           — BuckEmitter trait, EmitInput, EmitOutput, ConfigName types,
                        EmitInput construction (pipeline composer)
    string_writer.rs  — StringTemplateEmitter (v1 hand-rolled impl)
    write.rs          — file-system writer: takes EmitOutput, writes to disk atomically
  cli/
    buckify.rs        — new: parses args, builds EmitInput per tree, calls emitter + writer
  cli/mod.rs          — wire Buckify command to buckify::run (replaces stub::run)
```

Boundaries:

- `emit.rs` knows only about the input/output data shapes and the trait. No I/O, no Starlark syntax.
- `string_writer.rs` is the only place Starlark byte sequences live. Swappable behind the trait.
- `write.rs` handles paths + filesystem (create `config/` subdir, write atomically, overwrite). Doesn't know what Starlark looks like.
- `cli/buckify.rs` composes the S1+S2 pipeline into `EmitInput`s, then delegates emission + writing.

The `Display for Tag` impl from the `render_tag` tech-debt fold-in lands in `src/wheel/tag.rs` and is consumed by both `pick_wheels.rs` and (where useful) the buck emitter. The snapshot-test render helpers in `compat.rs::tests` get deleted in favor of `tag.to_string()`.

---

## 4. Type model

```rust
// src/buck/emit.rs

pub struct EmitInput {
    pub tree: String,
    pub configs: Vec<ConfigName>,   // sorted lex
    pub packages: Vec<EmitPackage>, // sorted by (name, version) lex
    pub third_party_dir: String,    // configured third_party_dir, used to render
                                    // Buck cell-relative paths in load() and select()
}

pub struct EmitPackage {
    pub name: String,
    pub version: String,
    pub deps: Vec<String>,                 // sorted alphabetically; rendered as ":<name>"
    pub wheels: BTreeMap<ConfigName, EmitWheel>,
}

pub struct EmitWheel {
    pub url: String,
    pub hash: String,   // includes "sha256:" prefix
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConfigName(String);  // e.g. "py312-linux-x86_64-gnu"

pub struct EmitOutput {
    pub buck: String,
    pub muntjac_bzl: String,
    pub config_buck: String,
    pub package_file: String,
}

pub trait BuckEmitter {
    fn emit(&self, input: &EmitInput) -> EmitOutput;
}
```

`ConfigName` is a newtype around `String` so it can derive `Ord` for `BTreeMap` keying. Construction is `ConfigName::new(py, platform_name) -> Self` — central place to format the `"py{maj}{min}-{platform}"` shape.

---

## 5. `EmitInput` construction (pipeline composition)

`cli/buckify.rs::run` builds `EmitInput` from a tree's worth of resolved data:

1. Load + validate config. If `--tree NAME` filter is set, restrict to that tree; otherwise process all trees.
2. For each tree:
   - Parse `uv.lock` once; build the dep graph once.
   - For each `(platform, py_version)` cell in the tree, run `ResolvedView::project` + `build_compatible_tags` + `pick_wheel` against every reachable registry/git package.
   - Cross-merge: per-package, build a `BTreeMap<ConfigName, EmitWheel>` from the picked wheels across all cells.
   - Resolve dep names: `EmitPackage.deps` is the list of *reachable* registry/git dep targets, formatted as `":<dep_name>"` (relative target in same BUCK). First-party deps are skipped at this stage.

### Error: `NoWheel` outcome

S3 only handles cases where every cell resolves to a real wheel. A `NoWheel` outcome on any cell for a registry package is a hard error:

```
error: package 'foo-1.0' has no wheel for cell (py312, linux-x86_64-gnu)
       and S3 does not yet handle native sdists. Restrict the affected
       python_versions or platforms in muntjac.toml until S5 lands.
```

The cell is named precisely, and the fix is documented in-line. S5's sdist prebake adds the real path forward.

### Error: dep-set cross-cell mismatch

For a given package, the `deps` list must be the same across all cells (PEP 508 markers can in principle vary it, but pure-python deps in S3 are typically marker-free). The implementation computes `deps` per-cell, then asserts equality across cells per package. Mismatches error:

```
error: package 'foo-1.0' has different deps across cells:
       (py311, linux-x86_64-gnu) → [bar, baz]
       (py312, linux-x86_64-gnu) → [bar]
       Per-cell select()-driven deps are deferred to S4.
```

This protects against accidentally emitting wrong BUCK when uv.lock has marker-gated deps. S4 will introduce per-cell `deps = select({...})` to handle the legitimate cases.

### `ConfigName` format

`py{major}{minor}-{platform_name}` where `platform_name` is the muntjac.toml platform key (e.g. `linux-x86_64-gnu`). Matches design spec §6.

---

## 6. Generated file contents

### `<third_party_dir>/BUCK`

```python
##
## @generated by muntjac
## Do not edit by hand.
##

load("//<third_party_dir>:muntjac.bzl", "pypi_package")

pypi_package(
    name = "certifi",
    version = "2025.4.26",
    deps = [],
    wheels = {
        "py311-linux-x86_64-gnu": ("https://files.pythonhosted.org/.../certifi-2025.4.26-py3-none-any.whl", "sha256:abc..."),
        "py312-linux-x86_64-gnu": ("https://files.pythonhosted.org/.../certifi-2025.4.26-py3-none-any.whl", "sha256:abc..."),
    },
    visibility = ["PUBLIC"],
)

pypi_package(
    name = "requests",
    version = "2.32.3",
    deps = [
        ":certifi",
        ":charset-normalizer",
        ":idna",
        ":urllib3",
    ],
    wheels = {
        "py311-linux-x86_64-gnu": ("...", "..."),
        "py312-linux-x86_64-gnu": ("...", "..."),
    },
    visibility = ["PUBLIC"],
)
```

- Packages emitted alphabetically by `(name, version)`.
- Wheels dict iterated by sorted `ConfigName`.
- `deps` list pre-sorted alphabetically.
- The `load(...)` path uses the configured `third_party_dir`, rendered as a Buck cell-relative path (leading `//` plus the dir).

### `<third_party_dir>/muntjac.bzl`

Body identical across trees except `_CONFIGS` is filled from the tree's `(platform × python)` cross product:

```python
##
## @generated by muntjac
##

load("@prelude//python:python_library.bzl", "prebuilt_python_library")
load("@prelude//utils:utils.bzl", "expect")

_CONFIGS = [
    "py311-linux-x86_64-gnu",
    "py312-linux-x86_64-gnu",
]

def pypi_package(name, version, wheels, deps = [], visibility = None, **kwargs):
    expect(set(wheels.keys()).issubset(set(_CONFIGS)),
           "unknown config in {}".format(name))

    for cfg, (url, sha256) in wheels.items():
        http_file(
            name = "{}-{}-{}-wheel".format(name, version, cfg),
            sha256 = sha256.removeprefix("sha256:"),
            urls = [url],
            visibility = [],
        )
        prebuilt_python_library(
            name = "{}-{}__{}".format(name, version, cfg),
            binary_src = ":{}-{}-{}-wheel".format(name, version, cfg),
            deps = deps,
            visibility = [],
        )

    native.alias(
        name = "{}-{}".format(name, version),
        actual = select({
            "//<third_party_dir>/config:{}".format(cfg): ":{}-{}__{}".format(name, version, cfg)
            for cfg in wheels.keys()
        }),
        visibility = [],
    )
    native.alias(
        name = name,
        actual = ":{}-{}".format(name, version),
        visibility = visibility or ["PUBLIC"],
    )
```

The `<third_party_dir>` placeholder in `select()` keys is replaced at emit time with the configured value.

### `<third_party_dir>/config/BUCK`

```python
##
## @generated by muntjac
## Do not edit by hand.
##

config_setting(
    name = "py311-linux-x86_64-gnu",
    constraint_values = [
        "//<third_party_dir>/config:py311",
        "//<third_party_dir>/config:linux-x86_64-gnu",
    ],
)

config_setting(
    name = "py312-linux-x86_64-gnu",
    constraint_values = [
        "//<third_party_dir>/config:py312",
        "//<third_party_dir>/config:linux-x86_64-gnu",
    ],
)

constraint_setting(name = "python_version")
constraint_value(name = "py311", constraint_setting = ":python_version")
constraint_value(name = "py312", constraint_setting = ":python_version")

constraint_setting(name = "platform")
constraint_value(name = "linux-x86_64-gnu", constraint_setting = ":platform")
```

Per-config `config_setting`s reference per-axis `constraint_value`s. The python axis and platform axis are each declared once; `config_setting`s combine them. `config_setting` declarations sorted by name; per-axis `constraint_value` declarations sorted within their axis.

### `<third_party_dir>/PACKAGE`

```python
##
## @generated by muntjac
## Do not edit by hand.
##

# Wires python_version + platform constraints so consumers' python_binary
# selects the right wheel via the alias-with-select pattern.

# (S3 emits a minimal PACKAGE that declares the constraints' visibility.
#  Full set_cfg_modifiers wiring lands in S4 once multi-platform select()
#  has actually been exercised against a buck2 build.)
```

Honest scope note: the PACKAGE file in S3 is mostly a placeholder header. The real `set_cfg_modifiers` plumbing requires `buck2 build` validation, which is S4's exit criterion. S3 emits the file deterministically (same bytes every time) so the determinism test passes, and S4 fills in the actual wiring.

---

## 7. Determinism

Byte-stability is S3's headline requirement. Three mechanisms enforce it:

1. **Sorted iteration everywhere.** Packages by `(name, version)` lex; wheels dicts by `ConfigName` lex; deps alphabetically; `_CONFIGS` lex; `config_setting`s by name; `constraint_value`s per-axis.
2. **No timestamps, usernames, or PIDs in output.** The `@generated by muntjac` header is constant text — no muntjac version string, no date. Embedding the version would mean every release bumps every generated file; deferred to a future `--add-version-header` mode if anyone asks.
3. **No randomness in HashMap iteration.** Use `BTreeMap` everywhere. `EmitInput` Vecs are pre-sorted by the pipeline composer; the emitter walks them in input order without re-sorting.

If a future contributor adds a `HashMap` or forgets to sort a Vec, the snapshot test catches it on first run.

---

## 8. CLI surface & filesystem behavior

```
muntjac buckify [--tree NAME]
```

- `--tree NAME`: reuses the existing global flag. If unset and there's only one tree, buckify writes that tree. If unset and multiple trees, buckify writes ALL trees (one set of output files per tree's `third_party_dir`).
- No `--check` / `--dry-run` flag in S3.
- Reads from `globals.workdir().join("muntjac.toml")`.

**Filesystem behavior:**

For each tree to buckify:
1. Resolve `third_party_dir` relative to the muntjac.toml directory.
2. Ensure `third_party_dir/` and `third_party_dir/config/` exist (create recursively if missing).
3. Atomically write each output file (write to `<file>.tmp` then rename). Atomicity guards against partial writes if buckify is interrupted.
4. Overwrite existing files unconditionally — they're `@generated` and we own them. The header line makes accidental hand-edits visible in code review.

**Behavior with stale files:**

If a package was previously declared and is now removed from `pyproject.toml` + relocked, it no longer appears in the new BUCK. Old per-package targets are simply absent — Buck's stale-target detection is the user's recourse. S3 doesn't run any garbage-collection pass.

**Error paths:**

- `muntjac.toml` missing → existing "config not found" error from S0.
- `uv.lock` missing → existing "lockfile not found" error from S1.
- Cycle detected → existing cycle error from S2.
- `NoWheel` outcome → new S3 error (§5).
- Dep-set cross-cell mismatch → new S3 error (§5).
- IO error writing output → bubbles up via `anyhow::Result` with context.

Exit code 0 on success; non-zero on any error.

---

## 9. Folded-in tech debt

### `render_tag` consolidation (Polish, targets S3)

Implement `Display for Tag` (and helpers) in `src/wheel/tag.rs`:

```rust
impl Display for Tag {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}-{}", self.python, self.abi, self.plat)
    }
}

impl Display for PythonTag { /* per spec §6 render */ }
impl Display for AbiTag    { /* per spec §6 render */ }
impl Display for PlatformTag { /* per spec §6 render */ }
```

Three call sites converge:
- `src/cli/debug/pick_wheels.rs::render_tag` → delete; consume `tag.to_string()`.
- `src/wheel/compat.rs::tests` render helpers → delete; snapshot tests format via `tag.to_string()`.
- New `src/buck/string_writer.rs` consumes `tag.to_string()` where it needs to render a Tag (probably not directly — `ConfigName` carries the rendered form).

Snapshot files in `src/wheel/snapshots/` need regeneration if Display produces any different bytes than the old helpers. Implementer verifies: if zero diffs, snapshots stay untouched; otherwise regenerate via `INSTA_UPDATE=always` and inspect.

### `expand_requires_python` floor doc (Polish, any-stage)

In `src/cli/init.rs`, extract the hard-coded `3.11` floor into a constant + doc comment:

```rust
/// The minimum Python minor version muntjac supports for new projects.
/// Older versions are clamped to this floor in `expand_requires_python`.
const MIN_SUPPORTED_PY_MINOR: u8 = 11;
```

If used elsewhere (e.g. `Config::validate` for python_version range checks), reference it there too.

### `RawConfig::platforms` `#[serde(default)]` (Polish, any-stage)

In `src/config.rs`, add `#[serde(default)]` to `RawConfig::platforms` and an explicit check in `Config::from_raw`:

```rust
if raw.platforms.is_empty() {
    return Err(ConfigError::MissingField("platforms"));
}
```

Add or update a test verifying the friendlier error message.

All three items move from `TECH_DEBT.md ## Open` to `## Resolved` at the end of S3.

---

## 10. Testing

### Unit tests, `src/buck/string_writer.rs`

Synthetic `EmitInput`s; each output kind snapshot-tested via `insta`:

- **Empty `EmitInput`** (no packages, two configs): emits valid `BUCK` header + zero `pypi_package` calls, valid `muntjac.bzl` with `_CONFIGS`, valid `config/BUCK`, valid `PACKAGE`.
- **Single package, no deps:** one `certifi`-like entry; snapshots of all four output strings.
- **Two packages with deps:** one depending on the other; verifies `deps = [":<dep>"]` rendering and alphabetic ordering.

### Unit tests, `src/buck/emit.rs` (input construction)

- **`NoWheel` error** fires with the precise message from §5, naming the cell.
- **Dep-set cross-cell mismatch error** fires with the message from §5.

### Integration fixture, `tests/fixtures/buck/01-pure-python/`

```
tests/fixtures/buck/01-pure-python/
├── muntjac.toml          — single linux-x86_64-gnu platform; python_versions = ["3.11", "3.12"]
├── pyproject.toml         — input for uv lock
├── uv.lock                — frozen
└── expected/
    ├── BUCK
    ├── muntjac.bzl
    ├── PACKAGE
    └── config/BUCK
```

Test:

```rust
#[test]
fn fixture_01_pure_python_golden() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/buck/01-pure-python");
    let tmp = tempfile::tempdir().unwrap();
    copy_fixture_to(&fixture, tmp.path());
    run_buckify(tmp.path());
    assert_files_match(tmp.path().join("<third_party_dir>"), &fixture.join("expected"));
}
```

Implementer picks helper-function names; the pattern is: copy fixture into tempdir, run `muntjac -C <tempdir> buckify`, byte-compare each output against `expected/`.

### Integration fixture, `tests/fixtures/buck/10-determinism/`

Reuses `01-pure-python` inputs (or its own equivalent). Test runs buckify twice into two tempdirs and asserts every output file is byte-identical across runs.

```rust
#[test]
fn fixture_10_determinism_two_runs_byte_identical() {
    let fixture = ...;
    let out_a = run_buckify_to_temp(fixture);
    let out_b = run_buckify_to_temp(fixture);
    for filename in ["BUCK", "muntjac.bzl", "PACKAGE", "config/BUCK"] {
        let bytes_a = std::fs::read(out_a.join(filename)).unwrap();
        let bytes_b = std::fs::read(out_b.join(filename)).unwrap();
        assert_eq!(bytes_a, bytes_b, "{filename} differs across runs");
    }
}
```

### Folded-in tech-debt tests

- **`Display for Tag`**: unit tests asserting `tag.to_string()` for canonical, abi3, pure-py3, and `Other(String)` cases.
- **`expand_requires_python` floor doc**: no new test (pure docs + constant extraction).
- **`RawConfig::platforms` `#[serde(default)]`**: new test asserting `MissingField("platforms")` for a config with zero `[platforms.*]`.

### Test budget estimate

~+20 tests over S2's 118 → ~138 total.

---

## 11. Exit criteria

Stage ships when all of the following hold green in CI on ubuntu-latest, ubuntu-24.04-arm, and macos-latest.

### Roadmap-mandated

- `muntjac buckify` on the `01-pure-python` fixture produces byte-identical artifacts matching `expected/BUCK`, `expected/muntjac.bzl`, `expected/PACKAGE`, `expected/config/BUCK`.
- Running buckify twice in a row produces no diff (`10-determinism` fixture).
- Sorted iteration order documented in `BuckEmitter` trait docs and tested: packages by `(name, version)` lex; wheels by `ConfigName` lex; deps alphabetic; `_CONFIGS` lex.

### S3-specific additions

- `NoWheel` outcome on any cell errors with the precise message from §5, naming the cell and pointing at S5.
- Dep-set cross-cell mismatch errors with the message from §5, deferring per-cell `select()` to S4.
- Generated files carry the `@generated by muntjac` header.
- `BuckEmitter` is a public trait; the v1 `StringTemplateEmitter` implements it.

### Folded-in tech debt (all moved to `TECH_DEBT.md ## Resolved`)

- `Display for Tag` (and helpers) implemented; the three duplicate `render_tag` copies (pick_wheels handler, compat snapshot tests, BUCK emitter callers) all delegate to it.
- `expand_requires_python` floor documented with `MIN_SUPPORTED_PY_MINOR` constant.
- `RawConfig::platforms` carries `#[serde(default)]`; empty platforms produce `MissingField("platforms")`.

### Demo

On the `01-pure-python` fixture, `muntjac buckify` writes a tidy four-file output set. The BUCK contains one `pypi_package(...)` per resolved dep; the `muntjac.bzl` macro is grep-friendly Starlark; the `config/BUCK` declares per-cell `config_setting`s; the PACKAGE has a placeholder header. Re-running buckify produces zero diff. Tests: ~138 total.

---

## 12. Risks & mitigations

- **PACKAGE is mostly a placeholder.** S4 will replace it with real `set_cfg_modifiers` wiring once buck2 build verifies the construction. Mitigation: explicit scope note in §6 and inline in the generated file; future S4 spec will identify this as a deliverable.

- **Snapshot tests over real PyPI versions can drift if the lockfile is regenerated.** Mitigation: `tests/fixtures/buck/README.md` (per the S2 convention) spells out the frozen-artifact rule. Test goldens travel in-tree.

- **Single-platform scope means we don't actually exercise multi-key `select()` until S4.** A bug in the `select()` rendering could survive S3 and surface only in S4. Mitigation: the `select()` is rendered uniformly regardless of key count (BTreeMap iteration with a comprehension); the single-key case still exercises the rendering path, just degenerately.

- **`BuckEmitter` trait shape may not generalize to richer emitters (e.g. with diagnostics, partial output).** Mitigation: this is a one-way door we accept. A future trait revision can be done with semver intent; the v1 shape is small enough to evolve.

- **`Display for Tag` regeneration may produce subtly different bytes than the old helpers.** Mitigation: implementer verifies by running the snapshot tests; any drift surfaces immediately and can be inspected before acceptance.

---

## 13. Open questions

None blocking. Deviations during implementation should be filed as follow-ups in `TECH_DEBT.md`.
