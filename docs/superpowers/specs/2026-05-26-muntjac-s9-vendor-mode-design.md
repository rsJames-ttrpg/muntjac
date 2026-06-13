# S9 — Vendor mode

**Status:** Designed 2026-05-26. Pending implementation plan.

**Ships as:** v0.3.0 (second pre-announce Phase 2 stage; build order 2/3, S11 → S9 → S10).

**Predecessor:** S11 (multi-tree, v0.2.0).

**Gates:** S10 (audit/unused depends on a vendored dir to detect "unused vendored wheels").

---

## 1. Goal

Give muntjac an air-gapped / reproducible-build mode. When `[buck] vendor = true`, `muntjac vendor` downloads every wheel referenced by `uv.lock` (across all configured python × platform configs, per tree) into `<third_party_dir>/vendor/`, and `muntjac buckify` emits `export_file` rules pointing at those committed wheels instead of `http_file(urls=…)`. Buck builds no longer touch the network; the committed wheel bytes are the source of truth.

### 1.1 Already built (no work needed)

S5 (sdist prebake) and S11 (multi-tree) did most of the heavy lifting:

- **`prebake:` URL prefix pattern** — `src/buck/emit.rs:311`, `src/buck/string_writer.rs:213–219`. The macro already knows how to translate a `<prefix>:<filename>` source into an `export_file(src = "<dir>/<filename>")`. S9 follows the identical pattern with a new `vendor:` prefix.
- **`is_sdist_only` + classifier + builder** — `src/cli/vendor.rs:180`, `src/sdist/`. The pure-python sdist → wheel pipeline is in place; S9 only changes where the resulting wheel is written.
- **Per-tree iteration** — `src/cli/vendor.rs:29` already loops `resolve_trees`; the multi-tree composition is free.
- **`[buck] vendor: bool` config field** — `src/config.rs:152`. Declared, defaulted to `false`, dormant since S0. S9 wires it up.
- **CLI `--tree`, `--frozen`, `--no-network` flags** — compose unchanged with the new `--mode` and `--no-prune` flags.

### 1.2 In scope (the actual work)

1. **Wire `[buck] vendor` end-to-end** — both `muntjac vendor` and `muntjac buckify` read it; CLI `--mode={committed,prebake-only}` overrides per-run.
2. **Registry wheel downloader** — new `src/vendor/download.rs`. Sequential, sha256-verified against `uv.lock`, abort on any failure.
3. **Prebake → `vendor/`** — in committed mode, the existing prebake pipeline writes its built wheels to `<third_party_dir>/vendor/` instead of `<third_party_dir>/prebake/`.
4. **Sync prune** — `muntjac vendor` deletes wheels in `vendor/` not in the expected set (`--no-prune` opts out). Sequential, deterministic.
5. **Emitter changes** — new `vendor:` URL prefix; conditionally emitted macro branch (only when `vendor_mode=true`, so prebake-only output stays byte-identical to v0.2.0); emit-time existence check abort.
6. **Error taxonomy** — `VendorError::{Download, HashMismatch, ModeNetworkConflict}`, `BuckifyError::MissingVendorWheels`. Byte-locked messages + exactness tests.
7. **`11-vendor` fixture** — single-tree, committed mode, mix of downloaded + prebake-built; `muntjac buckify` + `buck2 build` green on all 3 CI runners.
8. **Docs + version** — README vendor-mode section; bump to v0.3.0; CHANGELOG entry.

### 1.3 Out of scope (deferred)

| Item | Reason |
|------|--------|
| Git/path source vendoring | `Source::Git` and `Source::FirstParty` aren't emitted as `http_file` today; vendor mode scopes to `Source::Registry` only. Git vendoring is a separate problem (clone vs archive vs subtree). |
| Parallel downloads | Sequential is sufficient for a v0.3.0 dogfood; parallelism is a v0.4+ optimization. |
| Dropping S5's `prebake/.manifest.toml` | S5's on-disk format is touched only at the relocation level (committed mode writes nothing — the design relies on filesystem + PEP 427 naming); prebake-only mode is unchanged. Replacing S5's manifest with filesystem checks is logged as TECH_DEBT but out of S9 scope. |
| Per-tree mode mixing | `[buck] vendor` is config-level. All trees vendor or none. Per-tree mixing would fragment the cfg-shared-once invariant from S11. |
| Vendor of test/dev-only wheels not in any cfg | The expected set is derived from `python_versions_union × platforms`. Packages reachable only via `include_groups` are already in scope; packages reachable only via configs not in muntjac.toml aren't (same as today). |
| `muntjac vendor --check` (verify dir against uv.lock without writing) | Useful but additive; defer to v0.4+. |

---

## 2. Architecture

### 2.1 Mode model

`[buck] vendor: bool` (in `muntjac.toml`) is the persistent project flag — single source of truth for the project's mode. Both `muntjac vendor` and `muntjac buckify` read it. CLI override:

```
muntjac vendor   [--mode={committed,prebake-only}] [--no-prune]
muntjac buckify  [--mode={committed,prebake-only}]
```

The CLI flag affects that invocation only — it never writes back to the config. `--mode=committed` ⇔ `vendor = true`; `--mode=prebake-only` ⇔ `vendor = false`.

### 2.2 Mode pair semantics

| Mode | Vendor writes to | Buckify emits | Network at Buck-build |
|---|---|---|---|
| **prebake-only** (today's behavior, default) | `<third_party_dir>/prebake/` (gitignored) | `http_file(urls=…)` for registry wheels; `prebake:<filename>` for sdist-only pure-python | yes (Buck fetches via http_file) |
| **committed** (new) | `<third_party_dir>/vendor/` (committed to git) | `vendor:<filename>` → `export_file(src = "vendor/<filename>")` for every wheel | no |

### 2.3 No vendor manifest

In committed mode, **no manifest is written** to `vendor/`. The state is fully derivable:

- **Downloaded wheels** — filename and sha256 are in `uv.lock` directly.
- **Prebake outputs** — for any sdist-only pure-python registry package, the resulting wheel filename is PEP 427 deterministic: `{normalized_name}-{version}-py3-none-any.whl`. Existence of `vendor/<filename>` IS the "yes, we built this" signal.

The emitter does filesystem-existence checks at emit time to reach the same correctness guarantees today's `prebake/.manifest.toml` provides — with one fewer file format to maintain.

Prebake-only mode is unchanged — S5's `prebake/.manifest.toml` keeps being written and read.

### 2.4 Module layout

```
src/cli/vendor.rs        # orchestrator; gains mode dispatch + sync-prune
src/cli/buckify.rs       # gains mode read + emit-time existence check
src/vendor/              # NEW module
    mod.rs
    download.rs          # streaming wheel download + sha256 verify
    sync.rs              # prune stale wheels
src/sdist/prebake.rs     # unchanged; caller passes output dir parametrically
src/buck/emit.rs         # EmitInput / BuildEmitContext gain vendor_mode: bool
src/buck/string_writer.rs # conditional vendor: branch in macro
src/error.rs             # new VendorError + BuckifyError::MissingVendorWheels
src/config.rs            # no schema change; existing [buck] vendor wired up
```

### 2.5 Multi-tree composition

Each tree gets its own `vendor/` under its own `third_party_dir`. The existing per-tree iteration in `cli/vendor.rs` covers this directly — no new multi-tree machinery. `vendor_mode` is config-level (applies to all trees together).

---

## 3. `muntjac vendor` flow (committed mode)

Per tree, in order:

1. **Lock freshness** — unchanged from today. Re-run `uv lock` if `pyproject.toml` is newer than `uv.lock` (unless `--frozen` or `--no-network`).
2. **Parse `uv.lock`** — unchanged.
3. **Compute expected wheel set:**
   - **Downloaded set:** for each `Source::Registry` package, union of `wheels[cfg].filename` across every (python × platform) config in `python_versions × platforms` for the tree.
   - **Prebake set:** for each sdist-only `Source::Registry` package, the PEP 427 filename `{normalized_name}-{version}-py3-none-any.whl` (computed only; no I/O yet — actual production happens in step 5).
   - Git / first-party sources: excluded.
4. **Download** — sequentially, for each expected downloaded wheel:
   - Stream to `vendor/<filename>` (or a tempfile, atomic-rename in).
   - Verify sha256 against uv.lock as bytes arrive.
   - On network failure / 404 / hash mismatch: delete the partial file, abort with `VendorError::Download` or `VendorError::HashMismatch` naming the package, version, and URL.
5. **Prebake step** — for each sdist-only registry package (unchanged classification logic from S5):
   - Pure-python → build wheel via `uv build`; move directly to `vendor/<filename>` (skip the prebake/ intermediate).
   - Native → log skip notice; produce no file. (The buck-side emit will then naturally fall through to the "no wheel for cfg" error if any cfg references it, just as today.)
   - In committed mode, **neither `prebake/` nor `prebake/.manifest.toml` are written**. The prebake dir is left untouched by `muntjac vendor`.
6. **Sync prune** — list `vendor/*.whl`, compute set difference against expected; delete entries not expected unless `--no-prune`. Non-`.whl` files (any future `.tar.gz` or stray) are left alone. `vendor/.gitignore` (if present) is preserved.
7. **`.gitignore`** — none written by `muntjac vendor`; `vendor/` is committed.

In prebake-only mode, behavior is byte-identical to v0.2.0 — `vendor/` is not touched, `prebake/` + `prebake/.manifest.toml` are written as today.

**Mode-switch sharp edge:** `muntjac vendor` only manages the directory its current mode targets. Running `--mode=committed` after a prior `--mode=prebake-only` run leaves stale wheels in `prebake/`; the inverse leaves stale wheels in `vendor/`. Documented in README; cleanup is the user's responsibility (or covered by S10 `unused`).

---

## 4. `muntjac buckify` flow (committed mode)

1. Read `[buck] vendor` (or `--mode` override). Pass to emitter via `BuildEmitContext.vendor_mode`.
   - In committed mode, **skip the `prebake/.manifest.toml` read** at `cli/buckify.rs:48`. The manifest is irrelevant in committed mode — emit reads filesystem instead.
2. Build emit input — per package, per cfg, the source URL is chosen:
   - **Registry + has wheel for cfg, vendor_mode** → `vendor:<wheels[cfg].filename>`
   - **Registry + sdist-only pure-python, vendor_mode** → `vendor:<computed_pep427_filename>` (no manifest lookup needed)
   - **Registry + has wheel for cfg, !vendor_mode** → `<wheels[cfg].url>` (today's http_file behavior)
   - **Registry + sdist-only pure-python, !vendor_mode** → `prebake:<filename>` (today's behavior, via S5's manifest)
   - **Native sdist-only** → no entry; downstream "no wheel for cfg X" error is unchanged.
3. **Emit-time existence check (committed mode only):** before writing BUCK, walk every `vendor:<filename>` produced; verify each file exists at `<tree.third_party_dir>/vendor/<filename>`. Collect all missing into a `Vec<(package, version, filename)>`. If non-empty: abort with `BuckifyError::MissingVendorWheels { tree, missing }`. The error message lists each missing file and suggests `muntjac vendor`.

---

## 5. Emitter changes

### 5.1 Rust side

- `EmitInput`: new field `vendor_mode: bool`.
- `BuildEmitContext`: new field `vendor_mode: bool`.
- `build_emit_input` reads `vendor_mode` from context (falling back to `config.buck.vendor`).
- Per-package URL synthesis branches on `vendor_mode` (see §4 step 2).
- Existence check runs in `write_outputs` (or just before) when `vendor_mode=true`. Multi-tree: one check per tree, errors aggregated per-tree.

### 5.2 `muntjac.bzl` macro

The `pypi_package` macro grows a `vendor:` branch **conditionally** — emitted only when `vendor_mode=true`. This keeps prebake-only output byte-identical to v0.2.0 for fixtures 01–10.

In vendor mode:

```python
if src.startswith("prebake:"):
    rel = src[len("prebake:"):]
    native.export_file(name = target, src = "prebake/{}".format(rel), visibility = [])
elif src.startswith("vendor:"):
    rel = src[len("vendor:"):]
    native.export_file(name = target, src = "vendor/{}".format(rel), visibility = [])
else:
    native.http_file(name = target, sha256 = sha, urls = [src], visibility = [])
```

In prebake-only mode: the `elif` branch is omitted (the macro is byte-identical to v0.2.0).

### 5.3 Single-tree byte-identity

Single-tree `[buck] vendor = false` projects produce output byte-identical to v0.2.0 (fixtures 01–10 unchanged). This is the backwards-compat guarantee S11 established and S9 preserves.

---

## 6. CLI surface

### 6.1 New flags

```
muntjac vendor   [--mode={committed,prebake-only}] [--no-prune]
muntjac buckify  [--mode={committed,prebake-only}]
```

### 6.2 Interactions

- `--mode=committed` + `--no-network` → hard error: `VendorError::ModeNetworkConflict`. Message: "`--mode=committed` requires network access; remove `--no-network`, or vendor first then re-run with `--frozen`."
- `--mode=committed` + `--frozen` → allowed. `--frozen` only suppresses re-running `uv lock`; downloads still proceed.
- `--no-prune` is a no-op in prebake-only mode (no vendor/ to prune).
- `--mode` overrides `[buck] vendor` for that invocation; the config file is never written.
- `--tree` composes orthogonally — `muntjac vendor --tree modern --mode=committed` vendors just that tree.

### 6.3 `muntjac init`

No flag changes. Generated `muntjac.toml` keeps `[buck] vendor = false`. The user opts in by editing the config + running `muntjac vendor`. README documents the flow.

### 6.4 `muntjac fixups`

Unchanged. Fixup loading is mode-agnostic.

---

## 7. Validation & error taxonomy

### 7.1 New errors

`src/error.rs`:

```rust
#[derive(thiserror::Error, Debug)]
pub enum VendorError {
    #[error("downloading {package} {version} from {url}: {source}")]
    Download { package: String, version: String, url: String, source: reqwest::Error },

    #[error("sha256 mismatch for {package} {version}: expected {expected}, got {actual}")]
    HashMismatch { package: String, version: String, expected: String, actual: String },

    #[error("--mode=committed requires network access; remove --no-network, or vendor first then re-run with --frozen")]
    ModeNetworkConflict,
}
```

```rust
// extends existing BuckifyError
pub enum BuckifyError {
    // ...existing variants...
    #[error("vendor mode requires committed wheels; tree '{tree}' is missing:\n{}", format_missing(missing))]
    MissingVendorWheels { tree: String, missing: Vec<(String, String, String)> }, // (package, version, filename)
}
```

Each variant gets an exactness test in `src/error.rs` (same convention as S11's `DuplicateTreeDir`).

### 7.2 No new config validation

`[buck] vendor` is already a `bool` with `serde(default)` — no schema changes, no new `ConfigError` variants. The config-level mode flag composes with existing multi-tree validation unchanged.

---

## 8. Backwards compatibility

| Surface | Guarantee |
|---|---|
| Fixtures 01–10 (prebake-only) | **Byte-identical** to v0.2.0. Snapshot tests guard. |
| `prebake/.manifest.toml` format | Unchanged. S5 territory; S9 doesn't touch it. |
| `prebake:` URL prefix | Unchanged. The new `vendor:` is additive. |
| Existing CLI flags (`--tree`, `--frozen`, `--no-network`) | Unchanged. New flags (`--mode`, `--no-prune`) are additive. |
| `[buck] vendor` default | `false`. Existing projects keep prebake-only behavior on `cargo install`. |
| `muntjac init` output | Unchanged. |

---

## 9. Testing

### 9.1 Unit tests

- `src/vendor/download.rs::download_wheel_streams_and_verifies_sha256`
- `src/vendor/download.rs::download_aborts_on_sha_mismatch`
- `src/vendor/download.rs::download_aborts_on_404`
- `src/vendor/download.rs::download_deletes_partial_on_failure`
- `src/vendor/sync.rs::prune_deletes_stale_wheels`
- `src/vendor/sync.rs::prune_preserves_expected_wheels`
- `src/vendor/sync.rs::no_prune_skips_deletion`
- `src/cli/vendor.rs::vendor_committed_writes_to_vendor_dir`
- `src/cli/vendor.rs::vendor_prebake_only_byte_identical_to_v020`
- `src/buck/emit.rs::emit_vendor_url_for_downloaded_wheel`
- `src/buck/emit.rs::emit_vendor_url_for_prebake_built_wheel`
- `src/buck/emit.rs::missing_vendor_wheel_aggregates_then_fails`
- `src/error.rs`: exactness tests for each new `VendorError` and `MissingVendorWheels` variant.

### 9.2 Snapshot tests (insta)

- New: `snapshot_vendor_mode_muntjac_bzl` (proves the macro grows the `vendor:` branch).
- Existing snapshots for fixtures 01–10 must remain byte-identical with `vendor_mode=false`.

### 9.3 Integration tests

- `tests/vendor.rs` (NEW):
  - `vendor_committed_downloads_and_prebakes_into_vendor_dir`
  - `vendor_committed_prunes_stale_wheels`
  - `vendor_committed_with_no_prune_keeps_stale`
  - `vendor_committed_with_no_network_errors`
  - `buckify_committed_aborts_on_missing_wheels` (mode-mismatch DX check)
- `tests/multi_tree.rs` extension:
  - `buckify_committed_multi_tree_emits_per_tree_vendor_refs` (emit-only, no buck2)
- `tests/buckify.rs` extension:
  - `fixture_11_vendor_golden`

### 9.4 Fixture `tests/fixtures/buck/11-vendor/`

- `muntjac.toml`: single tree, `[buck] vendor = true`, `python_versions = ["3.12"]`, one or two platforms.
- `third-party/python/vendor/<wheels>`:
  - One real binary wheel (e.g., `idna-3.x-py3-none-any.whl`, ~70KB) — exercises download.
  - One prebake-built wheel from a tiny `flit-core`-backed package — exercises sdist→vendor path.
- `.buckconfig`, `toolchains/BUCK`, `PACKAGE` copied from fixture 02; prelude submodule pinned to the same SHA as fixtures 02/10.
- `expected/` golden: `BUCK`, `muntjac.bzl`, `config/BUCK`, `wiring.bzl` (vendor mode).
- Smoke target `//tests/smoke:demo` importing both packages.

### 9.5 CI (`.github/workflows/ci.yml`)

Add three steps (all 3 runners — linux-x86_64, linux-arm64, macos-arm64 — no arch gating):

- `muntjac buckify (11-vendor fixture)` — emit + golden compare
- `muntjac vendor --frozen (11-vendor fixture)` — idempotence sanity check (already-vendored dir should be a no-op or only re-stat)
- `buck2 build + run (11-vendor)` — verifies committed wheels actually build

---

## 10. Roll-up

| Item | Count |
|------|-------|
| New modules | 1 (`src/vendor/`) |
| New CLI flags | 2 (`--mode`, `--no-prune`) |
| New error variants | 4 (`VendorError::*` × 3, `BuckifyError::MissingVendorWheels`) |
| New fixtures | 1 (`11-vendor`) |
| New integration test files | 1 (`tests/vendor.rs`) |
| Config schema changes | 0 (existing `[buck] vendor` wired up) |
| Backwards-compat guarantees | fixtures 01–10 byte-identical |

---

## 11. Tech debt to log post-stage

- Multi-tree + vendor combination not exercised end-to-end at buck2-build level (only Rust integration tests + emit-only multi-tree check). A `12-vendor-multi-tree` fixture is a natural follow-up.
- Parallel downloads deferred — sequential for v0.3.0.
- Dropping S5's `prebake/.manifest.toml` in favor of filesystem checks deferred (scope avoided; could simplify by aligning prebake-only mode with the no-manifest committed-mode design).
- `muntjac vendor --check` (verify-without-write) defer.
- `muntjac vendor --offline-from <dir>` (mirror from a local PyPI cache) defer.
