# Muntjac S5 — Pure-python sdist prebake

**Status:** draft v1 (2026-05-22)
**Companion to:** [`2026-05-20-muntjac-design.md`](./2026-05-20-muntjac-design.md), [`2026-05-20-muntjac-roadmap.md`](./2026-05-20-muntjac-roadmap.md)
**Position:** stage S5 of Phase 1
**Depends on:** S0–S4 (scaffolding, lockfile, wheel selector, BUCK emitter, multi-platform) — all shipped
**Feeds into:** S6 (local fixups)

---

## 1. Scope

S5 closes the only remaining "package present in `uv.lock`, no usable wheel for some cell" gap on the v0.1.0 launch surface: **pure-python sdist-only packages**. After S5, those flow through the pipeline as if they had a `py3-none-any` wheel — `muntjac vendor` prebakes them via `uv build`, `muntjac buckify` emits BUCK rules that reference the prebaked artifact locally. **Native** sdists continue to be a hard error, but now the error is the canonical message contracted in design-spec §6, with the `(package, version, platform, python_version)` tuple substituted in.

**In scope**

- **Sdist classifier** (`src/sdist/classifier.rs`): pure-python vs native, per design-spec §5 invariant. Allowlisted backends (`flit_core`, `hatchling`, `setuptools` w/o `ext_modules`, `poetry-core`, `pdm-backend`) + adjacent-source heuristic (`Cargo.toml`, `meson.build`, `CMakeLists.txt`, `*.c`/`*.cpp`/`*.cc`/`*.cxx`/`*.pyx`). Conservative — false negatives allowed, false positives forbidden.
- **Prebake shellout** (`src/sdist/prebake.rs` + `src/uv.rs`): `uv build --wheel --out-dir <staging> <sdist_root>`. Captures stderr; verifies exactly one wheel produced; sha256s it.
- **`muntjac vendor` (S5 scope only):** lock-freshness check (`uv lock` if `pyproject.toml` is newer than `uv.lock` unless `--frozen`); download sdist tarballs from `sdist.url` with sha256 verification against `uv.lock`; extract; classify; prebake pure-python; write `third-party/python/prebake/.manifest.toml` + the built wheel. **No wheel caching** (deferred to S9).
- **Prebake manifest** (`.manifest.toml`): single source of truth for `buckify`. Each sdist-only package gets exactly one entry with `classification`, `sdist_sha256`, and (for pure-python) `wheel_filename` + `wheel_sha256`. Sorted by `(package, version)` for determinism.
- **BUCK emit extension:** `WheelSource::Prebake { rel_path, sha256 }` joins `WheelSource::Remote`. The emitter writes `("prebake:<filename>", "sha256:<hex>")` tuples in the wheels dict. The generated `muntjac.bzl` macro dispatches per cfg between `http_file` and `native.export_file`. Source-target rules are deduped by `(src, sha)` to avoid N identical `export_file` rules for a single pure-python wheel covering N cfgs.
- **Native-sdist error:** byte-for-byte the design-spec §6 text, with `(pkg, ver, py, platform)` substituted. Asserted by fixture `09-native-sdist-error`.
- **Buckify staleness checks:** missing manifest entry → "run `muntjac vendor`" error; manifest `sdist_sha256` ≠ `uv.lock` sdist hash → "prebake is stale" error.
- **Fixtures:** synthetic-tree classifier corpus under `tests/fixtures/sdist/classify/`; one real-tarball end-to-end fixture `tests/fixtures/buck/04-pure-python-sdist/`; one error-shape fixture `tests/fixtures/buck/09-native-sdist-error/`.
- **CI:** existing buck2 smoke workflow extended with a second smoke target running `import` against the prebaked package on `ubuntu-latest`.
- **TECH_DEBT bookkeeping:** the `cp313t` entry's target stage moves from "S5 or later" to "post-launch" — not folded in, per the deferral disposition in the entry itself.

**Out of scope**

- **Wheel caching into `~/.cache/muntjac`** — deferred to S9 (vendor mode). S5's `muntjac vendor` does not download or cache wheels.
- **Native-sdist Buck-time builds** (the `pypi_sdist_package(...)` v2 surface in design-spec §6) — deferred indefinitely. The `[sdist]` block in the fixup schema (design-spec §7) is a v1 schema commitment but has no runtime in v0.1.0.
- **`cp313t` / free-threaded ABI first-class support** — explicitly deferred per the TECH_DEBT entry's disposition.
- **`muntjac fixups show <pkg>` command** — the native-sdist error message references it as a forward link; the command ships in S6.
- **`replace_deps`** — also a forward reference in the error message; ships with the fixup schema in S6.
- **Wheel selection altered by fixups (e.g. `exclude_wheels`)** — the prebake path is forward-compatible (it triggers whenever the matrix produces a `Prebake` result) but no fixup integration ships in S5.
- **Multi-tree (`[tree.X]`)** — design-spec §3 escape hatch, deferred to S11.

---

## 2. Approach

Two new top-level pieces:

1. A **classifier** that operates on an unpacked sdist tree and returns a verdict (pure-python with a backend tag, or native with a structured reason). Pure file-tree inspection — no Python, no shellouts.
2. A **prebake shellout** to `uv build` that turns a pure-python sdist tree into a wheel.

These compose inside `muntjac vendor`, which is upgraded from its S0-era stub into a real CLI verb. Vendor's output is a manifest file + an in-tree `prebake/` directory of built wheels, both rooted at `<third_party_dir>/python/prebake/`.

`muntjac buckify` is taught two things: (a) read the manifest and route sdist-only packages either to a `Prebake` wheel source or to the canonical native-sdist error, and (b) detect staleness (missing or sha-mismatched manifest entries) and tell the user to re-run `muntjac vendor`.

The generated `muntjac.bzl` gets a small re-architecture: source-target rules (`http_file` / `export_file`) are emitted once per unique `(source, sha256)` and then referenced from per-cfg `prebuilt_python_library` rules. This is a strict improvement over the S4 shape — it deduplicates `http_file` calls for `py3-none-any` wheels too — and makes the prebake case trivial (one `export_file`, N library variants).

The decoupling line: **`buckify` is pure and never extracts a tarball**. All extraction + classification work lives in `vendor`. The manifest is the contract.

---

## 3. Module layout

```
src/
├── sdist/                          # NEW
│   ├── mod.rs                      # re-exports Classification, classify(), prebake(), Manifest, ManifestEntry
│   ├── classifier.rs               # pure-python vs native heuristic
│   ├── prebake.rs                  # `uv build` wrapper + sha256
│   ├── manifest.rs                 # .manifest.toml load/save + sorted iteration
│   └── error.rs                    # SdistError (Download, HashMismatch, Extract, Classify, UvNotFound, PrebakeFailed, etc.)
├── uv.rs                           # NEW — uv CLI wrappers (`uv build`, room for future `uv lock`)
├── cli/
│   └── vendor.rs                   # NEW — replaces the stub for the `vendor` verb
├── buck/
│   ├── emit.rs                     # WheelSource gets a Prebake variant; build_emit_input consults the manifest
│   └── bzl.rs                      # macro extended to dispatch http_file / export_file; source-target dedup
└── error.rs                        # adds SdistError to the top-level error enum
```

`src/sdist/error.rs` is a separate file (not folded into `src/error.rs`) following the same pattern as `src/lock/` and `src/wheel/` — each subsystem owns its error type and the top-level `Error` enum holds a transparent variant.

---

## 4. Type model

### Classification

```rust
// src/sdist/classifier.rs
pub enum Classification {
    PurePython { backend: AllowlistedBackend },
    Native { reason: NativeReason },
}

pub enum AllowlistedBackend {
    FlitCore,
    Hatchling,
    Setuptools,
    PoetryCore,
    PdmBackend,
}

pub enum NativeReason {
    UnknownBackend { build_backend: String },
    MissingPyprojectToml,
    SetuptoolsWithExtModules,
    AdjacentNativeSource { hit: NativeSourceHit },
}

pub enum NativeSourceHit {
    CargoToml(PathBuf),
    MesonBuild(PathBuf),
    CMakeLists(PathBuf),
    CExt(PathBuf),
    CppExt(PathBuf),
    PyxExt(PathBuf),
}

pub fn classify(sdist_root: &Path) -> Result<Classification, ClassifyError>;
```

`ClassifyError` is for I/O and parse failures (unreadable `pyproject.toml`, bad UTF-8 in `setup.py`). Failures do **not** default to `Native` — they bubble up. A malformed sdist is operator error, not a classification result.

### Algorithm

Priority order; first match wins:

1. Read `pyproject.toml` at the sdist root. If absent → `Native { MissingPyprojectToml }`.
2. Parse `[build-system].build-backend` (toml). If empty/missing → `Native { UnknownBackend { build_backend: "" } }`. If non-empty but not in the allowlist → `Native { UnknownBackend }`.
3. If the backend is `setuptools` (or `setuptools.build_meta`), look for `setup.py` at the root and substring-match `ext_modules=`. If hit → `Native { SetuptoolsWithExtModules }`. (Substring match — intentionally crude. False positives produced by comments are accepted as conservative behavior.)
4. Walk the tree (depth ≤ 4 from `sdist_root`, where `sdist_root` itself is depth 0; skipping `.git`, `__pycache__`, `*.egg-info`, `.venv`, `node_modules`):
   - any `Cargo.toml` (including at the root) → `Native { AdjacentNativeSource { CargoToml } }`
   - any `meson.build` → hit
   - any `CMakeLists.txt` → hit
   - any file with extension in `{c, cpp, cc, cxx, pyx}` → hit
   - First hit wins (file-system traversal order is sorted within each directory for determinism).
5. Otherwise → `PurePython { backend }` with the backend from step 2.

The walk constants (depth bound, exclusion list, file-extension set) are private to the module, documented with a doc-comment, and not currently exposed as configuration.

### Manifest

```rust
// src/sdist/manifest.rs
pub struct Manifest {
    pub version: u32,                 // currently 1
    pub entries: Vec<ManifestEntry>,  // sorted by (package, version)
}

pub struct ManifestEntry {
    pub package: String,
    pub version: String,
    pub sdist_sha256: String,         // hex, sans `sha256:` prefix
    pub classification: ManifestClassification,
}

pub enum ManifestClassification {
    PurePython {
        backend: AllowlistedBackend,
        wheel_filename: String,        // e.g. "tomli-2.0.1-py3-none-any.whl"
        wheel_sha256: String,
    },
    Native {
        reason: String,                // human-readable; e.g. "AdjacentNativeSource:Cargo.toml@<rel-path>"
    },
}
```

Serialized via `toml`/`toml_edit` (already on the crate list from S0). One `[[entries]]` table per package. Stable serialization order (entries sorted on save; floor for ties: package then version lexicographic).

Example:

```toml
# @generated by muntjac vendor
version = 1

[[entries]]
package = "pillow"
version = "11.0.0"
sdist_sha256 = "deadbeef..."
classification = "native"
native_reason = "AdjacentNativeSource:CExt@src/_imaging.c"

[[entries]]
package = "tomli"
version = "2.0.1"
sdist_sha256 = "feedface..."
classification = "pure-python"
backend = "flit-core"
wheel_filename = "tomli-2.0.1-py3-none-any.whl"
wheel_sha256 = "cafef00d..."
```

### WheelSource (in `src/buck/emit.rs`)

```rust
pub enum WheelSource {
    Remote { url: String, sha256: String },
    Prebake { rel_path: String, sha256: String },  // path relative to `<third_party_dir>/prebake/`
}
```

The wheel-matrix solver gains a new outcome path: when a package has no wheels in `uv.lock` and the manifest entry is `pure-python`, every cell of the matrix resolves to the same `WheelSource::Prebake` referencing the manifest's `wheel_filename` and `wheel_sha256`. Pure-python wheels are universal across `(platform, python_version)` cells.

When the manifest entry is `native`, the matrix solver returns a new `PickResult::NativeSdist` variant per affected cell. The emitter's existing `NoWheel` branch in `build_emit_input` now becomes "`NoWheel` OR `NativeSdist`" and dispatches to the right error message (see §6).

### SdistError

```rust
// src/sdist/error.rs
pub enum SdistError {
    Download { package: String, version: String, url: String, source: reqwest::Error },
    HashMismatch { package: String, version: String, expected: String, actual: String },
    Extract { package: String, version: String, source: std::io::Error },
    Classify(ClassifyError),
    UvNotFound,
    PrebakeFailed { package: String, version: String, stderr: String },
    PrebakeOutputUnexpected { package: String, version: String, found: Vec<String> },  // 0 or 2+ wheels
    ManifestWrite { path: PathBuf, source: std::io::Error },
    ManifestParse { path: PathBuf, source: toml::de::Error },
}
```

`PrebakeOutputUnexpected.found` carries the actual filenames so the error reports either "no wheels produced" or "produced N wheels: …".

---

## 5. Pipeline (`muntjac vendor`)

```
muntjac vendor [--frozen] [--offline]
```

Flow:

1. **Lock freshness.** Compare `pyproject.toml` mtime to `uv.lock` mtime. If pyproject is newer and neither `--frozen` nor `--offline` is set, shell out `uv lock` (single subprocess; inherits stdin/stdout/stderr). `--offline` implies `--frozen`.
2. **Parse `uv.lock`** via existing `lock::parser`.
3. **Identify sdist-only packages.** Walk the lockfile for `(package, version)` pairs where `sdist.is_some()` and `wheels.is_empty()`. (Registry-source packages only; first-party `[[package]]` entries with `source.editable` are skipped — they're handled by S3's first-party path.)
4. **Per sdist-only package:**
   a. Download `sdist.url` to a tempdir (`tempfile::tempdir()` per package, drops on success). Reqwest blocking client, no streaming sha check; full body to disk then verified against `sdist.hash`.
   b. Extract tarball via `flate2::read::GzDecoder` + `tar::Archive`. Refuse to extract files whose normalized path escapes the extraction root (path-traversal hardening — single check in the unpack loop).
   c. `classifier::classify(extract_root)` →
      - `Ok(PurePython { backend })` → step d.
      - `Ok(Native { reason })` → write a native manifest entry; **do not** error at vendor time. (`-v` logs the verdict.)
      - `Err(_)` → propagate.
   d. `prebake::build_wheel(extract_root, staging_dir)` → shell `uv build --wheel --out-dir <staging_dir> <extract_root>`. Validate exactly one `*.whl` in `staging_dir`; sha256 it.
   e. Move the wheel from staging to `<third_party_dir>/python/prebake/<wheel_filename>` (replace if present — sha256 mismatch on a re-run is fine; the new one wins).
5. **Write the manifest.** Sort entries by `(package, version)`. Render to `<third_party_dir>/python/prebake/.manifest.toml`.
6. **Write a `.gitignore`** at `<third_party_dir>/python/prebake/.gitignore` with body `*` (first run only — never overwrite). Documents the default posture; users who want to commit wheels can replace it.
7. Emit one-line summaries to stderr per processed sdist:

```
prebaked: tomli 2.0.1 → third-party/python/prebake/tomli-2.0.1-py3-none-any.whl
skipped:  pillow 11.0.0 (native; will error at buckify if no wheel matches)
```

### Invariants

- **Vendor is idempotent.** Re-running with the same `uv.lock` produces the same manifest + wheels. The sha256 check on prebake output is the canary: if `uv build` ever produces a different wheel sha for the same input, we want to know.
- **Vendor only writes inside `<third_party_dir>/python/prebake/`** — with the single exception of step 1 (the optional `uv lock` shellout, which writes `uv.lock` in the project root). No other paths in the user's tree are touched.
- **No partial state on failure.** A failed prebake aborts the run before manifest write; the previous manifest stays intact. (Future improvement: atomic manifest rename. v1 just writes; staleness errors at buckify time cover the partial-write window.)
- **`uv` discovery is path-only.** Probe `which uv` (via `std::process::Command::new("uv").arg("--version")` round-trip); if exit fails, return `SdistError::UvNotFound` with a clear "install uv" message. No bundling, no PEP 517 fallback.

---

## 6. BUCK emit shape

### Generated `BUCK` (prebaked package)

```python
##
## @generated by muntjac
## Do not edit by hand.
##

load("//third-party/python:muntjac.bzl", "pypi_package")

pypi_package(
    name    = "tomli",
    version = "2.0.1",
    deps    = [],
    wheels  = {
        "py311-linux-x86_64-gnu":  ("prebake:tomli-2.0.1-py3-none-any.whl", "sha256:cafef00d..."),
        "py311-linux-aarch64-gnu": ("prebake:tomli-2.0.1-py3-none-any.whl", "sha256:cafef00d..."),
        "py311-linux-x86_64-musl": ("prebake:tomli-2.0.1-py3-none-any.whl", "sha256:cafef00d..."),
        "py311-macos-x86_64":      ("prebake:tomli-2.0.1-py3-none-any.whl", "sha256:cafef00d..."),
        "py311-macos-arm64":       ("prebake:tomli-2.0.1-py3-none-any.whl", "sha256:cafef00d..."),
        "py312-linux-x86_64-gnu":  ("prebake:tomli-2.0.1-py3-none-any.whl", "sha256:cafef00d..."),
        # ... all cells map to the same tuple
    },
    visibility = ["PUBLIC"],
)
```

The `prebake:` scheme is a muntjac convention parsed by the macro. Everything after the colon is a filename inside `<third_party_dir>/python/prebake/`.

### Generated `muntjac.bzl` (S5 form)

```python
##
## @generated by muntjac
##

load("@prelude//python:python_library.bzl", "prebuilt_python_library")
load("@prelude//utils:utils.bzl", "expect")

_CONFIGS = [
    "py311-linux-x86_64-gnu", "py311-linux-aarch64-gnu", "py311-linux-x86_64-musl",
    "py311-macos-x86_64",     "py311-macos-arm64",
    "py312-linux-x86_64-gnu", "py312-linux-aarch64-gnu", "py312-linux-x86_64-musl",
    "py312-macos-x86_64",     "py312-macos-arm64",
]

def pypi_package(name, version, wheels, deps = [], visibility = None, **kwargs):
    expect(set(wheels.keys()).issubset(set(_CONFIGS)),
           "unknown config in {}".format(name))

    # Deduplicate sources by (src, sha) — pure-python wheels often span all cfgs
    # with the same content; we want one http_file/export_file rule, N library
    # variants. Index is assigned in iteration order (cfgs are sorted upstream).
    seen_sources = {}
    src_targets = {}
    for cfg in sorted(wheels.keys()):
        src, sha256 = wheels[cfg]
        sha = sha256.removeprefix("sha256:")
        key = (src, sha)
        if key in seen_sources:
            src_targets[cfg] = seen_sources[key]
            continue
        target = "{}-{}-src-{}".format(name, version, len(seen_sources))
        seen_sources[key] = target
        src_targets[cfg] = target

        if src.startswith("prebake:"):
            rel = src[len("prebake:"):]
            native.export_file(
                name = target,
                src  = "prebake/{}".format(rel),
                visibility = [],
            )
        else:
            http_file(
                name = target,
                sha256 = sha,
                urls = [src],
                visibility = [],
            )

    for cfg in sorted(wheels.keys()):
        prebuilt_python_library(
            name = "{}-{}__{}".format(name, version, cfg),
            binary_src = ":{}".format(src_targets[cfg]),
            deps = deps,
            visibility = [],
        )

    native.alias(
        name = "{}-{}".format(name, version),
        actual = select({
            "//third-party/python/config:{}".format(cfg): ":{}-{}__{}".format(name, version, cfg)
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

Differences vs. the S4 macro:

- Source-target rules are emitted once per unique `(src, sha)` (was: once per cfg, often N duplicates).
- Source targets are named `<name>-<version>-src-<N>` with `N` assigned in sorted-cfg iteration order. This is deterministic and survives macro re-runs.
- `prebake:`-prefixed sources route to `native.export_file` referencing a path under `prebake/`.
- The downstream `prebuilt_python_library` per-cfg loop is unchanged in shape; it just references the deduped source-target name.

The dedup is a strict win even without prebake — multiple cfgs can share the same `py3-none-any` PyPI wheel today and S4's macro currently emits one `http_file` per cfg in that case (10 identical fetches in the worst case). After S5, that collapses to one.

### Existing fixtures

S0–S4 fixtures all need their goldens regenerated to reflect the new source-target dedup. Snapshots will diff; regeneration happens in the implementation phase. **No semantic change to the produced binary** — the same prebuilt_python_library targets are still emitted, with the same names and the same binary_src; only the source-target rule count and names shift.

---

## 7. Native-sdist error path

The error message format, byte-for-byte from design-spec §6 with substitution:

```
error: {pkg} {ver} has no wheel for ({py_slug}, {platform_slug}) and is a native sdist.
       add a fixup at third-party/python/fixups/{pkg}/fixups.toml — see
       `muntjac fixups show {pkg}` for the current community fixup, or use
       `replace_deps` to point at a hand-rolled Buck target.
```

Slug formats reuse the emitter's cfg slug parts:
- `py_slug` is `py3XX` (e.g. `py312`).
- `platform_slug` is the platform half of `_CONFIGS` (e.g. `linux-x86_64-musl`, `macos-arm64`).

When a sdist-only package's manifest entry is `classification = "native"` and the wheel matrix fails to produce a wheel for one or more cells, **one error is emitted per affected cell** in lexicographic `(python_version, platform)` order. This matches the precision principle from design-spec §5 ("a failure names the exact `(package, version, platform, python_version)` tuple"). For a package that fails on all 10 cells, the user sees 10 lines — clear about the surface area.

Fixture `09-native-sdist-error` asserts the first error line (one cell's worth) byte-for-byte against a golden file.

Note on forward-references: `muntjac fixups show <pkg>` ships in S6; `replace_deps` ships in S6/S7. Both are referenced in the error today on the rationale that (a) by the time a user trips this in real use, S6 will be available, and (b) the fixture is internal to muntjac dev. If S6 slips materially, this paragraph gets revisited.

### Staleness errors

Distinct from the native-sdist case:

```
error: pure-python sdist {pkg} {ver} not prebaked. Run `muntjac vendor` first.
```

```
error: prebake of {pkg} {ver} is stale (sdist sha changed in uv.lock). Run `muntjac vendor`.
```

These fire at buckify time when:
- The lockfile has a sdist-only package and the manifest has no entry for it. (Could be: vendor never ran, or a freshly added dep.)
- The manifest has an entry but `sdist_sha256` differs from `uv.lock`'s sdist hash. (Could be: uv resolved to a different version, or upstream sdist was re-uploaded.)

In both cases, the error names the package and version, and points at `muntjac vendor` as the action. Distinct from the native-sdist message, since the resolution is different.

---

## 8. Testing

### Unit tests

- **`sdist::classifier`** — synthetic-tree fixtures under `tests/fixtures/sdist/classify/<case>/`. One directory per case containing the file tree the classifier should walk. Cases:
  - PurePython, one per backend: `flit_core`, `hatchling`, `setuptools_pure`, `poetry_core`, `pdm_backend`.
  - Native: `missing_pyproject`, `unknown_backend`, `setuptools_ext_modules`, `cargo_toml`, `meson_build`, `cmakelists`, `c_ext`, `cpp_ext`, `pyx_ext`.
  - Edge: `excluded_dir_has_native_source` (verifies the walk skipper — a `.git/Cargo.toml` doesn't trigger).
  - ~13 cases. Cheap to extend.

- **`sdist::prebake`** — one test that takes an in-tree synthetic Python package (a 20-line `pyproject.toml` + `src/synthtoml/__init__.py` using flit-core), packs it into a tarball at test setup with `tar`+`flate2`, then runs `prebake::build_wheel` and asserts a wheel is produced with a stable sha. **Skipped (not failed) when `uv` is not on `$PATH`** — same posture as the existing buck e2e tests.

- **`sdist::manifest`** — TOML roundtrip: build a `Manifest` with mixed pure-python + native entries, serialize, parse, assert structural equality. Plus a "sort order on save" test.

- **`buck::emit`** staleness paths — `build_emit_input` with a synthetic lockfile and a synthetic manifest: missing entry → `NotPrebaked` error; sdist-sha mismatch → `StalePrebake` error.

### Snapshot tests (insta)

- **`tests/fixtures/buck/04-pure-python-sdist/`** — real fixture using `tomli 2.0.1` (flit-core backend, ~14 KB sdist, zero runtime deps). `tomli` ships both an sdist and a wheel on PyPI; the fixture's `uv.lock` is hand-crafted to omit the wheel entry, forcing the sdist path. (Justification for hand-crafted lockfile: the test is verifying muntjac's behavior on sdist-only input, not uv's resolution behavior. The lockfile shape is still valid TOML uv would produce given a wheel-less universe.) Fixture contents:
  - `pyproject.toml` + `uv.lock` (hand-crafted to be wheel-less)
  - `muntjac.toml`
  - `third-party/python/prebake/.manifest.toml` (committed)
  - `third-party/python/prebake/tomli-2.0.1-py3-none-any.whl` (committed)
  - `goldens/BUCK`, `goldens/muntjac.bzl`, `goldens/wiring.bzl`, `goldens/config/BUCK`

  Snapshot tests run `muntjac buckify` and assert the goldens. The buckify step does **not** invoke uv — the manifest + wheel are present.

- **`tests/fixtures/buck/09-native-sdist-error/`** — fixture with a sdist-only package and a manifest entry of `classification = "native"`. The lockfile has the package on all 10 cells with no wheels. Test asserts the first error line byte-for-byte against `goldens/error.txt`.

- **Regenerated goldens for `01-pure-python`, `02-numpy-pandas`, `03-musllinux`** — source-target dedup changes the macro and the per-cfg `binary_src` references. No semantic change. Updated in the same commit that lands the macro change.

### End-to-end (skipped when `uv` not on PATH)

- **`tests/vendor_e2e.rs`** — runs `muntjac vendor` against a sibling fixture variant `04-pure-python-sdist-vendor-input/` which carries just `pyproject.toml`, `muntjac.toml`, and a `uv.lock` whose `sdist.url` points at a local httpserver (e.g. `http://127.0.0.1:<port>/tomli-2.0.1.tar.gz`). The test starts an httpserver (via `httpmock` or `wiremock`) serving the actual upstream sdist tarball from a committed `seed/tomli-2.0.1.tar.gz`, rewrites the URL into the lockfile at test setup, then runs vendor. Asserts the resulting manifest entry, wheel filename, and wheel sha match what the committed `04-pure-python-sdist/` ships.

  The local-httpserver indirection avoids depending on PyPI's CDN in CI. The committed `seed/` sdist is the actual upstream tarball, sha-pinned by `uv.lock`.

### CI workflow extension

The existing buck2 smoke workflow (`.github/workflows/buck-smoke.yml` from S4) gets a second target on `ubuntu-latest`:

```yaml
- name: buck2 build prebaked sdist smoke
  run: buck2 run //tests/smoke:prebake_demo
```

Where `tests/smoke/prebake_demo` is a hand-written `python_binary` under `tests/fixtures/buck/04-pure-python-sdist/tests/smoke/` running `import <pkg>; print(<pkg>.__version__)`. macOS + ARM smoke not extended in S5 — Linux x86_64 is enough to prove the prebake roundtrip; cross-platform is implicit (pure-python wheels are universal).

---

## 9. Exit criteria

1. `muntjac vendor` against fixture `04-pure-python-sdist-vendor-input/` produces a `prebake/.manifest.toml` and a built wheel that byte-match the committed forms in `04-pure-python-sdist/`.
2. `muntjac buckify` against `04-pure-python-sdist/` emits BUCK + muntjac.bzl that match committed goldens; the BUCK uses `prebake:` URLs and the bzl emits `native.export_file` for the prebake source.
3. Fixture `09-native-sdist-error/` runs `muntjac buckify` and the first error line matches `goldens/error.txt` byte-for-byte after substitution.
4. The classifier unit corpus covers all 5 allowlisted backends and all `NativeReason` variants; all unit tests pass.
5. CI on `ubuntu-latest` runs `buck2 run //tests/smoke:prebake_demo` against `04-pure-python-sdist/` and exits 0.
6. `muntjac buckify --check` on a tree with a missing or sdist-sha-stale manifest entry exits non-zero with a clear "run `muntjac vendor`" message.
7. Source-target dedup is in place; regenerated `01–03` goldens land in the same commit; no `prebuilt_python_library` semantic change (same names, same `binary_src` indirection).
8. TECH_DEBT.md `cp313t` entry's target stage is updated to reflect that S5 explicitly did not fold it in.

---

## 10. Risks & open questions

1. **`uv build` semantics drift.** `uv` is under active development; a future release could change the meaning of `--wheel` or `--out-dir`. Mitigation: pin a uv version range at implementation time and let CI fail loudly on bumps. (Pin location: `README.md` "requires uv ≥ X.Y" + a runtime check in `src/uv.rs` that probes `uv --version`.)
2. **Single-wheel assumption.** `uv build --wheel` on a pure-python sdist produces exactly one wheel today. If it ever produces multiple (e.g. plat-tagged + universal), `PrebakeOutputUnexpected` fires and we revisit. Not handled at v0.1.0.
3. **Backend version constraints.** Some allowlisted backends (`setuptools < 60`) lack PEP 517 isolation features. `uv build` handles its own isolation; if a sdist's `[build-system].requires` pins an older setuptools that doesn't work, the prebake errors out and the user sees `PrebakeFailed { stderr }` — they can then write a fixup (S6) or pin a different version.
4. **`replace_community` interaction (S6 forward-look).** When fixups land, a fixup could declare `exclude_wheels = ["*"]` to force a sdist build even when wheels exist. The prebake path already handles this because the wheel matrix produces `Prebake` whenever there's no usable wheel — no S5 code change needed for forward-compat.
5. **Path traversal in tar extraction.** Mitigated by the in-loop check (paths normalized; any `..` escaping the root rejects). Worth a dedicated unit test against a hand-crafted malicious tarball.
6. **Concurrent `muntjac vendor` runs.** v1 makes no lock. Two concurrent invocations could clobber each other's manifest writes. Acceptable — same posture as `cargo` / `uv` itself; a file-lock can be added later if it bites.
7. **Network use in `uv lock` shellout.** The `--frozen` and `--offline` flags propagate to uv. Without them, vendor will attempt a network call to refresh the lockfile if stale.

---

## 11. Stage handoff

After S5 ships:

- `muntjac vendor` is a real command (sdist-only scope; wheel caching remains a S9 deliverable).
- The fixup engine in S6 can layer `[sdist]` block fields on top of the prebake invocation — they're already in the v1 schema (design-spec §7) but inert until wired.
- The `04-pure-python-sdist` fixture remains in the corpus through S6/S7 to verify prebake survives fixup application.
- The `09-native-sdist-error` fixture's error text is the contract S6 inherits — when `muntjac fixups show <pkg>` ships, the message becomes truthful rather than forward-referencing.
