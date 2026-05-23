# Muntjac S6 — Local Fixups (Design)

> **Stage:** S6 (per `2026-05-20-muntjac-roadmap.md`).
> **Status:** Design.
> **Prerequisite reading:** `2026-05-20-muntjac-design.md` §7 (fixup schema & the moat).

---

## 1. Scope & non-goals

S6 introduces **local** per-package fixups: per-repo TOML files at
`<third_party_dir>/fixups/<pkg>/fixups.toml` that override or augment
what `muntjac buckify` emits for that package. This is the stage where
muntjac starts forming its moat — every wart in real-world Python
packaging (hard-coded paths, missing C-lib deps, useless transitives)
can now be patched without modifying muntjac itself.

S7 will add the community registry layer (community ⊕ local). S6 is
local-only but its merging algorithm is written so S7 is a drop-in
extension — `merge_into` runs once in v1, twice in v2.

### 1.1 Scope (applied in S6)

| Field | Application |
|---|---|
| `extra_deps`, `omit_deps`, `replace_deps` | applied in `EmitDeps` construction |
| `prefer_wheel`, `exclude_wheels` | applied as pre/post-filter on wheel picker |
| `overlay` | applied via `genrule(unzip → cp → zip)` in `pypi_package` macro |
| `entry_points = ["..."]` (explicit list) | emit `python_binary` rules + aliases |
| `visibility`, `labels` | applied to the top-level alias of each package |
| `runtime_env = {...}` | applied as `env = {...}` on emitted `python_binary` rules |
| `cfg(version, python, target_os, target_arch, target_env)` + `all`/`any`/`not` | full grammar |

### 1.2 Schema-only in S6 (no apply code; deferred)

| Field | Status |
|---|---|
| `entry_points = true` | parses; applying errors with a pointer to the explicit-list form |
| `replace_community = true` | parses; no-op in S6 (no community layer to disable) |
| `[sdist]` subtree | parses; v2 sdist surface — S6 doesn't read fixups for prebake |

### 1.3 Non-goals (out of S6)

- **Community registry** (S7): no `gix` fetch, no `~/.cache/muntjac/fixups/<sha>/`,
  no community ⊕ local layering.
- **`entry_points = true` auto-discovery** from wheel metadata: uv.lock
  doesn't carry entry points, and downloading wheels at buckify time
  cuts against buckify's no-network property.
- **`python_binary` entry-points that aren't `__main__`**: v1 uses a thin
  convention — `main_module = "<pkg>.__main__"`. Logged as TECH_DEBT.

---

## 2. Module layout & pipeline integration

```
src/fixup/                                  NEW MODULE
├── mod.rs                                  # re-exports; FixupSet, ResolvedFixup
├── schema.rs                               # FixupConfig (serde-derived, strict)
├── cfg.rs                                  # cfg() parser + evaluator
├── layer.rs                                # local-only merge (v1); ⊕ stub for S7
├── loader.rs                               # disk walk: fixups/<pkg>/fixups.toml
└── error.rs                                # FixupError (typed)

src/cli/fixups.rs                           NEW: `muntjac fixups show <pkg>`
src/buck/emit.rs                            MODIFIED: consume Option<&FixupSet>
src/buck/string_writer.rs                   MODIFIED: macro grows overlay + binary branches
```

### 2.1 Pipeline (modeled after S5's manifest threading)

```
cli/buckify.rs:
  1. parse muntjac.toml → Config
  2. parse uv.lock → Lockfile
  3. load prebake manifest (Option<&Manifest>)    ← S5
  4. load fixups disk-walk → FixupSet              ← NEW S6
  5. build_emit_input(config, tree, lockfile, manifest, fixups)
  6. emitter.emit(input) → write files
```

### 2.2 Core types

**`FixupSet`** — loader output, parsed-but-not-evaluated:

```rust
pub struct FixupSet {
    fixups: BTreeMap<PackageName, FixupConfig>,
}
```

**`FixupConfig`** — one package's fixup, schema-shape:

```rust
pub struct FixupConfig {
    pub top: FixupBody,                                   // applies to all cells
    pub cfg_sections: Vec<(CfgPredicate, FixupBody)>,     // order preserved
}

pub struct FixupBody {
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
    pub sdist: Option<SdistFixup>,           // schema-only in S6
    pub replace_community: bool,             // schema-only in S6
}

pub enum EntryPoints {
    Auto(bool),                              // `true` errors at apply
    Named(Vec<String>),
}
```

**`ResolvedFixup`** — per `(package, cell)` after predicate evaluation;
private inside the emitter:

```rust
struct ResolvedFixup {
    extra_deps: Vec<String>,                  // cumulative across sections
    omit_deps: Vec<String>,                   // cumulative
    replace_deps: BTreeMap<String, String>,
    prefer_wheel: Option<String>,
    exclude_wheels: Vec<String>,              // cumulative
    overlay: Option<PathBuf>,
    entry_points: Option<EntryPoints>,
    visibility: Option<Vec<String>>,
    labels: Vec<String>,                      // cumulative
    runtime_env: BTreeMap<String, String>,
}
```

### 2.3 Merge semantics (`merge_into`)

| Field kind | Semantics |
|---|---|
| `Vec<String>` (`extra_deps`, `omit_deps`, `labels`, `exclude_wheels`) | `lhs.extend(rhs)`; **not** deduped here — dedupe happens in the emitter after `omit_deps` is applied |
| `BTreeMap<String, String>` (`replace_deps`, `runtime_env`) | `lhs.extend(rhs)` — later section's keys override |
| `Option<T>` scalars (`overlay`, `prefer_wheel`, `entry_points`, `visibility`) | `if rhs.is_some() { lhs = rhs }` |
| `bool` (`replace_community`) | OR |

---

## 3. cfg() predicate grammar

Reindeer-compatible. Predicates parse from the section header
`['cfg(<expr>)']`.

### 3.1 Atoms

| Atom | Semantics |
|---|---|
| `version = "<spec>"` | PEP 440 version specifier (`>=10.0`, `==1.2.*`, `>1,<2`). Evaluated against the package's resolved version. |
| `python = "<spec>"` | PEP 440 spec against the cell's Python version. |
| `target_os = "linux" \| "macos" \| "windows"` | matches the OS segment of `Platform.target` |
| `target_arch = "x86_64" \| "aarch64" \| ...` | matches arch segment |
| `target_env = "gnu" \| "musl" \| ""` | matches env segment (empty = macOS/Windows) |

`python` extends the meta-design's listed atoms because Python
version-specific fixups are a common real-world need (a package gains
a transitive on 3.12, etc.).

### 3.2 Combinators

`all(a, b, ...)`, `any(a, b, ...)`, `not(a)`. Nestable.

### 3.3 Grammar (informal)

```
expr       := atom | combinator
atom       := IDENT '=' STRING
combinator := ('all' | 'any' | 'not') '(' expr (',' expr)* ')'
IDENT      := 'version' | 'python' | 'target_os' | 'target_arch' | 'target_env'
STRING     := '"' ... '"'
```

Hand-rolled parser, ~100 lines. Tokenize then recursive-descend. Parse
errors carry the offending byte offset for tooling-friendly messages.

### 3.4 Evaluation context

```rust
pub struct CfgContext<'a> {
    pub package_version: &'a pep440_rs::Version,
    pub python_version: pep440_rs::Version,    // e.g. "3.12"
    pub target_os: &'a str,                     // parsed from Platform.target
    pub target_arch: &'a str,
    pub target_env: &'a str,                    // "" for macos/windows
}
```

OS/arch/env split is derived from `Platform::target`:
`x86_64-unknown-linux-musl` → arch=`x86_64`, os=`linux`, env=`musl`.

---

## 4. Layering & resolution algorithm

```rust
// loader.rs
pub fn load_local(third_party_dir: &Path) -> Result<FixupSet, FixupError> {
    // Walk <tpd>/fixups/<pkg>/fixups.toml.
    // Each file -> FixupConfig (serde, deny_unknown_fields).
    // Keys normalized via PEP 503 (hyphens, lowercase).
}

// layer.rs / resolution
fn resolve_for_cell(
    fixup_set: &FixupSet,
    pkg_name: &str,
    pkg_version: &Version,
    cell: &CfgContext,
) -> Option<ResolvedFixup> {
    let cfg = fixup_set.fixups.get(pkg_name)?;
    let mut out = ResolvedFixup::default();
    merge_into(&mut out, &cfg.top);
    for (predicate, body) in &cfg.cfg_sections {
        if predicate.evaluate(cell) {
            merge_into(&mut out, body);
        }
    }
    Some(out)
}
```

### 4.1 S7 forward-compat

`load_local` becomes one of two layers. S7 adds
`load_community(registry_dir, packages) -> FixupSet`; the resolver runs
`merge_into` over the community FixupSet first, then local.
`replace_community = true` short-circuits the community layer for that
package. None of S6's algorithm changes.

### 4.2 Determinism

Stable: `cfg_sections` preserve TOML order, package iteration uses
BTreeMap, and emitter dedupe runs in a fixed order.

---

## 5. Emitter integration

### 5.1 New `build_emit_input` signature

```rust
pub fn build_emit_input(
    config: &Config,
    tree: &Tree,
    lockfile: &Lockfile,
    manifest: Option<&Manifest>,
    fixups: Option<&FixupSet>,        // NEW
) -> Result<EmitInput>
```

Application points inside the existing per-cell, per-package loop:

```
1. wheel selection (existing pick_wheel call)
   ├ pre-filter wheels by `exclude_wheels` globs
   └ if `prefer_wheel = "sha256:..."` set, override picker → use that wheel
                                             (error if not in wheels list)

2. dep formatting (existing format_cell_deps)
   ├ apply omit_deps     → drop matching ":<name>" entries
   ├ apply replace_deps  → substitute ":<name>" → "<buck-target>"
   └ apply extra_deps    → append (validated as well-formed Buck targets)

3. wheel rule emission (new branch in pypi_package macro)
   └ if overlay present → emit `genrule` + thread overlay file paths

4. binary rule emission (new EmitInput field)
   └ for each name in entry_points, emit `python_binary` target

5. top-level alias (existing native.alias)
   └ apply visibility, labels; runtime_env only on binary rules
```

`replace_deps` semantics — **per-consumer override.** When package X's
fixup contains `replace_deps = { numpy = "//company/numpy:numpy" }`,
X's emitted deps line substitutes `:numpy` → `//company/numpy:numpy`.
The `:numpy` BUCK rule still gets emitted because other consumers may
depend on it without an override.

### 5.2 `EmitInput` additions

```rust
pub struct EmitPackage {
    pub name: String,
    pub version: String,
    pub deps: EmitDeps,                          // unchanged
    pub wheels: BTreeMap<ConfigName, EmitWheel>, // unchanged
    pub overlay: Option<EmitOverlay>,            // NEW
    pub entry_points: Vec<String>,               // NEW (empty = no binaries)
    pub visibility: Option<Vec<String>>,         // NEW (None = default PUBLIC)
    pub labels: Vec<String>,                     // NEW
    pub runtime_env: BTreeMap<String, String>,   // NEW (binary rules only)
}

pub struct EmitOverlay {
    /// `(path_in_wheel, src_path_relative_to_third_party_dir)`.
    /// e.g. ("PIL/_imaging.py", "fixups/pillow/overlay/PIL/_imaging.py")
    pub files: Vec<(String, String)>,
}
```

Overlay files are discovered at load time via a recursive directory
walk under `fixups/<pkg>/overlay/`. In-wheel paths are derived from the
relative path under `overlay/`.

### 5.3 The overlay `genrule`

Emitted in `pypi_package` macro when overlay is present:

```starlark
def _overlay_wheel(name, version, src_target, overlay_files):
    # overlay_files: list of (path_in_wheel, source_label) tuples.
    # parent_dir strips the last `/`-separated segment; "" if the path has none.
    cp_cmds = " && ".join([
        "mkdir -p _u/{} && cp $(location {}) _u/{}".format(
            p.rsplit("/", 1)[0] if "/" in p else ".",
            label,
            p,
        )
        for (p, label) in overlay_files
    ])
    native.genrule(
        name = "{}-{}__overlaid".format(name, version),
        srcs = [src_target] + [label for (_, label) in overlay_files],
        out = "{}-{}-overlaid.whl".format(name, version),
        cmd = """
            set -e
            mkdir _u && cd _u && unzip -q $(location {src_target}) && cd ..
            {cp_cmds}
            cd _u && zip -qrX ../$OUT . -x '*/RECORD'
        """.format(src_target = src_target, cp_cmds = cp_cmds),
    )
```

Notes:

- `-x '*/RECORD'` — PEP 427 RECORD lists every entry's sha256;
  overlaying invalidates it. v1 strips it; the resulting wheel passes
  `prebuilt_python_library` because Buck doesn't verify RECORD.
  TECH_DEBT entry: regenerate RECORD properly when downstream tooling
  starts caring.
- `zip -qrX` — `-X` strips timestamps for byte-stable output.
- `cp_cmds` is interpolated at macro-expansion time from Rust-side
  `EmitOverlay.files`, so the Starlark stays straightforward.
- `unzip`/`zip` are assumed present on PATH at Buck-build time;
  documented in the §7 README pre-req section.

When `overlay` is set, the per-cell `prebuilt_python_library` rule
binds `binary_src = ":{pkg}-{ver}__overlaid"` instead of the raw
`http_file`/`prebake:` source target.

### 5.4 Entry-point binary rules

For each name in `entry_points`:

```starlark
native.python_binary(
    name = "<pkg>-<ver>__bin-<n>",
    main_module = "<importable_pkg>.__main__",
    deps = [":<pkg>-<ver>"],
    env = runtime_env,
    visibility = visibility or ["PUBLIC"],
    labels = labels,
)
native.alias(
    name = "<n>",
    actual = ":<pkg>-<ver>__bin-<n>",
)
```

`<importable_pkg>` is the dist name with hyphens replaced by underscores
(PEP 8 / PEP 503): `fake-pillow` → `fake_pillow.__main__`. This is the
standard Python import-name convention.

v1 uses the `__main__` convention — well-formed Python tools (ruff,
black, pip, etc.) all expose `__main__`. Entry-points that map to a
different module:function require an additional shim and are logged as
TECH_DEBT.

---

## 6. Errors

All fixup-loading & application errors flow through a typed
`FixupError` enum and surface as anyhow contexts in `muntjac buckify`.
Canonical messages — exact wording locked here.

| Variant | Trigger | Message template |
|---|---|---|
| `ParseError { file, source }` | TOML parse failure | `` failed to parse fixup at {file}:\n  {source} `` |
| `UnknownField { file, field }` | `serde(deny_unknown_fields)` rejection | `` unknown field `{field}` in fixup at {file}\n  v1 schema: see docs/superpowers/specs/2026-05-20-muntjac-design.md §7 `` |
| `CfgParse { file, section, source }` | predicate parse failure | `` failed to parse cfg() in {file} section `{section}`:\n  {source} `` |
| `EntryPointsAuto { pkg }` | `entry_points = true` | `entry_points = true is not supported in v1; list the binaries explicitly (e.g. entry_points = ["ruff"]).\n  package: {pkg}` |
| `PreferWheelNotFound { pkg, sha, cell }` | `prefer_wheel = "sha256:..."` doesn't match | `prefer_wheel sha256:{sha} not found for {pkg} on cell {cell}.\n  available wheel shas: {comma-separated}` |
| `ExcludeWheelsLeavesNone { pkg, cell }` | every wheel filtered out | `exclude_wheels eliminates every wheel for {pkg} on cell {cell}.\n  loosen the patterns or remove the fixup` |
| `OverlayPathOutsideTree { file }` | symlink escape | `overlay/ contains a symlink to outside the fixup directory: {file}\n  refusing for safety` |
| `OverlayEmpty { pkg, path }` | overlay set but directory empty/missing | `overlay = "{path}" is set for {pkg} but the directory is empty or missing` |
| `ReplaceDepInvalid { pkg, target }` | `replace_deps` value isn't a valid Buck target | `` replace_deps target for {pkg} is not a valid Buck target: `{target}`\n  expected //path:name or :name form `` |
| `ExtraDepInvalid { pkg, target }` | `extra_deps` value isn't a valid Buck target | same shape as `ReplaceDepInvalid` |
| `BadCfgAtom { atom }` | unknown atom in cfg() | `` unknown cfg atom `{atom}`; expected one of: version, python, target_os, target_arch, target_env `` |

**Buck target validation:** `^(//[^:]+)?:[a-zA-Z0-9_./-]+$`. Cheap; catches
typos before they hit Buck.

**No silent fallbacks.** A malformed fixup fails the buckify run with a
single error citing the file. No "ignored fixup, continuing" warnings.

---

## 7. Testing

### 7.1 Unit tests

| Module | What's tested |
|---|---|
| `schema.rs` | TOML round-trip; `deny_unknown_fields` rejects v2 keys; default values for all Optional fields; per-cfg-section parsing |
| `cfg.rs` | Parser: every atom; `all`/`any`/`not`; nested combinators; whitespace insensitivity; parse-error byte offset accuracy. Evaluator: PEP 440 ranges (`>=10.0`, `==1.2.*`, multi-clause); target_env="" matches macOS/Windows; case sensitivity |
| `layer.rs` | `merge_into` correctness: list union, scalar replace, map override; cfg-section ordering preserved; `ResolvedFixup::default()` is correct identity |
| `loader.rs` | Disk walk discovers `fixups/<pkg>/fixups.toml`; normalizes package names (PEP 503); skips non-fixup directories; symlink escape rejected |

### 7.2 Snapshot test fixture

`tests/fixtures/buck/05-local-fixup/` — single fixture exercising every
applied field. Synthetic `fake-pillow` package with hand-rolled tiny
wheels covering 2 platforms × 2 Pythons (= 4 cells):

```
tests/fixtures/buck/05-local-fixup/
├── pyproject.toml
├── muntjac.toml                     # 2 platforms, 2 pythons
├── uv.lock                          # fake-pillow + extras-target
├── third-party/python/
│   └── fixups/
│       └── fake-pillow/
│           ├── fixups.toml
│           └── overlay/
│               └── fake_pillow/
│                   └── _paths.py     # overrides hard-coded libjpeg path
└── expected/
    ├── BUCK                          # snapshot
    ├── muntjac.bzl                   # snapshot
    ├── config/BUCK                   # snapshot
    └── wiring.bzl                    # snapshot
```

The fixture's `fixups.toml`:

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

['cfg(target_os = "linux")']
extra_deps = ["//third-party/c:libssl"]

['cfg(all(version = ">=1.0", python = ">=3.12"))']
labels = ["needs-mod-3.12-shim"]
```

Exercises top-level + 2 cfg sections, list-union, scalar replace, and
combined predicates.

### 7.3 Integration tests

`tests/fixups_smoke.rs` (or extend `tests/buckify_smoke.rs`):

- Run `muntjac buckify` against the `05-local-fixup` fixture.
- Assert generated BUCK matches snapshot byte-for-byte.
- Assert `muntjac fixups show fake-pillow` round-trips a normalized
  TOML (snapshot).
- Skip with explanation if `unzip`/`zip` unavailable.

### 7.4 CI buck2 smoke

Add a step to `.github/workflows/ci.yml` (ubuntu-latest only):
`cd tests/fixtures/buck/05-local-fixup && buck2 build //third-party/python:fake-pillow`.
Verifies the overlay genrule produces a runnable wheel end-to-end.

### 7.5 Determinism

`10-determinism` (planned v0.1.0 fixture) already covers byte-stability.
S6 piggybacks: any non-determinism in fixup application surfaces there.

---

## 8. `muntjac fixups show <pkg>`

Single subcommand. No flags in v1.

**Behavior:**

1. Load `muntjac.toml`, resolve tree's `third_party_dir`.
2. Load `FixupSet` from `<tpd>/fixups/`.
3. Look up `pkg` (PEP 503 normalized).
4. If found: print the parsed `FixupConfig` as canonical TOML —
   top-level body, then each cfg section preserved verbatim.
5. If not found: exit 1 with
   `` no fixup for package `{pkg}` at <tpd>/fixups/ ``.

**Round-trip property:**
`muntjac fixups show pillow > out.toml && diff out.toml
third-party/python/fixups/pillow/fixups.toml` produces only formatting
diffs: key ordering (alphabetical) and absent default-value fields
(unset Optional / empty Vec / false bool are not re-emitted). All
user-set values round-trip identically.

This makes `show` useful as a parse validator: it shows what muntjac
actually parsed, not what's on disk.

---

## 9. Stage-exit checklist

- [ ] `cargo test` green
- [ ] `cargo fmt --check` + `cargo clippy --all-targets -- -D warnings` green (pre-commit hook in place)
- [ ] `05-local-fixup` insta snapshot matches generated BUCK / muntjac.bzl / config/BUCK / wiring.bzl
- [ ] `tests/fixups_smoke.rs` passes
- [ ] CI green on all 3 runners; ubuntu-latest also passes `buck2 build //third-party/python:fake-pillow` (overlay genrule executes)
- [ ] `muntjac fixups show fake-pillow` round-trips its own fixup
- [ ] Roadmap §S6 marked ✅ shipped, commit count logged
- [ ] TECH_DEBT entries logged:
  - (a) Overlay regenerates PEP 427 RECORD properly
  - (b) `entry_points = true` auto-discovery from wheel metadata
  - (c) `python_binary` entry-points that aren't `<pkg>.__main__`

---

## 10. Open questions / risks

| Risk | Mitigation |
|---|---|
| Buck's `genrule` semantics for the overlay step may differ across Buck2 releases | CI buck2 smoke pins a specific Buck2 release; failures surface in v1 |
| `unzip`/`zip` unavailable on Windows runners | S6 skips the CI buck2 smoke on non-Linux runners (same approach as S5) |
| `__main__` convention misses real-world tools | Logged as TECH_DEBT; entry_points with non-`__main__` modules will need a follow-up shim mechanism |
| PEP 503 package-name normalization edge cases | Use `pep508_rs::PackageName` (already a dep) — already PEP 503-compliant |
| Replacing a dep with `//company/...` whose target doesn't exist | Buck reports the missing target; muntjac's job is to emit the substitution, not validate the target's existence |
