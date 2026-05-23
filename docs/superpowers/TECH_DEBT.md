# Muntjac tech-debt log

Items flagged during stage reviews and implementer self-reports that didn't
block stage completion but should be addressed in a future stage.

**Format per item:**
- **Source** — which review/task flagged it
- **Severity** — Critical / Important / Minor / Polish
- **What** — the issue
- **Why it matters** — concrete impact
- **Fix** — suggested resolution
- **Target stage** — when to address (best guess; can slip)

When an item is resolved, move it to the **## Resolved** section at the bottom
with the commit SHA that closed it. Don't delete — the history is useful when a
similar issue surfaces.

---

## Open

### From S1 final stage review (2026-05-20, `s1-complete`)

### From S1 implementer self-reports

#### `BadGroupName` regex `[a-z][a-z0-9-]*` may be too restrictive
- **Source:** S1 Task 12 implementer concern
- **Severity:** Minor
- **What:** PEP 735 group names allow underscores and (per some interpretations) uppercase. Muntjac's validator rejects e.g. `test_extras`.
- **Why it matters:** Users may have valid pyproject.toml dependency-group names that muntjac refuses to reference.
- **Fix:** Cross-check the actual uv lockfile output against group names containing `_` / uppercase. Loosen the regex if real-world usage requires it.
- **Target:** S6 (when fixup engine starts consuming group config).

### From S2 final stage review (2026-05-21, `s2-complete`)

#### `cp313t` free-threaded ABI not first-class
- **Source:** S2 T8 code-quality review
- **Severity:** Minor
- **What:** Wheel filenames with `cp313t` (Python 3.13+ free-threaded ABI) currently parse as `AbiTag::Other("cp313t")`, which is correct for distinguishing from regular `cp313` but loses the structural info.
- **Why it matters:** Free-threaded wheels are becoming common (numpy, ML libraries ship them). The `Other` routing keeps them visibly distinct, but the wheel selector can't *prefer* a free-threaded wheel on a free-threaded interpreter — they all lose to compatible-list entries.
- **Fix:** Add a `free_threaded` boolean (or a `Threading` enum) to `AbiTag::CPython`. Extend `Config::Platform` and `PythonVersion` to carry a free-threading flag. Update the compatible-list builder.
- **Target:** post-launch (no concrete stage). S5 explicitly did not fold this in — see [S5 spec](specs/2026-05-22-muntjac-s5-sdist-prebake-design.md) §1 (Non-goals). Deferred until PEP 703 has more real-world signal or a user requests it.

### From S5 final stage review (2026-05-23, pre-tag)

#### `UnknownBackend { build_backend: "" }` is ambiguous for missing-vs-empty
- **Source:** S5 T2 code-quality review
- **Severity:** Minor (diagnostics)
- **What:** `classify()` reports `Native { UnknownBackend { build_backend: "" } }` for both "missing build-backend key" and "empty `build-backend = ''` string" cases. Users see "unknown build backend: ``" with no hint of which.
- **Why it matters:** Harder to diagnose a borked `pyproject.toml`.
- **Fix:** Split into `NativeReason::MissingBuildBackend` for the absent case; keep `UnknownBackend` for "present but unrecognized." One new variant + one match arm.
- **Target:** S6 (when fixup-side ergonomics matters more) or whenever next-touched.

#### `unwrap_or` → `unwrap_or_else` micro-optimization in classifier
- **Source:** S5 T2 code-quality review
- **Severity:** Polish
- **What:** `src/sdist/classifier.rs:128` uses `pathdiff::diff_paths(&path, root).unwrap_or(path.clone())` which eagerly clones even on `Some(...)`. Use `.unwrap_or_else(|| path.clone())`.
- **Why it matters:** Nothing perf-critical; the clippy lint `or_fun_call` would catch it under `-W clippy::pedantic`.
- **Fix:** 1 line.
- **Target:** Any time.

#### Trailing blank line at manifest EOF
- **Source:** S5 T3 code-quality review
- **Severity:** Polish
- **What:** `Manifest::save` ends the file with `\n\n` due to the per-entry blank-line `writeln!` pattern (or `\n\n` for the empty-entries case).
- **Why it matters:** Some lint tools flag trailing blank lines; downstream golden assertions could be brittle.
- **Fix:** `trim_end` the buffer before writing, append a single `\n`.
- **Target:** Any time.

#### Manifest schema version not enforced on load
- **Source:** S5 T3 code-quality review
- **Severity:** Minor
- **What:** `Manifest::load` accepts any `version` value silently; a future v=2 manifest would load as v=1 with potentially-misinterpreted fields.
- **Why it matters:** Forward-compat hazard for v0.2+.
- **Fix:** Add `if raw.version != 1 { return Err(ManifestParse { ... "unsupported manifest version `{n}`, expected 1" }); }` guard.
- **Target:** Post-S8 (only matters when a schema bump is contemplated).

#### No `#[serde(deny_unknown_fields)]` on `RawManifest`/`RawEntry`
- **Source:** S5 T3 code-quality review
- **Severity:** Polish
- **What:** Typo'd keys in a hand-edited manifest silently become defaults. With the strict pure-python validation in place this is partially mitigated, but unknown fields anywhere are still silently dropped.
- **Why it matters:** Bad UX for fixup authors editing the manifest by hand (which they shouldn't, since it's `@generated`).
- **Fix:** Add `#[serde(deny_unknown_fields)]` to both structs.
- **Target:** Any time; bundle with the trailing-newline + schema-version items.

#### T4 test name + comment don't match what's being tested
- **Source:** S5 T4 code-quality review
- **Severity:** Polish
- **What:** `uv_version_returns_uv_not_found_when_path_empty` doesn't actually call `uv_version()` and doesn't test the `UvNotFound` mapping; it sanity-checks that spawning a bogus binary fails. The comment "the helper above" refers to nothing.
- **Why it matters:** Future readers will be confused about test intent.
- **Fix:** Rename to `spawning_nonexistent_binary_fails` (or similar honest name) and drop or rewrite the misleading comment.
- **Target:** Any time.

#### `filter_map(|r| r.ok())` in prebake silently drops per-entry read_dir errors
- **Source:** S5 T5 code-quality review
- **Severity:** Minor (defensiveness)
- **What:** `src/sdist/prebake.rs::build_wheel` collects wheel files via `read_dir(out_dir).filter_map(|r| r.ok())`. Per-entry I/O errors are silently dropped; if a wheel exists but can't be statted, the loop returns 0-found and surfaces `PrebakeOutputUnexpected` instead of the real cause.
- **Why it matters:** Misleading diagnostic; low likelihood in a fresh tempdir.
- **Fix:** Use `for entry in rd { let entry = entry.with_context(...)?; ... }` loop.
- **Target:** Any time.

#### `SdistError::Extract` variant is overloaded
- **Source:** S5 T5 code-quality review
- **Severity:** Minor (diagnostics)
- **What:** `SdistError::Extract` originally meant tarball extraction (T10 use case). T5's `prebake.rs` uses it for `read_dir` / `File::open` / `Read` failures on the wheel output directory. Users see "failed to extract tarball" even for sha256-streaming I/O.
- **Why it matters:** Misleading error text.
- **Fix:** Split into `Extract` (tarball-only) and `WheelRead { package, version, source }` (read-after-prebake). 1 new variant + 4 call-site updates.
- **Target:** Any time.

#### `pypi_package` macro accepts `**kwargs` but never consumes them
- **Source:** S5 T6 code-quality review
- **Severity:** Polish
- **What:** `pypi_package(name, version, wheels, deps = [], visibility = None, **kwargs)` accepts `**kwargs` but never consumes/forwards them. Quiet API surface.
- **Why it matters:** Future readers may wonder if it's wired somewhere.
- **Fix:** Either drop `**kwargs` from the macro signature, or add a comment explaining "reserved for future per-package metadata."
- **Target:** S6 (when fixup-side metadata starts arriving).

#### `build_emit_input` is 213 lines after T8's sdist routing block
- **Source:** S5 T8 code-quality review
- **Severity:** Polish
- **What:** The function is on the edge of "scrollable at a glance" after T8 added the sdist-only routing block.
- **Why it matters:** Maintenance hazard if it grows further.
- **Fix:** Extract `route_sdist_only_package(...) -> Result<Option<(EmitWheel, Vec<String>)>, _>` helper.
- **Target:** S6 or whenever next-touched.

#### Magic string `"prebake:"` has no Rust-side const
- **Source:** S5 T8 code-quality review
- **Severity:** Polish
- **What:** The `"prebake:"` URL prefix appears in `src/buck/emit.rs` and is parsed in the `muntjac.bzl` macro. No Rust-side const ties them together.
- **Why it matters:** Grep-ability; coupling is implicit.
- **Fix:** Add `pub const PREBAKE_URL_PREFIX: &str = "prebake:";` to `src/sdist/mod.rs` (or wherever the discriminator naturally lives), use it in `emit.rs`.
- **Target:** Any time.

#### Symlink-traversal attack surface in `extract_tarball`
- **Source:** S5 T10 code-quality review (rated Important)
- **Severity:** Important (security, low real-world likelihood for curated corpus)
- **What:** T10's `extract_tarball` validates archive entry NAMES for `../` / `/` components, but doesn't validate symlink targets. A malicious tarball with a symlink whose target is `../../etc/passwd` would unpack the symlink as-is.
- **Why it matters:** PyPI sdists are mostly safe but the threat model isn't zero. Pre-existing for v0.1.0 corpus; should harden before processing arbitrary PyPI in S7's community-registry world.
- **Fix:** In the per-entry loop, also reject `EntryType::Symlink` / `Link` whose `link_name()` contains `..` or starts with `/`.
- **Target:** S7 (before community registry processes arbitrary packages) or earlier if convenient.

#### `tar-rs` PAX mtime gap is load-bearing for wheel determinism
- **Source:** S5 T11 implementer finding (fix `de34102`)
- **Severity:** Minor (documented in-line, but worth a TECH_DEBT entry)
- **What:** `tar-rs` doesn't honor PAX `mtime` extensions. T11 added an explicit PAX read + `filetime::set_file_mtime` restore. Without this, sdists built by GNU tar / setuptools (which use PAX) lose mtimes, and flit-core then refuses to ZIP files with <1980 timestamps.
- **Why it matters:** Hidden dependency on `filetime` crate behavior; if it ever changes, wheel determinism breaks silently.
- **Fix:** An upstream issue + PR to `tar-rs` would fix this for everyone. For muntjac, monitor `tar-rs` releases for a fix and drop the manual restore when one lands.
- **Target:** Post-launch / monitor upstream.

#### Fixture 04 lacks a root `.gitignore`
- **Source:** S5 T12 implementer note
- **Severity:** Polish
- **What:** Running `buckify` locally against fixture 04 leaves `buck-out/`, generated `BUCK`/`muntjac.bzl`/`wiring.bzl`/`config/` untracked. Fixture 02 has a `.gitignore` covering this.
- **Why it matters:** Noisy `git status` during local dev.
- **Fix:** Add a `.gitignore` to `tests/fixtures/buck/04-pure-python-sdist/` covering `buck-out/`, `third-party/python/BUCK`, `third-party/python/muntjac.bzl`, `third-party/python/wiring.bzl`, `third-party/python/config/`.
- **Target:** Any time; bundle with T13's polish pass.

---

## Resolved

### Cycle error formatting is opaque
- **Resolved:** S2, commit `a61b14d`
- **Summary:** `LockfileError::Cycle` storage changed to `Vec<Vec<String>>` with a hand-rolled `Display` (`#[error(fmt = fmt_cycle)]`). `detect_cycles` walks each SCC starting from the lex-smallest member, preferring unvisited outgoing edges. Output is sorted for determinism. New fixture-06 test asserts walk order with `assert_eq!(cycles[0], vec!["alpha@1.0", "beta@1.0"])`.

### Fixture 06-cycle-error assertion is too loose
- **Resolved:** S2, commit `af0192c`
- **Summary:** `tests/fixtures/lock/06-cycle-error/expected-error.txt` now contains both the header line and a member-identifier bullet line. Regressions that drop cycle member names from the error message will fail the substring assertion in `tests/print_deps.rs::fixture_06_cycle_error`.

### `print_deps.rs` uses `std::env::current_dir()` rather than `globals.workdir`
- **Resolved:** S2, commit `805c042`
- **Summary:** Added `Globals::workdir() -> io::Result<PathBuf>` that canonicalizes `-C` if set, else returns `std::env::current_dir()`. Removed `std::env::set_current_dir(path)` from `cli::run`. All three handlers (`print_deps`, `init`, `config_check`) now consume `globals.workdir()` directly. New integration test `print_deps_does_not_depend_on_process_cwd` pins the contract.

### `derive_env_strings` warning branch is unreachable
- **Resolved:** S2, commit `09fdeeb`
- **Summary:** `Config::validate`'s strict baseline check (added in commit `6152f90`) guarantees only known triples reach `derive_env_strings`. The fallback arm in `src/platform.rs::derive_env_strings` became `unreachable!("Config::validate must reject unknown target triple ... before reaching here")`.

### `marker_matches` in `src/platform.rs` is dead code
- **Resolved:** S2, commit `8e80bec`
- **Summary:** Documented `marker_matches` as the canonical helper for *non-edge* marker checks (e.g. `requires-python` constraints), distinguished from `graph::edge_applies` which composes it with extras-aware logic specific to dep edges. Kept exported with the updated doc comment.

### `pep508_rs` extras activation asymmetry needs a louder comment
- **Resolved:** S2, commit `8e80bec`
- **Summary:** Expanded the doc comment on `graph::reachable_with_extras` to spell out the asymmetry between `include_groups` (global, every-node) and `DepEdge.extra` (target-side, per-node) with a worked example walkthrough.

### Fixture goldens are version-pinned to current PyPI
- **Resolved:** S2, commit `b8b3651`
- **Summary:** Added `tests/fixtures/lock/README.md` and `tests/fixtures/wheel/README.md` explaining the frozen-artifact convention: lockfiles are committed; regenerating requires also regenerating goldens in the same commit; reviewers should diff both.

### `include_groups` duplicates are silently preserved
- **Resolved:** S2, commit `174c4ba`
- **Summary:** Added `Config::dedupe_include_groups` (called automatically from `Config::from_str`) that deduplicates while preserving order. A stderr warning fires if duplicates were dropped, so config typos surface visibly.

### `render_tag` duplicated between handler and snapshot tests
- **Resolved:** S3, commit `42431b3`
- **Summary:** Implemented `Display` for `Tag`/`PythonTag`/`AbiTag`/`PlatformTag` in `src/wheel/tag.rs`. The duplicate `render_tag` in `src/cli/debug/pick_wheels.rs` and the test-only `render_*` helpers in `src/wheel/compat.rs::tests` were both deleted; all call sites now use `tag.to_string()`. Snapshot output is byte-identical to the previous helpers.

### `expand_requires_python` floor of 3.11 is hard-coded
- **Resolved:** S3, commit `6110d29`
- **Summary:** Extracted the literal `11` floor in `src/cli/init.rs::expand_requires_python` into a named `pub(crate) const MIN_SUPPORTED_PY_MINOR: u8 = 11;` with a doc comment explaining the MVP rationale.

### `RawConfig::platforms` has no `#[serde(default)]`
- **Resolved:** S3, commit `08c42a6`
- **Summary:** Added `#[serde(default)]` to `RawConfig::platforms` and an explicit `is_empty()` check in `Config::from_raw` that returns `ConfigError::MissingField("platforms")`. A `muntjac.toml` with zero `[platforms.*]` tables now fails with the clearer error rather than a generic Parse error.

### `BadVersion` error variant overloaded for URL parse failures
- **Resolved:** S4, commit `1f8d1bf`
- **Summary:** Added `LockfileError::BadUrl { package, field: &'static str, url, reason }`. `src/lock/parser.rs` migrated four URL parse sites (sdist, wheel, registry, git) off `BadVersion`. New unit test asserts the variant on malformed wheel URLs.

### Tarjan SCC is recursive — could stack-overflow on adversarial input
- **Resolved:** S4, commit `cfabbd4`
- **Summary:** Converted `tarjan_scc` in `src/lock/graph.rs` to iterative form using an explicit `Vec<(node, edge_cursor)>` work stack. The recursive form actually overflowed at 5000 nodes on cargo's default 2MB test-thread stack — verified, this was a real bug, not just hardening. Algorithm output unchanged; existing graph tests pass. New stress test asserts a 5000-node linear chain processes without stack overflow.
