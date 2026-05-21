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

#### `BadVersion` error variant overloaded for URL parse failures
- **Source:** S1 final code-quality review
- **Severity:** Polish
- **What:** `src/lock/parser.rs` reuses `LockfileError::BadVersion` for sdist/wheel/git URL parse failures (with `reason: "wheel URL: ..."`). The error name implies a Python version string failed to parse.
- **Why it matters:** Confusing in logs — "BadVersion" for a malformed URL doesn't match user mental model.
- **Fix:** Add a `LockfileError::BadUrl { package: String, field: &'static str, url: String, reason: String }` variant. Migrate the URL parse failures to use it.
- **Target:** S4 (when wheel-URL handling surfaces during BUCK emission); originally targeted S2 but the wheel selector parses filenames not URLs, so the change can wait until URLs are more prominent.

### From S1 implementer self-reports

#### `BadGroupName` regex `[a-z][a-z0-9-]*` may be too restrictive
- **Source:** S1 Task 12 implementer concern
- **Severity:** Minor
- **What:** PEP 735 group names allow underscores and (per some interpretations) uppercase. Muntjac's validator rejects e.g. `test_extras`.
- **Why it matters:** Users may have valid pyproject.toml dependency-group names that muntjac refuses to reference.
- **Fix:** Cross-check the actual uv lockfile output against group names containing `_` / uppercase. Loosen the regex if real-world usage requires it.
- **Target:** S6 (when fixup engine starts consuming group config).

#### Tarjan SCC is recursive — could stack-overflow on adversarial input
- **Source:** S1 Task 9 implementer concern
- **Severity:** Polish
- **What:** `src/lock/graph.rs::strongconnect` is recursive. uv.lock files are typically <500 nodes, but a hand-crafted adversarial input or a future Bazel-style monorepo with >10k packages could overflow the stack.
- **Why it matters:** Not a security issue for v1; muntjac trusts uv.lock to be well-formed. But the failure mode would be panic, not error.
- **Fix:** Convert to iterative form with an explicit stack. Standard Tarjan iterative algorithm.
- **Target:** S4 or later, only if a real input triggers it.

### From S2 final stage review (2026-05-21, `s2-complete`)

#### `render_tag` duplicated between handler and snapshot tests
- **Source:** S2 T16 code-quality review
- **Severity:** Polish
- **What:** `src/cli/debug/pick_wheels.rs::render_tag` and `src/wheel/compat.rs::tests::render_tag` (plus their `render_python`/`render_abi`/`render_platform` helpers) are byte-for-byte identical. The snapshot copy is `#[cfg(test)]`-gated.
- **Why it matters:** If a new variant is added to `PlatformTag`/`PythonTag`/`AbiTag`, both copies must be updated by hand. The compiler will flag missing variants on each side, but the duplication is unnecessary.
- **Fix:** Implement `Display for Tag` in `src/wheel/tag.rs` (or lift `render_tag` to `pub(crate)`), then call it from both call sites.
- **Target:** S3 or any stage that touches the wheel module.

#### `cp313t` free-threaded ABI not first-class
- **Source:** S2 T8 code-quality review
- **Severity:** Minor
- **What:** Wheel filenames with `cp313t` (Python 3.13+ free-threaded ABI) currently parse as `AbiTag::Other("cp313t")`, which is correct for distinguishing from regular `cp313` but loses the structural info.
- **Why it matters:** Free-threaded wheels are becoming common (numpy, ML libraries ship them). The `Other` routing keeps them visibly distinct, but the wheel selector can't *prefer* a free-threaded wheel on a free-threaded interpreter — they all lose to compatible-list entries.
- **Fix:** Add a `free_threaded` boolean (or a `Threading` enum) to `AbiTag::CPython`. Extend `Config::Platform` and `PythonVersion` to carry a free-threading flag. Update the compatible-list builder.
- **Target:** S4+ once Python 3.13 free-threading stabilizes (`PEP 703` finalized).

### From S0 final stage review (2026-05-20, `s0-complete`)

#### `expand_requires_python` floor of 3.11 is hard-coded
- **Source:** S0 final code-quality review
- **Severity:** Polish
- **What:** `src/cli/init.rs` clamps Python versions to `>=3.11` regardless of what `pyproject.toml`'s `requires-python` says. So `requires-python = ">=3.10"` resolves to `[3.11, 3.12, 3.13]`, not `[3.10, ...]`.
- **Why it matters:** Currently intentional — muntjac's MVP doesn't support 3.10. But documented only inline; surprising for users.
- **Fix:** Add a doc comment on `expand_requires_python` explaining the floor. Better yet, sources the floor from a single constant `MIN_SUPPORTED_PY_MINOR = 11` referenced everywhere.
- **Target:** Any stage; mechanical refactor.

#### `RawConfig::platforms` has no `#[serde(default)]`
- **Source:** S0 final code-quality review
- **Severity:** Polish
- **What:** A `muntjac.toml` with zero `[platforms.*]` tables fails with a generic Parse error instead of a clearer `MissingField` error.
- **Why it matters:** Bad UX for a legitimate user mistake (forgot to declare any platform).
- **Fix:** Add `#[serde(default)]` to `RawConfig::platforms`, then add an explicit check in `Config::from_raw` that returns `ConfigError::MissingField("platforms")` if the BTreeMap is empty.
- **Target:** Any stage.

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
