# S9 — Vendor mode — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire `[buck] vendor: bool` end-to-end so `muntjac vendor --mode=committed` downloads all referenced wheels (plus prebake outputs) into per-tree `<third_party_dir>/vendor/`, and `muntjac buckify` emits `export_file` references at those committed wheels instead of `http_file(urls=…)` — yielding air-gapped Buck builds.

**Architecture:** Mode resolves from config (`[buck] vendor`) with CLI override (`--mode={committed,prebake-only}`). Committed mode unifies download + prebake outputs into one `vendor/` dir per tree; **no new manifest format** — state is fully derivable from `uv.lock` + filesystem + PEP 427 deterministic naming. The emitter grows a new `vendor:` URL prefix mirroring the existing `prebake:` pattern; the `pypi_package` macro's `vendor:` branch is emitted **conditionally** (only when `vendor_mode=true`), preserving byte-identical output for fixtures 01–10 in prebake-only mode. An emit-time filesystem existence check aborts buckify if any expected wheel is missing.

**Tech Stack:** Rust 2024, `thiserror` typed errors, `reqwest::blocking` streaming downloads, `sha2` for hash verification, `serde`/`toml` config, `insta` snapshots, `clap::ValueEnum` for `--mode`, `assert_cmd` integration tests.

**Spec:** [`docs/superpowers/specs/2026-05-26-muntjac-s9-vendor-mode-design.md`](../specs/2026-05-26-muntjac-s9-vendor-mode-design.md)

**Branch:** `s9-vendor-mode` (off `main` at `d46eb26`). Spec already committed at `7fc9385`.

---

## File Structure

**Modify:**
- `src/cli/mod.rs` — `ModeFlag` enum; `VendorArgs`/`BuckifyArgs`; `resolve_vendor_mode` helper; updated `Command` variants and `run()` dispatch.
- `src/cli/vendor.rs` — accept `VendorArgs`; resolve mode; committed-mode orchestration (download + prebake-into-vendor + sync prune); skip prebake manifest in committed mode.
- `src/cli/buckify.rs` — accept `BuckifyArgs`; resolve mode; skip prebake manifest read in committed mode; per-tree existence check.
- `src/buck/emit.rs` — `EmitInput.vendor_mode` field; `BuildEmitContext.vendor_mode` field; per-package URL synthesis branches on vendor_mode; PEP 427 filename helper; `check_vendor_wheels_present` function.
- `src/buck/string_writer.rs` — conditional `vendor:` macro arm; uses `EmitInput.vendor_mode`.
- `src/buck/mod.rs` — re-export updated emit types/functions.
- `src/error.rs` — `VendorError` enum (`Download`, `HashMismatch`, `ModeNetworkConflict`); `BuckifyError::MissingVendorWheels` (extends existing).
- `Cargo.toml` — version `0.2.0` → `0.3.0`.
- `Cargo.lock` — version bump.
- `CHANGELOG.md` — `[0.3.0]` entry.
- `README.md` — Vendor mode section; Status update.
- `docs/superpowers/specs/2026-05-20-muntjac-roadmap.md` — mark S9 ✅ shipped.
- `docs/superpowers/TECH_DEBT.md` — TD-S9-01..04.
- `.github/workflows/ci.yml` — `11-vendor` fixture steps on all 3 runners.

**Create:**
- `src/error.rs` — `BuckifyError` enum (currently only `VendorError`/`ConfigError`/`LockfileError`/`CacheError` live there; may need a new enum or extend an existing one — see Task 2).
- `src/vendor/mod.rs` — module declaration.
- `src/vendor/download.rs` — streaming wheel download + sha256 verify.
- `src/vendor/sync.rs` — sync-prune of stale wheels.
- `tests/vendor.rs` — integration tests.
- `tests/fixtures/buck/11-vendor/` — fixture tree (muntjac.toml, pyproject.toml, uv.lock, third-party/python/vendor/*.whl, .buckconfig, toolchains/BUCK, PACKAGE, prelude submodule pin, expected/ golden, tests/smoke/BUCK).
- Snapshot test in `tests/buckify.rs::fixture_11_vendor_golden`.
- Multi-tree emit test in `tests/multi_tree.rs::buckify_committed_multi_tree_emits_per_tree_vendor_refs`.

---

## Task 1: CLI flag plumbing — `--mode` + `--no-prune` + `resolve_vendor_mode`

**Files:**
- Modify: `src/cli/mod.rs`

- [ ] **Step 1: Write failing tests for `resolve_vendor_mode` and CLI parsing**

Add to `src/cli/mod.rs` `mod tests`:

```rust
#[test]
fn resolve_vendor_mode_uses_config_when_no_override() {
    let toml = r#"
manifest_path = "pyproject.toml"
third_party_dir = "third-party/python"
python_versions = ["3.12"]
[platforms]
linux-x86_64-gnu = { target = "x86_64-unknown-linux-gnu", manylinux = "2_17" }
[buck]
vendor = true
"#;
    let config: Config = toml.parse().unwrap();
    assert!(resolve_vendor_mode(&config, None));
}

#[test]
fn resolve_vendor_mode_override_committed_beats_config_false() {
    let toml = r#"
manifest_path = "pyproject.toml"
third_party_dir = "third-party/python"
python_versions = ["3.12"]
[platforms]
linux-x86_64-gnu = { target = "x86_64-unknown-linux-gnu", manylinux = "2_17" }
"#;
    let config: Config = toml.parse().unwrap();
    assert!(resolve_vendor_mode(&config, Some(ModeFlag::Committed)));
}

#[test]
fn resolve_vendor_mode_override_prebake_only_beats_config_true() {
    let toml = r#"
manifest_path = "pyproject.toml"
third_party_dir = "third-party/python"
python_versions = ["3.12"]
[platforms]
linux-x86_64-gnu = { target = "x86_64-unknown-linux-gnu", manylinux = "2_17" }
[buck]
vendor = true
"#;
    let config: Config = toml.parse().unwrap();
    assert!(!resolve_vendor_mode(&config, Some(ModeFlag::PrebakeOnly)));
}

#[test]
fn cli_parses_vendor_mode_and_no_prune() {
    let cli = Cli::try_parse_from([
        "muntjac",
        "vendor",
        "--mode",
        "committed",
        "--no-prune",
    ])
    .unwrap();
    match cli.command {
        Command::Vendor(args) => {
            assert!(matches!(args.mode, Some(ModeFlag::Committed)));
            assert!(args.no_prune);
        }
        _ => panic!("expected Vendor"),
    }
}

#[test]
fn cli_parses_buckify_mode() {
    let cli = Cli::try_parse_from([
        "muntjac",
        "buckify",
        "--mode",
        "prebake-only",
    ])
    .unwrap();
    match cli.command {
        Command::Buckify(args) => assert!(matches!(args.mode, Some(ModeFlag::PrebakeOnly))),
        _ => panic!("expected Buckify"),
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```
cargo test -p muntjac --lib cli:: -- --nocapture
```

Expected: 5 failures, e.g. `cannot find value 'ModeFlag' in this scope`, `Command::Vendor takes 0 args`.

- [ ] **Step 3: Implement `ModeFlag` enum, args structs, and helper**

In `src/cli/mod.rs`, add after the existing `Globals` impl block (around line 61) and before `resolve_trees`:

```rust
/// Per-invocation override for `[buck] vendor`. Affects this invocation only;
/// never writes back to the config. `Committed` ⇔ `vendor = true`,
/// `PrebakeOnly` ⇔ `vendor = false`.
#[derive(clap::ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeFlag {
    Committed,
    PrebakeOnly,
}

#[derive(Args, Debug, Clone, Default)]
pub struct VendorArgs {
    /// Override `[buck] vendor` for this invocation.
    #[arg(long, value_enum, value_name = "MODE")]
    pub mode: Option<ModeFlag>,

    /// Skip the sync-prune step in committed mode (keeps stale wheels).
    #[arg(long)]
    pub no_prune: bool,
}

#[derive(Args, Debug, Clone, Default)]
pub struct BuckifyArgs {
    /// Override `[buck] vendor` for this invocation.
    #[arg(long, value_enum, value_name = "MODE")]
    pub mode: Option<ModeFlag>,
}

/// Resolve the effective vendor mode: CLI override wins; else read `[buck] vendor`.
pub fn resolve_vendor_mode(config: &Config, mode_override: Option<ModeFlag>) -> bool {
    match mode_override {
        Some(ModeFlag::Committed) => true,
        Some(ModeFlag::PrebakeOnly) => false,
        None => config.buck.vendor,
    }
}
```

Update `Command` variants (around line 100):

```rust
    /// Vendor wheels for the project. In prebake-only mode (default), prebakes
    /// pure-python sdists into `<third_party_dir>/prebake/`. In committed mode
    /// (`[buck] vendor = true` or `--mode=committed`), additionally downloads
    /// all registry wheels into `<third_party_dir>/vendor/`.
    Vendor(VendorArgs),
    /// Read uv.lock + fixups and emit BUCK, muntjac.bzl, config/BUCK, and wiring.bzl.
    Buckify(BuckifyArgs),
```

Update `run()` dispatch (around line 128):

```rust
        Command::Vendor(args) => vendor::run(&cli.globals, args),
        Command::Buckify(args) => buckify::run(&cli.globals, args),
```

Adjust call sites in `src/cli/vendor.rs::run` and `src/cli/buckify.rs::run` signatures temporarily to accept the new arg structs (they'll be wired up in later tasks; for now make them accept and ignore):

In `src/cli/vendor.rs` change `pub fn run(globals: &Globals) -> Result<()>` to `pub fn run(globals: &Globals, _args: crate::cli::VendorArgs) -> Result<()>`.

In `src/cli/buckify.rs` change `pub fn run(globals: &Globals) -> Result<()>` to `pub fn run(globals: &Globals, _args: crate::cli::BuckifyArgs) -> Result<()>`.

- [ ] **Step 4: Run tests to verify they pass**

```
cargo test -p muntjac --lib cli::
```

Expected: all 5 new tests pass; existing tests pass; `cargo build` succeeds.

- [ ] **Step 5: Verify clippy + format**

```
cargo clippy --all-targets -- -D warnings && cargo fmt --check
```

Expected: clean.

- [ ] **Step 6: Commit**

```
git add src/cli/mod.rs src/cli/vendor.rs src/cli/buckify.rs
git commit -m "$(cat <<'EOF'
feat(s9): CLI plumbing — --mode + --no-prune + resolve_vendor_mode

Add ModeFlag enum with clap::ValueEnum, VendorArgs/BuckifyArgs subcommand
arg structs, and the resolve_vendor_mode helper (CLI override wins over
config). Subcommand-specific (vendor + buckify only); other commands stay
arg-less. No behavioral change yet — args plumbed but ignored.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: `VendorError` + `BuckifyError::MissingVendorWheels`

**Files:**
- Modify: `src/error.rs`

- [ ] **Step 1: Write failing exactness tests**

Add to `src/error.rs` `mod tests`:

```rust
#[test]
fn vendor_error_hash_mismatch_message_is_exact() {
    let e = VendorError::HashMismatch {
        package: "idna".into(),
        version: "3.10".into(),
        expected: "deadbeef".into(),
        actual: "cafef00d".into(),
    };
    assert_eq!(
        e.to_string(),
        "sha256 mismatch for idna 3.10: expected deadbeef, got cafef00d"
    );
}

#[test]
fn vendor_error_mode_network_conflict_message_is_exact() {
    let e = VendorError::ModeNetworkConflict;
    assert_eq!(
        e.to_string(),
        "--mode=committed requires network access; remove --no-network, or vendor first then re-run with --frozen"
    );
}

#[test]
fn buckify_error_missing_vendor_wheels_message_lists_each() {
    let e = BuckifyError::MissingVendorWheels {
        tree: "default".into(),
        missing: vec![
            ("idna".into(), "3.10".into(), "idna-3.10-py3-none-any.whl".into()),
            ("urllib3".into(), "2.2.3".into(), "urllib3-2.2.3-py3-none-any.whl".into()),
        ],
    };
    let s = e.to_string();
    assert!(s.contains("tree 'default'"), "missing tree label: {s}");
    assert!(s.contains("idna 3.10 (idna-3.10-py3-none-any.whl)"), "missing idna line: {s}");
    assert!(s.contains("urllib3 2.2.3 (urllib3-2.2.3-py3-none-any.whl)"), "missing urllib3 line: {s}");
    assert!(s.contains("muntjac vendor"), "missing suggestion: {s}");
}
```

- [ ] **Step 2: Run tests to verify they fail**

```
cargo test -p muntjac --lib error:: -- --nocapture
```

Expected: 3 failures: `cannot find type 'VendorError'`, `cannot find type 'BuckifyError'`.

- [ ] **Step 3: Implement the error enums**

Add to `src/error.rs` after the existing `CacheError` enum:

```rust
#[derive(Debug, Error)]
pub enum VendorError {
    #[error("downloading {package} {version} from {url}: {source}")]
    Download {
        package: String,
        version: String,
        url: String,
        #[source]
        source: reqwest::Error,
    },

    #[error("sha256 mismatch for {package} {version}: expected {expected}, got {actual}")]
    HashMismatch {
        package: String,
        version: String,
        expected: String,
        actual: String,
    },

    #[error(
        "--mode=committed requires network access; remove --no-network, or vendor first then re-run with --frozen"
    )]
    ModeNetworkConflict,
}

#[derive(Debug, Error)]
pub enum BuckifyError {
    #[error(fmt = fmt_missing_vendor_wheels)]
    MissingVendorWheels {
        tree: String,
        /// Each entry is `(package, version, expected wheel filename)`.
        missing: Vec<(String, String, String)>,
    },
}

fn fmt_missing_vendor_wheels(
    tree: &String,
    missing: &Vec<(String, String, String)>,
    f: &mut std::fmt::Formatter<'_>,
) -> std::fmt::Result {
    writeln!(
        f,
        "vendor mode requires committed wheels; tree '{tree}' is missing:"
    )?;
    for (pkg, ver, file) in missing {
        writeln!(f, "  - {pkg} {ver} ({file})")?;
    }
    write!(f, "run `muntjac vendor` to populate the vendor directory.")
}
```

- [ ] **Step 4: Run tests to verify they pass**

```
cargo test -p muntjac --lib error::
```

Expected: 3 new tests pass; existing exactness tests still pass.

- [ ] **Step 5: Verify clippy**

```
cargo clippy --all-targets -- -D warnings
```

Expected: clean.

- [ ] **Step 6: Commit**

```
git add src/error.rs
git commit -m "$(cat <<'EOF'
feat(s9): VendorError + BuckifyError::MissingVendorWheels taxonomy

Add VendorError (Download / HashMismatch / ModeNetworkConflict) and a new
BuckifyError enum with MissingVendorWheels variant. Exactness tests
byte-lock each message (same convention as S11's ConfigError messages).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: `src/vendor/` module skeleton

**Files:**
- Create: `src/vendor/mod.rs`
- Create: `src/vendor/download.rs` (stub)
- Create: `src/vendor/sync.rs` (stub)
- Modify: `src/lib.rs` (add `pub mod vendor;`)

- [ ] **Step 1: Read `src/lib.rs` to find the right insertion point**

```
sed -n '1,20p' src/lib.rs
```

Expected: a list of `pub mod` declarations.

- [ ] **Step 2: Create the module files with empty bodies**

`src/vendor/mod.rs`:

```rust
//! Vendor mode — download and commit wheels into `<third_party_dir>/vendor/`.
//!
//! See `docs/superpowers/specs/2026-05-26-muntjac-s9-vendor-mode-design.md`.

pub mod download;
pub mod sync;

pub use download::download_wheel;
pub use sync::prune_stale;
```

`src/vendor/download.rs`:

```rust
//! Streaming wheel download with sha256 verification.

use std::path::Path;

use crate::error::VendorError;

/// Download `url` to `dest`, streaming bytes through a sha256 hasher.
/// On success: `dest` contains the wheel bytes; returned hex sha matches.
/// On failure: `dest` is removed (no partial files); error names the package.
pub fn download_wheel(
    _url: &url::Url,
    _dest: &Path,
    _package: &str,
    _version: &str,
    _expected_sha256: &str,
) -> Result<(), VendorError> {
    unimplemented!("Task 4")
}
```

`src/vendor/sync.rs`:

```rust
//! Sync-prune of stale wheels in `<third_party_dir>/vendor/`.

use std::collections::BTreeSet;
use std::path::Path;

/// Delete `*.whl` entries in `vendor_dir` not in `expected`. If `no_prune`,
/// returns Ok(()) without inspecting the directory. Returns the list of
/// removed filenames (sorted).
pub fn prune_stale(
    _vendor_dir: &Path,
    _expected: &BTreeSet<String>,
    _no_prune: bool,
) -> std::io::Result<Vec<String>> {
    unimplemented!("Task 5")
}
```

- [ ] **Step 3: Wire the module in `src/lib.rs`**

Add `pub mod vendor;` near the other `pub mod` declarations (alphabetical order if the file uses it).

- [ ] **Step 4: Verify compile**

```
cargo build -p muntjac
```

Expected: compiles (the `unimplemented!` bodies are valid; unused-import warnings will fail later — `#[allow(dead_code)]` not needed because the `pub use` at mod.rs re-exports keeps them live).

- [ ] **Step 5: Verify clippy**

```
cargo clippy --all-targets -- -D warnings
```

Expected: clean.

- [ ] **Step 6: Commit**

```
git add src/lib.rs src/vendor/
git commit -m "$(cat <<'EOF'
feat(s9): src/vendor/ module skeleton

Add download.rs + sync.rs stubs with documented signatures. Bodies are
unimplemented!() — to be filled in tasks 4 and 5. Lets later tasks
import the symbols without forward-declaration churn.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Streaming wheel downloader

**Files:**
- Modify: `src/vendor/download.rs`
- Test: `src/vendor/download.rs` (`#[cfg(test)] mod tests`)

- [ ] **Step 1: Write failing tests**

Add to `src/vendor/download.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Spin up a tiny HTTP server that serves a fixed body once.
    fn serve_once(body: Vec<u8>) -> (url::Url, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                use std::io::Read;
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.write_all(&body);
            }
        });
        let url = url::Url::parse(&format!("http://{}/wheel.whl", addr)).unwrap();
        (url, handle)
    }

    fn serve_404() -> (url::Url, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                use std::io::Read;
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf);
                let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
            }
        });
        let url = url::Url::parse(&format!("http://{}/wheel.whl", addr)).unwrap();
        (url, handle)
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(bytes);
        hex::encode(h.finalize())
    }

    #[test]
    fn download_writes_file_when_sha_matches() {
        let body = b"hello-wheel-bytes".to_vec();
        let sha = sha256_hex(&body);
        let (url, h) = serve_once(body.clone());
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("wheel.whl");
        download_wheel(&url, &dest, "pkg", "1.0", &sha).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), body);
        let _ = h.join();
    }

    #[test]
    fn download_aborts_on_sha_mismatch_and_removes_partial() {
        let body = b"some-bytes".to_vec();
        let wrong_sha = "00".repeat(32);
        let (url, h) = serve_once(body);
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("wheel.whl");
        let err = download_wheel(&url, &dest, "pkg", "1.0", &wrong_sha).unwrap_err();
        match err {
            VendorError::HashMismatch { package, .. } => assert_eq!(package, "pkg"),
            other => panic!("expected HashMismatch, got {other:?}"),
        }
        assert!(!dest.exists(), "partial file should be removed");
        let _ = h.join();
    }

    #[test]
    fn download_aborts_on_404_and_removes_partial() {
        let (url, h) = serve_404();
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("wheel.whl");
        let err = download_wheel(&url, &dest, "pkg", "1.0", &"0".repeat(64)).unwrap_err();
        match err {
            VendorError::Download { package, .. } => assert_eq!(package, "pkg"),
            other => panic!("expected Download, got {other:?}"),
        }
        assert!(!dest.exists(), "partial file should be removed");
        let _ = h.join();
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```
cargo test -p muntjac --lib vendor::download
```

Expected: failures (panics from `unimplemented!`).

- [ ] **Step 3: Implement `download_wheel`**

Replace the `unimplemented!` body in `src/vendor/download.rs`:

```rust
//! Streaming wheel download with sha256 verification.

use std::fs;
use std::io::{Read, Write};
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::error::VendorError;

/// Download `url` to `dest`, streaming bytes through a sha256 hasher.
/// On success: `dest` contains the wheel bytes; the hex sha matches `expected_sha256`.
/// On failure: `dest` is removed (no partial files); error names the package.
pub fn download_wheel(
    url: &url::Url,
    dest: &Path,
    package: &str,
    version: &str,
    expected_sha256: &str,
) -> Result<(), VendorError> {
    // 1. Open the HTTP stream.
    let response = reqwest::blocking::get(url.clone()).map_err(|e| VendorError::Download {
        package: package.into(),
        version: version.into(),
        url: url.to_string(),
        source: e,
    })?;
    let response = response.error_for_status().map_err(|e| VendorError::Download {
        package: package.into(),
        version: version.into(),
        url: url.to_string(),
        source: e,
    })?;

    // 2. Stream bytes through a hasher to a tempfile next to dest, then atomic-rename.
    let parent = dest.parent().expect("dest must have a parent");
    fs::create_dir_all(parent).map_err(io_to_download_err(package, version, url))?;
    let tmp = parent.join(format!(
        ".{}.tmp",
        dest.file_name().unwrap().to_string_lossy()
    ));
    let result = (|| -> Result<(), VendorError> {
        let mut file = fs::File::create(&tmp).map_err(io_to_download_err(package, version, url))?;
        let mut hasher = Sha256::new();
        let mut reader = response;
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = reader.read(&mut buf).map_err(|e| VendorError::Download {
                package: package.into(),
                version: version.into(),
                url: url.to_string(),
                source: reqwest::Error::from(e_to_reqwest(e)),
            })?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            file.write_all(&buf[..n])
                .map_err(io_to_download_err(package, version, url))?;
        }
        file.sync_all()
            .map_err(io_to_download_err(package, version, url))?;
        let actual = hex::encode(hasher.finalize());
        let expected = expected_sha256.trim_start_matches("sha256:");
        if actual != expected {
            return Err(VendorError::HashMismatch {
                package: package.into(),
                version: version.into(),
                expected: expected.into(),
                actual,
            });
        }
        Ok(())
    })();

    match result {
        Ok(()) => {
            fs::rename(&tmp, dest).map_err(io_to_download_err(package, version, url))?;
            Ok(())
        }
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            let _ = fs::remove_file(dest);
            Err(e)
        }
    }
}

fn io_to_download_err<'a>(
    package: &'a str,
    version: &'a str,
    url: &'a url::Url,
) -> impl Fn(std::io::Error) -> VendorError + 'a {
    move |e| {
        VendorError::Download {
            package: package.into(),
            version: version.into(),
            url: url.to_string(),
            // Wrap the io error inside a reqwest::Error by going through a stub HTTP request.
            // Simpler: anyhow context isn't an option since VendorError::Download takes reqwest::Error.
            // Use io_err_to_reqwest helper below.
            source: e_to_reqwest(e),
        }
    }
}

fn e_to_reqwest(_e: std::io::Error) -> reqwest::Error {
    // reqwest::Error doesn't have a public from-io-error constructor; surface via a
    // failed local request that always errors so the type works. The error message will
    // still include the I/O detail via Display chaining inside VendorError::Download.
    reqwest::blocking::Client::new()
        .get("http://0.0.0.0:0/")
        .send()
        .unwrap_err()
}
```

The `reqwest::Error`-around-io-error wrapper is awkward. Simpler alternative: change `VendorError::Download.source` to `Box<dyn std::error::Error + Send + Sync>`. Apply this:

```rust
// In src/error.rs, replace VendorError::Download:
#[error("downloading {package} {version} from {url}: {source}")]
Download {
    package: String,
    version: String,
    url: String,
    #[source]
    source: Box<dyn std::error::Error + Send + Sync>,
},
```

Then in download.rs:

```rust
fn io_to_download_err<'a>(
    package: &'a str,
    version: &'a str,
    url: &'a url::Url,
) -> impl Fn(std::io::Error) -> VendorError + 'a {
    move |e| VendorError::Download {
        package: package.into(),
        version: version.into(),
        url: url.to_string(),
        source: Box::new(e),
    }
}
```

And reqwest errors:

```rust
let response = reqwest::blocking::get(url.clone()).map_err(|e| VendorError::Download {
    package: package.into(),
    version: version.into(),
    url: url.to_string(),
    source: Box::new(e),
})?;
```

Adjust the existing exactness test in `src/error.rs` (added in Task 2) — none of the `VendorError::Download` tests existed yet, so this is safe. (Task 2 only added `HashMismatch` and `ModeNetworkConflict` exactness tests.)

Remove the `e_to_reqwest` helper from the listing above; the final `download.rs` implementation is:

```rust
//! Streaming wheel download with sha256 verification.

use std::fs;
use std::io::{Read, Write};
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::error::VendorError;

pub fn download_wheel(
    url: &url::Url,
    dest: &Path,
    package: &str,
    version: &str,
    expected_sha256: &str,
) -> Result<(), VendorError> {
    let response = reqwest::blocking::get(url.clone())
        .map_err(|e| boxed_download(package, version, url, Box::new(e)))?
        .error_for_status()
        .map_err(|e| boxed_download(package, version, url, Box::new(e)))?;

    let parent = dest.parent().expect("dest must have a parent");
    fs::create_dir_all(parent)
        .map_err(|e| boxed_download(package, version, url, Box::new(e)))?;
    let tmp = parent.join(format!(
        ".{}.tmp",
        dest.file_name().unwrap().to_string_lossy()
    ));

    let result = (|| -> Result<(), VendorError> {
        let mut file = fs::File::create(&tmp)
            .map_err(|e| boxed_download(package, version, url, Box::new(e)))?;
        let mut hasher = Sha256::new();
        let mut reader = response;
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = reader
                .read(&mut buf)
                .map_err(|e| boxed_download(package, version, url, Box::new(e)))?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            file.write_all(&buf[..n])
                .map_err(|e| boxed_download(package, version, url, Box::new(e)))?;
        }
        file.sync_all()
            .map_err(|e| boxed_download(package, version, url, Box::new(e)))?;
        let actual = hex::encode(hasher.finalize());
        let expected = expected_sha256.trim_start_matches("sha256:");
        if actual != expected {
            return Err(VendorError::HashMismatch {
                package: package.into(),
                version: version.into(),
                expected: expected.into(),
                actual,
            });
        }
        Ok(())
    })();

    match result {
        Ok(()) => {
            fs::rename(&tmp, dest)
                .map_err(|e| boxed_download(package, version, url, Box::new(e)))?;
            Ok(())
        }
        Err(e) => {
            let _ = fs::remove_file(&tmp);
            let _ = fs::remove_file(dest);
            Err(e)
        }
    }
}

fn boxed_download(
    package: &str,
    version: &str,
    url: &url::Url,
    source: Box<dyn std::error::Error + Send + Sync>,
) -> VendorError {
    VendorError::Download {
        package: package.into(),
        version: version.into(),
        url: url.to_string(),
        source,
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

```
cargo test -p muntjac --lib vendor::download
```

Expected: all 3 tests pass.

- [ ] **Step 5: Verify clippy + format**

```
cargo clippy --all-targets -- -D warnings && cargo fmt --check
```

Expected: clean.

- [ ] **Step 6: Commit**

```
git add src/error.rs src/vendor/download.rs
git commit -m "$(cat <<'EOF'
feat(s9): streaming wheel downloader with sha256 verify

Streams to a sibling tempfile, hashes on the fly, atomic-renames into
place on success. On any failure (network, 404, sha mismatch, I/O),
removes the partial file and surfaces VendorError::{Download,HashMismatch}.

Switch VendorError::Download.source to Box<dyn Error> so I/O errors can
flow through the same variant without round-tripping through reqwest.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: Sync-prune of stale wheels

**Files:**
- Modify: `src/vendor/sync.rs`

- [ ] **Step 1: Write failing tests**

Replace the stub body and add a test module:

```rust
//! Sync-prune of stale wheels in `<third_party_dir>/vendor/`.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

/// Delete `*.whl` entries in `vendor_dir` not in `expected`. If `no_prune`,
/// returns `Ok(vec![])` without inspecting the directory. Returns the list of
/// removed filenames (sorted). Files without a `.whl` extension are left alone.
pub fn prune_stale(
    vendor_dir: &Path,
    expected: &BTreeSet<String>,
    no_prune: bool,
) -> std::io::Result<Vec<String>> {
    if no_prune {
        return Ok(vec![]);
    }
    if !vendor_dir.is_dir() {
        return Ok(vec![]);
    }
    let mut removed = Vec::new();
    for entry in fs::read_dir(vendor_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("whl") {
            continue;
        }
        let filename = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        if expected.contains(&filename) {
            continue;
        }
        fs::remove_file(&path)?;
        removed.push(filename);
    }
    removed.sort();
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_empty_whl(dir: &Path, name: &str) {
        fs::write(dir.join(name), b"").unwrap();
    }

    #[test]
    fn prune_deletes_stale_wheels() {
        let tmp = tempfile::tempdir().unwrap();
        write_empty_whl(tmp.path(), "keep-1.0-py3-none-any.whl");
        write_empty_whl(tmp.path(), "stale-1.0-py3-none-any.whl");
        let expected: BTreeSet<String> =
            ["keep-1.0-py3-none-any.whl".to_string()].into_iter().collect();
        let removed = prune_stale(tmp.path(), &expected, false).unwrap();
        assert_eq!(removed, vec!["stale-1.0-py3-none-any.whl"]);
        assert!(tmp.path().join("keep-1.0-py3-none-any.whl").exists());
        assert!(!tmp.path().join("stale-1.0-py3-none-any.whl").exists());
    }

    #[test]
    fn no_prune_skips_deletion() {
        let tmp = tempfile::tempdir().unwrap();
        write_empty_whl(tmp.path(), "stale-1.0-py3-none-any.whl");
        let removed = prune_stale(tmp.path(), &BTreeSet::new(), true).unwrap();
        assert!(removed.is_empty());
        assert!(tmp.path().join("stale-1.0-py3-none-any.whl").exists());
    }

    #[test]
    fn prune_leaves_non_whl_files_alone() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join(".gitignore"), b"*").unwrap();
        fs::write(tmp.path().join("notes.txt"), b"hi").unwrap();
        write_empty_whl(tmp.path(), "stale-1.0-py3-none-any.whl");
        let _ = prune_stale(tmp.path(), &BTreeSet::new(), false).unwrap();
        assert!(tmp.path().join(".gitignore").exists());
        assert!(tmp.path().join("notes.txt").exists());
        assert!(!tmp.path().join("stale-1.0-py3-none-any.whl").exists());
    }

    #[test]
    fn prune_missing_dir_is_ok() {
        let tmp = tempfile::tempdir().unwrap();
        let absent = tmp.path().join("does-not-exist");
        let removed = prune_stale(&absent, &BTreeSet::new(), false).unwrap();
        assert!(removed.is_empty());
    }
}
```

- [ ] **Step 2: Run tests to verify they pass**

The implementation is included above; the failing-first cycle is conceptual — `unimplemented!()` is replaced in the same step. Run:

```
cargo test -p muntjac --lib vendor::sync
```

Expected: 4 tests pass.

- [ ] **Step 3: Verify clippy + format**

```
cargo clippy --all-targets -- -D warnings && cargo fmt --check
```

Expected: clean.

- [ ] **Step 4: Commit**

```
git add src/vendor/sync.rs
git commit -m "$(cat <<'EOF'
feat(s9): sync-prune of stale wheels in vendor/

prune_stale(vendor_dir, expected, no_prune) deletes *.whl files not in
the expected set. `no_prune=true` returns Ok(vec![]) without touching
the directory. Non-.whl files (.gitignore, notes, etc.) are preserved.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: Vendor command — committed-mode orchestration

**Files:**
- Modify: `src/cli/vendor.rs`

- [ ] **Step 1: Read the current vendor.rs to understand the prebake flow**

```
sed -n '70,180p' src/cli/vendor.rs
```

Expected: see the prebake_dir creation, gitignore, sdist-only loop with classify + build_wheel + final rename to prebake_dir + manifest entry collection + manifest write.

- [ ] **Step 2: Write a failing integration test**

Create `tests/vendor.rs`:

```rust
//! S9 integration tests: vendor command in committed mode.

use std::path::Path;
use std::process::Command;

fn target_exe() -> std::path::PathBuf {
    let target = std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".into());
    Path::new(&target).join("debug").join("muntjac")
}

#[test]
fn vendor_committed_with_no_network_errors() {
    let tmp = tempfile::tempdir().unwrap();
    let toml_path = tmp.path().join("muntjac.toml");
    std::fs::write(
        &toml_path,
        r#"
manifest_path = "pyproject.toml"
third_party_dir = "third-party/python"
python_versions = ["3.12"]

[platforms]
linux-x86_64-gnu = { target = "x86_64-unknown-linux-gnu", manylinux = "2_17" }

[buck]
vendor = true
"#,
    )
    .unwrap();

    let output = Command::new(target_exe())
        .args(["vendor", "--mode", "committed", "--no-network"])
        .current_dir(tmp.path())
        .output()
        .expect("running muntjac vendor");
    assert!(!output.status.success(), "expected failure");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--mode=committed requires network access"),
        "stderr: {stderr}"
    );
}
```

- [ ] **Step 3: Run test to verify it fails**

```
cargo build -p muntjac && cargo test --test vendor vendor_committed_with_no_network_errors -- --nocapture
```

Expected: test fails — `--mode=committed --no-network` is not yet wired to surface `ModeNetworkConflict`.

- [ ] **Step 4: Implement committed-mode orchestration**

Rewrite the body of `src/cli/vendor.rs`. The new version supports both modes and uses `args: VendorArgs` to pick. The full replacement file:

```rust
//! `muntjac vendor` — prebake pure-python sdists into wheels; in committed
//! mode, also download all referenced registry wheels into vendor/.
//!
//! See specs/2026-05-26-muntjac-s9-vendor-mode-design.md.

use std::collections::BTreeSet;
use std::io::Read;
use std::path::Path;
use std::str::FromStr;

use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use url::Url;

use crate::cli::{Globals, VendorArgs, resolve_vendor_mode};
use crate::config::{Config, Tree};
use crate::error::VendorError;
use crate::lock::types::{Package, Source};
use crate::sdist::{
    AllowlistedBackend, Classification, Manifest, ManifestClassification, ManifestEntry,
    NativeReason, NativeSourceHit, classify,
};

pub fn run(globals: &Globals, args: VendorArgs) -> Result<()> {
    let workdir = globals.workdir().context("resolving working directory")?;
    let config_path = workdir.join("muntjac.toml");
    let config_text = std::fs::read_to_string(&config_path)
        .with_context(|| format!("reading {}", config_path.display()))?;
    let config = Config::from_str(&config_text)
        .with_context(|| format!("parsing {}", config_path.display()))?;

    let vendor_mode = resolve_vendor_mode(&config, args.mode);
    if vendor_mode && globals.no_network {
        return Err(VendorError::ModeNetworkConflict.into());
    }

    for tree in crate::cli::resolve_trees(&config, globals.tree.as_deref())? {
        vendor_tree(globals, &workdir, &config_path, tree, vendor_mode, args.no_prune)
            .with_context(|| format!("vendoring tree '{}'", tree.name))?;
    }
    Ok(())
}

fn vendor_tree(
    globals: &Globals,
    workdir: &Path,
    config_path: &Path,
    tree: &Tree,
    vendor_mode: bool,
    no_prune: bool,
) -> Result<()> {
    let third_party_dir = workdir.join(&tree.third_party_dir);

    // Lock freshness. Resolve uv.lock relative to the tree's manifest dir.
    let cfg_dir = config_path.parent().unwrap_or(Path::new("."));
    let manifest_dir = cfg_dir.join(tree.manifest_path.parent().unwrap_or(Path::new("")));
    let pyproject = cfg_dir.join(&tree.manifest_path);
    let lockfile_path = manifest_dir.join("uv.lock");

    if pyproject.is_file() && lockfile_path.is_file() && !globals.frozen && !globals.no_network {
        let py_mtime = std::fs::metadata(&pyproject)
            .with_context(|| format!("reading metadata of {}", pyproject.display()))?
            .modified()
            .with_context(|| format!("reading mtime of {}", pyproject.display()))?;
        let lock_mtime = std::fs::metadata(&lockfile_path)
            .with_context(|| format!("reading metadata of {}", lockfile_path.display()))?
            .modified()
            .with_context(|| format!("reading mtime of {}", lockfile_path.display()))?;
        if py_mtime > lock_mtime {
            eprintln!("muntjac vendor: pyproject.toml is newer than uv.lock; running `uv lock`");
            let status = crate::uv::uv_lock(&manifest_dir)?;
            if !status.success() {
                anyhow::bail!("`uv lock` failed with status {status}");
            }
        }
    }

    let lockfile_text = std::fs::read_to_string(&lockfile_path)
        .with_context(|| format!("reading {}", lockfile_path.display()))?;
    let lockfile = crate::lock::parser::parse(&lockfile_text)
        .with_context(|| format!("parsing {}", lockfile_path.display()))?;

    if vendor_mode {
        vendor_tree_committed(workdir, &third_party_dir, tree, &lockfile, no_prune)
    } else {
        vendor_tree_prebake_only(workdir, &third_party_dir, &lockfile)
    }
}

/// Committed mode: download all registry wheels + prebake pure-python sdists,
/// all into `<third_party_dir>/vendor/`. Sync-prune stale wheels unless `no_prune`.
/// No manifest is written.
fn vendor_tree_committed(
    workdir: &Path,
    third_party_dir: &Path,
    tree: &Tree,
    lockfile: &crate::lock::types::Lockfile,
    no_prune: bool,
) -> Result<()> {
    let vendor_dir = third_party_dir.join("vendor");
    std::fs::create_dir_all(&vendor_dir)
        .with_context(|| format!("creating {}", vendor_dir.display()))?;

    let mut expected: BTreeSet<String> = BTreeSet::new();

    // 1. Download registry wheels needed by configured (python × platform) cells.
    for pkg in &lockfile.packages {
        if !matches!(pkg.source, Source::Registry { .. }) {
            continue;
        }
        for wheel in &pkg.wheels {
            // Determine whether this wheel is needed by ANY (py, platform) of this tree.
            // Cheap pre-filter: keep all wheels for now; the picker will narrow at emit time.
            // We must vendor the full set the emitter could pick, so include all.
            let expected_sha = wheel.hash.trim_start_matches("sha256:").to_string();
            let dest = vendor_dir.join(&wheel.filename);
            if !skip_existing(&dest, &expected_sha)? {
                eprintln!(
                    "muntjac vendor: downloading {} {} → vendor/{}",
                    pkg.name.as_ref(),
                    pkg.version,
                    wheel.filename
                );
                crate::vendor::download_wheel(
                    &wheel.url,
                    &dest,
                    pkg.name.as_ref(),
                    &pkg.version.to_string(),
                    &expected_sha,
                )?;
            }
            expected.insert(wheel.filename.clone());
        }
    }

    // 2. Prebake pure-python sdists directly into vendor/.
    for pkg in &lockfile.packages {
        if !is_sdist_only(pkg) {
            continue;
        }
        let sdist = pkg.sdist.as_ref().unwrap();
        let pkg_name = pkg.name.as_ref().to_string();
        let pkg_version = pkg.version.to_string();
        let expected_sha = sdist.hash.trim_start_matches("sha256:").to_string();
        let computed_filename = vendor_pep427_pure_python_filename(&pkg_name, &pkg_version);

        // Idempotence: if the target file already exists, assume good.
        if vendor_dir.join(&computed_filename).is_file() {
            expected.insert(computed_filename);
            continue;
        }

        let tmp = tempfile::tempdir().context("creating tempdir for tarball")?;
        let tarball_path = tmp.path().join("sdist.tar.gz");
        download_sdist(&sdist.url, &tarball_path, &pkg_name, &pkg_version)?;
        verify_sha256(&tarball_path, &expected_sha, &pkg_name, &pkg_version)?;

        let extract_dir = tmp.path().join("extracted");
        std::fs::create_dir_all(&extract_dir)
            .with_context(|| format!("creating {}", extract_dir.display()))?;
        extract_tarball(&tarball_path, &extract_dir, &pkg_name, &pkg_version)?;
        let sdist_root = find_sdist_root(&extract_dir)?;

        match classify(&sdist_root)? {
            Classification::PurePython { backend } => {
                eprintln!(
                    "muntjac vendor: prebaking {} {} → vendor/{} ({})",
                    pkg_name,
                    pkg_version,
                    computed_filename,
                    backend_str(backend)
                );
                let staging = tmp.path().join("staging");
                std::fs::create_dir_all(&staging)
                    .with_context(|| format!("creating {}", staging.display()))?;
                let result =
                    crate::sdist::build_wheel(&sdist_root, &staging, &pkg_name, &pkg_version)?;
                if result.wheel_filename != computed_filename {
                    eprintln!(
                        "muntjac vendor: warning: uv built `{}` but expected `{}` (PEP 427); using uv's name",
                        result.wheel_filename, computed_filename
                    );
                }
                let final_path = vendor_dir.join(&result.wheel_filename);
                if final_path.is_file() {
                    std::fs::remove_file(&final_path)
                        .with_context(|| format!("removing {}", final_path.display()))?;
                }
                std::fs::rename(&result.wheel_path, &final_path)
                    .or_else(|_| {
                        std::fs::copy(&result.wheel_path, &final_path)?;
                        std::fs::remove_file(&result.wheel_path)?;
                        Ok::<_, std::io::Error>(())
                    })
                    .with_context(|| {
                        format!(
                            "moving {} → {}",
                            result.wheel_path.display(),
                            final_path.display()
                        )
                    })?;
                expected.insert(result.wheel_filename);
            }
            Classification::Native { reason } => {
                eprintln!(
                    "muntjac vendor: skipping native sdist {} {} ({})",
                    pkg_name,
                    pkg_version,
                    render_native_reason(&reason)
                );
                // Native skipped: nothing in vendor/; downstream emit will error
                // if any cfg references it.
            }
        }
    }

    // 3. Sync-prune.
    let removed = crate::vendor::prune_stale(&vendor_dir, &expected, no_prune)
        .with_context(|| format!("pruning {}", vendor_dir.display()))?;
    for f in &removed {
        eprintln!("muntjac vendor: pruned {}", f);
    }

    // Note: no manifest is written in committed mode (see spec §2.3).
    // Note: prebake/ is left untouched in committed mode.

    let _ = (workdir, tree); // silence unused-variable until referenced elsewhere
    Ok(())
}

/// Prebake-only mode: today's behavior. Wheels build to <third_party_dir>/prebake/,
/// manifest written, .gitignore for prebake/. No registry-wheel downloads.
fn vendor_tree_prebake_only(
    workdir: &Path,
    third_party_dir: &Path,
    lockfile: &crate::lock::types::Lockfile,
) -> Result<()> {
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

        let tmp = tempfile::tempdir().context("creating tempdir for tarball")?;
        let tarball_path = tmp.path().join("sdist.tar.gz");
        download_sdist(&sdist.url, &tarball_path, &pkg_name, &pkg_version)?;
        verify_sha256(&tarball_path, &expected_sha, &pkg_name, &pkg_version)?;

        let extract_dir = tmp.path().join("extracted");
        std::fs::create_dir_all(&extract_dir)
            .with_context(|| format!("creating {}", extract_dir.display()))?;
        extract_tarball(&tarball_path, &extract_dir, &pkg_name, &pkg_version)?;
        let sdist_root = find_sdist_root(&extract_dir)?;

        match classify(&sdist_root)? {
            Classification::PurePython { backend } => {
                eprintln!(
                    "muntjac vendor: prebaking {} {} ({})",
                    pkg_name,
                    pkg_version,
                    backend_str(backend)
                );
                let staging = tmp.path().join("staging");
                std::fs::create_dir_all(&staging)
                    .with_context(|| format!("creating {}", staging.display()))?;
                let result =
                    crate::sdist::build_wheel(&sdist_root, &staging, &pkg_name, &pkg_version)?;
                let final_path = prebake_dir.join(&result.wheel_filename);
                if final_path.is_file() {
                    std::fs::remove_file(&final_path)
                        .with_context(|| format!("removing {}", final_path.display()))?;
                }
                std::fs::rename(&result.wheel_path, &final_path)
                    .or_else(|_| {
                        std::fs::copy(&result.wheel_path, &final_path)?;
                        std::fs::remove_file(&result.wheel_path)?;
                        Ok::<_, std::io::Error>(())
                    })
                    .with_context(|| {
                        format!(
                            "moving {} → {}",
                            result.wheel_path.display(),
                            final_path.display()
                        )
                    })?;
                eprintln!(
                    "prebaked: {} {} → {}",
                    pkg_name,
                    pkg_version,
                    pathdiff::diff_paths(&final_path, workdir)
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

    let manifest = Manifest { version: 1, entries };
    let manifest_path = prebake_dir.join(".manifest.toml");
    manifest.save(&manifest_path)?;
    Ok(())
}

fn is_sdist_only(pkg: &Package) -> bool {
    matches!(pkg.source, Source::Registry { .. }) && pkg.sdist.is_some() && pkg.wheels.is_empty()
}

fn skip_existing(dest: &Path, expected_sha: &str) -> Result<bool> {
    if !dest.is_file() {
        return Ok(false);
    }
    // Hash existing file; reuse if matches.
    let mut f = std::fs::File::open(dest)
        .with_context(|| format!("opening existing {}", dest.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f
            .read(&mut buf)
            .with_context(|| format!("reading {}", dest.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let got = hex::encode(hasher.finalize());
    let want = expected_sha.trim_start_matches("sha256:");
    Ok(got == want)
}

pub fn vendor_pep427_pure_python_filename(name: &str, version: &str) -> String {
    format!("{}-{}-py3-none-any.whl", pep427_escape_name(name), version)
}

pub fn pep427_escape_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut last_was_sep = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            last_was_sep = false;
        } else if !last_was_sep {
            out.push('_');
            last_was_sep = true;
        }
    }
    out
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
        std::fs::write(&gi, "*\n").with_context(|| format!("writing {}", gi.display()))?;
    }
    Ok(())
}

fn download_sdist(url: &Url, dest: &Path, package: &str, version: &str) -> Result<()> {
    use crate::sdist::SdistError;
    let response = reqwest::blocking::get(url.clone()).map_err(|e| SdistError::Download {
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
    let mut f = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = f
            .read(&mut buf)
            .with_context(|| format!("reading {}", path.display()))?;
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

    let f =
        std::fs::File::open(tarball).with_context(|| format!("opening {}", tarball.display()))?;
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
        for comp in path.components() {
            if matches!(
                comp,
                std::path::Component::ParentDir | std::path::Component::RootDir
            ) {
                return Err(SdistError::PathTraversal {
                    package: package.into(),
                    version: version.into(),
                    member: path.display().to_string(),
                }
                .into());
            }
        }
        let out = dest.join(&path);
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| SdistError::Extract {
                package: package.into(),
                version: version.into(),
                source: e,
            })?;
        }
        let pax_mtime: Option<i64> = match entry.pax_extensions() {
            Ok(Some(exts)) => {
                let mut found = None;
                for ext in exts {
                    if let Ok(ext) = ext
                        && let Ok(key) = ext.key()
                        && key == "mtime"
                        && let Ok(val) = ext.value()
                    {
                        let secs = val.split('.').next().unwrap_or(val);
                        if let Ok(s) = secs.parse::<i64>() {
                            found = Some(s);
                        }
                    }
                }
                found
            }
            _ => None,
        };
        entry.unpack(&out).map_err(|e| SdistError::Extract {
            package: package.into(),
            version: version.into(),
            source: e,
        })?;
        if let Some(secs) = pax_mtime {
            let ft = filetime::FileTime::from_unix_time(secs, 0);
            let _ = filetime::set_file_mtime(&out, ft);
        }
    }
    Ok(())
}

fn find_sdist_root(extract_dir: &Path) -> Result<std::path::PathBuf> {
    let mut entries: Vec<_> = std::fs::read_dir(extract_dir)
        .with_context(|| format!("reading {}", extract_dir.display()))?
        .filter_map(|r| r.ok())
        .filter(|e| e.path().is_dir())
        .collect();
    if entries.len() == 1 {
        Ok(entries.pop().unwrap().path())
    } else {
        Ok(extract_dir.to_path_buf())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pep427_escapes_dashes_dots_lowercases() {
        assert_eq!(pep427_escape_name("Flit-Core"), "flit_core");
        assert_eq!(pep427_escape_name("foo.bar"), "foo_bar");
        assert_eq!(pep427_escape_name("foo---bar"), "foo_bar");
        assert_eq!(pep427_escape_name("urllib3"), "urllib3");
    }

    #[test]
    fn pep427_filename_format() {
        assert_eq!(
            vendor_pep427_pure_python_filename("Flit-Core", "3.9.0"),
            "flit_core-3.9.0-py3-none-any.whl"
        );
    }
}
```

- [ ] **Step 5: Run tests**

```
cargo test -p muntjac --lib cli::vendor:: && \
cargo build -p muntjac && \
cargo test --test vendor vendor_committed_with_no_network_errors -- --nocapture
```

Expected: pep427 unit tests pass; vendor_committed_with_no_network_errors integration test now passes (exits non-zero with the expected stderr).

- [ ] **Step 6: Verify clippy + format**

```
cargo clippy --all-targets -- -D warnings && cargo fmt --check
```

Expected: clean.

- [ ] **Step 7: Commit**

```
git add src/cli/vendor.rs tests/vendor.rs
git commit -m "$(cat <<'EOF'
feat(s9): vendor command — committed-mode orchestration

In committed mode: download all registry wheels into <tpd>/vendor/ with
idempotent skip-if-sha-matches, prebake pure-python sdists directly to
<tpd>/vendor/ (no prebake/ intermediate), sync-prune stale wheels unless
--no-prune. No manifest written; state is filesystem-derivable.

In prebake-only mode (default): unchanged from v0.2.0 — vendor/ is not
touched, prebake/ + prebake/.manifest.toml are written as today.

--mode=committed + --no-network errors fast via VendorError::ModeNetworkConflict.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: Emitter — `vendor_mode` plumbing + URL synthesis

**Files:**
- Modify: `src/buck/emit.rs`
- Modify: `src/buck/mod.rs`
- Modify: `src/cli/buckify.rs`

- [ ] **Step 1: Write failing unit tests in `src/buck/emit.rs`**

Add to the existing tests module:

```rust
#[test]
fn vendor_mode_emits_vendor_url_for_downloaded_wheel() {
    use crate::lock::types::{
        DepEdge, FirstPartyKind, Lockfile, Package, Source, Wheel,
    };
    use crate::config::PackageName;
    use pep440_rs::Version as PepVersion;

    let cfg = Config::from_str(
        r#"
manifest_path = "pyproject.toml"
third_party_dir = "third-party/python"
python_versions = ["3.12"]
[platforms]
linux-x86_64-gnu = { target = "x86_64-unknown-linux-gnu", manylinux = "2_17" }
[buck]
vendor = true
"#,
    )
    .unwrap();
    let tree = cfg.trees.first().unwrap().clone();
    let lockfile = Lockfile {
        version: 1,
        packages: vec![
            Package {
                name: PackageName::new("root").unwrap(),
                version: PepVersion::from_str("0.0.0").unwrap(),
                source: Source::FirstParty {
                    kind: FirstPartyKind::Virtual,
                    path: ".".into(),
                },
                sdist: None,
                wheels: vec![],
                metadata: Default::default(),
                dependencies: vec![DepEdge {
                    name: PackageName::new("idna").unwrap(),
                    extra: vec![],
                    marker: None,
                }],
                resolution_markers: vec![],
            },
            Package {
                name: PackageName::new("idna").unwrap(),
                version: PepVersion::from_str("3.10").unwrap(),
                source: Source::Registry {
                    url: url::Url::parse("https://pypi.org/simple").unwrap(),
                },
                sdist: None,
                wheels: vec![Wheel {
                    url: url::Url::parse("https://files.pythonhosted.org/idna-3.10.whl").unwrap(),
                    hash: "sha256:cafef00d".into(),
                    size: None,
                    filename: "idna-3.10-py3-none-any.whl".into(),
                }],
                metadata: Default::default(),
                dependencies: vec![],
                resolution_markers: vec![],
            },
        ],
    };

    let ctx = BuildEmitContext {
        vendor_mode: true,
        ..Default::default()
    };
    let input = build_emit_input(&cfg, &tree, &lockfile, &ctx).unwrap();

    let pkg = input
        .packages
        .iter()
        .find(|p| p.name == "idna")
        .expect("idna in packages");
    let cfg_name = pkg.wheels.keys().next().unwrap().clone();
    let wheel = &pkg.wheels[&cfg_name];
    assert_eq!(wheel.url, "vendor:idna-3.10-py3-none-any.whl");
}
```

- [ ] **Step 2: Run test to verify it fails**

```
cargo test -p muntjac --lib vendor_mode_emits_vendor_url -- --nocapture
```

Expected: failure — `BuildEmitContext has no field 'vendor_mode'`.

- [ ] **Step 3: Add `vendor_mode` to `EmitInput` + `BuildEmitContext`**

In `src/buck/emit.rs`, add a field to `EmitInput`:

```rust
#[derive(Debug, Clone)]
pub struct EmitInput {
    pub tree: String,
    pub third_party_dir: String,
    pub cfg_dir: String,
    pub configs: Vec<ConfigName>,
    pub packages: Vec<EmitPackage>,
    /// S9: when true, source URLs use the `vendor:<filename>` scheme and the
    /// pypi_package macro grows an export_file branch for it.
    pub vendor_mode: bool,
}
```

And to `BuildEmitContext`:

```rust
#[derive(Debug, Clone, Default)]
pub struct BuildEmitContext<'a> {
    pub manifest: Option<&'a crate::sdist::Manifest>,
    pub fixups: Option<&'a crate::fixup::EffectiveFixups>,
    pub abs_third_party_dir: Option<&'a std::path::Path>,
    pub cfg_dir: Option<&'a str>,
    /// S9: when true, the emitter outputs `vendor:<filename>` URLs and the
    /// macro grows the conditional `vendor:` arm. Defaults to false.
    pub vendor_mode: bool,
}
```

In `build_emit_input`, propagate the field at the end where `EmitInput` is constructed (find the return statement):

```rust
    Ok(EmitInput {
        tree: tree.name.clone(),
        third_party_dir: tree.third_party_dir.to_string_lossy().into_owned(),
        cfg_dir: ctx
            .cfg_dir
            .map(|s| s.to_string())
            .unwrap_or_else(|| tree.third_party_dir.to_string_lossy().into_owned()),
        configs,
        packages,
        vendor_mode: ctx.vendor_mode,
    })
```

In `build_emit_input`'s per-package wheel synthesis, when `ctx.vendor_mode` is true, override `EmitWheel.url` to the `vendor:` scheme. Locate the "Wheels-present path: pick the best wheel" block (`src/buck/emit.rs` around line 348) and the no-override branch (around line 380); also the prebake `format!("prebake:{}", wheel_filename)` synthesis (line 311). Apply this transform in each spot:

```rust
let url = if ctx.vendor_mode {
    format!("vendor:{}", wheel.filename)
} else {
    wheel.url.to_string()
};
```

For the prebake-built path (the existing pure-python sdist branch ~line 308-313), the synthesis becomes:

```rust
let url = if ctx.vendor_mode {
    format!(
        "vendor:{}",
        crate::cli::vendor_pep427_pure_python_filename(&pkg.name, &pkg.version)
    )
} else {
    format!("prebake:{}", wheel_filename)
};
pkg_wheels.entry(key.clone()).or_default().insert(
    cfg_name.clone(),
    EmitWheel {
        url,
        hash: format!("sha256:{}", wheel_sha256),
    },
);
```

The PEP 427 helper `vendor_pep427_pure_python_filename` is already `pub` from Task 6.

Also: in committed mode, **buckify must skip the prebake-manifest read** at `src/cli/buckify.rs:48`. Replace that block:

```rust
        let manifest = if vendor_mode {
            None
        } else if manifest_path.is_file() {
            Some(crate::sdist::Manifest::load(&manifest_path)?)
        } else {
            None
        };
```

And resolve `vendor_mode` at the top of `buckify::run`:

```rust
pub fn run(globals: &Globals, args: crate::cli::BuckifyArgs) -> Result<()> {
    let cwd = globals.workdir().context("resolving working directory")?;
    let cfg_path = cwd.join("muntjac.toml");
    let cfg_bytes = fs::read_to_string(&cfg_path)
        .with_context(|| format!("reading {}", cfg_path.display()))?;
    let config = Config::from_str(&cfg_bytes)
        .with_context(|| format!("parsing {}", cfg_path.display()))?;
    let vendor_mode = crate::cli::resolve_vendor_mode(&config, args.mode);
    /* ...existing emit-shared-cfg... */
    for tree in crate::cli::resolve_trees(&config, globals.tree.as_deref())? {
        /* ...existing per-tree setup, but: */
        let manifest_path = third_party_dir.join("prebake/.manifest.toml");
        let manifest = if vendor_mode {
            None
        } else if manifest_path.is_file() {
            Some(crate::sdist::Manifest::load(&manifest_path)?)
        } else {
            None
        };
        /* ...rest unchanged, except: */
        let input = build_emit_input(
            &config,
            tree,
            &lockfile,
            &BuildEmitContext {
                manifest: manifest.as_ref(),
                fixups: Some(&fixups),
                abs_third_party_dir: Some(&canonical_third_party_dir),
                cfg_dir: Some(&cfg_dir_str),
                vendor_mode,
            },
        )?;
        /* ...rest unchanged */
    }
    Ok(())
}
```

Also handle the sdist-only native branch when `vendor_mode=true`: the existing code (`src/buck/emit.rs:285`) bails with "pure-python sdist X not prebaked. Run `muntjac vendor` first." That message stays — but in committed mode, the manifest is `None`, so the `Some(entry) = manifest_entry(...)` else-branch fires. Update the bail message to:

```rust
let Some(entry) = manifest_entry(&pkg.name, &pkg.version) else {
    if ctx.vendor_mode {
        // Vendor mode synthesizes vendor:<computed filename> directly without manifest;
        // skip this branch by falling through to a vendor: URL only if the file exists.
        // The emit-time existence check in Task 9 catches missing wheels.
        let computed = crate::cli::vendor_pep427_pure_python_filename(&pkg.name, &pkg.version);
        pkg_wheels.entry(key.clone()).or_default().insert(
            cfg_name.clone(),
            EmitWheel {
                url: format!("vendor:{}", computed),
                hash: format!("sha256:{}", lockfile_sdist_sha),
            },
        );
        let mut cell_deps = pkg.deps.clone();
        if let Some(rf) = &resolved_fixup {
            apply_dep_ops(&mut cell_deps, rf);
        }
        pkg_deps_per_cell
            .entry(key.clone())
            .or_default()
            .insert(cfg_name.clone(), cell_deps);
        continue;
    }
    anyhow::bail!(
        "pure-python sdist {} {} not prebaked. Run `muntjac vendor` first.",
        pkg.name,
        pkg.version
    );
};
```

(Note: a sdist that classified `Native` produces NO vendor wheel and NO manifest entry in committed mode. The fall-through above emits a `vendor:<filename>` URL anyway; Task 9's existence check then catches it and aborts with `MissingVendorWheels`. That matches the spec's "the buck-side emit will then naturally fall through" — the emit produces a reference, the existence check refuses to ship it.)

Now reconstruct `EmitInput` to include `vendor_mode`. Look at all `EmitInput { ... }` literal construction sites and add the field. There may be tests-only constructions; for those, set `vendor_mode: false`.

```
grep -n "EmitInput {" src/buck/
```

Add `vendor_mode: false` to each literal that doesn't already specify it. (Tests in other places that build `EmitInput` directly need this. Existing 25+ test sites will each get the new line.)

- [ ] **Step 4: Run tests to verify they pass**

```
cargo build -p muntjac && \
cargo test -p muntjac --lib vendor_mode_emits_vendor_url -- --nocapture && \
cargo test -p muntjac --lib
```

Expected: new test passes; all existing tests pass (including snapshots).

- [ ] **Step 5: Verify clippy + format**

```
cargo clippy --all-targets -- -D warnings && cargo fmt --check
```

Expected: clean.

- [ ] **Step 6: Commit**

```
git add src/buck/emit.rs src/buck/mod.rs src/cli/buckify.rs src/cli/vendor.rs
git commit -m "$(cat <<'EOF'
feat(s9): emitter — vendor_mode plumbing + URL synthesis

EmitInput.vendor_mode + BuildEmitContext.vendor_mode flow through
build_emit_input; per-package URL synthesis switches between
http registry URLs / prebake: prefix / vendor: prefix based on the flag.

buckify::run resolves vendor_mode from config + --mode override and
skips the prebake/.manifest.toml read in committed mode.

Sdist-only pure-python in committed mode synthesizes
vendor:<pep427_filename> without a manifest; Task 9 will add the
emit-time existence check that catches the native-skipped case.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: Conditional `vendor:` macro branch + snapshot

**Files:**
- Modify: `src/buck/string_writer.rs`
- Create: `src/buck/snapshots/muntjac__buck__string_writer__tests__snapshot_vendor_mode_muntjac_bzl.snap` (via insta)

- [ ] **Step 1: Write the failing snapshot test**

Add to the existing `#[cfg(test)] mod tests` in `src/buck/string_writer.rs`:

```rust
#[test]
fn snapshot_vendor_mode_muntjac_bzl() {
    let input = EmitInput {
        tree: "default".into(),
        third_party_dir: "third-party/python".into(),
        cfg_dir: "third-party/python".into(),
        configs: vec![ConfigName::new("3.12", "linux-x86_64-gnu")],
        packages: vec![],
        vendor_mode: true,
    };
    let bzl = emit_muntjac_bzl(&input);
    insta::assert_snapshot!(bzl);
}
```

- [ ] **Step 2: Run test to verify it fails**

```
cargo test -p muntjac --lib snapshot_vendor_mode_muntjac_bzl -- --nocapture
```

Expected: failure — snapshot doesn't yet exist, AND the macro doesn't yet emit a `vendor:` branch.

- [ ] **Step 3: Implement conditional macro emission**

In `src/buck/string_writer.rs::emit_muntjac_bzl`, locate the prebake-branch block (around line 213):

```rust
    writeln!(s, "        if src.startswith(\"prebake:\"):").unwrap();
    writeln!(s, "            rel = src[len(\"prebake:\"):]").unwrap();
    writeln!(s, "            native.export_file(").unwrap();
    writeln!(s, "                name = target,").unwrap();
    writeln!(s, "                src = \"prebake/{{}}\".format(rel),").unwrap();
    writeln!(s, "                visibility = [],").unwrap();
    writeln!(s, "            )").unwrap();
    writeln!(s, "        else:").unwrap();
    writeln!(s, "            native.http_file(").unwrap();
```

Replace with a conditional injection of the `vendor:` arm:

```rust
    writeln!(s, "        if src.startswith(\"prebake:\"):").unwrap();
    writeln!(s, "            rel = src[len(\"prebake:\"):]").unwrap();
    writeln!(s, "            native.export_file(").unwrap();
    writeln!(s, "                name = target,").unwrap();
    writeln!(s, "                src = \"prebake/{{}}\".format(rel),").unwrap();
    writeln!(s, "                visibility = [],").unwrap();
    writeln!(s, "            )").unwrap();
    if input.vendor_mode {
        writeln!(s, "        elif src.startswith(\"vendor:\"):").unwrap();
        writeln!(s, "            rel = src[len(\"vendor:\"):]").unwrap();
        writeln!(s, "            native.export_file(").unwrap();
        writeln!(s, "                name = target,").unwrap();
        writeln!(s, "                src = \"vendor/{{}}\".format(rel),").unwrap();
        writeln!(s, "                visibility = [],").unwrap();
        writeln!(s, "            )").unwrap();
    }
    writeln!(s, "        else:").unwrap();
    writeln!(s, "            native.http_file(").unwrap();
```

- [ ] **Step 4: Run the test; accept the new snapshot**

```
cargo test -p muntjac --lib snapshot_vendor_mode_muntjac_bzl -- --nocapture
```

Expected: test fails with "snapshot file does not yet exist" — review the printed output, ensure it contains `elif src.startswith("vendor:"):`, then accept:

```
cargo insta accept -p muntjac
```

Re-run:

```
cargo test -p muntjac --lib snapshot_vendor_mode_muntjac_bzl
```

Expected: passes.

- [ ] **Step 5: Verify ALL existing snapshots still pass (byte-identity guard)**

```
cargo test -p muntjac --lib
```

Expected: all snapshots pass. The conditional emission means `vendor_mode=false` produces byte-identical output to v0.2.0 — `snapshot_empty_muntjac_bzl`, `snapshot_multi_package_muntjac_bzl`, and `muntjac_bzl_with_header_and_native` must remain unchanged. If any of these fail, the conditional emission has a leak — fix and re-run.

- [ ] **Step 6: Verify clippy + format**

```
cargo clippy --all-targets -- -D warnings && cargo fmt --check
```

Expected: clean.

- [ ] **Step 7: Commit**

```
git add src/buck/string_writer.rs src/buck/snapshots/muntjac__buck__string_writer__tests__snapshot_vendor_mode_muntjac_bzl.snap
git commit -m "$(cat <<'EOF'
feat(s9): pypi_package macro — conditional vendor: branch

When EmitInput.vendor_mode is true, the macro grows an `elif
src.startswith("vendor:")` arm that does export_file(src="vendor/<rel>").
When false (default), the macro is byte-identical to v0.2.0 —
fixtures 01–10 snapshots stay unchanged.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: Emit-time existence check + `MissingVendorWheels`

**Files:**
- Modify: `src/buck/emit.rs`
- Modify: `src/buck/mod.rs`
- Modify: `src/cli/buckify.rs`

- [ ] **Step 1: Write failing tests**

Add to `src/buck/emit.rs` `mod tests`:

```rust
#[test]
fn check_vendor_wheels_aggregates_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let vendor_dir = tmp.path();
    std::fs::write(vendor_dir.join("present-1.0-py3-none-any.whl"), b"").unwrap();

    let input = EmitInput {
        tree: "default".into(),
        third_party_dir: "third-party/python".into(),
        cfg_dir: "third-party/python".into(),
        configs: vec![ConfigName::new("3.12", "linux-x86_64-gnu")],
        packages: vec![
            EmitPackage {
                name: "present".into(),
                version: "1.0".into(),
                deps: EmitDeps::Uniform(vec![]),
                wheels: [(
                    ConfigName::new("3.12", "linux-x86_64-gnu"),
                    EmitWheel {
                        url: "vendor:present-1.0-py3-none-any.whl".into(),
                        hash: "sha256:00".into(),
                    },
                )]
                .into_iter()
                .collect(),
                overlay: None,
                entry_points: vec![],
                visibility: None,
                labels: vec![],
                runtime_env: Default::default(),
            },
            EmitPackage {
                name: "missing".into(),
                version: "2.0".into(),
                deps: EmitDeps::Uniform(vec![]),
                wheels: [(
                    ConfigName::new("3.12", "linux-x86_64-gnu"),
                    EmitWheel {
                        url: "vendor:missing-2.0-py3-none-any.whl".into(),
                        hash: "sha256:00".into(),
                    },
                )]
                .into_iter()
                .collect(),
                overlay: None,
                entry_points: vec![],
                visibility: None,
                labels: vec![],
                runtime_env: Default::default(),
            },
        ],
        vendor_mode: true,
    };

    let err = check_vendor_wheels_present(&input, vendor_dir).unwrap_err();
    match err {
        crate::error::BuckifyError::MissingVendorWheels { tree, missing } => {
            assert_eq!(tree, "default");
            assert_eq!(missing.len(), 1);
            assert_eq!(
                missing[0],
                ("missing".into(), "2.0".into(), "missing-2.0-py3-none-any.whl".into())
            );
        }
    }
}

#[test]
fn check_vendor_wheels_passes_when_all_present() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(tmp.path().join("present-1.0-py3-none-any.whl"), b"").unwrap();

    let input = EmitInput {
        tree: "default".into(),
        third_party_dir: "third-party/python".into(),
        cfg_dir: "third-party/python".into(),
        configs: vec![ConfigName::new("3.12", "linux-x86_64-gnu")],
        packages: vec![EmitPackage {
            name: "present".into(),
            version: "1.0".into(),
            deps: EmitDeps::Uniform(vec![]),
            wheels: [(
                ConfigName::new("3.12", "linux-x86_64-gnu"),
                EmitWheel {
                    url: "vendor:present-1.0-py3-none-any.whl".into(),
                    hash: "sha256:00".into(),
                },
            )]
            .into_iter()
            .collect(),
            overlay: None,
            entry_points: vec![],
            visibility: None,
            labels: vec![],
            runtime_env: Default::default(),
        }],
        vendor_mode: true,
    };

    assert!(check_vendor_wheels_present(&input, tmp.path()).is_ok());
}

#[test]
fn check_vendor_wheels_noop_when_not_vendor_mode() {
    let tmp = tempfile::tempdir().unwrap();
    let input = EmitInput {
        tree: "default".into(),
        third_party_dir: "third-party/python".into(),
        cfg_dir: "third-party/python".into(),
        configs: vec![],
        packages: vec![],
        vendor_mode: false,
    };
    assert!(check_vendor_wheels_present(&input, tmp.path()).is_ok());
}
```

- [ ] **Step 2: Run tests to verify they fail**

```
cargo test -p muntjac --lib check_vendor_wheels -- --nocapture
```

Expected: failure — `check_vendor_wheels_present` not defined.

- [ ] **Step 3: Implement `check_vendor_wheels_present`**

Add to `src/buck/emit.rs`:

```rust
use std::path::Path;

/// Verify every `vendor:<filename>` URL emitted in `input` corresponds to an
/// existing file in `vendor_dir`. Aggregates ALL missing into one
/// `BuckifyError::MissingVendorWheels` before returning.
///
/// No-op when `input.vendor_mode == false`.
pub fn check_vendor_wheels_present(
    input: &EmitInput,
    vendor_dir: &Path,
) -> Result<(), crate::error::BuckifyError> {
    if !input.vendor_mode {
        return Ok(());
    }
    let mut missing: Vec<(String, String, String)> = Vec::new();
    let mut seen_filenames: std::collections::BTreeSet<String> =
        std::collections::BTreeSet::new();
    for pkg in &input.packages {
        for (_cfg, wheel) in &pkg.wheels {
            if let Some(rel) = wheel.url.strip_prefix("vendor:") {
                if !seen_filenames.insert(rel.to_string()) {
                    continue;
                }
                if !vendor_dir.join(rel).is_file() {
                    missing.push((pkg.name.clone(), pkg.version.clone(), rel.to_string()));
                }
            }
        }
    }
    if missing.is_empty() {
        return Ok(());
    }
    missing.sort();
    Err(crate::error::BuckifyError::MissingVendorWheels {
        tree: input.tree.clone(),
        missing,
    })
}
```

- [ ] **Step 4: Re-export from `src/buck/mod.rs`**

```rust
pub use emit::{
    BuckEmitter, BuildEmitContext, ConfigName, EmitInput, EmitOutput, EmitPackage, EmitWheel,
    SharedCfgInput, SharedCfgOutput, build_emit_input, build_shared_cfg_input,
    check_vendor_wheels_present,
};
```

- [ ] **Step 5: Wire the check in `src/cli/buckify.rs`**

After `let input = build_emit_input(...)` and before `write_outputs(...)`:

```rust
        crate::buck::check_vendor_wheels_present(
            &input,
            &third_party_dir.join("vendor"),
        )?;
```

The `?` propagates `BuckifyError::MissingVendorWheels` as an `anyhow::Error` (it implements `std::error::Error` via `thiserror`).

- [ ] **Step 6: Run tests to verify they pass**

```
cargo test -p muntjac --lib check_vendor_wheels
```

Expected: all 3 tests pass.

- [ ] **Step 7: Add integration test for missing-wheel DX**

Append to `tests/vendor.rs`:

```rust
#[test]
fn buckify_committed_aborts_on_missing_wheels() {
    // Minimal Buck-shaped tree but vendor/ is empty.
    let tmp = tempfile::tempdir().unwrap();

    std::fs::write(
        tmp.path().join("muntjac.toml"),
        r#"
manifest_path = "pyproject.toml"
third_party_dir = "third-party/python"
python_versions = ["3.12"]

[platforms]
linux-x86_64-gnu = { target = "x86_64-unknown-linux-gnu", manylinux = "2_17" }

[buck]
vendor = true
"#,
    )
    .unwrap();

    std::fs::write(
        tmp.path().join("pyproject.toml"),
        r#"[project]
name = "stub"
version = "0.0.0"
requires-python = ">=3.12"
dependencies = ["idna==3.10"]
"#,
    )
    .unwrap();

    // A minimal uv.lock referencing idna with a wheel URL.
    std::fs::write(
        tmp.path().join("uv.lock"),
        r#"version = 1
revision = 1
requires-python = ">=3.12"

[[package]]
name = "stub"
version = "0.0.0"
source = { virtual = "." }
dependencies = [{ name = "idna" }]

[[package]]
name = "idna"
version = "3.10"
source = { registry = "https://pypi.org/simple" }
wheels = [{ url = "https://files.pythonhosted.org/idna-3.10-py3-none-any.whl", hash = "sha256:cafef00dcafef00dcafef00dcafef00dcafef00dcafef00dcafef00dcafef00d", size = 1, filename = "idna-3.10-py3-none-any.whl" }]
"#,
    )
    .unwrap();

    let output = Command::new(target_exe())
        .args(["buckify"])
        .current_dir(tmp.path())
        .output()
        .expect("running muntjac buckify");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("missing")
            && stderr.contains("idna-3.10-py3-none-any.whl"),
        "expected missing-wheel error mentioning idna, got: {stderr}"
    );
}
```

- [ ] **Step 8: Run integration test**

```
cargo build -p muntjac && \
cargo test --test vendor buckify_committed_aborts_on_missing_wheels -- --nocapture
```

Expected: passes (buckify errors with the missing-wheel message).

- [ ] **Step 9: Verify clippy + format**

```
cargo clippy --all-targets -- -D warnings && cargo fmt --check
```

Expected: clean.

- [ ] **Step 10: Commit**

```
git add src/buck/emit.rs src/buck/mod.rs src/cli/buckify.rs tests/vendor.rs
git commit -m "$(cat <<'EOF'
feat(s9): emit-time existence check — MissingVendorWheels DX

check_vendor_wheels_present walks every vendor:<filename> URL the emitter
produced and verifies the file exists at <third_party_dir>/vendor/.
Aggregates ALL missing into one BuckifyError::MissingVendorWheels per
tree — the user sees the full list at once, not one-at-a-time iterations.

In prebake-only mode (vendor_mode=false), the check is a no-op.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 10: `11-vendor` fixture + golden test

**Files:**
- Create: `tests/fixtures/buck/11-vendor/muntjac.toml`
- Create: `tests/fixtures/buck/11-vendor/pyproject.toml`
- Create: `tests/fixtures/buck/11-vendor/uv.lock`
- Create: `tests/fixtures/buck/11-vendor/third-party/python/vendor/*.whl` (committed binaries)
- Create: `tests/fixtures/buck/11-vendor/third-party/python/BUCK` (placeholder; muntjac will overwrite)
- Create: `tests/fixtures/buck/11-vendor/.buckconfig`
- Create: `tests/fixtures/buck/11-vendor/toolchains/BUCK`
- Create: `tests/fixtures/buck/11-vendor/PACKAGE`
- Create: `tests/fixtures/buck/11-vendor/tests/smoke/BUCK`
- Create: `tests/fixtures/buck/11-vendor/tests/smoke/demo.py`
- Create: `tests/fixtures/buck/11-vendor/expected/` (golden BUCK + muntjac.bzl + config/BUCK + wiring.bzl)
- Create: `tests/fixtures/buck/11-vendor/prelude` (submodule pin matching 10-multi-tree's SHA `b4e55417b4edf582be8fb20f24e1afc5866987ce`)
- Modify: `tests/buckify.rs` — add `fixture_11_vendor_golden`

- [ ] **Step 1: Choose the two real packages**

Pick `idna` (binary wheel, tiny — ~70KB) for the download path, and `flit_core` (sdist-only, hatchling-rebakeable) for the prebake-into-vendor path. Verify sizes:

```
curl -sI https://files.pythonhosted.org/packages/76/c6/c88e154df9c4e1a2a66ccf0005a88dfb2650c1dffb6f5ce603dfbd452ce3/idna-3.10-py3-none-any.whl | head -5
```

Expected: a `Content-Length` line around `70588`.

Actual recommended pair to commit (PEP 503 names): `idna-3.10-py3-none-any.whl` (downloaded) and `tomli-2.0.1-py3-none-any.whl` (sdist-only in some packagings; alternatively `iniconfig-2.0.0-py3-none-any.whl`). If you change picks, regenerate the uv.lock accordingly.

- [ ] **Step 2: Build the fixture directory structure**

Set up an external scratch dir, init a uv project, add the two deps, run `uv lock`:

```
mkdir -p /tmp/s9-fixture && cd /tmp/s9-fixture
uv init --bare --name muntjac-s9-demo --python 3.12
uv add idna==3.10 iniconfig==2.0.0
uv lock
```

Then copy outputs into the fixture:

```
mkdir -p /home/jackm/repos/muntjac/tests/fixtures/buck/11-vendor/third-party/python/vendor
mkdir -p /home/jackm/repos/muntjac/tests/fixtures/buck/11-vendor/tests/smoke
mkdir -p /home/jackm/repos/muntjac/tests/fixtures/buck/11-vendor/toolchains
cp /tmp/s9-fixture/pyproject.toml /home/jackm/repos/muntjac/tests/fixtures/buck/11-vendor/
cp /tmp/s9-fixture/uv.lock /home/jackm/repos/muntjac/tests/fixtures/buck/11-vendor/
```

- [ ] **Step 3: Create `muntjac.toml`**

```toml
manifest_path = "pyproject.toml"
third_party_dir = "third-party/python"
python_versions = ["3.12"]

[platforms]
linux-x86_64-gnu = { target = "x86_64-unknown-linux-gnu", manylinux = "2_17" }
linux-aarch64-gnu = { target = "aarch64-unknown-linux-gnu", manylinux = "2_17" }
macos-arm64 = { target = "aarch64-apple-darwin", macos_min = "11.0" }

[buck]
vendor = true

[fixups]
registry = "none"
allow_local_overrides = true
```

- [ ] **Step 4: Vendor the wheels in committed mode**

Build muntjac, then run vendor pointing at the fixture:

```
cargo build -p muntjac
target/debug/muntjac -C tests/fixtures/buck/11-vendor vendor --mode=committed
```

Expected: downloads idna's binary wheel + prebakes iniconfig's sdist into `tests/fixtures/buck/11-vendor/third-party/python/vendor/`. Inspect:

```
ls tests/fixtures/buck/11-vendor/third-party/python/vendor/
```

Should list: `idna-3.10-py3-none-any.whl`, `iniconfig-2.0.0-py3-none-any.whl`.

- [ ] **Step 5: Run buckify to generate goldens**

```
target/debug/muntjac -C tests/fixtures/buck/11-vendor buckify
```

Then move the generated files into `expected/`:

```
mkdir -p tests/fixtures/buck/11-vendor/expected/third-party/python/config
cp tests/fixtures/buck/11-vendor/third-party/python/BUCK tests/fixtures/buck/11-vendor/expected/third-party/python/BUCK
cp tests/fixtures/buck/11-vendor/third-party/python/muntjac.bzl tests/fixtures/buck/11-vendor/expected/third-party/python/muntjac.bzl
cp tests/fixtures/buck/11-vendor/third-party/python/config/BUCK tests/fixtures/buck/11-vendor/expected/third-party/python/config/BUCK
cp tests/fixtures/buck/11-vendor/third-party/python/wiring.bzl tests/fixtures/buck/11-vendor/expected/third-party/python/wiring.bzl
```

Visually verify each file: `BUCK` should contain `vendor:idna-3.10-py3-none-any.whl`; `muntjac.bzl` should have the `elif src.startswith("vendor:"):` arm.

- [ ] **Step 6: Copy buck infra files from fixture 02 / 10**

```
cp tests/fixtures/buck/02-numpy-pandas/.buckconfig tests/fixtures/buck/11-vendor/.buckconfig
cp tests/fixtures/buck/02-numpy-pandas/PACKAGE tests/fixtures/buck/11-vendor/PACKAGE
cp tests/fixtures/buck/02-numpy-pandas/toolchains/BUCK tests/fixtures/buck/11-vendor/toolchains/BUCK
```

- [ ] **Step 7: Add the prelude submodule**

```
cd tests/fixtures/buck/11-vendor
git submodule add https://github.com/facebook/buck2-prelude prelude
cd prelude && git checkout b4e55417b4edf582be8fb20f24e1afc5866987ce && cd ..
cd /home/jackm/repos/muntjac
```

- [ ] **Step 8: Create the smoke target and demo script**

`tests/fixtures/buck/11-vendor/tests/smoke/BUCK`:

```
python_binary(
    name = "demo",
    main = "demo.py",
    deps = [
        "//third-party/python:idna",
        "//third-party/python:iniconfig",
    ],
    visibility = ["PUBLIC"],
)
```

`tests/fixtures/buck/11-vendor/tests/smoke/demo.py`:

```python
import idna
import iniconfig

print("idna:", idna.__version__)
print("iniconfig:", iniconfig.__version__)
```

- [ ] **Step 9: Add the golden test in `tests/buckify.rs`**

Append after the existing `fixture_10_multi_tree_golden`:

```rust
#[test]
fn fixture_11_vendor_golden() {
    let src = fixture("11-vendor");
    let tmp = tempfile::tempdir().unwrap();
    copy_fixture_to(&src, tmp.path());
    run_buckify(tmp.path());

    let expected_root = src.join("expected");
    assert_files_match(
        tmp.path(),
        &expected_root,
        &[
            "third-party/python/BUCK",
            "third-party/python/muntjac.bzl",
            "third-party/python/config/BUCK",
            "third-party/python/wiring.bzl",
        ],
    );
}
```

- [ ] **Step 10: Run the golden test**

```
cargo test --test buckify fixture_11_vendor_golden -- --nocapture
```

Expected: passes (the buckify-from-scratch output matches the goldens copied earlier).

- [ ] **Step 11: Add the multi-tree+vendor emit-only test**

Append to `tests/multi_tree.rs`:

```rust
#[test]
fn buckify_committed_multi_tree_emits_per_tree_vendor_refs() {
    use std::process::Command;

    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("muntjac.toml"),
        r#"
[platforms]
linux-x86_64-gnu = { target = "x86_64-unknown-linux-gnu", manylinux = "2_17" }

[buck]
vendor = true

[tree.modern]
manifest_path = "modern/pyproject.toml"
third_party_dir = "tp/modern"
python_versions = ["3.12"]

[tree.legacy]
manifest_path = "legacy/pyproject.toml"
third_party_dir = "tp/legacy"
python_versions = ["3.12"]
"#,
    )
    .unwrap();

    for tree in &["modern", "legacy"] {
        let dir = tmp.path().join(tree);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("pyproject.toml"),
            format!(
                r#"[project]
name = "{tree}-app"
version = "0.0.0"
requires-python = ">=3.12"
dependencies = ["idna==3.10"]
"#
            ),
        )
        .unwrap();
        std::fs::write(
            dir.join("uv.lock"),
            r#"version = 1
revision = 1
requires-python = ">=3.12"

[[package]]
name = "stub"
version = "0.0.0"
source = { virtual = "." }
dependencies = [{ name = "idna" }]

[[package]]
name = "idna"
version = "3.10"
source = { registry = "https://pypi.org/simple" }
wheels = [{ url = "https://files.pythonhosted.org/idna-3.10-py3-none-any.whl", hash = "sha256:cafef00dcafef00dcafef00dcafef00dcafef00dcafef00dcafef00dcafef00d", size = 1, filename = "idna-3.10-py3-none-any.whl" }]
"#,
        )
        .unwrap();
        // Pre-populate vendor dir with the wheel (so the existence check passes).
        let vend = tmp.path().join("tp").join(tree).join("vendor");
        std::fs::create_dir_all(&vend).unwrap();
        std::fs::write(vend.join("idna-3.10-py3-none-any.whl"), b"").unwrap();
    }

    let target = std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".into());
    let exe = std::path::Path::new(&target).join("debug").join("muntjac");
    let output = Command::new(&exe)
        .args(["buckify"])
        .current_dir(tmp.path())
        .output()
        .expect("running muntjac buckify");
    assert!(
        output.status.success(),
        "buckify failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    for tree in &["modern", "legacy"] {
        let buck = std::fs::read_to_string(tmp.path().join("tp").join(tree).join("BUCK")).unwrap();
        assert!(
            buck.contains("vendor:idna-3.10-py3-none-any.whl"),
            "tree {tree} BUCK missing vendor: ref. content:\n{buck}"
        );
    }
}
```

- [ ] **Step 12: Run the multi-tree test**

```
cargo build -p muntjac && \
cargo test --test multi_tree buckify_committed_multi_tree_emits_per_tree_vendor_refs -- --nocapture
```

Expected: passes — both trees emit `vendor:idna-…` references against their own `vendor/` dirs.

- [ ] **Step 13: Verify the smoke target actually builds locally (sanity)**

This is local-only; CI will repeat it on 3 runners in Task 11.

```
cd tests/fixtures/buck/11-vendor && buck2 build //tests/smoke:demo && buck2 run //tests/smoke:demo && cd /home/jackm/repos/muntjac
```

Expected: prints `idna: 3.10` and `iniconfig: 2.0.0`.

- [ ] **Step 14: Verify clippy + format**

```
cargo clippy --all-targets -- -D warnings && cargo fmt --check
```

Expected: clean.

- [ ] **Step 15: Commit**

```
git add tests/fixtures/buck/11-vendor/ tests/buckify.rs tests/multi_tree.rs .gitmodules
git commit -m "$(cat <<'EOF'
test(s9): 11-vendor fixture + golden + multi-tree emit-only

11-vendor commits idna (downloaded) + iniconfig (prebake-built) wheels
under third-party/python/vendor/. buckify emits vendor:<filename> refs;
the macro grows the elif vendor: arm; buck2 build/run wires the wheels
into a python_binary smoke target. expected/ goldens cover BUCK +
muntjac.bzl + config/BUCK + wiring.bzl.

tests/multi_tree.rs gains buckify_committed_multi_tree_emits_per_tree_vendor_refs
exercising the per-tree vendor: path at the emit boundary (no buck2 needed).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 11: CI — `11-vendor` steps on all runners

**Files:**
- Modify: `.github/workflows/ci.yml`

- [ ] **Step 1: Read the existing CI workflow for context**

```
grep -n "10-multi-tree\|buck2 build" .github/workflows/ci.yml | head -20
```

Expected: lists where fixture 10's buckify and buck2 build steps live across the runners.

- [ ] **Step 2: Add 3 steps mirroring fixture 10's pattern**

After each existing `buck2 build + run both trees (10-multi-tree)` step, append:

```yaml
      - name: muntjac buckify (11-vendor fixture)
        run: target/debug/muntjac -C tests/fixtures/buck/11-vendor buckify

      - name: muntjac vendor --frozen (11-vendor idempotence)
        run: target/debug/muntjac -C tests/fixtures/buck/11-vendor vendor --mode=committed --frozen

      - name: buck2 build + run (11-vendor)
        run: |
          cd tests/fixtures/buck/11-vendor
          buck2 build //tests/smoke:demo
          buck2 run //tests/smoke:demo
```

Apply identically to each of the 3 runner jobs (linux-x86_64, linux-arm64, macos-arm64). **No `if: matrix.runner == ...` gating** — every runner runs all three steps. (Per `feedback_ci_no_arch_polymorphism.md`: arch-conditional CI is logged as TECH_DEBT, not implemented inline.)

- [ ] **Step 3: Run yamllint / actionlint locally if available**

```
actionlint .github/workflows/ci.yml
```

Expected: clean (warnings about external actions are OK).

- [ ] **Step 4: Commit + push for CI to run**

```
git add .github/workflows/ci.yml
git commit -m "$(cat <<'EOF'
ci(s9): build + run 11-vendor on all runners

Add three steps after the 10-multi-tree block on each runner:
- muntjac buckify (11-vendor)
- muntjac vendor --frozen (idempotence — should be a near-no-op)
- buck2 build + run //tests/smoke:demo

All three runners (linux-x86_64, linux-arm64, macos-arm64) run all steps;
no arch gating per the no-arch-polymorphism rule.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
git push origin s9-vendor-mode
```

Expected: CI runs on the branch; wait for all 3 runners to go green before continuing.

---

## Task 12: Docs + version + TECH_DEBT + roadmap

**Files:**
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Modify: `CHANGELOG.md`
- Modify: `README.md`
- Modify: `docs/superpowers/specs/2026-05-20-muntjac-roadmap.md`
- Modify: `docs/superpowers/TECH_DEBT.md`

- [ ] **Step 1: Bump version 0.2.0 → 0.3.0**

In `Cargo.toml`:

```toml
version = "0.3.0"
```

Then:

```
cargo build -p muntjac
```

This refreshes `Cargo.lock` automatically. Verify:

```
grep -A1 'name = "muntjac"' Cargo.lock | head -3
```

Expected: `version = "0.3.0"`.

- [ ] **Step 2: Add CHANGELOG entry**

Insert at the top of `CHANGELOG.md` after the header, before `## [0.2.0]`:

```markdown
## [0.3.0] — 2026-05-26

### Added

- **Vendor mode (S9)** — `[buck] vendor = true` (or `muntjac vendor --mode=committed`) downloads all referenced registry wheels into `<third_party_dir>/vendor/`, and the emitter replaces `http_file(urls=…)` with `export_file(src = "vendor/…")` references for air-gapped Buck builds. Prebake outputs land in the same `vendor/` directory; no manifest file is written — state is derivable from `uv.lock` + filesystem + PEP 427 deterministic naming.
- `muntjac vendor --no-prune` opts out of the sync-prune step (keeps stale `vendor/*.whl`).
- `muntjac buckify --mode={committed,prebake-only}` per-invocation override of `[buck] vendor`.
- Emit-time existence check: in vendor mode, `muntjac buckify` aborts before writing BUCK if any expected wheel is missing from `vendor/`, listing all missing files.
- `--mode=committed` + `--no-network` hard-errors via `VendorError::ModeNetworkConflict`.

### Changed

- The `pypi_package` macro grows a conditional `elif src.startswith("vendor:")` arm — emitted **only** when vendor mode is active. Prebake-only output remains byte-identical to v0.2.0.

### Compatibility

- Existing projects (`[buck] vendor = false`, the default) see no behavioural change. Fixtures 01–10 emit byte-identical output to v0.2.0.
- S5's `prebake/.manifest.toml` is unchanged in prebake-only mode. In committed mode, neither `prebake/` nor its manifest is written.
```

- [ ] **Step 3: Add README vendor-mode section**

Insert after the `## Configuration` section (around `:71` in the current README), before `## Multi-tree`:

```markdown
## Vendor mode (air-gapped / reproducible builds)

By default, muntjac emits `http_file(urls=…)` for each registry wheel — Buck fetches them from PyPI on build. For air-gapped CI, vendored dependencies, or fully reproducible builds, opt in to vendor mode:

```toml
[buck]
vendor = true
```

Then:

```sh
muntjac vendor             # downloads every referenced wheel into <third_party_dir>/vendor/
muntjac buckify            # emits export_file refs to the committed wheels
buck2 build //...          # no network needed
```

The `vendor/` directory is committed to git. Pure-python sdists are prebaked directly into `vendor/` (no separate `prebake/`). Re-running `muntjac vendor` keeps the directory in sync with `uv.lock` — stale wheels are pruned unless you pass `--no-prune`.

CLI overrides:

- `muntjac vendor --mode=committed` / `--mode=prebake-only` — override `[buck] vendor` for one invocation.
- `muntjac buckify --mode=…` — same override for emit.
- `muntjac vendor --no-prune` — keep stale wheels in `vendor/`.

`--mode=committed` + `--no-network` is rejected with a clear error.
```

Also update the Status section to mention vendor mode shipped in v0.3.0 and bump version references.

- [ ] **Step 4: Mark S9 ✅ in roadmap**

In `docs/superpowers/specs/2026-05-20-muntjac-roadmap.md`, find the S9 entry and add the `**Shipped:**` line under it (mirroring S11's format):

```markdown
### S9 — Vendor mode → v0.3.0

`muntjac vendor --mode=committed` downloads wheels into each tree's `<third_party_dir>/vendor/` and the emitter replaces `http_file(urls=…)` with relative source refs. Buck builds become air-gapped. Built tree-aware on top of S11.

✅ **Shipped 2026-05-26:** 12 commits, tag `s9-complete`. v0.3.0 cut.
```

Also update the specs index table row for S9 to point at the spec + plan.

- [ ] **Step 5: Log TECH_DEBT entries**

Append to `docs/superpowers/TECH_DEBT.md`:

```markdown
- **TD-S9-01: Multi-tree + vendor combination has no buck2-build fixture.** Today, `buckify_committed_multi_tree_emits_per_tree_vendor_refs` proves emit-level correctness but never runs `buck2 build`. A `12-vendor-multi-tree` fixture (two trees, each with its own committed `vendor/`) would close the gap.
- **TD-S9-02: Sequential wheel downloads.** `src/vendor/download.rs::download_wheel` is invoked one wheel at a time from `vendor_tree_committed`. Parallelizing (rayon or futures::stream::buffer_unordered) would cut large-tree vendor latency. Deferred to keep S9 scope tight.
- **TD-S9-03: S5's `prebake/.manifest.toml` is bookkeeping not load-bearing for the build.** S9 committed mode operates without a manifest (uv.lock + filesystem + PEP 427 naming suffices). The same simplification could apply to prebake-only mode; deferred to avoid touching S5's on-disk format.
- **TD-S9-04: `muntjac vendor --check`** (verify the directory matches `uv.lock` without writing) is a natural follow-up for CI lanes that want to enforce committed vendor without running the full vendor step. Defer to v0.4+.
```

- [ ] **Step 6: Run the full test suite + clippy + format**

```
cargo test -p muntjac && \
cargo clippy --all-targets -- -D warnings && \
cargo fmt --check
```

Expected: green across the board.

- [ ] **Step 7: Verify `cargo publish` packaging**

```
cargo publish --dry-run
```

Expected: passes (the existing `exclude = ["tests/fixtures/**", "docs/**"]` in Cargo.toml from S8a keeps the 11-vendor binary wheels out of the package; tarball stays well under 10 MiB).

- [ ] **Step 8: Commit**

```
git add Cargo.toml Cargo.lock CHANGELOG.md README.md docs/superpowers/specs/2026-05-20-muntjac-roadmap.md docs/superpowers/TECH_DEBT.md
git commit -m "$(cat <<'EOF'
docs(s9): v0.3.0 — README vendor section + CHANGELOG + roadmap ✅ + TD

Cargo.toml 0.2.0 → 0.3.0. CHANGELOG [0.3.0] entry covering vendor mode,
--no-prune, --mode override, emit-time existence check, byte-identity
guarantee for prebake-only mode.

README gains a Vendor mode section with the opt-in toml snippet and CLI
flow. Roadmap marks S9 ✅ shipped at 12 commits. Four TECH_DEBT entries
(TD-S9-01..04) cover the post-stage parking lot from the spec.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 9: Tag the stage**

```
git tag -a s9-complete -m "S9 — vendor mode — 12 commits, ships as v0.3.0"
```

Do NOT push the tag yet — per the established cadence, the maintainer pushes `v0.3.0` (which triggers release.yml's crates.io publish) manually after their dogfood pass.

---

## Self-review checklist (run before declaring the plan complete)

1. **Spec §1.2 coverage** (the eight in-scope items):
   - Wire `[buck] vendor` end-to-end ✅ Task 1, Task 6, Task 7.
   - Registry wheel downloader ✅ Task 4.
   - Prebake → `vendor/` ✅ Task 6.
   - Sync prune ✅ Task 5 + Task 6 (step 3 inside `vendor_tree_committed`).
   - Emitter changes (URL synthesis + conditional macro) ✅ Task 7, Task 8.
   - Error taxonomy ✅ Task 2.
   - 11-vendor fixture ✅ Task 10.
   - Docs + version ✅ Task 12.

2. **Spec §1.3 deferred** — each appears in TD-S9-01..04 (Task 12 step 5). ✅

3. **Spec §8 backwards compat** — Task 8 step 5 explicitly re-runs every existing snapshot to guard byte-identity for fixtures 01–10. ✅

4. **Type/signature consistency:**
   - `ModeFlag` (Task 1) used in Task 6, Task 7. ✅
   - `VendorArgs` / `BuckifyArgs` (Task 1) used in Task 6, Task 7. ✅
   - `vendor_pep427_pure_python_filename` defined `pub` in Task 6; consumed by Task 6's idempotence check and by Task 7's emit-time URL synthesis. Single canonical name across both tasks. ✅
   - `check_vendor_wheels_present` (Task 9) signature matches the buckify call site in Task 9 step 5. ✅

5. **Placeholder scan:** no `TBD` / `TODO` / `fill in details` / `add error handling` / `similar to Task N` in the plan body. ✅

---

## Execution handoff

Plan complete and saved to `docs/superpowers/plans/2026-05-26-muntjac-s9-vendor-mode.md`. Two execution options:

**1. Subagent-Driven (recommended)** — fresh subagent per task + two-stage review between tasks (spec compliance, then code quality). Matches the S11 cadence that just shipped clean.

**2. Inline Execution** — execute tasks in this session via `superpowers:executing-plans`, batch with checkpoints.

Which approach?
