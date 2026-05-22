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
- **Target:** S5 or later. The free-threaded Python non-goal in the main design spec was lifted in S4 (commit ab21bbe), so this item is no longer constrained by project posture. Implementation deferred until PEP 703 has more real-world signal or a user requests it.

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
