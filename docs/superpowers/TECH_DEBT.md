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

#### Cycle error formatting is opaque
- **Source:** S1 final code-quality review
- **Severity:** Important
- **What:** `src/error.rs` `LockfileError::Cycle(Vec<String>)` uses `#[error("dependency cycle(s) detected: {0:?}")]`. Debug-formatting a `Vec<String>` renders as Rust-quoted: `["a@1.0 -> b@1.0", "c@1.0 -> d@1.0"]`.
- **Why it matters:** End-user error messages contain rust syntax (`["..."]`). Multiple cycles aren't visually separated. The arrow direction in the cycle string reflects sorted `NodeId` order, not actual edge direction — misleading for debugging.
- **Fix:** Hand-roll a `Display` impl that joins cycles with newlines and a leading `- ` bullet. Rotate each cycle's SCC to start at the lexicographically smallest member and walk along actual outgoing edges.
- **Target:** S2 (cheap; do it before more cycle-emitting code lands).

#### Fixture 06-cycle-error assertion is too loose
- **Source:** S1 final code-quality review
- **Severity:** Important
- **What:** `tests/fixtures/lock/06-cycle-error/expected-error.txt` contains only the substring `dependency cycle(s) detected`. The test passes even if cycle members are wrong or missing.
- **Why it matters:** Regressions that drop cycle member names from the error message wouldn't be caught.
- **Fix:** Append at least one expected member identifier (e.g. `alpha@1.0`) to the fixture's expected-error.txt. The `assert_error` helper already uses substring matching so node ordering doesn't have to be byte-stable.
- **Target:** S2 (pair with the cycle-formatting fix above).

#### `marker_matches` in `src/platform.rs` is dead code
- **Source:** S1 final code-quality review
- **Severity:** Polish
- **What:** Only `edge_applies` (in `src/lock/graph.rs`) is used downstream after Task 10. `marker_matches` is exported but never called.
- **Why it matters:** Either delete or document. If left, future readers think it's the canonical helper.
- **Fix:** Either remove, or add a doc comment saying "use this for non-edge marker checks (e.g. when S2 picks wheels based on tag-implied platform constraints)" and use it deliberately in S2.
- **Target:** S2 (decide one way or the other when wheel selector lands).

#### `derive_env_strings` warning branch is unreachable
- **Source:** S1 final code-quality review
- **Severity:** Polish
- **What:** `src/platform.rs::derive_env_strings` prints `warning: unknown target triple` and returns empty strings for unrecognized triples. But `Config::validate` (S0) rejects unknown triples upstream, so this branch can't fire in practice.
- **Why it matters:** Dead defensive code obscures the actual invariants.
- **Fix:** Replace with `unreachable!("Config::validate must have rejected this triple")`, or remove the print and document the invariant.
- **Target:** S2 (when wheel selector touches the same code path).

#### `BadVersion` error variant overloaded for URL parse failures
- **Source:** S1 final code-quality review
- **Severity:** Polish
- **What:** `src/lock/parser.rs` reuses `LockfileError::BadVersion` for sdist/wheel/git URL parse failures (with `reason: "wheel URL: ..."`). The error name implies a Python version string failed to parse.
- **Why it matters:** Confusing in logs — "BadVersion" for a malformed URL doesn't match user mental model.
- **Fix:** Add a `LockfileError::BadUrl { package: String, field: &'static str, url: String, reason: String }` variant. Migrate the URL parse failures to use it.
- **Target:** S2 (when URL parsing surfaces more often).

#### Fixture goldens are version-pinned to current PyPI
- **Source:** S1 final code-quality review
- **Severity:** Polish
- **What:** Goldens like `tests/fixtures/lock/01-pure-python/expected-print-deps.json` contain concrete version pins (`certifi@2026.5.20`, `requests@2.34.2`, etc.). The `uv.lock` files travel with the goldens so PyPI changes don't break tests — but if a contributor regenerates fixtures with `uv lock` against live PyPI, both lockfile and golden drift together.
- **Why it matters:** No silent rot, but a contributor might mistakenly regenerate just the lockfile and not the golden, then debug a phantom test failure.
- **Fix:** Add `tests/fixtures/lock/README.md` explaining: lockfiles are frozen artifacts; regenerating requires also regenerating the golden; reviewers should diff both.
- **Target:** S2 or any stage that adds another fixture.

#### `pep508_rs` extras activation asymmetry needs a louder comment
- **Source:** S1 final code-quality review
- **Severity:** Polish
- **What:** `src/lock/graph.rs::reachable_with_extras` (added in commit `913ef58`) handles two extras sources with different scopes:
  - `include_groups` from `muntjac.toml` is **global** (activated on every node)
  - `DepEdge.extra` from `pkg[extra]` requests is **target-side** (activated only on the requested target node, propagates via re-enqueue when new extras unlock edges)
- **Why it matters:** Subtle. A future maintainer reading the code might assume both work the same way.
- **Fix:** Expand the doc comment on `reachable_with_extras` to call out the asymmetry with example.
- **Target:** S2.

#### `print_deps.rs` uses `std::env::current_dir()` rather than `globals.workdir`
- **Source:** S1 final code-quality review
- **Severity:** Important
- **What:** `src/cli/debug/print_deps.rs` calls `std::env::current_dir()` to find `muntjac.toml`. The `-C <path>` global flag works only because the CLI's `run()` calls `std::env::set_current_dir(path)` before dispatching.
- **Why it matters:** The implicit chdir is fragile. If S2 introduces parallel work or a long-running mode, mutating the global cwd will surprise. Test isolation also depends on this happening early enough.
- **Fix:** Plumb `&Globals` through, or add a `globals.workdir() -> PathBuf` helper that resolves the right base path. Audit all CLI subcommands to use it consistently.
- **Target:** S2 or S3 (before more CLI surface lands).

### From S1 implementer self-reports

#### `BadGroupName` regex `[a-z][a-z0-9-]*` may be too restrictive
- **Source:** S1 Task 12 implementer concern
- **Severity:** Minor
- **What:** PEP 735 group names allow underscores and (per some interpretations) uppercase. Muntjac's validator rejects e.g. `test_extras`.
- **Why it matters:** Users may have valid pyproject.toml dependency-group names that muntjac refuses to reference.
- **Fix:** Cross-check the actual uv lockfile output against group names containing `_` / uppercase. Loosen the regex if real-world usage requires it.
- **Target:** S6 (when fixup engine starts consuming group config).

#### `include_groups` duplicates are silently preserved
- **Source:** S1 Task 12 implementer concern
- **Severity:** Polish
- **What:** `[lockfile] include_groups = ["test", "test"]` parses without error and the resulting `Vec<String>` has duplicates.
- **Why it matters:** Downstream uses set semantics for matching, so behavior is correct — but a typo in the config silently does nothing instead of being flagged.
- **Fix:** Either dedup with a warning in `Config::validate`, or accept and document.
- **Target:** S2.

#### Tarjan SCC is recursive — could stack-overflow on adversarial input
- **Source:** S1 Task 9 implementer concern
- **Severity:** Polish
- **What:** `src/lock/graph.rs::strongconnect` is recursive. uv.lock files are typically <500 nodes, but a hand-crafted adversarial input or a future Bazel-style monorepo with >10k packages could overflow the stack.
- **Why it matters:** Not a security issue for v1; muntjac trusts uv.lock to be well-formed. But the failure mode would be panic, not error.
- **Fix:** Convert to iterative form with an explicit stack. Standard Tarjan iterative algorithm.
- **Target:** S4 or later, only if a real input triggers it.

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

(none yet — items move here with the commit SHA that closed them.)
