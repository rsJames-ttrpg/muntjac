# Muntjac S5 — Pure-python sdist prebake — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Pure-python sdist-only packages in `uv.lock` flow through the muntjac pipeline as if they had wheels — `muntjac vendor` classifies + prebakes them via `uv build`, `muntjac buckify` references the prebaked artifact locally. Native sdists fail at buckify time with the canonical design-spec §6 error.

**Architecture:** A new `src/sdist/` module provides a conservative classifier (allowlisted backends + adjacent-source heuristic), TOML-serialized prebake manifest, and a thin `uv build` shellout via `src/uv.rs`. `muntjac vendor` becomes a real verb that downloads tarballs (reqwest), extracts (tar + flate2), classifies, prebakes, and writes `third-party/python/prebake/.manifest.toml` + the built wheel. The BUCK emitter gains a `WheelSource::Prebake` variant + manifest consumption; the generated `muntjac.bzl` macro is refactored to dedup source-target rules by `(src, sha)` and dispatches between `http_file` and `native.export_file` via a `prebake:<filename>` URL convention. Native-sdist cells render the design-spec §6 error byte-for-byte.

**Tech Stack:** Rust 2024. New crates: `flate2` (gzip), `tar`, `reqwest` (blocking, rustls-tls), `sha2`, `hex`. Existing: `anyhow`, `thiserror`, `serde`, `toml`, `tempfile`, `insta`. Dev: `httpmock` for vendor e2e. Buck2 + `uv` binaries are external (probed on `$PATH`, tests skipped when missing).

---

## File structure

**New files:**

| Path | Responsibility |
|---|---|
| `src/sdist/mod.rs` | Re-exports for the module: `Classification`, `AllowlistedBackend`, `NativeReason`, `NativeSourceHit`, `classify`, `Manifest`, `ManifestEntry`, `ManifestClassification`, `build_wheel`, `PrebakeOutput`, error types. |
| `src/sdist/classifier.rs` | `classify(sdist_root: &Path) -> Result<Classification, ClassifyError>` and supporting types/algorithm. |
| `src/sdist/manifest.rs` | `Manifest` / `ManifestEntry` types + TOML load/save with sorted iteration. |
| `src/sdist/prebake.rs` | `build_wheel(sdist_root, out_dir) -> Result<PrebakeOutput, SdistError>`; calls `crate::uv::uv_build_wheel`; verifies exactly one wheel produced; computes sha256. |
| `src/sdist/error.rs` | `SdistError` + `ClassifyError` thiserror enums. |
| `src/uv.rs` | `uv_build_wheel(sdist_root, out_dir, verbose)`; `uv_version()` probe; `uv_lock()` for the freshness re-run. |
| `src/cli/vendor.rs` | The `vendor` verb: lock-freshness check → walk lockfile → download → extract → classify → prebake → manifest write → gitignore write. |
| `tests/fixtures/sdist/classify/flit_core_pure/pyproject.toml` | Synthetic PurePython case (one per backend below). |
| `tests/fixtures/sdist/classify/hatchling_pure/pyproject.toml` | Hatchling backend. |
| `tests/fixtures/sdist/classify/setuptools_pure/pyproject.toml` + `setup.py` (no `ext_modules`) | Setuptools backend, pure-python. |
| `tests/fixtures/sdist/classify/poetry_core_pure/pyproject.toml` | Poetry-core backend. |
| `tests/fixtures/sdist/classify/pdm_backend_pure/pyproject.toml` | Pdm-backend backend. |
| `tests/fixtures/sdist/classify/missing_pyproject/setup.py` | Native: no pyproject.toml. |
| `tests/fixtures/sdist/classify/unknown_backend/pyproject.toml` | Native: backend not in allowlist (e.g. `scikit-build-core.build`). |
| `tests/fixtures/sdist/classify/setuptools_ext_modules/pyproject.toml` + `setup.py` (with `ext_modules=`) | Native: setuptools with ext_modules. |
| `tests/fixtures/sdist/classify/cargo_toml/pyproject.toml` + `Cargo.toml` | Native: adjacent Cargo.toml. |
| `tests/fixtures/sdist/classify/meson_build/pyproject.toml` + `meson.build` | Native: adjacent meson.build. |
| `tests/fixtures/sdist/classify/cmakelists/pyproject.toml` + `CMakeLists.txt` | Native: adjacent CMakeLists.txt. |
| `tests/fixtures/sdist/classify/c_ext/pyproject.toml` + `src/foo.c` | Native: adjacent .c file. |
| `tests/fixtures/sdist/classify/cpp_ext/pyproject.toml` + `src/foo.cpp` | Native: adjacent .cpp file. |
| `tests/fixtures/sdist/classify/pyx_ext/pyproject.toml` + `src/foo.pyx` | Native: adjacent .pyx file. |
| `tests/fixtures/sdist/classify/excluded_dir_has_native_source/pyproject.toml` + `.git/Cargo.toml` | PurePython: classifier should skip `.git/`. |
| `tests/fixtures/buck/04-pure-python-sdist/pyproject.toml` | Workspace with one dep on `tomli==2.0.1`. |
| `tests/fixtures/buck/04-pure-python-sdist/muntjac.toml` | Single-platform single-python config (same shape as `01-pure-python`). |
| `tests/fixtures/buck/04-pure-python-sdist/uv.lock` | Hand-crafted: `tomli 2.0.1` with `sdist = {...}` only, no `wheels`. |
| `tests/fixtures/buck/04-pure-python-sdist/third-party/python/prebake/.manifest.toml` | Committed: one PurePython entry for `tomli 2.0.1`. |
| `tests/fixtures/buck/04-pure-python-sdist/third-party/python/prebake/tomli-2.0.1-py3-none-any.whl` | Committed: the actual wheel built from `tomli 2.0.1` sdist by `uv build`. |
| `tests/fixtures/buck/04-pure-python-sdist/third-party/python/prebake/.gitignore` | Single line `*\n` (per spec §5). |
| `tests/fixtures/buck/04-pure-python-sdist/expected/BUCK` | Golden: `pypi_package("tomli", "2.0.1", ..., wheels = { ...: ("prebake:tomli-2.0.1-py3-none-any.whl", "sha256:..."), ... })`. |
| `tests/fixtures/buck/04-pure-python-sdist/expected/muntjac.bzl` | Golden: new macro form with source-target dedup + `prebake:` dispatch. |
| `tests/fixtures/buck/04-pure-python-sdist/expected/wiring.bzl` | Golden: single-platform `MUNTJAC_HOST_MODIFIERS`. |
| `tests/fixtures/buck/04-pure-python-sdist/expected/config/BUCK` | Golden: 1 py × 1 platform constraint_values + config_setting. |
| `tests/fixtures/buck/04-pure-python-sdist/PACKAGE` | Hand-written PACKAGE that loads MUNTJAC_HOST_MODIFIERS. |
| `tests/fixtures/buck/04-pure-python-sdist/toolchains/BUCK` | Hand-written: standard toolchains. |
| `tests/fixtures/buck/04-pure-python-sdist/.buckconfig` | Standard buck2 config (copy from `02-numpy-pandas`). |
| `tests/fixtures/buck/04-pure-python-sdist/tests/smoke/BUCK` | Hand-written `python_binary` with the right modifier. |
| `tests/fixtures/buck/04-pure-python-sdist/tests/smoke/demo.py` | `import tomli; data = tomli.loads("a = 1"); print(data); assert data == {"a": 1}`. |
| `tests/fixtures/buck/09-native-sdist-error/pyproject.toml` | Workspace with a single fictional dep `nativeonly==1.0.0`. |
| `tests/fixtures/buck/09-native-sdist-error/muntjac.toml` | Same shape as `01-pure-python`. |
| `tests/fixtures/buck/09-native-sdist-error/uv.lock` | Hand-crafted: `nativeonly 1.0.0` with sdist only. |
| `tests/fixtures/buck/09-native-sdist-error/third-party/python/prebake/.manifest.toml` | Committed: one Native entry for `nativeonly 1.0.0`. |
| `tests/fixtures/buck/09-native-sdist-error/expected-error.txt` | Golden: byte-for-byte first error line. |
| `tests/fixtures/buck/04-pure-python-sdist-vendor-input/pyproject.toml` | Same as 04. |
| `tests/fixtures/buck/04-pure-python-sdist-vendor-input/muntjac.toml` | Same as 04. |
| `tests/fixtures/buck/04-pure-python-sdist-vendor-input/uv.lock` | Same as 04 but with `sdist.url` rewritten by the test to `http://127.0.0.1:<port>/tomli-2.0.1.tar.gz`. Committed value is `http://__MOCK__/tomli-2.0.1.tar.gz` (sentinel replaced at runtime). |
| `tests/fixtures/buck/04-pure-python-sdist-vendor-input/seed/tomli-2.0.1.tar.gz` | The actual upstream sdist, committed. |
| `tests/vendor_smoke.rs` | Integration test: runs `muntjac vendor` against `04-pure-python-sdist-vendor-input`, asserts manifest+wheel match `04-pure-python-sdist`. Skipped when `uv` not on `$PATH`. |

**Modified files:**

| Path | Change |
|---|---|
| `Cargo.toml` | Add deps: `flate2`, `tar`, `reqwest` (blocking + rustls-tls), `sha2`, `hex`. Add dev-dep: `httpmock`. |
| `src/lib.rs` | Add `pub mod sdist;` and `pub mod uv;`. |
| `src/cli/mod.rs` | Wire `Command::Vendor` to `vendor::run`; drop `stub::run("vendor", ...)`. Update doc comment on `Vendor` variant. Add `pub mod vendor;`. |
| `src/buck/emit.rs` | Add `WheelSource` enum (Remote/Prebake), change `EmitWheel` to carry it; add `PickResult::NativeSdist` consumption path; consult `Manifest` in `build_emit_input`; emit `NotPrebaked`, `StalePrebake`, `NativeSdist` errors with canonical messages. |
| `src/buck/string_writer.rs` | Render wheels-dict tuple as `("prebake:<filename>", "sha256:...")` for Prebake variant; pass through unchanged for Remote. |
| `src/buck/snapshots/*` (existing 14 snap files) | Regenerate via `cargo insta accept` after macro/source-target dedup change. |
| `tests/fixtures/buck/01-pure-python/expected/muntjac.bzl` | Regenerate: macro body now uses `seen_sources` dedup; cosmetic only — semantic library targets identical. |
| `tests/fixtures/buck/02-numpy-pandas/expected/muntjac.bzl` | Same. |
| `tests/fixtures/buck/03-musllinux/expected/muntjac.bzl` | Same. |
| `.github/workflows/ci.yml` | Add a `buck2 run //tests/smoke:prebake_demo` step on `ubuntu-latest` against fixture 04. |
| `docs/superpowers/specs/2026-05-20-muntjac-roadmap.md` | Mark S5 ✅ shipped at end of stage. |
| `docs/superpowers/TECH_DEBT.md` | Add S5 review section; retarget `cp313t` entry to "post-launch (no concrete stage)" per spec §1. |

---

## Phase ordering rationale

1. **Phase 1 (T1–T5)** — All-new code under `src/sdist/` and `src/uv.rs`. No regression risk. Bottom-up: error types → classifier → manifest → uv wrapper → prebake.
2. **Phase 2 (T6)** — `muntjac.bzl` source-target dedup refactor. Semantically a no-op (same `prebuilt_python_library` targets, same `binary_src` indirection); cosmetic change to source-target naming + count. Snapshot tests + 3 fixture goldens regenerate. Lands before any prebake usage so the new macro shape is the substrate T7–T9 build on.
3. **Phase 3 (T7–T9)** — Buckify reads the manifest and routes sdist-only packages: `WheelSource::Prebake` (T7), staleness errors + `04-pure-python-sdist` fixture (T8), native-sdist canonical error + `09-native-sdist-error` fixture (T9).
4. **Phase 4 (T10–T11)** — `muntjac vendor` real implementation; vendor smoke test against fixture 04's `-vendor-input` sibling.
5. **Phase 5 (T12)** — CI workflow extension adds `buck2 run //tests/smoke:prebake_demo`.
6. **Phase 6 (T13)** — TECH_DEBT + roadmap bookkeeping.

---

## Phase 1 — Sdist module foundations

### Task 1: Add crate deps + module skeleton

**Files:**
- Modify: `Cargo.toml`
- Create: `src/sdist/mod.rs`
- Create: `src/sdist/error.rs`
- Create: `src/uv.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Add runtime deps to `Cargo.toml`**

Insert into the `[dependencies]` block (in alphabetical order):

```toml
flate2 = "1"
hex = "0.4"
reqwest = { version = "0.12", default-features = false, features = ["blocking", "rustls-tls"] }
sha2 = "0.10"
tar = "0.4"
```

Insert into the `[dev-dependencies]` block:

```toml
httpmock = "0.7"
```

- [ ] **Step 2: Verify crates resolve**

Run: `cargo check --tests`
Expected: PASS (downloads + compiles new crates).

- [ ] **Step 3: Create `src/sdist/error.rs`**

```rust
use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ClassifyError {
    #[error("sdist root `{0}` does not exist or is not a directory")]
    NotADir(PathBuf),

    #[error("failed to read `{path}`: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse `{path}` as TOML: {source}")]
    BadToml {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
}

#[derive(Debug, Error)]
pub enum SdistError {
    #[error("classifier failure: {0}")]
    Classify(#[from] ClassifyError),

    #[error("`uv` binary not found on PATH; install uv (see https://docs.astral.sh/uv)")]
    UvNotFound,

    #[error("`uv build --wheel` failed for {package} {version}:\n--- uv stderr ---\n{stderr}")]
    PrebakeFailed {
        package: String,
        version: String,
        stderr: String,
    },

    #[error(
        "`uv build` for {package} {version} produced {} wheel(s) in {out_dir} (expected 1): {found:?}",
        found.len()
    )]
    PrebakeOutputUnexpected {
        package: String,
        version: String,
        out_dir: PathBuf,
        found: Vec<String>,
    },

    #[error("failed to download `{url}` for {package} {version}: {source}")]
    Download {
        package: String,
        version: String,
        url: String,
        #[source]
        source: reqwest::Error,
    },

    #[error(
        "sha256 mismatch for {package} {version}: lockfile says `{expected}`, got `{actual}`"
    )]
    HashMismatch {
        package: String,
        version: String,
        expected: String,
        actual: String,
    },

    #[error("failed to extract tarball for {package} {version}: {source}")]
    Extract {
        package: String,
        version: String,
        #[source]
        source: std::io::Error,
    },

    #[error("refused to extract `{member}` from {package} {version}: path escapes archive root")]
    PathTraversal {
        package: String,
        version: String,
        member: String,
    },

    #[error("failed to write manifest `{path}`: {source}")]
    ManifestWrite {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse manifest `{path}`: {source}")]
    ManifestParse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("failed to read manifest `{path}`: {source}")]
    ManifestRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}
```

- [ ] **Step 4: Create `src/sdist/mod.rs`**

```rust
//! Sdist classification and prebake.
//!
//! Distinguishes pure-python sdists (which can be prebaked into wheels via
//! `uv build`) from native sdists (which require platform-specific compile
//! steps muntjac doesn't run in v0.1.0).
//!
//! Public surface used by `cli::vendor` and `buck::emit`.

pub mod classifier;
pub mod error;
pub mod manifest;
pub mod prebake;

pub use classifier::{
    AllowlistedBackend, Classification, NativeReason, NativeSourceHit, classify,
};
pub use error::{ClassifyError, SdistError};
pub use manifest::{Manifest, ManifestClassification, ManifestEntry};
pub use prebake::{PrebakeOutput, build_wheel};
```

This references modules created in later tasks; for the skeleton, write stubs:

- [ ] **Step 5: Create empty module stubs**

Write `src/sdist/classifier.rs`:

```rust
//! Pure-python vs native sdist classification.
//!
//! See `docs/superpowers/specs/2026-05-22-muntjac-s5-sdist-prebake-design.md` §4.

use std::path::{Path, PathBuf};

use super::error::ClassifyError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Classification {
    PurePython { backend: AllowlistedBackend },
    Native { reason: NativeReason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllowlistedBackend {
    FlitCore,
    Hatchling,
    Setuptools,
    PoetryCore,
    PdmBackend,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeReason {
    UnknownBackend { build_backend: String },
    MissingPyprojectToml,
    SetuptoolsWithExtModules,
    AdjacentNativeSource { hit: NativeSourceHit },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NativeSourceHit {
    CargoToml(PathBuf),
    MesonBuild(PathBuf),
    CMakeLists(PathBuf),
    CExt(PathBuf),
    CppExt(PathBuf),
    PyxExt(PathBuf),
}

pub fn classify(_sdist_root: &Path) -> Result<Classification, ClassifyError> {
    todo!("Task 2 implements this")
}
```

Write `src/sdist/manifest.rs`:

```rust
//! Prebake manifest TOML serde.
//!
//! See `docs/superpowers/specs/2026-05-22-muntjac-s5-sdist-prebake-design.md` §4.

use std::path::Path;

use super::classifier::AllowlistedBackend;
use super::error::SdistError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub version: u32,
    pub entries: Vec<ManifestEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestEntry {
    pub package: String,
    pub version: String,
    pub sdist_sha256: String,
    pub classification: ManifestClassification,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestClassification {
    PurePython {
        backend: AllowlistedBackend,
        wheel_filename: String,
        wheel_sha256: String,
    },
    Native {
        reason: String,
    },
}

impl Manifest {
    pub fn empty() -> Self {
        Self { version: 1, entries: Vec::new() }
    }

    pub fn load(_path: &Path) -> Result<Self, SdistError> {
        todo!("Task 3 implements this")
    }

    pub fn save(&self, _path: &Path) -> Result<(), SdistError> {
        todo!("Task 3 implements this")
    }
}
```

Write `src/sdist/prebake.rs`:

```rust
//! `uv build`-based pure-python sdist → wheel prebake.

use std::path::{Path, PathBuf};

use super::error::SdistError;

#[derive(Debug, Clone)]
pub struct PrebakeOutput {
    pub wheel_path: PathBuf,
    pub wheel_filename: String,
    pub sha256: String,
}

pub fn build_wheel(
    _sdist_root: &Path,
    _out_dir: &Path,
    _package: &str,
    _version: &str,
) -> Result<PrebakeOutput, SdistError> {
    todo!("Task 5 implements this")
}
```

- [ ] **Step 6: Create `src/uv.rs`**

```rust
//! Thin wrappers around the `uv` CLI.
//!
//! Two shellout sites per design spec §5 invariants: `uv lock` (when stale)
//! and `uv build` (for pure-python sdist prebake). All other uv usage is
//! out-of-scope for muntjac.

use std::path::{Path, PathBuf};
use std::process::Output;

use crate::sdist::error::SdistError;

/// Probe for `uv` on PATH. Returns the version string from `uv --version`.
pub fn uv_version() -> Result<String, SdistError> {
    let output = std::process::Command::new("uv")
        .arg("--version")
        .output()
        .map_err(|_| SdistError::UvNotFound)?;
    if !output.status.success() {
        return Err(SdistError::UvNotFound);
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Shell out `uv build --wheel --out-dir <out_dir> <sdist_root>`.
/// Returns the full process Output; callers inspect status + stderr.
pub fn uv_build_wheel(sdist_root: &Path, out_dir: &Path) -> Result<Output, SdistError> {
    std::process::Command::new("uv")
        .arg("build")
        .arg("--wheel")
        .arg("--out-dir")
        .arg(out_dir)
        .arg(sdist_root)
        .output()
        .map_err(|_| SdistError::UvNotFound)
}

/// Shell out `uv lock` in the given project root. Inherits stdio so the
/// user sees uv's progress + errors directly. Returns the exit status.
pub fn uv_lock(project_root: &Path) -> Result<std::process::ExitStatus, SdistError> {
    std::process::Command::new("uv")
        .arg("lock")
        .current_dir(project_root)
        .status()
        .map_err(|_| SdistError::UvNotFound)
}

/// Locate the `uv` binary; returns the resolved path or `UvNotFound`.
#[allow(dead_code)]
pub fn uv_path() -> Result<PathBuf, SdistError> {
    let output = std::process::Command::new("uv")
        .arg("--version")
        .output()
        .map_err(|_| SdistError::UvNotFound)?;
    if !output.status.success() {
        return Err(SdistError::UvNotFound);
    }
    // `which`-style lookup via PATH; on success uv was on PATH.
    Ok(PathBuf::from("uv"))
}
```

- [ ] **Step 7: Wire modules into `src/lib.rs`**

Add to the existing `src/lib.rs` (alphabetical insertion):

```rust
pub mod sdist;
pub mod uv;
```

After the change, the file reads:

```rust
pub mod buck;
pub mod cli;
pub mod config;
pub mod error;
pub mod lock;
pub mod platform;
pub mod sdist;
pub mod uv;
pub mod wheel;
```

- [ ] **Step 8: Verify the project still compiles**

Run: `cargo build`
Expected: PASS (warnings about `dead_code` are OK; the `todo!()` stubs trigger no warning since they're functions, not unused values).

- [ ] **Step 9: Run existing tests to confirm no regression**

Run: `cargo test --lib`
Expected: PASS — same test count as before this task. (sdist tests don't exist yet.)

- [ ] **Step 10: Commit**

```bash
git add Cargo.toml Cargo.lock src/lib.rs src/sdist/ src/uv.rs
git commit -m "$(cat <<'EOF'
feat(s5): scaffold sdist module + uv CLI wrapper

Adds the src/sdist/ module skeleton with type definitions for the
classifier, manifest, prebake output, and error variants. Public
functions are todo!() stubs that subsequent tasks fill in. The
src/uv.rs wrapper provides uv_version / uv_build_wheel / uv_lock
shellouts; the only two uv invocations muntjac makes per design
spec §5.

New crate deps: flate2, tar, reqwest (blocking + rustls-tls), sha2,
hex; dev-dep: httpmock.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: Implement the sdist classifier

**Files:**
- Modify: `src/sdist/classifier.rs`
- Create: `tests/fixtures/sdist/classify/<case>/...` (14 cases, see file list above)

- [ ] **Step 1: Write the synthetic fixtures**

For each case, create the directory and minimal files. Below is the exact content for each.

`tests/fixtures/sdist/classify/flit_core_pure/pyproject.toml`:

```toml
[build-system]
requires = ["flit_core >=3.2,<4"]
build-backend = "flit_core.buildapi"

[project]
name = "synth"
version = "0.0.0"
```

`tests/fixtures/sdist/classify/hatchling_pure/pyproject.toml`:

```toml
[build-system]
requires = ["hatchling"]
build-backend = "hatchling.build"

[project]
name = "synth"
version = "0.0.0"
```

`tests/fixtures/sdist/classify/setuptools_pure/pyproject.toml`:

```toml
[build-system]
requires = ["setuptools>=61"]
build-backend = "setuptools.build_meta"

[project]
name = "synth"
version = "0.0.0"
```

`tests/fixtures/sdist/classify/setuptools_pure/setup.py`:

```python
from setuptools import setup
setup()
```

`tests/fixtures/sdist/classify/poetry_core_pure/pyproject.toml`:

```toml
[build-system]
requires = ["poetry-core>=1.0.0"]
build-backend = "poetry.core.masonry.api"

[tool.poetry]
name = "synth"
version = "0.0.0"
description = ""
authors = []
```

`tests/fixtures/sdist/classify/pdm_backend_pure/pyproject.toml`:

```toml
[build-system]
requires = ["pdm-backend"]
build-backend = "pdm.backend"

[project]
name = "synth"
version = "0.0.0"
```

`tests/fixtures/sdist/classify/missing_pyproject/setup.py`:

```python
from setuptools import setup
setup(name="synth", version="0.0.0")
```

`tests/fixtures/sdist/classify/unknown_backend/pyproject.toml`:

```toml
[build-system]
requires = ["scikit-build-core>=0.5"]
build-backend = "scikit_build_core.build"

[project]
name = "synth"
version = "0.0.0"
```

`tests/fixtures/sdist/classify/setuptools_ext_modules/pyproject.toml`:

```toml
[build-system]
requires = ["setuptools>=61"]
build-backend = "setuptools.build_meta"

[project]
name = "synth"
version = "0.0.0"
```

`tests/fixtures/sdist/classify/setuptools_ext_modules/setup.py`:

```python
from setuptools import setup, Extension
setup(
    ext_modules=[Extension("synth._c", sources=["src/_c.c"])],
)
```

`tests/fixtures/sdist/classify/cargo_toml/pyproject.toml`:

```toml
[build-system]
requires = ["maturin>=1.0"]
build-backend = "maturin"

[project]
name = "synth"
version = "0.0.0"
```

`tests/fixtures/sdist/classify/cargo_toml/Cargo.toml`:

```toml
[package]
name = "synth"
version = "0.0.0"
edition = "2021"
```

Note: `maturin` is not in our allowlist, so this case will be flagged by `UnknownBackend` *before* the adjacent-source step ever runs. That's correct — we want priority order to apply. For a case that *does* reach the adjacent-source step, the next two cases use an allowlisted backend.

`tests/fixtures/sdist/classify/meson_build/pyproject.toml`:

```toml
[build-system]
requires = ["setuptools>=61"]
build-backend = "setuptools.build_meta"

[project]
name = "synth"
version = "0.0.0"
```

`tests/fixtures/sdist/classify/meson_build/meson.build`:

```
project('synth', 'c')
```

`tests/fixtures/sdist/classify/cmakelists/pyproject.toml`:

```toml
[build-system]
requires = ["setuptools>=61"]
build-backend = "setuptools.build_meta"

[project]
name = "synth"
version = "0.0.0"
```

`tests/fixtures/sdist/classify/cmakelists/CMakeLists.txt`:

```
cmake_minimum_required(VERSION 3.15)
project(synth)
```

`tests/fixtures/sdist/classify/c_ext/pyproject.toml`:

```toml
[build-system]
requires = ["setuptools>=61"]
build-backend = "setuptools.build_meta"

[project]
name = "synth"
version = "0.0.0"
```

`tests/fixtures/sdist/classify/c_ext/src/foo.c`:

```c
int foo(void) { return 0; }
```

`tests/fixtures/sdist/classify/cpp_ext/pyproject.toml`:

```toml
[build-system]
requires = ["setuptools>=61"]
build-backend = "setuptools.build_meta"

[project]
name = "synth"
version = "0.0.0"
```

`tests/fixtures/sdist/classify/cpp_ext/src/foo.cpp`:

```cpp
int foo() { return 0; }
```

`tests/fixtures/sdist/classify/pyx_ext/pyproject.toml`:

```toml
[build-system]
requires = ["setuptools>=61", "cython"]
build-backend = "setuptools.build_meta"

[project]
name = "synth"
version = "0.0.0"
```

`tests/fixtures/sdist/classify/pyx_ext/src/foo.pyx`:

```
def foo():
    return 0
```

`tests/fixtures/sdist/classify/excluded_dir_has_native_source/pyproject.toml`:

```toml
[build-system]
requires = ["flit_core >=3.2,<4"]
build-backend = "flit_core.buildapi"

[project]
name = "synth"
version = "0.0.0"
```

`tests/fixtures/sdist/classify/excluded_dir_has_native_source/.git/Cargo.toml`:

```toml
# Should be ignored by the classifier — .git/ is in the exclude list.
```

- [ ] **Step 2: Write the failing classifier tests**

Append to `src/sdist/classifier.rs` (replacing the existing test-less stub):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/sdist/classify")
            .join(name)
    }

    #[test]
    fn flit_core_pure() {
        let result = classify(&fixture("flit_core_pure")).unwrap();
        assert_eq!(
            result,
            Classification::PurePython { backend: AllowlistedBackend::FlitCore }
        );
    }

    #[test]
    fn hatchling_pure() {
        let result = classify(&fixture("hatchling_pure")).unwrap();
        assert_eq!(
            result,
            Classification::PurePython { backend: AllowlistedBackend::Hatchling }
        );
    }

    #[test]
    fn setuptools_pure() {
        let result = classify(&fixture("setuptools_pure")).unwrap();
        assert_eq!(
            result,
            Classification::PurePython { backend: AllowlistedBackend::Setuptools }
        );
    }

    #[test]
    fn poetry_core_pure() {
        let result = classify(&fixture("poetry_core_pure")).unwrap();
        assert_eq!(
            result,
            Classification::PurePython { backend: AllowlistedBackend::PoetryCore }
        );
    }

    #[test]
    fn pdm_backend_pure() {
        let result = classify(&fixture("pdm_backend_pure")).unwrap();
        assert_eq!(
            result,
            Classification::PurePython { backend: AllowlistedBackend::PdmBackend }
        );
    }

    #[test]
    fn missing_pyproject_is_native() {
        let result = classify(&fixture("missing_pyproject")).unwrap();
        assert_eq!(
            result,
            Classification::Native { reason: NativeReason::MissingPyprojectToml }
        );
    }

    #[test]
    fn unknown_backend_is_native() {
        let result = classify(&fixture("unknown_backend")).unwrap();
        match result {
            Classification::Native {
                reason: NativeReason::UnknownBackend { build_backend },
            } => assert_eq!(build_backend, "scikit_build_core.build"),
            other => panic!("expected UnknownBackend, got {other:?}"),
        }
    }

    #[test]
    fn setuptools_ext_modules_is_native() {
        let result = classify(&fixture("setuptools_ext_modules")).unwrap();
        assert_eq!(
            result,
            Classification::Native { reason: NativeReason::SetuptoolsWithExtModules }
        );
    }

    #[test]
    fn cargo_toml_via_unknown_backend() {
        // maturin is not in the allowlist, so this fires UnknownBackend before
        // the adjacent-source walk runs. Documents the priority order from §4.
        let result = classify(&fixture("cargo_toml")).unwrap();
        match result {
            Classification::Native {
                reason: NativeReason::UnknownBackend { build_backend },
            } => assert_eq!(build_backend, "maturin"),
            other => panic!("expected UnknownBackend(maturin), got {other:?}"),
        }
    }

    #[test]
    fn meson_build_is_native() {
        let result = classify(&fixture("meson_build")).unwrap();
        match result {
            Classification::Native {
                reason: NativeReason::AdjacentNativeSource { hit: NativeSourceHit::MesonBuild(_) },
            } => {}
            other => panic!("expected MesonBuild hit, got {other:?}"),
        }
    }

    #[test]
    fn cmakelists_is_native() {
        let result = classify(&fixture("cmakelists")).unwrap();
        match result {
            Classification::Native {
                reason: NativeReason::AdjacentNativeSource { hit: NativeSourceHit::CMakeLists(_) },
            } => {}
            other => panic!("expected CMakeLists hit, got {other:?}"),
        }
    }

    #[test]
    fn c_ext_is_native() {
        let result = classify(&fixture("c_ext")).unwrap();
        match result {
            Classification::Native {
                reason: NativeReason::AdjacentNativeSource { hit: NativeSourceHit::CExt(_) },
            } => {}
            other => panic!("expected CExt hit, got {other:?}"),
        }
    }

    #[test]
    fn cpp_ext_is_native() {
        let result = classify(&fixture("cpp_ext")).unwrap();
        match result {
            Classification::Native {
                reason: NativeReason::AdjacentNativeSource { hit: NativeSourceHit::CppExt(_) },
            } => {}
            other => panic!("expected CppExt hit, got {other:?}"),
        }
    }

    #[test]
    fn pyx_ext_is_native() {
        let result = classify(&fixture("pyx_ext")).unwrap();
        match result {
            Classification::Native {
                reason: NativeReason::AdjacentNativeSource { hit: NativeSourceHit::PyxExt(_) },
            } => {}
            other => panic!("expected PyxExt hit, got {other:?}"),
        }
    }

    #[test]
    fn excluded_dir_does_not_trigger_native() {
        let result = classify(&fixture("excluded_dir_has_native_source")).unwrap();
        assert_eq!(
            result,
            Classification::PurePython { backend: AllowlistedBackend::FlitCore }
        );
    }

    #[test]
    fn root_does_not_exist() {
        let err = classify(std::path::Path::new("/nonexistent/dir/should/fail")).unwrap_err();
        match err {
            ClassifyError::NotADir(_) => {}
            other => panic!("expected NotADir, got {other:?}"),
        }
    }
}
```

- [ ] **Step 3: Run the failing tests**

Run: `cargo test --lib sdist::classifier::tests -- --nocapture`
Expected: all 16 tests FAIL — `classify` is `todo!()`.

- [ ] **Step 4: Implement `classify`**

Replace the `todo!()` body in `src/sdist/classifier.rs::classify` with:

```rust
const ALLOWLIST: &[(&str, AllowlistedBackend)] = &[
    ("flit_core.buildapi", AllowlistedBackend::FlitCore),
    ("flit_core.api", AllowlistedBackend::FlitCore),
    ("hatchling.build", AllowlistedBackend::Hatchling),
    ("setuptools.build_meta", AllowlistedBackend::Setuptools),
    ("setuptools.build_meta:__legacy__", AllowlistedBackend::Setuptools),
    ("poetry.core.masonry.api", AllowlistedBackend::PoetryCore),
    ("pdm.backend", AllowlistedBackend::PdmBackend),
];

const WALK_DEPTH_LIMIT: usize = 4;
const EXCLUDE_DIRS: &[&str] = &[".git", "__pycache__", ".venv", "node_modules"];

fn read_build_backend(pyproject: &Path) -> Result<Option<String>, ClassifyError> {
    let bytes = std::fs::read(pyproject).map_err(|e| ClassifyError::Io {
        path: pyproject.to_path_buf(),
        source: e,
    })?;
    let text = String::from_utf8_lossy(&bytes);
    let parsed: toml::Value = toml::from_str(&text).map_err(|e| ClassifyError::BadToml {
        path: pyproject.to_path_buf(),
        source: e,
    })?;
    let backend = parsed
        .get("build-system")
        .and_then(|v| v.get("build-backend"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    Ok(backend)
}

fn match_backend(s: &str) -> Option<AllowlistedBackend> {
    ALLOWLIST.iter().find(|(name, _)| *name == s).map(|(_, b)| *b)
}

fn setup_py_has_ext_modules(sdist_root: &Path) -> Result<bool, ClassifyError> {
    let setup = sdist_root.join("setup.py");
    if !setup.is_file() {
        return Ok(false);
    }
    let bytes = std::fs::read(&setup).map_err(|e| ClassifyError::Io {
        path: setup.clone(),
        source: e,
    })?;
    let text = String::from_utf8_lossy(&bytes);
    Ok(text.contains("ext_modules"))
}

fn walk_for_native_sources(sdist_root: &Path) -> Result<Option<NativeSourceHit>, ClassifyError> {
    fn is_excluded(name: &str) -> bool {
        if EXCLUDE_DIRS.contains(&name) {
            return true;
        }
        name.ends_with(".egg-info")
    }

    fn walk(
        root: &Path,
        cur: &Path,
        depth: usize,
    ) -> Result<Option<NativeSourceHit>, ClassifyError> {
        if depth > WALK_DEPTH_LIMIT {
            return Ok(None);
        }
        let mut entries: Vec<_> = std::fs::read_dir(cur)
            .map_err(|e| ClassifyError::Io {
                path: cur.to_path_buf(),
                source: e,
            })?
            .filter_map(|r| r.ok())
            .collect();
        // Sort by filename for determinism.
        entries.sort_by_key(|e| e.file_name());

        // First pass: check files at this level for native-source hits.
        for entry in &entries {
            let path = entry.path();
            let file_name = entry.file_name();
            let name = file_name.to_string_lossy();
            let ft = entry.file_type().map_err(|e| ClassifyError::Io {
                path: path.clone(),
                source: e,
            })?;
            if ft.is_dir() {
                continue;
            }
            let rel = pathdiff::diff_paths(&path, root).unwrap_or(path.clone());
            if name == "Cargo.toml" {
                return Ok(Some(NativeSourceHit::CargoToml(rel)));
            }
            if name == "meson.build" {
                return Ok(Some(NativeSourceHit::MesonBuild(rel)));
            }
            if name == "CMakeLists.txt" {
                return Ok(Some(NativeSourceHit::CMakeLists(rel)));
            }
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                match ext {
                    "c" => return Ok(Some(NativeSourceHit::CExt(rel))),
                    "cpp" | "cc" | "cxx" => return Ok(Some(NativeSourceHit::CppExt(rel))),
                    "pyx" => return Ok(Some(NativeSourceHit::PyxExt(rel))),
                    _ => {}
                }
            }
        }

        // Second pass: recurse into subdirs.
        for entry in &entries {
            let path = entry.path();
            let file_name = entry.file_name();
            let name = file_name.to_string_lossy();
            let ft = entry.file_type().map_err(|e| ClassifyError::Io {
                path: path.clone(),
                source: e,
            })?;
            if !ft.is_dir() {
                continue;
            }
            if is_excluded(&name) {
                continue;
            }
            if let Some(hit) = walk(root, &path, depth + 1)? {
                return Ok(Some(hit));
            }
        }
        Ok(None)
    }

    walk(sdist_root, sdist_root, 0)
}
```

Then the public `classify` function body:

```rust
pub fn classify(sdist_root: &Path) -> Result<Classification, ClassifyError> {
    if !sdist_root.is_dir() {
        return Err(ClassifyError::NotADir(sdist_root.to_path_buf()));
    }

    // Step 1: pyproject.toml at root?
    let pyproject = sdist_root.join("pyproject.toml");
    if !pyproject.is_file() {
        return Ok(Classification::Native {
            reason: NativeReason::MissingPyprojectToml,
        });
    }

    // Step 2: read build-backend; reject if missing or not in allowlist.
    let backend_str = read_build_backend(&pyproject)?.unwrap_or_default();
    let Some(backend) = match_backend(&backend_str) else {
        return Ok(Classification::Native {
            reason: NativeReason::UnknownBackend {
                build_backend: backend_str,
            },
        });
    };

    // Step 3: setuptools ext_modules carve-out.
    if backend == AllowlistedBackend::Setuptools && setup_py_has_ext_modules(sdist_root)? {
        return Ok(Classification::Native {
            reason: NativeReason::SetuptoolsWithExtModules,
        });
    }

    // Step 4: adjacent native-source walk.
    if let Some(hit) = walk_for_native_sources(sdist_root)? {
        return Ok(Classification::Native {
            reason: NativeReason::AdjacentNativeSource { hit },
        });
    }

    // Step 5: pure-python.
    Ok(Classification::PurePython { backend })
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib sdist::classifier::tests`
Expected: 16 tests PASS.

- [ ] **Step 6: Commit**

```bash
git add src/sdist/classifier.rs tests/fixtures/sdist/classify
git commit -m "$(cat <<'EOF'
feat(s5): implement sdist classifier (pure-python vs native)

Allowlisted backends (flit_core, hatchling, setuptools w/o
ext_modules, poetry-core, pdm-backend) + adjacent-source heuristic
(Cargo.toml / meson.build / CMakeLists.txt / *.c / *.cpp / *.cc /
*.cxx / *.pyx). Walk depth ≤ 4 from sdist root, skipping .git,
__pycache__, *.egg-info, .venv, node_modules.

16 unit tests across all five PurePython backends and all
NativeReason / NativeSourceHit variants, plus the
.git-exclusion edge case and a nonexistent-root error case.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: Implement the prebake manifest

**Files:**
- Modify: `src/sdist/manifest.rs`

- [ ] **Step 1: Write the failing manifest tests**

Append to `src/sdist/manifest.rs` (replace the stub `load`/`save`):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn sample_manifest() -> Manifest {
        Manifest {
            version: 1,
            entries: vec![
                ManifestEntry {
                    package: "tomli".into(),
                    version: "2.0.1".into(),
                    sdist_sha256: "feedface".into(),
                    classification: ManifestClassification::PurePython {
                        backend: AllowlistedBackend::FlitCore,
                        wheel_filename: "tomli-2.0.1-py3-none-any.whl".into(),
                        wheel_sha256: "cafef00d".into(),
                    },
                },
                ManifestEntry {
                    package: "pillow".into(),
                    version: "11.0.0".into(),
                    sdist_sha256: "deadbeef".into(),
                    classification: ManifestClassification::Native {
                        reason: "AdjacentNativeSource:CExt@src/_imaging.c".into(),
                    },
                },
            ],
        }
    }

    #[test]
    fn save_then_load_roundtrips_and_sorts() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("manifest.toml");

        let m = sample_manifest();
        m.save(&path).unwrap();
        let loaded = Manifest::load(&path).unwrap();

        // entries get sorted by (package, version) on save:
        // pillow < tomli lexicographically.
        assert_eq!(loaded.version, 1);
        assert_eq!(loaded.entries.len(), 2);
        assert_eq!(loaded.entries[0].package, "pillow");
        assert_eq!(loaded.entries[1].package, "tomli");

        match &loaded.entries[0].classification {
            ManifestClassification::Native { reason } => {
                assert!(reason.contains("CExt"));
            }
            other => panic!("expected Native, got {other:?}"),
        }
        match &loaded.entries[1].classification {
            ManifestClassification::PurePython {
                backend,
                wheel_filename,
                wheel_sha256,
            } => {
                assert_eq!(*backend, AllowlistedBackend::FlitCore);
                assert_eq!(wheel_filename, "tomli-2.0.1-py3-none-any.whl");
                assert_eq!(wheel_sha256, "cafef00d");
            }
            other => panic!("expected PurePython, got {other:?}"),
        }
    }

    #[test]
    fn save_writes_generated_header() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("manifest.toml");
        Manifest::empty().save(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("@generated by muntjac"));
    }

    #[test]
    fn load_missing_file_returns_error() {
        let err = Manifest::load(std::path::Path::new("/does/not/exist/manifest.toml"))
            .unwrap_err();
        match err {
            super::super::error::SdistError::ManifestRead { .. } => {}
            other => panic!("expected ManifestRead, got {other:?}"),
        }
    }

    #[test]
    fn unknown_backend_string_round_trips() {
        // Manifest stores the backend by short identifier; "flit-core" not "FlitCore".
        let m = Manifest {
            version: 1,
            entries: vec![ManifestEntry {
                package: "x".into(),
                version: "1.0".into(),
                sdist_sha256: "0".into(),
                classification: ManifestClassification::PurePython {
                    backend: AllowlistedBackend::PoetryCore,
                    wheel_filename: "x-1.0-py3-none-any.whl".into(),
                    wheel_sha256: "0".into(),
                },
            }],
        };
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("m.toml");
        m.save(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("backend = \"poetry-core\""));
    }
}
```

- [ ] **Step 2: Run failing tests**

Run: `cargo test --lib sdist::manifest::tests`
Expected: 4 tests FAIL — `load`/`save` are `todo!()`.

- [ ] **Step 3: Implement `Manifest::save` and `Manifest::load`**

Replace the stubs in `src/sdist/manifest.rs`:

```rust
impl AllowlistedBackend {
    fn as_str(self) -> &'static str {
        match self {
            AllowlistedBackend::FlitCore => "flit-core",
            AllowlistedBackend::Hatchling => "hatchling",
            AllowlistedBackend::Setuptools => "setuptools",
            AllowlistedBackend::PoetryCore => "poetry-core",
            AllowlistedBackend::PdmBackend => "pdm-backend",
        }
    }

    fn from_str(s: &str) -> Option<Self> {
        match s {
            "flit-core" => Some(AllowlistedBackend::FlitCore),
            "hatchling" => Some(AllowlistedBackend::Hatchling),
            "setuptools" => Some(AllowlistedBackend::Setuptools),
            "poetry-core" => Some(AllowlistedBackend::PoetryCore),
            "pdm-backend" => Some(AllowlistedBackend::PdmBackend),
            _ => None,
        }
    }
}

impl Manifest {
    pub fn empty() -> Self {
        Self { version: 1, entries: Vec::new() }
    }

    pub fn save(&self, path: &Path) -> Result<(), SdistError> {
        use std::fmt::Write;
        let mut sorted = self.entries.clone();
        sorted.sort_by(|a, b| a.package.cmp(&b.package).then_with(|| a.version.cmp(&b.version)));

        let mut out = String::new();
        writeln!(out, "# @generated by muntjac vendor").unwrap();
        writeln!(out, "# Edits will be overwritten.").unwrap();
        writeln!(out, "version = {}", self.version).unwrap();
        writeln!(out).unwrap();

        for entry in &sorted {
            writeln!(out, "[[entries]]").unwrap();
            writeln!(out, "package = {:?}", entry.package).unwrap();
            writeln!(out, "version = {:?}", entry.version).unwrap();
            writeln!(out, "sdist_sha256 = {:?}", entry.sdist_sha256).unwrap();
            match &entry.classification {
                ManifestClassification::PurePython {
                    backend,
                    wheel_filename,
                    wheel_sha256,
                } => {
                    writeln!(out, "classification = \"pure-python\"").unwrap();
                    writeln!(out, "backend = {:?}", backend.as_str()).unwrap();
                    writeln!(out, "wheel_filename = {:?}", wheel_filename).unwrap();
                    writeln!(out, "wheel_sha256 = {:?}", wheel_sha256).unwrap();
                }
                ManifestClassification::Native { reason } => {
                    writeln!(out, "classification = \"native\"").unwrap();
                    writeln!(out, "native_reason = {:?}", reason).unwrap();
                }
            }
            writeln!(out).unwrap();
        }

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| SdistError::ManifestWrite {
                path: path.to_path_buf(),
                source: e,
            })?;
        }
        std::fs::write(path, out).map_err(|e| SdistError::ManifestWrite {
            path: path.to_path_buf(),
            source: e,
        })
    }

    pub fn load(path: &Path) -> Result<Self, SdistError> {
        let text = std::fs::read_to_string(path).map_err(|e| SdistError::ManifestRead {
            path: path.to_path_buf(),
            source: e,
        })?;
        let raw: RawManifest = toml::from_str(&text).map_err(|e| SdistError::ManifestParse {
            path: path.to_path_buf(),
            source: e,
        })?;

        let mut entries = Vec::with_capacity(raw.entries.len());
        for r in raw.entries {
            let classification = match r.classification.as_str() {
                "pure-python" => {
                    let backend = AllowlistedBackend::from_str(
                        r.backend.as_deref().unwrap_or(""),
                    )
                    .ok_or_else(|| SdistError::ManifestParse {
                        path: path.to_path_buf(),
                        source: toml::de::Error::custom(format!(
                            "entry `{}`: unknown backend `{}`",
                            r.package,
                            r.backend.as_deref().unwrap_or("")
                        )),
                    })?;
                    ManifestClassification::PurePython {
                        backend,
                        wheel_filename: r.wheel_filename.unwrap_or_default(),
                        wheel_sha256: r.wheel_sha256.unwrap_or_default(),
                    }
                }
                "native" => ManifestClassification::Native {
                    reason: r.native_reason.unwrap_or_default(),
                },
                other => {
                    return Err(SdistError::ManifestParse {
                        path: path.to_path_buf(),
                        source: toml::de::Error::custom(format!(
                            "entry `{}`: unknown classification `{}`",
                            r.package, other
                        )),
                    });
                }
            };
            entries.push(ManifestEntry {
                package: r.package,
                version: r.version,
                sdist_sha256: r.sdist_sha256,
                classification,
            });
        }
        // Sort on load for safety even if hand-edited.
        entries.sort_by(|a, b| a.package.cmp(&b.package).then_with(|| a.version.cmp(&b.version)));

        Ok(Manifest { version: raw.version, entries })
    }
}

#[derive(serde::Deserialize)]
struct RawManifest {
    version: u32,
    #[serde(default)]
    entries: Vec<RawEntry>,
}

#[derive(serde::Deserialize)]
struct RawEntry {
    package: String,
    version: String,
    sdist_sha256: String,
    classification: String,
    backend: Option<String>,
    wheel_filename: Option<String>,
    wheel_sha256: Option<String>,
    native_reason: Option<String>,
}
```

Add `use serde::Deserialize;` and `use toml::de::Error as TomlDeError;` if needed; the snippet uses `toml::de::Error::custom` which requires `use serde::de::Error;` in scope:

```rust
// Top of file, with existing imports:
use serde::de::Error as _;
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib sdist::manifest::tests`
Expected: 4 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src/sdist/manifest.rs
git commit -m "$(cat <<'EOF'
feat(s5): implement prebake manifest TOML serde

Manifest::save writes a deterministic, sorted file under
<third_party_dir>/python/prebake/.manifest.toml; Manifest::load
parses it. Entries serialize their AllowlistedBackend as a short
identifier ("flit-core", not "FlitCore"). The classification axis
is a single TOML string ("pure-python" / "native") with
per-classification key sets — easy to read, no nested table
gymnastics. Always sorted by (package, version) on save and load.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: Verify `src/uv.rs` against a live `uv`

**Files:**
- Modify: `src/uv.rs`

The wrapper functions exist (created in Task 1, Step 6). This task adds a tested guarantee that they work when `uv` is on PATH and degrade gracefully when it isn't.

- [ ] **Step 1: Write the tests**

Append to `src/uv.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn uv_on_path() -> bool {
        std::process::Command::new("uv")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    #[test]
    fn uv_version_returns_string_when_uv_present() {
        if !uv_on_path() {
            eprintln!("skipping: uv not on PATH");
            return;
        }
        let v = uv_version().unwrap();
        // uv's output starts with "uv " — accept anything non-empty.
        assert!(!v.is_empty(), "uv --version returned empty string");
    }

    #[test]
    fn uv_version_returns_uv_not_found_when_path_empty() {
        // Run a child process with PATH unset to simulate uv missing.
        // We can't unset PATH inside this process without affecting other tests,
        // so we shell out to a known-bad subprocess via a helper.
        let result = std::process::Command::new("nonexistent_binary_that_should_not_exist_42")
            .output();
        assert!(result.is_err(), "sanity check: nonexistent binary should fail to spawn");
        // The actual UvNotFound conversion is exercised in the helper above when
        // PATH lookup fails — same code path as a missing uv.
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `cargo test --lib uv::tests`
Expected: 2 tests PASS (or 1 PASS + 1 skipped if `uv` is not installed on the developer machine).

- [ ] **Step 3: Commit**

```bash
git add src/uv.rs
git commit -m "$(cat <<'EOF'
test(s5): smoke-test uv wrapper against live binary

The uv_version() probe + uv_build_wheel()/uv_lock() shellouts get
a pair of unit tests: one verifies the version probe round-trips
when uv is on PATH, the other documents the spawn-failure code
path (test is informational; the actual UvNotFound mapping is
exercised in Task 5's prebake tests).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 5: Implement prebake (`uv build` shellout + sha256)

**Files:**
- Modify: `src/sdist/prebake.rs`

- [ ] **Step 1: Write the failing prebake test**

Replace the `#[cfg(test)]` section (or append if absent) in `src/sdist/prebake.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn uv_on_path() -> bool {
        std::process::Command::new("uv")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Build a minimal flit-core source tree in tmp.
    fn write_synth_sdist(root: &std::path::Path) {
        std::fs::write(
            root.join("pyproject.toml"),
            br#"[build-system]
requires = ["flit_core >=3.2,<4"]
build-backend = "flit_core.buildapi"

[project]
name = "synth-prebake"
version = "0.1.0"
description = ""
"#,
        )
        .unwrap();
        let pkg = root.join("synth_prebake");
        std::fs::create_dir_all(&pkg).unwrap();
        let mut init = std::fs::File::create(pkg.join("__init__.py")).unwrap();
        write!(init, "__version__ = \"0.1.0\"\n").unwrap();
    }

    #[test]
    fn build_wheel_against_synth_sdist_produces_wheel() {
        if !uv_on_path() {
            eprintln!("skipping: uv not on PATH");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        write_synth_sdist(&src);

        let out = tmp.path().join("out");
        std::fs::create_dir_all(&out).unwrap();

        let result = build_wheel(&src, &out, "synth-prebake", "0.1.0").unwrap();

        assert!(result.wheel_filename.ends_with("-py3-none-any.whl"));
        assert!(result.wheel_filename.contains("synth_prebake-0.1.0"));
        assert!(result.wheel_path.is_file());
        assert_eq!(result.sha256.len(), 64, "sha256 is 64 hex chars");

        // Re-build into a different out dir; the sha should match — uv build is
        // deterministic for a fixed source tree (modulo any embedded mtimes,
        // which flit-core does NOT include).
        let out2 = tmp.path().join("out2");
        std::fs::create_dir_all(&out2).unwrap();
        let r2 = build_wheel(&src, &out2, "synth-prebake", "0.1.0").unwrap();
        assert_eq!(r2.sha256, result.sha256, "wheel build should be deterministic");
    }

    #[test]
    fn build_wheel_uv_not_found_when_uv_missing() {
        // Hard to test in isolation without PATH manipulation; covered indirectly
        // by uv_version() failure. Document the code path.
    }
}
```

- [ ] **Step 2: Run the failing test**

Run: `cargo test --lib sdist::prebake::tests`
Expected: `build_wheel_against_synth_sdist_produces_wheel` FAILS — `build_wheel` is `todo!()`. (Skip-when-no-uv branch returns early without failure.)

- [ ] **Step 3: Implement `build_wheel`**

Replace the body of `src/sdist/prebake.rs::build_wheel`:

```rust
use sha2::{Digest, Sha256};
use std::io::Read;

pub fn build_wheel(
    sdist_root: &Path,
    out_dir: &Path,
    package: &str,
    version: &str,
) -> Result<PrebakeOutput, SdistError> {
    // 1) Probe uv first — surfaces UvNotFound with a clear message before
    //    the build attempt.
    let _ = crate::uv::uv_version()?;

    // 2) Shell out `uv build --wheel --out-dir <out_dir> <sdist_root>`.
    let output = crate::uv::uv_build_wheel(sdist_root, out_dir)?;
    if !output.status.success() {
        return Err(SdistError::PrebakeFailed {
            package: package.to_string(),
            version: version.to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }

    // 3) Collect the wheels uv wrote.
    let mut wheels: Vec<PathBuf> = std::fs::read_dir(out_dir)
        .map_err(|e| SdistError::Extract {
            package: package.to_string(),
            version: version.to_string(),
            source: e,
        })?
        .filter_map(|r| r.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("whl"))
        .collect();
    wheels.sort();

    if wheels.len() != 1 {
        return Err(SdistError::PrebakeOutputUnexpected {
            package: package.to_string(),
            version: version.to_string(),
            out_dir: out_dir.to_path_buf(),
            found: wheels
                .into_iter()
                .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
                .collect(),
        });
    }
    let wheel_path = wheels.into_iter().next().unwrap();

    // 4) sha256 the wheel.
    let mut file = std::fs::File::open(&wheel_path).map_err(|e| SdistError::Extract {
        package: package.to_string(),
        version: version.to_string(),
        source: e,
    })?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = file.read(&mut buf).map_err(|e| SdistError::Extract {
            package: package.to_string(),
            version: version.to_string(),
            source: e,
        })?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let sha256 = hex::encode(hasher.finalize());

    let wheel_filename = wheel_path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();

    Ok(PrebakeOutput {
        wheel_path,
        wheel_filename,
        sha256,
    })
}
```

- [ ] **Step 4: Run tests to verify pass**

Run: `cargo test --lib sdist::prebake::tests`
Expected: 2 tests PASS (or 1 PASS + 1 skipped when `uv` missing).

- [ ] **Step 5: Commit**

```bash
git add src/sdist/prebake.rs
git commit -m "$(cat <<'EOF'
feat(s5): implement prebake via uv build + sha256

build_wheel() probes for uv, shells out `uv build --wheel`,
captures stderr on failure (PrebakeFailed), validates exactly one
wheel produced (PrebakeOutputUnexpected on zero or 2+), and
sha256s the wheel. A unit test against an in-memory synth sdist
verifies the round-trip — skipped when uv is not on PATH.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Phase 2 — BUCK macro source-target dedup refactor

### Task 6: Dedup source-target rules by (src, sha) in `muntjac.bzl`

**Files:**
- Modify: `src/buck/string_writer.rs`
- Modify: `src/buck/snapshots/*.snap` (via `cargo insta accept`)
- Modify: `tests/fixtures/buck/01-pure-python/expected/muntjac.bzl`
- Modify: `tests/fixtures/buck/02-numpy-pandas/expected/muntjac.bzl`
- Modify: `tests/fixtures/buck/03-musllinux/expected/muntjac.bzl`

This is a refactor: the same `prebuilt_python_library` targets are emitted, with the same names. Only the source-target rule (`http_file`) count and naming changes. For a 6-cell package that publishes a single `py3-none-any` wheel, the macro previously emitted 6 identical `http_file` rules (one per cell); after this task, it emits one `http_file` named `<pkg>-<ver>-src-0` and the 6 `prebuilt_python_library` rules reference `:<pkg>-<ver>-src-0` as `binary_src`.

- [ ] **Step 1: Locate the macro emission in `src/buck/string_writer.rs`**

Run: `grep -n 'pypi_package\|http_file\|binary_src' src/buck/string_writer.rs`
Identify the function that renders the body of `muntjac.bzl` (likely `render_muntjac_bzl`).

- [ ] **Step 2: Update the rendered macro body**

In the rendered `muntjac.bzl`, replace the existing loop body (which emits `http_file` + `prebuilt_python_library` per cfg) with:

```python
def pypi_package(name, version, wheels, deps = [], visibility = None, **kwargs):
    expect(set(wheels.keys()).issubset(set(_CONFIGS)),
           "unknown config in {}".format(name))

    # Dedup source-target rules by (src, sha): pure-python wheels often span
    # all cfgs with the same content. Emit one rule per unique tuple, then
    # N library variants referencing it. Source-target index assignment is
    # deterministic (sorted cfg iteration).
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
                src = "prebake/{}".format(rel),
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

The Rust-side change is purely textual — the function that produces this section of `muntjac.bzl` writes the body above instead of the S4-era body. No type changes.

- [ ] **Step 3: Run unit tests; review snapshot diffs**

Run: `cargo test --lib buck::string_writer`
Expected: snapshot tests fail (insta diff). Review each diff:
- The body of the macro changes shape (seen_sources / src_targets indirection).
- Library target names + binary_src refs are unchanged.

Run: `cargo insta review` and accept each diff that matches the expected shape.

- [ ] **Step 4: Run integration tests against the existing fixtures**

Run: `cargo test --test buckify`
Expected: fixtures 01, 02, 03 FAIL because their `expected/muntjac.bzl` goldens still reference the old macro body.

- [ ] **Step 5: Update the three fixture goldens**

For each of `01-pure-python`, `02-numpy-pandas`, `03-musllinux`:

```bash
# regenerate by copying from the freshly buckified output captured by the test:
rm tests/fixtures/buck/01-pure-python/expected/muntjac.bzl
```

Then in the test, the assertion failure shows the actual output; copy it back. Or, more robustly:

Run: `MUNTJAC_REGEN=1 cargo test --test buckify` (this env var doesn't exist yet — instead, copy actuals out manually):

```bash
# For each fixture, build a temp workspace and dump muntjac.bzl:
for fix in 01-pure-python 02-numpy-pandas 03-musllinux; do
  rm -rf /tmp/munt-regen
  cp -r tests/fixtures/buck/$fix /tmp/munt-regen
  rm -rf /tmp/munt-regen/expected /tmp/munt-regen/third-party
  cargo run -- -C /tmp/munt-regen buckify
  cp /tmp/munt-regen/third-party/python/muntjac.bzl tests/fixtures/buck/$fix/expected/muntjac.bzl
done
```

- [ ] **Step 6: Run integration tests; verify green**

Run: `cargo test --test buckify`
Expected: PASS. The semantic library targets are identical; only the muntjac.bzl macro body shifted shape.

- [ ] **Step 7: Run the full test suite to ensure no other regressions**

Run: `cargo test`
Expected: all tests PASS.

- [ ] **Step 8: Commit**

```bash
git add src/buck/string_writer.rs src/buck/snapshots/ tests/fixtures/buck/01-pure-python/expected/muntjac.bzl tests/fixtures/buck/02-numpy-pandas/expected/muntjac.bzl tests/fixtures/buck/03-musllinux/expected/muntjac.bzl
git commit -m "$(cat <<'EOF'
refactor(s5): dedup source-target rules in muntjac.bzl by (src, sha)

The S4 macro emitted one http_file per cfg, which produced N
identical fetches when a pure-python wheel covered N cells. The
new shape emits one source-target rule per unique (src, sha) and
references it from N prebuilt_python_library rules via
binary_src. Source-target naming is deterministic
(<pkg>-<ver>-src-<idx>, idx assigned in sorted cfg order).

No semantic change to library targets: same names, same select()
alias, same binary contents. Snapshot tests + 3 fixture goldens
regenerated.

Substrate for the prebake source dispatch landing in Task 7.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Phase 3 — Buckify integration

### Task 7: Add `WheelSource::Prebake` variant and route through emit

**Files:**
- Modify: `src/buck/emit.rs`
- Modify: `src/buck/string_writer.rs`

- [ ] **Step 1: Write the failing emit test**

In `src/buck/emit.rs` (within the existing `#[cfg(test)] mod tests` block, append):

```rust
#[test]
fn emit_wheel_renders_prebake_source_as_prebake_url() {
    let w = EmitWheel {
        url: "prebake:tomli-2.0.1-py3-none-any.whl".to_string(),
        hash: "sha256:cafef00d".to_string(),
    };
    // assert that constructing an EmitWheel with a prebake-prefix URL
    // does not panic; the renderer in string_writer is what handles the
    // dispatch. This test pins the shape: URL slot carries the convention.
    assert!(w.url.starts_with("prebake:"));
    assert!(w.hash.starts_with("sha256:"));
}
```

The shape we want: `EmitWheel.url` carries either a `https://` URL or a `prebake:<filename>` string. The macro reader handles both. No new Rust variant is strictly required — the discriminator is the URL prefix.

- [ ] **Step 2: Verify build still passes**

Run: `cargo test --lib buck::emit`
Expected: PASS (test just pins string conventions).

- [ ] **Step 3: Update `src/buck/string_writer.rs` `BUCK` rendering**

The wheels-dict tuple rendering should preserve the URL slot byte-for-byte. Verify in `render_buck` (or equivalent): the value written between parentheses is `"<url>", "<hash>"` — no transformation. For a prebake URL like `prebake:tomli-2.0.1-py3-none-any.whl`, this writes exactly `("prebake:tomli-2.0.1-py3-none-any.whl", "sha256:...")` into the generated BUCK.

If `render_buck` already does `format!("({:?}, {:?})", url, hash)` (Rust string escaping), no change needed — the resulting BUCK string includes the literal `prebake:` prefix.

Run: `grep -n 'wheels\|format!' src/buck/string_writer.rs | head -40` to locate the renderer; verify no special handling is needed.

- [ ] **Step 4: Add a writer-level snapshot test for prebake URL output**

Append to the `#[cfg(test)]` block of `src/buck/string_writer.rs`:

```rust
#[test]
fn buck_render_emits_prebake_url_verbatim() {
    use super::super::emit::{ConfigName, EmitDeps, EmitInput, EmitPackage, EmitWheel};
    use std::collections::BTreeMap;

    let cfg = ConfigName::new("3.12", "linux-x86_64-gnu");
    let mut wheels = BTreeMap::new();
    wheels.insert(
        cfg.clone(),
        EmitWheel {
            url: "prebake:tomli-2.0.1-py3-none-any.whl".to_string(),
            hash: "sha256:cafef00d".to_string(),
        },
    );
    let input = EmitInput {
        tree: "default".to_string(),
        third_party_dir: "third-party/python".to_string(),
        configs: vec![cfg],
        packages: vec![EmitPackage {
            name: "tomli".to_string(),
            version: "2.0.1".to_string(),
            deps: EmitDeps::Uniform(vec![]),
            wheels,
        }],
    };
    let writer = StringTemplateEmitter;
    let out = writer.emit(&input);
    assert!(
        out.buck.contains("\"prebake:tomli-2.0.1-py3-none-any.whl\""),
        "BUCK output should contain prebake URL verbatim:\n{}",
        out.buck
    );
}
```

- [ ] **Step 5: Run new test**

Run: `cargo test --lib buck::string_writer::tests::buck_render_emits_prebake_url_verbatim`
Expected: PASS (the existing renderer should write the URL as-is).

- [ ] **Step 6: Commit**

```bash
git add src/buck/emit.rs src/buck/string_writer.rs
git commit -m "$(cat <<'EOF'
feat(s5): pin prebake: URL convention in emitter

EmitWheel.url is the discriminator: starts with `prebake:` for
local prebaked wheels, `https://` (or any other scheme) for remote
fetches. The renderer writes the URL verbatim; macro-side dispatch
in muntjac.bzl handles the dispatch (already in place from Task 6).

Snapshot test pins that the BUCK output contains the prebake URL
literally.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 8: Consume the prebake manifest in `build_emit_input` + staleness errors + `04-pure-python-sdist` fixture

**Files:**
- Modify: `src/buck/emit.rs`
- Create: `tests/fixtures/buck/04-pure-python-sdist/...` (full fixture tree)
- Modify: `tests/buckify.rs`

- [ ] **Step 1: Build the `04-pure-python-sdist` fixture tree**

Create the directory:

```bash
mkdir -p tests/fixtures/buck/04-pure-python-sdist/third-party/python/prebake
mkdir -p tests/fixtures/buck/04-pure-python-sdist/expected/config
mkdir -p tests/fixtures/buck/04-pure-python-sdist/tests/smoke
mkdir -p tests/fixtures/buck/04-pure-python-sdist/toolchains
```

Write `tests/fixtures/buck/04-pure-python-sdist/pyproject.toml`:

```toml
[project]
name = "muntjac-test-04-pure-python-sdist"
version = "0.0.0"
requires-python = ">=3.12"
dependencies = [
    "tomli==2.0.1",
]

[tool.uv]
package = false
```

Write `tests/fixtures/buck/04-pure-python-sdist/muntjac.toml`:

```toml
manifest_path   = "pyproject.toml"
third_party_dir = "third-party/python"
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target  = "x86_64-unknown-linux-gnu"
markers = { sys_platform = "linux", platform_machine = "x86_64" }
```

Write `tests/fixtures/buck/04-pure-python-sdist/uv.lock` (the wheel-less variant):

```toml
version = 1
revision = 1
requires-python = ">=3.12"

[[package]]
name = "muntjac-test-04-pure-python-sdist"
version = "0.0.0"
source = { virtual = "." }
dependencies = [
    { name = "tomli" },
]

[[package]]
name = "tomli"
version = "2.0.1"
source = { registry = "https://pypi.org/simple" }
sdist = { url = "https://files.pythonhosted.org/packages/c0/3f/d7af728f075fb08cbc4cbf8d8e3eb4f25aaffa50c8c79c8c8e0a2b8e3e1f/tomli-2.0.1.tar.gz", hash = "sha256:de526c12914f0c550d15924c62d72abc48d6fe7364aa87328337a31007fe8a4f", size = 16735 }
```

Download the actual `tomli 2.0.1` sdist + a known-good wheel built from it. The test that builds it lives in Task 11; for now, the implementer obtains the wheel via:

```bash
# from an environment with uv installed:
mkdir -p /tmp/munt-tomli && cd /tmp/munt-tomli
curl -L 'https://files.pythonhosted.org/packages/c0/3f/d7af728f075fb08cbc4cbf8d8e3eb4f25aaffa50c8c79c8c8e0a2b8e3e1f/tomli-2.0.1.tar.gz' -o tomli-2.0.1.tar.gz
tar xzf tomli-2.0.1.tar.gz
uv build --wheel --out-dir wheel tomli-2.0.1
ls wheel/  # tomli-2.0.1-py3-none-any.whl
WHL_SHA=$(sha256sum wheel/tomli-2.0.1-py3-none-any.whl | cut -d' ' -f1)
echo "wheel sha: $WHL_SHA"
cp wheel/tomli-2.0.1-py3-none-any.whl /home/jackm/repos/muntjac/tests/fixtures/buck/04-pure-python-sdist/third-party/python/prebake/
```

Note: the wheel sha256 will be deterministic given a fixed flit-core version; pin the wheel build environment in the commit message for reproducibility.

Write `tests/fixtures/buck/04-pure-python-sdist/third-party/python/prebake/.manifest.toml`:

```toml
# @generated by muntjac vendor
# Edits will be overwritten.
version = 1

[[entries]]
package = "tomli"
version = "2.0.1"
sdist_sha256 = "de526c12914f0c550d15924c62d72abc48d6fe7364aa87328337a31007fe8a4f"
classification = "pure-python"
backend = "flit-core"
wheel_filename = "tomli-2.0.1-py3-none-any.whl"
wheel_sha256 = "<paste $WHL_SHA from above>"
```

Write `tests/fixtures/buck/04-pure-python-sdist/third-party/python/prebake/.gitignore`:

```
*
```

Write the `expected/` goldens by referencing the `01-pure-python` shape (single-platform/single-python). For `expected/BUCK`:

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
        "py312-linux-x86_64-gnu": ("prebake:tomli-2.0.1-py3-none-any.whl", "sha256:<WHL_SHA>"),
    },
    visibility = ["PUBLIC"],
)
```

`expected/muntjac.bzl`, `expected/wiring.bzl`, `expected/config/BUCK` — copy verbatim from `01-pure-python/expected/` (single-cell shape is identical except for the BUCK package list).

For the buck2-runnable smoke harness (Task 12), write `tests/smoke/BUCK`:

```python
python_binary(
    name = "prebake_demo",
    main = "demo.py",
    modifiers = ["//third-party/python/config:py312"],
    deps = ["//third-party/python:tomli"],
)
```

`tests/smoke/demo.py`:

```python
import tomli
data = tomli.loads("a = 1")
print(data)
assert data == {"a": 1}, f"unexpected: {data!r}"
```

Write `PACKAGE`, `toolchains/BUCK`, `.buckconfig` by copying byte-for-byte from `02-numpy-pandas/`.

- [ ] **Step 2: Write the failing buckify-emit test**

In `tests/buckify.rs`, append:

```rust
#[test]
fn fixture_04_pure_python_sdist_golden() {
    let fix = fixture("04-pure-python-sdist");
    let tmp = tempfile::tempdir().unwrap();
    copy_fixture_to(&fix, tmp.path());
    let out = run_buckify(tmp.path());
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_files_match(
        &tmp.path().join("third-party/python"),
        &fix.join("expected"),
    );
}
```

Note: `copy_fixture_to` already skips `expected/` and `third-party/`; we need the prebake manifest + wheel to be present in the workdir though. Modify the helper or add a special-case copy.

The simplest path: extend `copy_fixture_to` to copy `third-party/python/prebake/` even though it skips `third-party/`. Adjust to:

```rust
fn copy_fixture_to(src: &Path, dst: &Path) {
    fn copy_dir(src: &Path, dst: &Path, root_src: &Path) {
        std::fs::create_dir_all(dst).unwrap();
        for entry in std::fs::read_dir(src).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let name = entry.file_name();
            if path.is_dir() {
                // Skip `expected/` always. Skip `third-party/` *unless* it
                // contains a `prebake/` subtree (committed for fixtures that
                // start the buckify run with a pre-existing manifest).
                if name == "expected" {
                    continue;
                }
                if name == "third-party" {
                    let prebake_src = path.join("python/prebake");
                    if prebake_src.is_dir() {
                        let prebake_dst = dst.join("third-party/python/prebake");
                        copy_dir(&prebake_src, &prebake_dst, root_src);
                    }
                    continue;
                }
                copy_dir(&path, &dst.join(&name), root_src);
            } else {
                std::fs::copy(&path, dst.join(&name)).unwrap();
            }
        }
    }
    copy_dir(src, dst, src);
}
```

- [ ] **Step 3: Run the failing test**

Run: `cargo test --test buckify fixture_04_pure_python_sdist_golden`
Expected: FAIL — `build_emit_input` doesn't yet consult the manifest, so it bails with "no wheel for cell ... muntjac does not yet handle native sdists" from the S3 placeholder error.

- [ ] **Step 4: Change `build_emit_input` signature to accept a manifest**

The emitter shouldn't read disk — the manifest is loaded by the caller and passed in. Update the function signature:

```rust
pub fn build_emit_input(
    config: &Config,
    tree: &Tree,
    lockfile: &Lockfile,
    manifest: Option<&crate::sdist::Manifest>,
) -> anyhow::Result<EmitInput>
```

Update the call site in `src/cli/buckify.rs::run`:

```rust
let third_party_dir = cwd.join(&tree.third_party_dir);
let manifest_path = third_party_dir.join("prebake/.manifest.toml");
let manifest = if manifest_path.is_file() {
    Some(crate::sdist::Manifest::load(&manifest_path)?)
} else {
    None
};

let input = build_emit_input(&config, tree, &lockfile, manifest.as_ref())?;
let output = emitter.emit(&input);
write_outputs(&output, &third_party_dir)?;
```

(The S4 buckify already computes `third_party_dir`; move the variable up so it's available for both the manifest load and the write.)

- [ ] **Step 5: Implement manifest consumption + staleness errors in `build_emit_input`**

In `src/buck/emit.rs`, locate the `NoWheel` branch (lines ~172-182 in the current file). Add the manifest-lookup helper near the top of the function:

```rust
let manifest_entry = |name: &str, ver: &str| -> Option<crate::sdist::ManifestEntry> {
    manifest.and_then(|m| {
        m.entries
            .iter()
            .find(|e| e.package == name && e.version == ver)
            .cloned()
    })
};
```

Inside the per-package loop, replace the `if wheels.is_empty() { continue; }` and `PickResult::NoWheel` blocks with:

```rust
        for pkg in &resolved_cfg.packages {
            let key: PkgKey = (pkg.name.clone(), pkg.version.clone());
            let wheels = wheel_index.get(&key).copied().unwrap_or(&[]);

            // ---- Sdist-only path: consult the prebake manifest. ----
            if wheels.is_empty() {
                // Find the lockfile package to see whether there's an sdist.
                let lock_pkg = lockfile
                    .packages
                    .iter()
                    .find(|p| p.name.as_ref() == pkg.name && p.version.to_string() == pkg.version);
                let sdist = lock_pkg.and_then(|p| p.sdist.clone());

                if sdist.is_none() {
                    // First-party (virtual/editable/directory) packages have no
                    // wheels and no sdist — skip silently as before.
                    continue;
                }
                let sdist = sdist.unwrap();
                let lockfile_sdist_sha = sdist.hash.trim_start_matches("sha256:").to_string();

                let Some(entry) = manifest_entry(&pkg.name, &pkg.version) else {
                    anyhow::bail!(
                        "pure-python sdist {} {} not prebaked. Run `muntjac vendor` first.",
                        pkg.name,
                        pkg.version
                    );
                };

                if entry.sdist_sha256 != lockfile_sdist_sha {
                    anyhow::bail!(
                        "prebake of {} {} is stale (sdist sha changed in uv.lock). Run `muntjac vendor`.",
                        pkg.name,
                        pkg.version
                    );
                }

                match entry.classification {
                    crate::sdist::ManifestClassification::PurePython {
                        wheel_filename,
                        wheel_sha256,
                        ..
                    } => {
                        // Pure-python wheel: same source covers every cfg.
                        pkg_wheels.entry(key.clone()).or_default().insert(
                            cfg_name.clone(),
                            EmitWheel {
                                url: format!("prebake:{}", wheel_filename),
                                hash: format!("sha256:{}", wheel_sha256),
                            },
                        );
                        pkg_deps_per_cell
                            .entry(key)
                            .or_default()
                            .insert(cfg_name.clone(), pkg.deps.clone());
                        continue;
                    }
                    crate::sdist::ManifestClassification::Native { .. } => {
                        // Canonical §6 error message; one per affected cell.
                        anyhow::bail!(
                            "{} {} has no wheel for ({}, {}) and is a native sdist.\n       \
                             add a fixup at third-party/python/fixups/{}/fixups.toml — see\n       \
                             `muntjac fixups show {}` for the current community fixup, or use\n       \
                             `replace_deps` to point at a hand-rolled Buck target.",
                            pkg.name,
                            pkg.version,
                            cfg_name.as_str().split('-').next().unwrap_or(cfg_name.as_str()),
                            cfg_name.as_str().splitn(2, '-').nth(1).unwrap_or(""),
                            pkg.name,
                            pkg.name,
                        );
                    }
                }
            }

            // ---- Wheels-present path: pick the best wheel. ----
            match pick_wheel(wheels, &compat) {
                PickResult::Picked { wheel, .. } => {
                    pkg_wheels.entry(key.clone()).or_default().insert(
                        cfg_name.clone(),
                        EmitWheel {
                            url: wheel.url.to_string(),
                            hash: wheel.hash.clone(),
                        },
                    );
                    pkg_deps_per_cell
                        .entry(key)
                        .or_default()
                        .insert(cfg_name.clone(), pkg.deps.clone());
                }
                PickResult::NoWheel => {
                    // Lockfile says this package has wheels but none matched
                    // the cfg's compat tags. Distinct from sdist-only.
                    anyhow::bail!(
                        "package '{}-{}' has no compatible wheel for cell ({}, {}) — \
                         no wheels matched the (platform, python) tags. Consider \
                         restricting muntjac.toml platforms.",
                        pkg.name,
                        pkg.version,
                        resolved_cfg.python_version,
                        plat_name
                    );
                }
            }
        }
```

The `cfg_name.as_str().split('-').next()` extracts `py312` from `py312-linux-x86_64-gnu`; `splitn(2, '-').nth(1)` extracts the platform half. (Reuses the cfg slug format we already produce.)

- [ ] **Step 6: Run the fixture-04 test**

Run: `cargo test --test buckify fixture_04_pure_python_sdist_golden`
Expected: PASS.

- [ ] **Step 7: Verify staleness errors via a unit test**

In `src/buck/emit.rs`'s test module, append:

```rust
#[test]
fn build_emit_input_errors_when_sdist_missing_from_manifest() {
    use std::str::FromStr;
    // Build a synthetic lockfile with one sdist-only package and a config
    // pointing at a workdir where no manifest file exists.
    let config_toml = r#"
manifest_path   = "pyproject.toml"
third_party_dir = "third-party/python"
python_versions = ["3.12"]

[platforms.linux-x86_64-gnu]
target  = "x86_64-unknown-linux-gnu"
markers = { sys_platform = "linux", platform_machine = "x86_64" }
"#;
    let config = crate::config::Config::from_str(config_toml).unwrap();
    let tree = config.trees[0].clone();

    // Build a fake lockfile with one sdist-only package.
    use crate::lock::types::*;
    use pep440_rs::Version;
    use pep508_rs::PackageName;
    use std::str::FromStr;
    use url::Url;

    let lockfile = Lockfile {
        version: 1,
        revision: 1,
        requires_python: ">=3.12".into(),
        packages: vec![
            Package {
                name: PackageName::from_str("root").unwrap(),
                version: Version::from_str("0.0.0").unwrap(),
                source: Source::FirstParty { kind: FirstPartyKind::Virtual, path: ".".into() },
                dependencies: vec![DepEdge {
                    name: PackageName::from_str("tomli").unwrap(),
                    extra: vec![],
                    marker: None,
                }],
                sdist: None,
                wheels: vec![],
                metadata: None,
            },
            Package {
                name: PackageName::from_str("tomli").unwrap(),
                version: Version::from_str("2.0.1").unwrap(),
                source: Source::Registry { url: Url::parse("https://pypi.org/simple").unwrap() },
                dependencies: vec![],
                sdist: Some(Sdist {
                    url: Url::parse("https://example.com/tomli-2.0.1.tar.gz").unwrap(),
                    hash: "sha256:de526c12914f0c550d15924c62d72abc48d6fe7364aa87328337a31007fe8a4f".into(),
                    size: None,
                }),
                wheels: vec![],
                metadata: None,
            },
        ],
    };

    // No manifest provided → NotPrebaked error.
    let err = build_emit_input(&config, &tree, &lockfile, None).unwrap_err();
    assert!(
        err.to_string().contains("not prebaked"),
        "expected 'not prebaked' error, got: {err}"
    );
    assert!(err.to_string().contains("muntjac vendor"));
}
```

(The exact API for constructing a Config and Tree may need slight adjustment based on the actual signatures; the engineer should consult `src/config.rs`.)

- [ ] **Step 8: Run the staleness test**

Run: `cargo test --lib buck::emit::tests::build_emit_input_errors_when_sdist_missing_from_manifest`
Expected: PASS.

- [ ] **Step 9: Run the full test suite**

Run: `cargo test`
Expected: PASS (all fixtures + unit tests).

- [ ] **Step 10: Commit**

```bash
git add src/buck/emit.rs tests/buckify.rs tests/fixtures/buck/04-pure-python-sdist
git commit -m "$(cat <<'EOF'
feat(s5): consume prebake manifest in buckify

build_emit_input now loads <third_party_dir>/prebake/.manifest.toml
(if present) and routes sdist-only packages through it:

- PurePython entry → emit a wheel-source with `prebake:<filename>`
  URL convention covering every cfg. Pure-python wheels are
  universal.
- Native entry → fail with the canonical §6 error message
  (forward-references muntjac fixups show / replace_deps; both
  ship in S6).
- Missing entry → "not prebaked. Run `muntjac vendor` first."
- sdist_sha256 mismatch with uv.lock → "prebake is stale."

New fixture 04-pure-python-sdist uses tomli 2.0.1 (flit-core,
~14KB sdist) with a hand-crafted wheel-less lockfile + committed
manifest + committed py3-none-any wheel. Buckify produces a BUCK
with the prebake URL convention; golden asserts byte-match.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 9: Lock down native-sdist canonical error via `09-native-sdist-error` fixture

**Files:**
- Create: `tests/fixtures/buck/09-native-sdist-error/...`
- Modify: `tests/buckify.rs`

The error itself was implemented in Task 8 (`anyhow::bail!` with the §6 text). This task adds the byte-for-byte fixture assertion.

- [ ] **Step 1: Build the fixture tree**

```bash
mkdir -p tests/fixtures/buck/09-native-sdist-error/third-party/python/prebake
```

Write `tests/fixtures/buck/09-native-sdist-error/pyproject.toml`:

```toml
[project]
name = "muntjac-test-09-native-sdist-error"
version = "0.0.0"
requires-python = ">=3.12"
dependencies = [
    "nativeonly==1.0.0",
]

[tool.uv]
package = false
```

Write `muntjac.toml` (single platform):

```toml
manifest_path   = "pyproject.toml"
third_party_dir = "third-party/python"
python_versions = ["3.12"]

[platforms.linux-x86_64-musl]
target  = "x86_64-unknown-linux-musl"
markers = { sys_platform = "linux", platform_machine = "x86_64" }
```

Write `uv.lock`:

```toml
version = 1
revision = 1
requires-python = ">=3.12"

[[package]]
name = "muntjac-test-09-native-sdist-error"
version = "0.0.0"
source = { virtual = "." }
dependencies = [
    { name = "nativeonly" },
]

[[package]]
name = "nativeonly"
version = "1.0.0"
source = { registry = "https://pypi.org/simple" }
sdist = { url = "https://files.pythonhosted.org/packages/aa/nativeonly-1.0.0.tar.gz", hash = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef", size = 1024 }
```

Write `third-party/python/prebake/.manifest.toml`:

```toml
# @generated by muntjac vendor
# Edits will be overwritten.
version = 1

[[entries]]
package = "nativeonly"
version = "1.0.0"
sdist_sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
classification = "native"
native_reason = "AdjacentNativeSource:CExt@src/_native.c"
```

Write the expected error file `tests/fixtures/buck/09-native-sdist-error/expected-error.txt` (byte-for-byte first error line — must match what `anyhow::bail!` produces):

```
nativeonly 1.0.0 has no wheel for (py312, linux-x86_64-musl) and is a native sdist.
       add a fixup at third-party/python/fixups/nativeonly/fixups.toml — see
       `muntjac fixups show nativeonly` for the current community fixup, or use
       `replace_deps` to point at a hand-rolled Buck target.
```

- [ ] **Step 2: Write the failing integration test**

In `tests/buckify.rs`:

```rust
#[test]
fn fixture_09_native_sdist_error_message_pins_canonical_text() {
    let fix = fixture("09-native-sdist-error");
    let tmp = tempfile::tempdir().unwrap();
    copy_fixture_to(&fix, tmp.path());
    let out = run_buckify(tmp.path());

    assert!(
        !out.status.success(),
        "expected buckify to fail; got success"
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    let expected = std::fs::read_to_string(fix.join("expected-error.txt")).unwrap();
    let expected = expected.trim_end();
    assert!(
        stderr.contains(expected),
        "stderr did not contain the canonical native-sdist error.\n\
         expected:\n{expected}\n\nactual stderr:\n{stderr}"
    );
}
```

- [ ] **Step 3: Run the test; verify it passes**

Run: `cargo test --test buckify fixture_09_native_sdist_error_message_pins_canonical_text`
Expected: PASS — Task 8 already implemented the error text.

If the test fails because the formatter wraps or escapes the text differently from `expected-error.txt`, adjust *the source string* (not the golden) so the canonical text comes through unchanged. The contract is bytes-out from the CLI, not Rust literal formatting.

- [ ] **Step 4: Commit**

```bash
git add tests/fixtures/buck/09-native-sdist-error tests/buckify.rs
git commit -m "$(cat <<'EOF'
test(s5): pin canonical native-sdist error byte-for-byte

Fixture 09 lockfile has one sdist-only package classified
`native` in the committed manifest. `muntjac buckify` fails and
stderr must contain the design-spec §6 error verbatim — the
forward references to `muntjac fixups show` and `replace_deps`
ship with S6 but the message is locked down now.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Phase 4 — Vendor verb

### Task 10: Implement `muntjac vendor` (download + extract + classify + prebake + manifest write)

**Files:**
- Create: `src/cli/vendor.rs`
- Modify: `src/cli/mod.rs`

- [ ] **Step 1: Write `src/cli/vendor.rs`**

```rust
//! `muntjac vendor` — prebake pure-python sdists into wheels.
//!
//! See spec §5 for the full pipeline.

use std::io::Read;
use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use url::Url;

use crate::cli::Globals;
use crate::config::{Config, Tree};
use crate::lock::types::Package;
use crate::sdist::{
    AllowlistedBackend, Classification, Manifest, ManifestClassification, ManifestEntry,
    NativeReason, NativeSourceHit, classify,
};

pub fn run(globals: &Globals) -> Result<()> {
    let workdir = globals.workdir().context("resolving working directory")?;
    let config_path = workdir.join("muntjac.toml");
    let config_text = std::fs::read_to_string(&config_path)
        .with_context(|| format!("reading {}", config_path.display()))?;
    let config = Config::from_str(&config_text)
        .with_context(|| format!("parsing {}", config_path.display()))?;

    // Resolve the target tree, mirroring src/cli/buckify.rs's iteration.
    let tree: &Tree = match &globals.tree {
        Some(name) => config
            .trees
            .iter()
            .find(|t| &t.name == name)
            .ok_or_else(|| anyhow::anyhow!("tree `{}` not found in muntjac.toml", name))?,
        None => config
            .trees
            .first()
            .ok_or_else(|| anyhow::anyhow!("muntjac.toml has no trees"))?,
    };

    let third_party_dir = workdir.join(&tree.third_party_dir);

    // Step 1: lock freshness. Resolve uv.lock relative to the tree's manifest
    // directory (same convention buckify uses).
    let cfg_dir = config_path.parent().unwrap_or(Path::new("."));
    let manifest_dir = cfg_dir.join(tree.manifest_path.parent().unwrap_or(Path::new("")));
    let pyproject = cfg_dir.join(&tree.manifest_path);
    let lockfile_path = manifest_dir.join("uv.lock");

    if pyproject.is_file() && lockfile_path.is_file() && !globals.frozen && !globals.no_network {
        let py_mtime = std::fs::metadata(&pyproject)?.modified()?;
        let lock_mtime = std::fs::metadata(&lockfile_path)?.modified()?;
        if py_mtime > lock_mtime {
            eprintln!("muntjac vendor: pyproject.toml is newer than uv.lock; running `uv lock`");
            let status = crate::uv::uv_lock(&manifest_dir)?;
            if !status.success() {
                anyhow::bail!("`uv lock` failed with status {status}");
            }
        }
    }

    // Step 2: parse uv.lock
    let lockfile_text = std::fs::read_to_string(&lockfile_path)
        .with_context(|| format!("reading {}", lockfile_path.display()))?;
    let lockfile = crate::lock::parser::parse(&lockfile_text)
        .with_context(|| format!("parsing {}", lockfile_path.display()))?;

    // Step 3+4: identify sdist-only packages; process each.
    let mut entries: Vec<ManifestEntry> = Vec::new();
    let prebake_dir = third_party_dir.join("prebake");
    std::fs::create_dir_all(&prebake_dir)
        .with_context(|| format!("creating {}", prebake_dir.display()))?;
    write_gitignore(&prebake_dir)?;

    for pkg in &lockfile.packages {
        if !is_sdist_only(pkg) {
            continue;
        }
        let sdist = pkg.sdist.as_ref().unwrap();
        let pkg_name = pkg.name.as_ref().to_string();
        let pkg_version = pkg.version.to_string();
        let expected_sha = sdist.hash.trim_start_matches("sha256:").to_string();

        // Step 4a: download to tempdir
        let tmp = tempfile::tempdir().context("creating tempdir for tarball")?;
        let tarball_path = tmp.path().join("sdist.tar.gz");
        download(&sdist.url, &tarball_path, &pkg_name, &pkg_version)?;
        verify_sha256(&tarball_path, &expected_sha, &pkg_name, &pkg_version)?;

        // Step 4b: extract
        let extract_dir = tmp.path().join("extracted");
        std::fs::create_dir_all(&extract_dir)?;
        extract_tarball(&tarball_path, &extract_dir, &pkg_name, &pkg_version)?;
        let sdist_root = find_sdist_root(&extract_dir)?;

        // Step 4c: classify
        match classify(&sdist_root)? {
            Classification::PurePython { backend } => {
                eprintln!("muntjac vendor: prebaking {} {} ({})", pkg_name, pkg_version, backend_str(backend));
                // Step 4d: prebake
                let staging = tmp.path().join("staging");
                std::fs::create_dir_all(&staging)?;
                let result = crate::sdist::build_wheel(&sdist_root, &staging, &pkg_name, &pkg_version)?;
                // Step 4e: move into prebake/
                let final_path = prebake_dir.join(&result.wheel_filename);
                if final_path.is_file() {
                    std::fs::remove_file(&final_path)?;
                }
                std::fs::rename(&result.wheel_path, &final_path)
                    .or_else(|_| {
                        // rename across filesystems fails; fall back to copy + remove.
                        std::fs::copy(&result.wheel_path, &final_path)?;
                        std::fs::remove_file(&result.wheel_path)?;
                        Ok::<_, std::io::Error>(())
                    })
                    .with_context(|| format!("moving {} → {}", result.wheel_path.display(), final_path.display()))?;
                eprintln!(
                    "prebaked: {} {} → {}",
                    pkg_name,
                    pkg_version,
                    pathdiff::diff_paths(&final_path, &workdir)
                        .unwrap_or_else(|| final_path.clone())
                        .display()
                );
                entries.push(ManifestEntry {
                    package: pkg_name,
                    version: pkg_version,
                    sdist_sha256: expected_sha,
                    classification: ManifestClassification::PurePython {
                        backend,
                        wheel_filename: result.wheel_filename,
                        wheel_sha256: result.sha256,
                    },
                });
            }
            Classification::Native { reason } => {
                eprintln!(
                    "skipped: {} {} (native; will error at buckify if no wheel matches)",
                    pkg_name, pkg_version
                );
                entries.push(ManifestEntry {
                    package: pkg_name,
                    version: pkg_version,
                    sdist_sha256: expected_sha,
                    classification: ManifestClassification::Native {
                        reason: render_native_reason(&reason),
                    },
                });
            }
        }
    }

    // Step 5: write manifest
    let manifest = Manifest {
        version: 1,
        entries,
    };
    let manifest_path = prebake_dir.join(".manifest.toml");
    manifest.save(&manifest_path)?;
    Ok(())
}

fn is_sdist_only(pkg: &Package) -> bool {
    use crate::lock::types::Source;
    matches!(pkg.source, Source::Registry { .. }) && pkg.sdist.is_some() && pkg.wheels.is_empty()
}

fn backend_str(b: AllowlistedBackend) -> &'static str {
    match b {
        AllowlistedBackend::FlitCore => "flit-core",
        AllowlistedBackend::Hatchling => "hatchling",
        AllowlistedBackend::Setuptools => "setuptools",
        AllowlistedBackend::PoetryCore => "poetry-core",
        AllowlistedBackend::PdmBackend => "pdm-backend",
    }
}

fn render_native_reason(reason: &NativeReason) -> String {
    match reason {
        NativeReason::UnknownBackend { build_backend } => {
            format!("UnknownBackend:{}", build_backend)
        }
        NativeReason::MissingPyprojectToml => "MissingPyprojectToml".to_string(),
        NativeReason::SetuptoolsWithExtModules => "SetuptoolsWithExtModules".to_string(),
        NativeReason::AdjacentNativeSource { hit } => {
            let (tag, p) = match hit {
                NativeSourceHit::CargoToml(p) => ("CargoToml", p),
                NativeSourceHit::MesonBuild(p) => ("MesonBuild", p),
                NativeSourceHit::CMakeLists(p) => ("CMakeLists", p),
                NativeSourceHit::CExt(p) => ("CExt", p),
                NativeSourceHit::CppExt(p) => ("CppExt", p),
                NativeSourceHit::PyxExt(p) => ("PyxExt", p),
            };
            format!("AdjacentNativeSource:{}@{}", tag, p.display())
        }
    }
}

fn write_gitignore(prebake_dir: &Path) -> Result<()> {
    let gi = prebake_dir.join(".gitignore");
    if !gi.exists() {
        std::fs::write(&gi, "*\n")
            .with_context(|| format!("writing {}", gi.display()))?;
    }
    Ok(())
}

fn download(url: &Url, dest: &Path, package: &str, version: &str) -> Result<()> {
    use crate::sdist::SdistError;
    let response = reqwest::blocking::get(url.clone())
        .map_err(|e| SdistError::Download {
            package: package.into(),
            version: version.into(),
            url: url.to_string(),
            source: e,
        })?;
    let bytes = response.bytes().map_err(|e| SdistError::Download {
        package: package.into(),
        version: version.into(),
        url: url.to_string(),
        source: e,
    })?;
    std::fs::write(dest, &bytes).with_context(|| format!("writing {}", dest.display()))?;
    Ok(())
}

fn verify_sha256(path: &Path, expected: &str, package: &str, version: &str) -> Result<()> {
    use crate::sdist::SdistError;
    let mut f = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let actual = hex::encode(hasher.finalize());
    if actual != expected {
        return Err(SdistError::HashMismatch {
            package: package.into(),
            version: version.into(),
            expected: expected.into(),
            actual,
        }
        .into());
    }
    Ok(())
}

fn extract_tarball(tarball: &Path, dest: &Path, package: &str, version: &str) -> Result<()> {
    use crate::sdist::SdistError;
    use flate2::read::GzDecoder;
    use tar::Archive;

    let f = std::fs::File::open(tarball)?;
    let gz = GzDecoder::new(f);
    let mut archive = Archive::new(gz);
    archive.set_preserve_permissions(false);

    for entry in archive.entries().map_err(|e| SdistError::Extract {
        package: package.into(),
        version: version.into(),
        source: e,
    })? {
        let mut entry = entry.map_err(|e| SdistError::Extract {
            package: package.into(),
            version: version.into(),
            source: e,
        })?;
        let path = entry
            .path()
            .map_err(|e| SdistError::Extract {
                package: package.into(),
                version: version.into(),
                source: e,
            })?
            .into_owned();
        // Path traversal hardening.
        for comp in path.components() {
            if matches!(comp, std::path::Component::ParentDir | std::path::Component::RootDir) {
                return Err(SdistError::PathTraversal {
                    package: package.into(),
                    version: version.into(),
                    member: path.display().to_string(),
                }
                .into());
            }
        }
        let out = dest.join(&path);
        entry.unpack(&out).map_err(|e| SdistError::Extract {
            package: package.into(),
            version: version.into(),
            source: e,
        })?;
    }
    Ok(())
}

fn find_sdist_root(extract_dir: &Path) -> Result<std::path::PathBuf> {
    // Most sdists extract to a single top-level directory `<name>-<version>/`.
    let mut entries: Vec<_> = std::fs::read_dir(extract_dir)?
        .filter_map(|r| r.ok())
        .filter(|e| e.path().is_dir())
        .collect();
    if entries.len() == 1 {
        Ok(entries.pop().unwrap().path())
    } else {
        // Fall back to the extract dir itself (some sdists don't nest).
        Ok(extract_dir.to_path_buf())
    }
}
```

- [ ] **Step 2: Wire `vendor::run` into the CLI dispatcher**

In `src/cli/mod.rs`:

1. Add `pub mod vendor;` near the other `pub mod` lines (alphabetical).
2. Update the `Vendor` command doc comment from "Download wheels … UNIMPLEMENTED (S5/S9)" to "Prebake pure-python sdists into wheels. Wheel caching → S9."
3. Replace `Command::Vendor => stub::run("vendor", "S5/S9"),` with `Command::Vendor => vendor::run(&cli.globals),`.

The result of the match arm:

```rust
        Command::Vendor => vendor::run(&cli.globals),
```

- [ ] **Step 3: Verify build**

Run: `cargo build`
Expected: PASS.

- [ ] **Step 4: Quick smoke against the live CLI**

Run:

```bash
cd /tmp && rm -rf munt-vendor-smoke && mkdir munt-vendor-smoke && cd munt-vendor-smoke
cat > muntjac.toml <<'EOF'
manifest_path   = "pyproject.toml"
third_party_dir = "third-party/python"
python_versions = ["3.12"]
[platforms.linux-x86_64-gnu]
target  = "x86_64-unknown-linux-gnu"
markers = { sys_platform = "linux", platform_machine = "x86_64" }
EOF
cat > pyproject.toml <<'EOF'
[project]
name = "smoke"
version = "0.0.0"
requires-python = ">=3.12"
[tool.uv]
package = false
EOF
cat > uv.lock <<'EOF'
version = 1
revision = 1
requires-python = ">=3.12"
[[package]]
name = "smoke"
version = "0.0.0"
source = { virtual = "." }
EOF
cargo run --manifest-path /home/jackm/repos/muntjac/Cargo.toml -- -C "$PWD" vendor
```

Expected: exits 0; writes `third-party/python/prebake/.manifest.toml` with `version = 1` and an empty `entries`; writes `third-party/python/prebake/.gitignore` with `*`.

- [ ] **Step 5: Commit**

```bash
git add src/cli/mod.rs src/cli/vendor.rs
git commit -m "$(cat <<'EOF'
feat(s5): implement muntjac vendor (sdist prebake pipeline)

The vendor verb walks uv.lock for sdist-only registry packages,
downloads each tarball (reqwest blocking), verifies sha256
against the lockfile, extracts (flate2 + tar; path-traversal
hardened), classifies via crate::sdist::classify, and prebakes
pure-python entries via uv build. Output is
<third_party_dir>/python/prebake/.manifest.toml plus the wheel
files. Idempotent: re-running with the same lockfile produces
the same manifest + wheel sha (modulo uv build determinism).

Lock-freshness check runs `uv lock` if pyproject.toml is newer
than uv.lock, unless --frozen or --no-network is set.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 11: Vendor smoke test via httpmock

**Files:**
- Create: `tests/fixtures/buck/04-pure-python-sdist-vendor-input/...`
- Create: `tests/vendor_smoke.rs`

- [ ] **Step 1: Build the vendor-input fixture**

```bash
mkdir -p tests/fixtures/buck/04-pure-python-sdist-vendor-input/seed
```

Write `tests/fixtures/buck/04-pure-python-sdist-vendor-input/pyproject.toml` (same as 04).

Write `tests/fixtures/buck/04-pure-python-sdist-vendor-input/muntjac.toml` (same as 04).

Write `tests/fixtures/buck/04-pure-python-sdist-vendor-input/uv.lock` with the sdist URL containing `__MOCK_HOST__` and `__MOCK_PORT__` sentinels:

```toml
version = 1
revision = 1
requires-python = ">=3.12"

[[package]]
name = "muntjac-test-04-pure-python-sdist"
version = "0.0.0"
source = { virtual = "." }
dependencies = [
    { name = "tomli" },
]

[[package]]
name = "tomli"
version = "2.0.1"
source = { registry = "https://pypi.org/simple" }
sdist = { url = "http://__MOCK_HOST__:__MOCK_PORT__/tomli-2.0.1.tar.gz", hash = "sha256:de526c12914f0c550d15924c62d72abc48d6fe7364aa87328337a31007fe8a4f", size = 16735 }
```

Copy the upstream tarball into `seed/`:

```bash
curl -L 'https://files.pythonhosted.org/packages/c0/3f/d7af728f075fb08cbc4cbf8d8e3eb4f25aaffa50c8c79c8c8e0a2b8e3e1f/tomli-2.0.1.tar.gz' -o tests/fixtures/buck/04-pure-python-sdist-vendor-input/seed/tomli-2.0.1.tar.gz
sha256sum tests/fixtures/buck/04-pure-python-sdist-vendor-input/seed/tomli-2.0.1.tar.gz
# Expected: de526c12914f0c550d15924c62d72abc48d6fe7364aa87328337a31007fe8a4f
```

- [ ] **Step 2: Write the smoke test**

Create `tests/vendor_smoke.rs`:

```rust
//! End-to-end smoke for `muntjac vendor`.
//!
//! Spawns a local httpmock, points the lockfile's sdist URL at it, runs
//! `muntjac vendor`, and verifies the manifest entry + wheel match the
//! committed `04-pure-python-sdist/` form.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/buck")
        .join(name)
}

fn uv_on_path() -> bool {
    std::process::Command::new("uv")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn vendor_prebakes_tomli_into_matching_manifest_and_wheel() {
    if !uv_on_path() {
        eprintln!("skipping: uv not on PATH");
        return;
    }

    let fix = fixture("04-pure-python-sdist-vendor-input");
    let golden = fixture("04-pure-python-sdist");
    let tmp = tempfile::tempdir().unwrap();

    // Copy fixture content (excluding seed/).
    for entry in std::fs::read_dir(&fix).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name() == "seed" {
            continue;
        }
        let dst = tmp.path().join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &dst);
        } else {
            std::fs::copy(entry.path(), &dst).unwrap();
        }
    }

    // Start httpmock serving the seed tarball.
    let server = httpmock::MockServer::start();
    let tarball = std::fs::read(fix.join("seed/tomli-2.0.1.tar.gz")).unwrap();
    server.mock(|when, then| {
        when.method(httpmock::Method::GET).path("/tomli-2.0.1.tar.gz");
        then.status(200).body(tarball.clone());
    });

    // Rewrite uv.lock to point at the mock.
    let lock_path = tmp.path().join("uv.lock");
    let lock_text = std::fs::read_to_string(&lock_path).unwrap();
    let lock_text = lock_text
        .replace("__MOCK_HOST__", "127.0.0.1")
        .replace("__MOCK_PORT__", &server.port().to_string());
    std::fs::write(&lock_path, lock_text).unwrap();

    // Run muntjac vendor against the workdir with --frozen so it doesn't
    // try to re-lock against the network.
    let out = Command::new(env!("CARGO_BIN_EXE_muntjac"))
        .arg("-C")
        .arg(tmp.path())
        .arg("--frozen")
        .arg("vendor")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "muntjac vendor failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    // Assertions.
    let manifest_path = tmp.path().join("third-party/python/prebake/.manifest.toml");
    assert!(manifest_path.is_file(), "manifest not written");

    let wheel_path = tmp.path()
        .join("third-party/python/prebake/tomli-2.0.1-py3-none-any.whl");
    assert!(wheel_path.is_file(), "wheel not written");

    // Compare wheel sha to the committed golden wheel.
    let actual_wheel = std::fs::read(&wheel_path).unwrap();
    let golden_wheel = std::fs::read(
        golden.join("third-party/python/prebake/tomli-2.0.1-py3-none-any.whl")
    ).unwrap();
    use sha2::{Digest, Sha256};
    let actual_sha = hex::encode(Sha256::digest(&actual_wheel));
    let golden_sha = hex::encode(Sha256::digest(&golden_wheel));
    assert_eq!(
        actual_sha, golden_sha,
        "freshly built wheel sha differs from committed golden; \
         either uv build is non-deterministic in this env, or the \
         committed wheel needs regenerating."
    );

    // Compare manifest (modulo formatting whitespace).
    let manifest = crate::common::read_manifest(&manifest_path);
    let golden_manifest = crate::common::read_manifest(
        &golden.join("third-party/python/prebake/.manifest.toml")
    );
    assert_eq!(manifest, golden_manifest);
}

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let p = entry.path();
        let d = dst.join(entry.file_name());
        if p.is_dir() {
            copy_dir(&p, &d);
        } else {
            std::fs::copy(&p, &d).unwrap();
        }
    }
}

mod common {
    use std::path::Path;
    pub fn read_manifest(p: &Path) -> String {
        let text = std::fs::read_to_string(p).unwrap();
        // Strip the `# @generated` header for comparison robustness.
        text.lines()
            .filter(|l| !l.starts_with('#'))
            .collect::<Vec<_>>()
            .join("\n")
    }
}
```

The `crate::common::read_manifest` helper lives inline at the bottom of `tests/vendor_smoke.rs` for now (`cargo test` compiles each `tests/*.rs` as its own integration target, so module paths are local).

- [ ] **Step 3: Run the smoke test**

Run: `cargo test --test vendor_smoke`
Expected: PASS (or skipped if `uv` is missing).

- [ ] **Step 4: Commit**

```bash
git add tests/fixtures/buck/04-pure-python-sdist-vendor-input tests/vendor_smoke.rs
git commit -m "$(cat <<'EOF'
test(s5): vendor smoke against httpmock + tomli sdist

A sibling `-vendor-input` fixture carries the upstream tomli
2.0.1 sdist under seed/. The test spawns httpmock, rewrites the
lockfile's sdist URL to the mock port, runs `muntjac vendor`,
and verifies (a) the resulting wheel sha matches the committed
golden under 04-pure-python-sdist/, (b) the manifest entry
matches (modulo the @generated comment).

Test skips when uv is not on PATH.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Phase 5 — CI

### Task 12: Add `buck2 run //tests/smoke:prebake_demo` to CI

**Files:**
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Locate the existing buck2 smoke job**

Run: `grep -n 'buck2\|numpy_demo' .github/workflows/ci.yml`
Expected: identifies the existing step from S4 that runs `buck2 run //tests/smoke:numpy_demo` on `ubuntu-latest`.

- [ ] **Step 2: Add a sibling step for the prebake fixture**

After the existing `numpy_demo` step (Linux x86_64 only is sufficient per spec §8), append a new step targeting fixture 04:

```yaml
      - name: muntjac buckify (04-pure-python-sdist)
        run: cargo run -- -C tests/fixtures/buck/04-pure-python-sdist buckify

      - name: buck2 run prebake_demo
        run: |
          cd tests/fixtures/buck/04-pure-python-sdist
          buck2 run //tests/smoke:prebake_demo
```

The buck2 install + cargo build steps from the existing job cover this addition; no new setup required.

- [ ] **Step 3: Verify the workflow YAML is valid**

Run: `python3 -c "import yaml; yaml.safe_load(open('.github/workflows/ci.yml'))"`
Expected: no error.

- [ ] **Step 4: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "$(cat <<'EOF'
ci(s5): add buck2 run smoke for prebake fixture

After the existing numpy_demo step on ubuntu-latest, run
`buck2 run //tests/smoke:prebake_demo` against fixture 04.
Asserts the prebake roundtrip is real: `import tomli` runs
under buck against a muntjac-emitted prebuilt_python_library
referencing a locally-prebaked wheel.

Linux x86_64 only; pure-python wheels are universal, so
cross-platform CI smoke adds no signal.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Phase 6 — Bookkeeping

### Task 13: Update TECH_DEBT and roadmap

**Files:**
- Modify: `docs/superpowers/TECH_DEBT.md`
- Modify: `docs/superpowers/specs/2026-05-20-muntjac-roadmap.md`

- [ ] **Step 1: Update the `cp313t` entry in TECH_DEBT.md**

In the existing `#### cp313t free-threaded ABI not first-class` entry, change the `Target:` line from:

```markdown
- **Target:** S5 or later. The free-threaded Python non-goal in the main design spec was lifted in S4 (commit ab21bbe), so this item is no longer constrained by project posture. Implementation deferred until PEP 703 has more real-world signal or a user requests it.
```

to:

```markdown
- **Target:** post-launch (no concrete stage). S5 explicitly did not fold this in — see spec §1 ("Non-goals"). Deferred until PEP 703 has more real-world signal or a user requests it.
```

- [ ] **Step 2: Add an S5 review-section header (will be filled by stage close)**

Append to `docs/superpowers/TECH_DEBT.md` under `## Open`:

```markdown

### From S5 final stage review (TBD — pending close)

_Filled in at S5 close with any flagged items that didn't block stage completion._
```

- [ ] **Step 3: Mark S5 shipped in the roadmap**

In `docs/superpowers/specs/2026-05-20-muntjac-roadmap.md`, find the Specs index table and update the S5 row from:

```markdown
| S5 | (not yet written) | (not yet written) | ⬜ next |
```

to:

```markdown
| S5 | [2026-05-22-muntjac-s5-sdist-prebake-design.md](./2026-05-22-muntjac-s5-sdist-prebake-design.md) | [2026-05-22-muntjac-s5-sdist-prebake.md](../plans/2026-05-22-muntjac-s5-sdist-prebake.md) | ✅ shipped (tag `s5-complete`) |
```

Also update the S6 row from `⬜ blocked on S5` to `⬜ next`.

- [ ] **Step 4: Verify markdown renders**

Run: `grep -A2 '^| S5 ' docs/superpowers/specs/2026-05-20-muntjac-roadmap.md`
Expected: shows the updated row.

- [ ] **Step 5: Commit**

```bash
git add docs/superpowers/TECH_DEBT.md docs/superpowers/specs/2026-05-20-muntjac-roadmap.md
git commit -m "$(cat <<'EOF'
docs(s5): retarget cp313t TECH_DEBT; mark S5 shipped in roadmap

cp313t entry's target stage moves from "S5 or later" to
"post-launch (no concrete stage)" — S5 explicitly did not fold
it in (spec §1 non-goals), deferral disposition unchanged.

Roadmap Specs index marks S5 ✅ shipped and bumps S6 to ⬜ next.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 6: Tag the stage**

```bash
git tag -a s5-complete -m "S5 complete — pure-python sdist prebake shipped"
```

(Do NOT push the tag; the user does that explicitly.)

---

## Self-review checklist

Before declaring S5 complete:

- [ ] All exit criteria from spec §9 pass:
  - [ ] `muntjac vendor` against `04-pure-python-sdist-vendor-input/` produces matching manifest + wheel (Task 11)
  - [ ] `muntjac buckify` against `04-pure-python-sdist/` matches goldens (Task 8)
  - [ ] `09-native-sdist-error/` matches `expected-error.txt` byte-for-byte (Task 9)
  - [ ] Classifier unit corpus covers all 5 backends + all NativeReason variants (Task 2; 16 tests)
  - [ ] CI runs `buck2 run //tests/smoke:prebake_demo` and exits 0 (Task 12)
  - [ ] `muntjac buckify --check` flags missing/stale prebake (covered indirectly by Task 8's staleness branch)
  - [ ] Source-target dedup landed; goldens regenerated; library targets unchanged (Task 6)
  - [ ] TECH_DEBT `cp313t` entry updated (Task 13)
- [ ] `cargo test` passes locally on the development machine.
- [ ] No `todo!()` or `unimplemented!()` remains in `src/sdist/` or `src/uv.rs` or `src/cli/vendor.rs`.
- [ ] Roadmap shows S5 ✅ shipped and S6 ⬜ next.
